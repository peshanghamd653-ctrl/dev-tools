//! Boot phase timing.
//!
//! The app has always logged a single `startup_ms` number, which tells you
//! *that* startup regressed but not *where*. [`BootTimings`] breaks the boot
//! into the phases that can actually get slower — opening the pool, running
//! migrations, the first query, and module registration — so a regression
//! points at a culprit instead of starting an investigation.
//!
//! The instrumentation is `Instant::now()` plus `elapsed()` per phase: a few
//! nanoseconds each, no allocation, nothing conditional. It is always on.

use std::time::Duration;

/// Per-phase durations recorded by `Kernel::boot` and `Kernel::register_module`.
///
/// Read it back with `Kernel::boot_timings()`. Phases are recorded in the
/// order they run; `module_registration` accumulates across every
/// `register_module` call, which happen *after* `boot` returns, so it is not
/// included in [`BootTimings::boot`] — use [`BootTimings::total`] for the
/// kernel-side total.
///
/// The named phases are a *subset* of [`BootTimings::boot`], not a partition of
/// it: whatever else `Kernel::boot` does (today, the automatic pre-migration
/// and daily database snapshots) shows up only in the total. A growing gap
/// between `boot` and the sum of the phases is itself a signal worth chasing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BootTimings {
    /// Opening the SQLite pool: directory creation, file open, WAL setup.
    pub pool_open: Duration,
    /// Running (or verifying, on a warm DB) the embedded migrations.
    pub migrations: Duration,
    /// `ensure_default_workspace` — the first real query against the DB.
    pub default_workspace: Duration,
    /// Everything inside `Kernel::boot`, including the three phases above.
    pub boot: Duration,
    /// Cumulative time in `register_module`, summed over all modules.
    pub module_registration: Duration,
    /// How many `register_module` calls that sum covers.
    pub modules_registered: usize,
}

impl BootTimings {
    /// Kernel boot plus module registration — the kernel's share of the
    /// `startup_ms` the app logs. The rest of `startup_ms` (tracing setup,
    /// Tauri builder, per-module table init, webview) is not measured here.
    pub fn total(&self) -> Duration {
        self.boot + self.module_registration
    }

    pub(crate) fn record_module(&mut self, elapsed: Duration) {
        self.module_registration += elapsed;
        self.modules_registered += 1;
    }
}

#[cfg(test)]
mod tests {
    use crate::module::{Module, ModuleCtx};
    use crate::Kernel;
    use std::time::{Duration, Instant};

    /// A module that does nothing, so registration cost is the kernel's own
    /// bookkeeping and not the module's work.
    struct NoopModule;

    impl Module for NoopModule {
        fn id(&self) -> &'static str {
            "noop"
        }
        fn register(&self, _ctx: &ModuleCtx<'_>) {}
    }

    async fn boot_in_temp_dir() -> (tempfile::TempDir, Kernel) {
        let dir = tempfile::tempdir().expect("tempdir");
        let kernel = Kernel::boot(&dir.path().join("timing.db"))
            .await
            .expect("kernel boot");
        (dir, kernel)
    }

    /// Wall-clock tripwire on kernel boot. Read the threshold rationale before
    /// changing anything here.
    ///
    /// **Budget: best of 3 cold boots must come in under 5 s.**
    ///
    /// *What the number is based on.* Cold `Kernel::boot` against a fresh temp
    /// DB, debug build, measured on the development machine while several other
    /// agents were saturating its CPU and disk with `cargo build`:
    ///
    /// | sample | quiet-ish | saturated |
    /// |---|---|---|
    /// | single boot, p50 | ~100 ms | ~380–750 ms |
    /// | single boot, worst of 25 | ~270–400 ms | ~1375 ms |
    /// | best-of-3, worst of 15 trials | ~690 ms | ~920 ms |
    ///
    /// So a *single* sample on one machine, in one session, spans a factor of
    /// ~20 (69 ms → 1375 ms). Boot here is dominated by filesystem work — file
    /// creation, WAL setup, the pre-migration/daily backup's `VACUUM INTO`, and
    /// on Windows an antivirus pass over each newly created `.db` — none of
    /// which is stable under load. Taking the best of 3 collapses that spread
    /// to ~8x, because all three boots have to be unlucky at once. 5000 ms is
    /// ~5.4x the worst best-of-3 ever observed, and failing it requires three
    /// consecutive boots to each exceed 5 s on a machine where the median is
    /// ~0.3 s.
    ///
    /// *What this therefore does and does not catch.* It catches categorical
    /// breakage: a blocking network or IPC call added to the boot path, a
    /// migration that starts rewriting a real user's tables, per-file scanning
    /// at startup, a deadlock (which would otherwise hang CI forever instead of
    /// failing with a phase breakdown). It does **not** catch a 2x regression,
    /// and — being honest rather than reassuring — it does not reliably catch
    /// even a 10x one: 10x of the ~100 ms quiet-machine baseline is 1 s, which
    /// passes. No wall-clock threshold can do better here, because the noise
    /// band is wider than the signal. That is why the two structural tests
    /// below, not this one, are the real regression guards; this is a floor,
    /// not a budget.
    ///
    /// *If it fails*, do not raise the number. Run with `--nocapture`, read the
    /// per-phase breakdown, and find the phase that moved. If it ever *flakes*
    /// — fails, then passes on rerun with normal-looking phases — delete this
    /// assertion rather than inflating it until it asserts nothing. A perf test
    /// people learn to ignore is worse than no perf test.
    #[tokio::test]
    async fn cold_boot_stays_within_budget() {
        const BUDGET: Duration = Duration::from_secs(5);
        const ATTEMPTS: usize = 3;

        let mut runs: Vec<crate::BootTimings> = Vec::with_capacity(ATTEMPTS);
        for _ in 0..ATTEMPTS {
            let (_dir, kernel) = boot_in_temp_dir().await;
            runs.push(kernel.boot_timings());
        }
        let best = *runs
            .iter()
            .min_by_key(|t| t.boot)
            .expect("at least one boot");
        let samples: Vec<Duration> = runs.iter().map(|t| t.boot).collect();
        // Visible with `cargo test -p devos-kernel -- --nocapture`; these are
        // the numbers to look at when deciding whether the budget is sane.
        eprintln!("cold boots: {samples:?}\nfastest: {best:?}");

        assert!(
            best.boot <= BUDGET,
            "fastest of {ATTEMPTS} cold kernel boots took {:?}, over the \
             {BUDGET:?} budget (all: {samples:?}). Phases of the fastest: \
             pool_open={:?} migrations={:?} default_workspace={:?}",
            best.boot,
            best.pool_open,
            best.migrations,
            best.default_workspace,
        );

        // Phase accounting must add up, or the breakdown above is a lie and
        // the next person debugging a regression is misled by it. The phases
        // are a subset of boot, so they sum to less, never more.
        let phases = best.pool_open + best.migrations + best.default_workspace;
        assert!(
            phases <= best.boot,
            "phases ({phases:?}) exceed total boot ({:?})",
            best.boot
        );
        assert!(
            best.pool_open > Duration::ZERO && best.migrations > Duration::ZERO,
            "phases must actually be measured, got {best:?}"
        );
    }

    /// Structural, not timing-based: migrations are applied once and only once,
    /// no matter how many times the kernel boots against the same file. A
    /// regression here (re-running DDL, or re-checksumming every migration
    /// against the file on every boot) is the classic way boot time grows with
    /// the age of a user's install — the thing a wall-clock test on a *fresh*
    /// temp DB structurally cannot see.
    #[tokio::test]
    async fn migrations_apply_exactly_once_across_boots() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("reboot.db");

        // `execution_time` is written when a migration is applied. If a boot
        // re-applies one, that number is re-measured and will not match.
        let read_ledger = |pool: sqlx::SqlitePool| async move {
            sqlx::query_as::<_, (i64, i64)>(
                "SELECT version, execution_time FROM _sqlx_migrations ORDER BY version",
            )
            .fetch_all(&pool)
            .await
            .expect("read _sqlx_migrations")
        };

        let first = Kernel::boot(&db).await.expect("first boot");
        let applied = read_ledger(first.pool.clone()).await;
        assert!(!applied.is_empty(), "expected at least one migration");
        first.pool.close().await;

        let second = Kernel::boot(&db).await.expect("second boot");
        let reapplied = read_ledger(second.pool.clone()).await;

        assert_eq!(
            applied, reapplied,
            "second boot re-ran migrations instead of skipping them"
        );

        // And the default workspace is not duplicated on the second boot.
        let workspaces = crate::repo::list_workspaces(&second.pool).await.unwrap();
        assert_eq!(
            workspaces.len(),
            1,
            "reboot duplicated the default workspace"
        );
    }

    /// Structural: module registration is in-memory bookkeeping, with no DB
    /// round trip per module. Asserted *relative to a measured DB round trip on
    /// the same machine in the same run*, so it doesn't encode this machine's
    /// speed as a magic number and doesn't care how loaded the box is.
    ///
    /// Registering a module is a `Vec` push and a trait call. Measured here:
    /// 64 registrations take ~37 µs, 64 `SELECT 1` round trips take 26–47 ms —
    /// a gap of 700–1300x. Asserting only 8x leaves ~100x of slack for
    /// scheduler noise while still failing loudly if someone gives
    /// `register_module` a DB round trip per module, which would put the ratio
    /// at or below 1x. Both sides are best-of-3 to strip out the occasional
    /// multi-millisecond preemption on a loaded machine.
    #[tokio::test]
    async fn module_registration_does_no_per_module_db_work() {
        const MODULES: usize = 64;

        let (_dir, mut kernel) = boot_in_temp_dir().await;

        let mut best_registration = Duration::MAX;
        let mut best_db = Duration::MAX;
        for _ in 0..3 {
            let started = Instant::now();
            for _ in 0..MODULES {
                kernel.register_module(&NoopModule);
            }
            best_registration = best_registration.min(started.elapsed());

            let started = Instant::now();
            for _ in 0..MODULES {
                sqlx::query("SELECT 1")
                    .execute(&kernel.pool)
                    .await
                    .expect("select 1");
            }
            best_db = best_db.min(started.elapsed());
        }

        eprintln!(
            "{MODULES} module registrations: {best_registration:?} vs \
             {MODULES} DB round trips: {best_db:?}"
        );
        assert!(
            best_registration * 8 < best_db,
            "registering {MODULES} modules took {best_registration:?}, which is not \
             convincingly cheaper than {MODULES} DB round trips ({best_db:?}) — \
             register_module appears to have grown per-module I/O"
        );

        // The kernel's own accounting agrees about how much work it did.
        let timings = kernel.boot_timings();
        assert_eq!(timings.modules_registered, MODULES * 3);
        assert_eq!(kernel.module_ids().len(), MODULES * 3);
        assert!(
            timings.module_registration < timings.boot,
            "registering {} modules ({:?}) should stay far below the cost of \
             booting the kernel itself ({:?})",
            timings.modules_registered,
            timings.module_registration,
            timings.boot
        );
    }
}
