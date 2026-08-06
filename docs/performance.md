# Performance

## Budgets

| Metric | Target | Status |
|---|---|---|
| Cold start → kernel ready | < 1000 ms, **release build** | unverified — release has never been measured |
| `Kernel::boot` (kernel's own share of startup) | no separate budget yet | measured per phase, asserted loosely in CI |
| Base RAM | < 200 MB | not yet profiled |
| Interaction | 60 fps | not yet profiled |

**The startup target is a release-build target.** Every number the project has
ever recorded is from a `pnpm tauri dev` **debug** build — unoptimized, with a
dev-only file watcher attached — and a debug number cannot be compared to it in
either direction. A recent session logged `startup_ms=1175` on a warm start and
`startup_ms=2699` cold. Those are above 1000 ms, and that tells us nothing about
whether the budget is met: `cargo build --release` (LTO, `codegen-units = 1`,
strip — see the workspace `Cargo.toml`) has still not been measured. The budget
is neither met nor missed today. It is unverified, and measuring it is the
single most useful thing anyone could do to this document.

An older revision of this file reported 854–1461 ms and called the budget "on
track" in debug. Those numbers predate several modules being added to the boot
path; the 1175 / 2699 ms figures above supersede them.

## What is measured

### `startup_ms` — the whole-process number

Logged once per boot by `src-tauri/src/lib.rs`
(`tracing::info!(startup_ms, "devos kernel ready")`) and surfaced in
Settings → About via the `app_info` command. It spans process start →
tracing init → `Kernel::boot` → module registration → each module's table
init, and stops *before* the webview necessarily finishes its first paint.
It is the honest end-to-end number, and it is also a single scalar: it tells
you *that* startup regressed, never *where*.

### `BootTimings` — the per-phase breakdown

`BootTimings` (`crates/devos-kernel/src/timing.rs`) is filled in by
`Kernel::boot` and `Kernel::register_module`, and read back off the kernel with
`kernel.boot_timings()`:

| Field | Phase |
|---|---|
| `pool_open` | data-dir creation, SQLite file open, WAL setup |
| `migrations` | applying (or, on a warm DB, verifying) embedded migrations |
| `default_workspace` | `ensure_default_workspace` — the first real query |
| `boot` | all of `Kernel::boot`, phases included |
| `module_registration` / `modules_registered` | cumulative across `register_module` |

The same breakdown is logged at `info` on every boot as `kernel boot phases`
(`boot_ms`, `pool_open_us`, `migrations_us`, `default_workspace_us`), and each
`module registered` line now carries its own `elapsed_us`. The instrumentation
is `Instant::now()` and `elapsed()` per phase — a handful of nanoseconds, no
allocation, no feature flag, always on.

The phases deliberately do **not** sum to `boot`. The remainder is everything
else `Kernel::boot` does; today that is mostly the automatic database snapshot
(pre-migration and daily `VACUUM INTO`) that runs before and after migrations.
If the gap between the phase sum and `boot_ms` grows, that gap is the thing to
go look at.

Observed `Kernel::boot` (debug build, cold temp DB, development machine):

| | machine quiet | machine saturated by other builds |
|---|---|---|
| single boot, median | ~100 ms | ~380–750 ms |
| single boot, worst of 25 | ~270–400 ms | ~1375 ms |
| best of 3 boots, worst of 15 trials | ~690 ms | ~920 ms |

Two things follow from that table. First, kernel boot is a minority of the
~1175 ms `startup_ms` — most of the rest is Tauri setup, per-module table init,
and the webview. Second, wall-clock startup measurement on a loaded machine has
a noise band of roughly 20x, which is what shapes the test below.

## What is asserted in CI

Three tests in `crates/devos-kernel/src/timing.rs`, run by
`cargo test -p devos-kernel`. This closes the "no performance regression tests"
gap in [testing.md](testing.md) — but read what each one actually promises.

1. **`cold_boot_stays_within_budget`** — the fastest of 3 cold boots must come
   in under **5 s**. That is ~5.4x the slowest best-of-3 ever observed, on a
   machine whose median is ~0.3 s, and all three boots must blow the budget to
   fail it. It is a tripwire for categorical breakage — a blocking network call
   added to the boot path, a migration that starts rewriting user tables,
   per-file scanning at startup, a deadlock that would otherwise hang CI
   forever — and it is deliberately not sensitive enough to catch a 2x, or even
   reliably a 10x, regression. It cannot be: 10x of a quiet-machine boot is
   ~1 s, which is inside the noise band of a loaded one. The threshold's full
   derivation is in the doc comment on the test; if it ever flakes, the
   instruction there is to delete it rather than inflate it.
2. **`migrations_apply_exactly_once_across_boots`** — structural. Boots twice
   against the same file and asserts `_sqlx_migrations` is byte-identical the
   second time, and that the default workspace is not duplicated. This catches
   the regression a fresh-temp-DB timing test structurally cannot see: boot
   work that grows with the age of a user's install.
3. **`module_registration_does_no_per_module_db_work`** — structural, and
   self-calibrating rather than absolute. Registers 64 modules and asserts that
   is at least 8x cheaper than 64 `SELECT 1` round trips measured in the same
   run on the same machine. Measured gap: ~37 µs versus 26–47 ms, i.e.
   700–1300x, so the assertion has about 100x of slack. If someone gives
   `register_module` a DB round trip per module the ratio collapses to ~1x and
   the test fails regardless of how fast or loaded the machine is.

The structural pair are the real regression guards. The timing assertion is a
floor, not a budget — a debug-build boot on a shared CI machine is a weak
signal, and pretending otherwise would just train people to ignore red builds.

## Techniques already in place

- **Route-level code splitting.** TanStack Router's `autoCodeSplitting: true`
  means each feature page (`/terminal`, `/git`, `/ai`, …) is its own chunk,
  loaded on first navigation — confirmed in the Vite build output
  (`terminal-*.js`, `git-*.js`, `ai-*.js` as separate files).
- **One event channel, not polling**, for kernel state changes
  (`devos://event`). Git status is the one deliberate exception — it
  polls every 4s via `refetchInterval`, since watching the filesystem for
  git-relevant changes isn't implemented yet.
- **SQLite WAL mode** for concurrent-safe reads without blocking writers.
- **Release profile tuned**: `lto = true`, `codegen-units = 1`, `strip =
  true` in the workspace `Cargo.toml`.
- **Terminal sessions live in Rust**, not re-created on route change — no
  pty spawn cost on navigation, no lost scrollback.
- **Streaming everywhere output can be large**: pty output and AI tokens
  both flow through Tauri `Channel`s instead of buffering a full response
  before returning.

## Planned, not yet done

- **Release-build startup measurement.** The one measurement that would let the
  headline budget be called met or missed. Nothing else in this list matters as
  much.
- Phase timing for the part of startup *outside* the kernel — Tauri builder,
  per-module `init()` table creation, first paint. `startup_ms` minus
  `BootTimings::total()` is currently an undifferentiated blob, and on the
  numbers above it is the majority of startup.
- Virtualized lists (TanStack Virtual) — not yet needed since no list
  (git history, terminal scrollback is xterm-native) exceeds ~100 rows in
  practice yet. Add when git history or job lists grow unbounded.
- RAM profiling and a documented baseline.
- Apache ECharts — listed in the original stack, not yet pulled in since
  no page charts anything yet; load lazily, per-page, when it is.
