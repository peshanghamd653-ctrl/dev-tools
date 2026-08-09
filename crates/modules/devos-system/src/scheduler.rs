//! The sampling loop behind the performance profiler's history chart.
//!
//! Mirrors `devos_monitor::run_scheduler`'s shape (spawned by the desktop
//! shell rather than spawning itself, so the crate stays runtime-agnostic),
//! but there is no transition logic here — a sample is recorded every tick,
//! unconditionally, because "what did CPU/memory look like over the last
//! couple of hours" needs a continuous trail, not just the moments something
//! changed.

use std::sync::Arc;
use std::time::Duration;

use devos_kernel::Kernel;

use crate::probe::SystemProbe;

/// How often a sample is recorded. Frequent enough that a short-lived spike
/// still shows up as more than one point; coarse enough that a few hours of
/// history stays a few hundred rows.
const TICK: Duration = Duration::from_secs(30);

/// How far back history is kept. Long enough to see "what happened this
/// afternoon"; short enough that `system_history` stays a small, fast read
/// (at the tick above, three hours is 360 rows).
const RETENTION_HOURS: i64 = 3;
const HOUR_MS: i64 = 60 * 60 * 1000;

/// Run the sampling loop for the life of the process.
pub async fn run_scheduler(kernel: Arc<Kernel>, probe: Arc<SystemProbe>) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        if let Err(error) = tick(&kernel, &probe).await {
            // A transient database hiccup must not end sampling for the rest
            // of the session — log it and pick back up on the next tick.
            tracing::warn!(%error, "system history tick failed");
        }
    }
}

async fn tick(kernel: &Kernel, probe: &Arc<SystemProbe>) -> crate::SystemResult<()> {
    let now = chrono::Utc::now().timestamp_millis();

    let probe = probe.clone();
    // Sampling sysinfo is synchronous and may briefly sleep (see
    // `SystemProbe::snapshot`), so it stays off the async worker running this
    // loop.
    let snapshot = tokio::task::spawn_blocking(move || probe.snapshot())
        .await
        .map_err(|e| crate::SystemError::Join(e.to_string()))?;

    crate::repo::record_sample(
        &kernel.pool,
        now,
        snapshot.cpu_usage,
        snapshot.mem_used,
        snapshot.mem_total,
    )
    .await?;
    crate::repo::prune_before(&kernel.pool, now - RETENTION_HOURS * HOUR_MS).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_kernel() -> (tempfile::TempDir, Kernel) {
        let dir = tempfile::tempdir().unwrap();
        let kernel = Kernel::boot(&dir.path().join("devos.db")).await.unwrap();
        crate::repo::init(&kernel.pool).await.unwrap();
        (dir, kernel)
    }

    #[tokio::test]
    async fn a_tick_records_one_sample_and_prunes_old_ones() {
        let (_dir, kernel) = test_kernel().await;
        let probe = Arc::new(SystemProbe::new());

        let now = chrono::Utc::now().timestamp_millis();
        crate::repo::record_sample(&kernel.pool, now - 10 * HOUR_MS, 1.0, 1, 100)
            .await
            .unwrap();

        tick(&kernel, &probe).await.unwrap();

        let history = crate::repo::list_history(&kernel.pool, 0).await.unwrap();
        assert_eq!(
            history.len(),
            1,
            "the ancient sample was pruned, only the fresh one remains: {history:?}"
        );
        assert!(history[0].mem_total > 0, "a real snapshot was recorded");
    }
}
