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

## The webview bundle

`startup_ms` stops before the webview finishes its first paint, so none of the
numbers above include the cost of parsing the app's JavaScript. That parse
happens on every launch and is charged against the same sub-1000 ms budget.

### What was in the entry chunk

Measured with a Rollup `generateBundle` hook reading `renderedLength` per
module — not estimated. The entry chunk was **649.71 kB minified / 204.76 kB
gzipped**, and it had tripped Rollup's default 500 kB warning since the shell
was built. Top contributors (pre-minification, which is what `renderedLength`
reports; the whole chunk is 1673 kB by that measure, so scale by ~0.38 for
minified bytes):

| Module | Pre-min | Deferrable? |
|---|---|---|
| `react-dom` | 548.3 kB | no — the framework |
| `zod` | 145.0 kB | **yes** — only the two form dialogs |
| `@tanstack/router-core` | 129.0 kB | no — the router |
| app code | 104.6 kB | partly — 52 kB of it was overlays |
| `tailwind-merge` | 99.8 kB | no — behind every `cn()` |
| `react-hook-form` | 87.8 kB | **yes** — only the two form dialogs |
| `@tanstack/query-core` | 73.7 kB | no — the data layer |
| `sonner` | 62.8 kB | no — see below |
| `@radix-ui/*` + `@floating-ui/*` | ~148 kB | no — Topbar, Sidebar, tooltips |
| `lucide-react` | 23.6 kB | already tree-shaken (icons emit ~0.2 kB chunks) |

Two suspects were cleared by the measurement rather than by assumption. **xterm
was already lazy** — `CommandPalette` dynamically imports
`@/features/terminal/session`, which is why `session-*.js` (334 kB) is its own
chunk. **`react-markdown` + `remark-gfm` were already lazy** inside the route
chunk `ai-*.js` (174 kB). **`lucide-react` was already tree-shaking correctly.**
None of those three were ever in the entry chunk.

### What was done

The command palette and the three global dialogs (`CreateWorkspaceDialog`,
`AddProjectDialog`, `CaptureIssueDialog`) are all closed on boot and render
nothing until opened — but importing them statically dragged `zod`,
`react-hook-form`, `@hookform/resolvers`, `cmdk` and the screenshot annotator
into the entry chunk. They now sit behind `lazy()` boundaries in
`AppShell.tsx`.

The mounting rule matters as much as the boundary. They mount **one idle tick
after first paint** (`requestIdleCallback`, with a 200 ms `setTimeout` fallback
for the older WebKitGTK that Tauri ships against), in their closed state — so
from that moment the app behaves exactly as it did with static imports,
including open animations and retained form state. Until idle fires, the two
store selectors that watch the open flags stay live, so a `Ctrl+K` in the first
few hundred milliseconds still opens the palette instead of being swallowed.
Once idle has fired, those selectors are pinned to a constant `false`, so
toggling an overlay never re-renders the shell. The `Suspense` fallback is
`null`, which is honest here: a closed overlay renders nothing anyway, so there
is no pane to leave blank and no layout to shift.

| | before | after | change |
|---|---|---|---|
| entry chunk | 649.71 kB | **506.41 kB** | −143.3 kB (−22%) |
| entry chunk, gzipped | 204.76 kB | **160.85 kB** | −43.9 kB (−21%) |
| total JS emitted | ~1274 kB | ~1288 kB | +14 kB |

Total bytes went up slightly — that is the point. The same code is still
shipped; 143 kB of it simply no longer blocks first paint.

### Why the remainder cannot be split further

What is left was checked module by module and is all required to paint the
shell: `react-dom` alone is ~207 kB minified (41% of the chunk), plus TanStack
Router and Query, `tailwind-merge` behind every `cn()`, and Radix
tooltip/menu + floating-ui behind `Topbar` and `Sidebar`. `sonner` looks
deferrable but is not — `useKernelEvents` imports `toast` from it, so the
module is eager no matter where `<Toaster/>` is mounted, and it is one module
containing both. Radix's dropdown menu is likewise reachable from `Topbar` and
`NotificationBell`, both eager.

`manualChunks` was considered and deliberately **not** used. It would push the
entry chunk under 500 kB, but every piece is statically imported by the entry,
so Vite emits `modulepreload` for all of them and the webview parses the same
bytes before the same first paint. It would move the number without moving the
work — and an unmeasured claim of a win is worth nothing. `build.chunkSizeWarningLimit`
is instead set to **560 kB** in `vite.config.ts`: today's 506 kB plus ~54 kB of
headroom, tight enough that another eager dependency the size of `zod` trips
it. If it fires, find what joined the boot path and give it a lazy boundary;
raise the number only alongside a measurement showing the new weight is
genuinely needed for first paint.

The honest caveat: all of the above is a bytes-and-modules measurement. The
actual first-paint improvement in the Tauri webview has **not** been measured,
for the same reason the headline budget has not been — nobody has instrumented
a release build. 143 kB less JavaScript to parse cannot be slower, but how much
faster is unknown.

## Techniques already in place

- **Route-level code splitting.** TanStack Router's `autoCodeSplitting: true`
  means each feature page (`/terminal`, `/git`, `/ai`, …) is its own chunk,
  loaded on first navigation — confirmed in the Vite build output
  (`terminal-*.js`, `git-*.js`, `ai-*.js` as separate files).
- **Overlay-level code splitting.** The command palette and the three global
  dialogs load one idle tick after first paint rather than with the shell —
  see [The webview bundle](#the-webview-bundle).
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
- **First-paint measurement for the webview.** The entry chunk shrank 22% (see
  [The webview bundle](#the-webview-bundle)) but the payoff was never observed
  in the running app, only in the build output. Until first paint is timed,
  further bundle work is guesswork — the next 100 kB is much harder to remove
  than the last 143 kB, and worth removing only if parse time proves to matter.
- Virtualized lists (TanStack Virtual) — not yet needed since no list
  (git history, terminal scrollback is xterm-native) exceeds ~100 rows in
  practice yet. Add when git history or job lists grow unbounded.
- RAM profiling and a documented baseline.
- Apache ECharts — listed in the original stack, not yet pulled in since
  no page charts anything yet; load lazily, per-page, when it is.
