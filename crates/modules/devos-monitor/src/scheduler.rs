//! The check loop, and the transition rule it exists to enforce.
//!
//! Alerts follow *changes* in reachability, not check results. A site that is
//! down for an hour on a 15-second tick would otherwise produce 240
//! notifications; it produces one. That rule is the reason the scheduler owns
//! transition logic at all instead of the checker, and it is kept in
//! [`alert_for`] — a pure function, testable without a timer, a database, or
//! a socket — rather than buried in the async loop below.

use std::sync::Arc;
use std::time::Duration;

use devos_kernel::Kernel;

use crate::{Monitor, MonitorCheck, MonitorResult};

/// How often the loop looks for work. Monitors carry their own intervals
/// (floored at 60s); this only bounds how late a due check can run.
const TICK: Duration = Duration::from_secs(15);

/// Checks older than this are dropped on every tick. Long enough for the 24h
/// aggregates and a week of eyeballing, short enough that a minute-interval
/// monitor stays around ten thousand rows.
const RETENTION_DAYS: i64 = 7;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// A reachability change worth telling the user about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorAlert {
    /// Kernel notification level: `warning` going down, `info` coming back.
    pub level: &'static str,
    pub title: String,
    pub body: Option<String>,
}

/// The notification a check warrants, given what the previous check said.
///
/// `previous_ok` is `None` for a monitor's first-ever check, which is treated
/// as "assumed up": a first check that fails alerts, because the user just
/// registered a URL that is already unreachable and that is worth saying,
/// while a first check that succeeds stays quiet, because a thing you just
/// set up working is not news.
pub fn alert_for(
    name: &str,
    previous_ok: Option<bool>,
    check: &MonitorCheck,
) -> Option<MonitorAlert> {
    match (previous_ok.unwrap_or(true), check.ok) {
        (true, false) => Some(MonitorAlert {
            level: "warning",
            title: format!("{name} is down"),
            body: Some(failure_detail(check)),
        }),
        (false, true) => Some(MonitorAlert {
            level: "info",
            title: format!("{name} recovered"),
            body: Some(format!("responded in {} ms", check.duration_ms)),
        }),
        // Still down, or still up. Nothing changed, so nothing is said.
        _ => None,
    }
}

/// Why a check counts as down, in the words the user will see.
fn failure_detail(check: &MonitorCheck) -> String {
    match (&check.error, check.status) {
        (Some(error), _) => error.clone(),
        (None, Some(status)) => format!("HTTP {status}"),
        (None, None) => "check failed".to_string(),
    }
}

/// Whether a monitor is due, given its newest check.
///
/// A monitor that has never been checked is always due, so a freshly created
/// one shows a result on the next tick instead of an empty row for a full
/// interval.
fn is_due(previous: Option<&MonitorCheck>, interval_secs: i64, now_ms: i64) -> bool {
    match previous {
        Some(check) => now_ms - check.checked_at >= interval_secs * 1_000,
        None => true,
    }
}

/// Run the check loop for the life of the process.
///
/// Spawned by the desktop shell rather than spawning itself, so the crate
/// stays free of assumptions about which runtime it is living in.
pub async fn run_scheduler(kernel: Arc<Kernel>) {
    let mut ticker = tokio::time::interval(TICK);
    // Without `Delay`, a tick that overran (several monitors timing out at
    // 15s apiece) would be followed by a burst of catch-up ticks firing back
    // to back, which is the opposite of what a rate floor is for.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        if let Err(error) = tick(&kernel).await {
            // Failing to even list monitors is a database problem. Log it and
            // try again next tick rather than killing the loop — a transient
            // lock must not silently end monitoring for the whole session.
            tracing::warn!(%error, "monitor tick failed");
        }
    }
}

/// One pass: prune history, then check whatever is due.
async fn tick(kernel: &Kernel) -> MonitorResult<()> {
    let now = chrono::Utc::now().timestamp_millis();

    if let Err(error) = crate::repo::prune_checks(&kernel.pool, now - RETENTION_DAYS * DAY_MS).await
    {
        tracing::warn!(%error, "pruning monitor history failed");
    }

    // Checks run one at a time. With a 60s interval floor, a 15s per-request
    // timeout, and the handful of monitors this page is built for, there is
    // ample headroom without the complexity of a join set.
    for monitor in crate::repo::list_monitors(&kernel.pool).await? {
        if !monitor.enabled {
            continue;
        }
        // One monitor's trouble must not end the tick: a single unreachable
        // host would otherwise silence every other monitor behind it.
        if let Err(error) = check_if_due(kernel, &monitor, now).await {
            tracing::warn!(monitor = %monitor.name, %error, "monitor check failed");
        }
    }
    Ok(())
}

async fn check_if_due(kernel: &Kernel, monitor: &Monitor, now_ms: i64) -> MonitorResult<()> {
    let previous = crate::repo::latest_check(&kernel.pool, &monitor.id).await?;
    if !is_due(previous.as_ref(), monitor.interval_secs, now_ms) {
        return Ok(());
    }
    run_and_record(kernel, monitor, previous).await?;
    Ok(())
}

/// Check one monitor right now, whatever its interval says.
///
/// Shares [`run_and_record`] with the scheduler on purpose: a manual check
/// and a scheduled one must not be able to drift apart on what gets stored or
/// what gets announced.
pub async fn check_now(kernel: &Kernel, id: &str) -> MonitorResult<MonitorCheck> {
    let monitor = crate::repo::get_monitor(&kernel.pool, id).await?;
    let previous = crate::repo::latest_check(&kernel.pool, id).await?;
    run_and_record(kernel, &monitor, previous).await
}

async fn run_and_record(
    kernel: &Kernel,
    monitor: &Monitor,
    previous: Option<MonitorCheck>,
) -> MonitorResult<MonitorCheck> {
    let check = crate::run_check(&monitor.id, &monitor.url).await?;
    crate::repo::record_check(&kernel.pool, &check).await?;
    if let Some(alert) = alert_for(&monitor.name, previous.map(|c| c.ok), &check) {
        kernel
            .notify("monitor", alert.level, &alert.title, alert.body.as_deref())
            .await?;
    }
    Ok(check)
}

#[cfg(test)]
mod tests {
    use super::*;
    use devos_kernel::repo as kernel_repo;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn check(ok: bool, status: Option<u16>, error: Option<&str>) -> MonitorCheck {
        MonitorCheck {
            id: "c1".into(),
            monitor_id: "m1".into(),
            status,
            ok,
            duration_ms: 42,
            error: error.map(str::to_string),
            checked_at: 1_700_000_000_000,
        }
    }

    async fn test_kernel() -> (tempfile::TempDir, Kernel) {
        let dir = tempfile::tempdir().unwrap();
        let kernel = Kernel::boot(&dir.path().join("devos.db")).await.unwrap();
        crate::repo::init(&kernel.pool).await.unwrap();
        (dir, kernel)
    }

    /// Bind and immediately release a port, so a monitor can point at
    /// somewhere reliably closed and then have a server appear there.
    async fn reserved_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    }

    /// One-shot HTTP server on a known port; same hermetic technique as
    /// `devos-api`, but pinned so a monitor URL can outlive it.
    async fn one_shot_server_on(port: u16, response: &'static str) -> tokio::task::JoinHandle<()> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 8192];
            // What the monitor sent is asserted in `check.rs`; here only the
            // fact that the port answers matters.
            let _request_bytes = socket.read(&mut request).await.unwrap();
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();
        })
    }

    #[test]
    fn alerts_fire_only_on_transitions() {
        let up = check(true, Some(200), None);
        let down = check(false, None, Some("connection refused"));

        let went_down = alert_for("Docs", Some(true), &down).expect("ok -> fail alerts");
        assert_eq!(went_down.level, "warning");
        assert_eq!(went_down.title, "Docs is down");

        let recovered = alert_for("Docs", Some(false), &up).expect("fail -> ok alerts");
        assert_eq!(recovered.level, "info");
        assert_eq!(recovered.title, "Docs recovered");

        // The whole point: an hour of downtime is one notification, not 240.
        assert_eq!(alert_for("Docs", Some(false), &down), None, "fail -> fail");
        assert_eq!(alert_for("Docs", Some(true), &up), None, "ok -> ok");
    }

    #[test]
    fn a_first_check_alerts_only_when_it_fails() {
        // No previous state is treated as "assumed up".
        assert_eq!(
            alert_for("Docs", None, &check(true, Some(200), None)),
            None,
            "a monitor that works from the start is not news"
        );
        let first_failure =
            alert_for("Docs", None, &check(false, None, Some("dns error"))).expect("alerts");
        assert_eq!(first_failure.title, "Docs is down");
        assert_eq!(first_failure.body.as_deref(), Some("dns error"));
    }

    #[test]
    fn down_alerts_explain_themselves() {
        let transport =
            alert_for("Docs", Some(true), &check(false, None, Some("timed out"))).expect("alerts");
        assert_eq!(transport.body.as_deref(), Some("timed out"));

        let http = alert_for("Docs", Some(true), &check(false, Some(503), None)).expect("alerts");
        assert_eq!(http.body.as_deref(), Some("HTTP 503"));

        let recovered =
            alert_for("Docs", Some(false), &check(true, Some(200), None)).expect("alerts");
        assert_eq!(recovered.body.as_deref(), Some("responded in 42 ms"));
    }

    #[test]
    fn due_is_decided_by_the_interval_and_the_newest_check() {
        let now = 1_700_000_000_000;
        assert!(is_due(None, 60, now), "never checked is always due");

        let just_now = MonitorCheck {
            checked_at: now - 30_000,
            ..check(true, Some(200), None)
        };
        assert!(!is_due(Some(&just_now), 60, now), "30s into a 60s interval");

        let stale = MonitorCheck {
            checked_at: now - 60_000,
            ..check(true, Some(200), None)
        };
        assert!(
            is_due(Some(&stale), 60, now),
            "exactly one interval elapsed"
        );
        assert!(is_due(Some(&stale), 30, now), "well past a short interval");
        assert!(
            !is_due(Some(&stale), 300, now),
            "not yet into a 5m interval"
        );
    }

    #[tokio::test]
    async fn a_monitor_that_stays_down_notifies_once() {
        let (_dir, kernel) = test_kernel().await;
        let monitor = crate::repo::create_monitor(&kernel.pool, "Docs", "http://127.0.0.1:1/", 60)
            .await
            .unwrap();

        for _ in 0..3 {
            let check = check_now(&kernel, &monitor.id).await.unwrap();
            assert!(!check.ok);
            assert_eq!(check.status, None);
        }

        let notifications = kernel_repo::list_notifications(&kernel.pool, 10)
            .await
            .unwrap();
        assert_eq!(
            notifications.len(),
            1,
            "three failing checks, one notification: {notifications:?}"
        );
        assert_eq!(notifications[0].module, "monitor");
        assert_eq!(notifications[0].level, "warning");
        assert_eq!(notifications[0].title, "Docs is down");

        let status = &crate::repo::list_status(&kernel.pool).await.unwrap()[0];
        assert_eq!(status.recent.len(), 3, "every check is still recorded");
        assert_eq!(status.uptime_pct, 0.0);
    }

    #[tokio::test]
    async fn coming_back_up_notifies_again() {
        let (_dir, kernel) = test_kernel().await;
        let port = reserved_port().await;
        let monitor = crate::repo::create_monitor(
            &kernel.pool,
            "Docs",
            &format!("http://127.0.0.1:{port}/"),
            60,
        )
        .await
        .unwrap();

        let down = check_now(&kernel, &monitor.id).await.unwrap();
        assert!(!down.ok);

        let server = one_shot_server_on(
            port,
            "HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
        )
        .await;
        let up = check_now(&kernel, &monitor.id).await.unwrap();
        server.await.unwrap();
        assert!(up.ok, "expected a 200, got {up:?}");

        let notifications = kernel_repo::list_notifications(&kernel.pool, 10)
            .await
            .unwrap();
        assert_eq!(notifications.len(), 2, "down then recovered");
        assert_eq!(notifications[0].title, "Docs recovered", "newest first");
        assert_eq!(notifications[0].level, "info");
        assert_eq!(notifications[1].title, "Docs is down");
    }

    #[tokio::test]
    async fn a_tick_skips_disabled_monitors_and_prunes_history() {
        let (_dir, kernel) = test_kernel().await;
        let monitor = crate::repo::create_monitor(&kernel.pool, "Docs", "http://127.0.0.1:1/", 60)
            .await
            .unwrap();
        crate::repo::set_enabled(&kernel.pool, &monitor.id, false)
            .await
            .unwrap();

        // An ancient check, which the tick should prune whether or not the
        // monitor it belongs to is enabled.
        let ancient = MonitorCheck {
            id: "ancient".into(),
            monitor_id: monitor.id.clone(),
            checked_at: chrono::Utc::now().timestamp_millis() - 30 * DAY_MS,
            ..check(true, Some(200), None)
        };
        crate::repo::record_check(&kernel.pool, &ancient)
            .await
            .unwrap();

        tick(&kernel).await.unwrap();
        assert!(
            crate::repo::latest_check(&kernel.pool, &monitor.id)
                .await
                .unwrap()
                .is_none(),
            "disabled monitor was checked, and the old row was not pruned"
        );

        crate::repo::set_enabled(&kernel.pool, &monitor.id, true)
            .await
            .unwrap();
        tick(&kernel).await.unwrap();
        let recorded = crate::repo::latest_check(&kernel.pool, &monitor.id)
            .await
            .unwrap()
            .expect("an enabled, never-checked monitor is due");
        assert!(!recorded.ok);

        // Second tick lands inside the interval, so nothing new is recorded.
        tick(&kernel).await.unwrap();
        assert_eq!(
            crate::repo::latest_check(&kernel.pool, &monitor.id)
                .await
                .unwrap()
                .map(|c| c.id),
            Some(recorded.id),
            "a monitor inside its interval was checked again"
        );
    }
}
