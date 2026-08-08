# Performance

## Budgets

| Metric | Target | Status |
|---|---|---|
| Cold start → kernel ready | < 1000 ms, **release build** | **not met — 2.7–6.3 s over 30 launches**, and 18.1 s on the one launch a day that snapshots the database (2026-08-08, machine not idle). 90–96% of the 2.7–6.3 s is WebView2 creation, now measured. 534 ms on 2026-08-07 and **not reproducible today, on the same binary** |
| `Kernel::boot` (kernel's own share of startup) | no separate budget yet | measured per phase, asserted loosely in CI |
| Base RAM | < 200 MB | not yet profiled |
| Interaction | 60 fps | not yet profiled |

**The startup target is a release-build target,** and a debug number cannot be
compared to it in either direction. The project's early numbers were all from a
`pnpm tauri dev` **debug** build — unoptimized, with a dev-only file watcher
attached: one session logged `startup_ms=1175` warm and `2699` cold. Those are
above 1000 ms and tell us nothing about whether the budget is met, because a
debug build carries no LTO, no `codegen-units = 1`, no strip, and a dev-server
round trip for every asset. Every number below this paragraph is from a release
build, and says which one.

**Measured 2026-08-07: `startup_ms=534`, `boot_ms=20`.** Taken from the NSIS
installer's output running out of `%LOCALAPPDATA%\DevOS` — a real install, not
`cargo run --release` — with all thirteen modules registering and the frontend
served from the embedded `frontendDist` rather than Vite. Phase split:
`pool_open_us=16795`, `migrations_us=1959`, `default_workspace_us=211`.

So against *that* database the budget was met with room: 534 ms against 1000.
Two caveats were recorded at the time. This is one machine, warm — a cold first
launch on a slower disk will be higher, and nobody had measured that. And
`startup_ms` stops at "kernel ready", not at first paint; the webview
initialising and React mounting happen after that number is logged, so what a
user *perceives* is longer.

### The budget is not met, and it is the webview

**Measured 2026-08-08 on the v0.1.0 release install: `startup_ms=2141` and
`2436` across two launches.** Same machine, same installed binary, read off the
dashboard's own "Kernel startup" tile. That is 4x the 534 ms figure and well
over the 1000 ms budget. It was recorded here first, before anything measured
where the time went, alongside three candidates: database size, the updater
plugin, and machine load.

**Later the same day, with per-phase instrumentation, the answer came back:
90–96% of `startup_ms` is tauri creating the window and its WebView2
control, before this app's setup hook runs at all.** The database is not it,
and the updater is not it. What the readings above have in common with the
readings below is the machine, not the data.

#### The controlled experiment

Same instrumented release binary (`cargo build --release`), `DEVOS_DATA_DIR`
pointed at (a) an empty directory and (b) a **copy** of the author's 107 MB
database — never the original — several launches each, plus deliberate
variations in machine load and in how the previous launch was shut down. 31
instrumented launches; the table below holds 30 of them, and the 31st is the
day's first launch against the 107 MB copy, which is a different story and gets
its own [section](#the-daily-backup-13-seconds-once-a-day).

The machine was **not idle** — that is stated first because it is the caveat
that matters. Throughout: ~25–38% CPU across 8 logical cores, 14 `claude.exe`
processes, 6 unrelated `msedgewebview2.exe` processes. Getting to a genuinely
idle desktop was not possible in this session.

| condition | n | `startup_ms` | of which `webview_us` | share |
|---|---|---|---|---|
| empty database | 6 | 5338–6263 ms | 5057–5835 ms | 91–95% |
| 107 MB database (steady state) | 5 | 4930–5853 ms | 4711–5258 ms | 90–96% |
| 107 MB database, 20 s between launches | 8 | 3499–4411 ms | 3268–4211 ms | 92–96% |
| 107 MB database, graceful window close | 6 | 2872–5407 ms | 2700–5129 ms | 93–95% |
| 107 MB database, 8 CPU burners at 100% | 5 | 2666–6126 ms | 2141–5492 ms | 73–91% |

Everything else, across all 31 launches, is small and stable:

| phase | range | what it spans |
|---|---|---|
| `tracing_init_us` | 0.2–2.3 ms | the subscriber |
| `plugins_registered_us` | 0.2–2.0 ms | building the three plugins |
| `context_us` | 1.9–321 ms | `generate_context!` |
| `app_build_us` | 25–306 ms | `App::build`, **including every plugin's `initialize`** |
| `kernel_boot_us` | 62–269 ms | all of `Kernel::boot` |
| `modules_us` | 9–26 ms | 13 × `register_module` |
| `tables_us` | 11–203 ms | seven modules' `CREATE TABLE IF NOT EXISTS` |

#### What each hypothesis turned out to be

**Database size — not the cause of the 2141/2436 ms, but a real cost
elsewhere.** An empty database is if anything *slower* than the 107 MB one
(median 5.7 s vs 5.2 s in the same session); the difference is noise, and it
points the wrong way for the hypothesis. An independent reading the same day
agrees — 5375 ms against a freshly created empty database on the installed
build. So database size does not explain `startup_ms`.

It does, however, own the single largest number measured all day, in a place
nobody was looking — see [The daily backup](#the-daily-backup-13-seconds-once-a-day).

**The updater plugin — cleared.** Constructing the three plugins is
`plugins_registered_us`, 0.2–2.0 ms. *Initialising* them happens inside
`App::build`, and `app_build_us` — which contains opener, dialog and updater
initialisation together — is 25–306 ms. Whatever reqwest/rustls costs at
startup, it is bounded above by that and cannot be seconds. The plugin was
added after the 534 ms measurement, which made it a fair suspect; it is not
the culprit.

**Machine load — correlated, and not sufficient.** At ~25% CPU the webview
phase was 3.3–4.2 s; at ~35% it was ~5 s. But driving the machine to 100% with
eight CPU burners did *not* make it monotonically worse — it went bimodal,
producing both the fastest launch of the day (2666 ms) and one of the slowest
(6126 ms). Load moves this number and does not control it, and no condition
tried came within 3x of the ~400 ms webview phase that a 534 ms total requires.

#### What is still unexplained

**The 534 ms of 2026-08-07 does not reproduce.** The **shipped, uninstrumented
v0.1.0 binary** from `%LOCALAPPDATA%\DevOS`, run four times in the same session
against an empty database, logged `startup_ms` of 5285, 5368, 5052 and 5074 ms.
Same binary, same machine, a database at least as cheap as the small one the
534 ms reading used — and 9x slower.

So this is **not a code regression introduced after 2026-08-07** — the binary
that produced 534 ms produces ~5 s today, with none of the code written since.
Something about the machine, or the WebView2 runtime installed on it, changed
between the two dates, and this file does not know what. Ordinary load is
measured above and does not cover the gap. The disk agrees that something is
different: `pool_open_us` was 16795 µs on 2026-08-07 and is 99000–257000 µs
(0.10–0.26 s) in every launch since, a 6–15x change in a phase that only opens
a file.

The honest position: **`startup_ms` is now fully attributed to a phase
(`webview_us`), but why that phase costs 2–6 s on this machine today, when the
same binary once reached 534 ms end to end, is not attributed.** It needs a
launch on a genuinely idle desktop, and ideally on a second machine — neither
of which this session could produce. Do not repeat the sub-second figure
anywhere user-facing until one of them does.

An older revision of this file reported 854–1461 ms and called the budget "on
track" in debug. Those numbers predate several modules being added to the boot
path; the 1175 / 2699 ms figures above supersede them.

### The daily backup: 13 seconds, once a day

The one place database size does show up, and it is worse than anything the
budget discussion was arguing about.

`Kernel::boot` takes a rotating snapshot of the database at most once per
calendar day (`crate::backup::run_daily_backup`, `VACUUM INTO`). On the first
launch of the day against the 107 MB copy this took **13.02 s**
(`daily_backup_us=13024172`), producing an 80 MB file and pushing that launch's
`startup_ms` to **18074 ms**. The next five launches skipped it in 4–8 ms. The
same phase against an empty database costs 58 ms.

This was invisible before today for a structural reason worth naming: it fires
on exactly one launch per calendar day, so a user hits it once and a developer
measuring the next launch never sees it. It is also **not** what produced the
2141/2436 ms readings — those are far too small — and it is inside `boot_ms`,
so `BootTimings` was always going to show it; nothing was reading `boot_ms` on
the launch that mattered.

Nothing has been changed about it. The snapshot is taken before the app can
write anything, which is the property that makes it worth having, and moving it
off the boot path is a correctness question about restore, not a performance
tweak. **Recommendation, not done here:** run the daily snapshot after the app
is interactive rather than before, or skip it when the database is large enough
that it would dominate startup and take it on a schedule instead. Either is a
change to `crates/devos-kernel/src/backup.rs` and deserves its own review.

## What is measured

### `startup_ms` — the whole-process number, and its phases

Logged once per boot by `src-tauri/src/lib.rs`
(`tracing::info!(startup_ms, …, "devos kernel ready")`) and surfaced in
Settings → About via the `app_info` command. It spans the top of `run()` →
tracing init → tauri builder → window and webview creation → `Kernel::boot` →
module registration → each module's table init, and stops *before* the webview
necessarily finishes its first paint.

It used to be a single scalar — it told you *that* startup regressed, never
*where*, which is what made the 2026-08-08 regression take a day to place. It
now carries a phase breakdown on the same line, in the same `Instant`/`elapsed`
style as `BootTimings`:

| Field | Phase |
|---|---|
| `tracing_init_us` | installing the tracing subscriber |
| `plugins_registered_us` | constructing opener, dialog, updater |
| `context_us` | `generate_context!` — config, embedded assets, icons |
| `app_build_us` | `App::build`: runtime creation and **every plugin's `initialize`** |
| `webview_us` | event-loop start + window + **WebView2 environment and controller** |
| `data_dir_us` | resolving `DEVOS_DATA_DIR` / `app_data_dir` |
| `kernel_boot_us` | all of `Kernel::boot` (see `BootTimings` for its inside) |
| `modules_us` | 13 × `register_module` |
| `tables_us` | seven modules' `CREATE TABLE IF NOT EXISTS`, also logged individually as `module tables initialised` |

`webview_us` is the one that needs explaining, and it is the one that turned
out to matter. Tauri creates the windows declared in `tauri.conf.json` — and on
Windows, with each one, a WebView2 environment and controller — inside its
*own* `setup`, immediately **before** it calls the closure passed to
`Builder::setup`, and that whole thing runs from the event loop's `Ready`
event inside `App::run`. So it is synchronous, it is blocking, it happens
before a single line of DevOS's own startup code, and it is charged to
`startup_ms` in full. Measuring it required splitting `Builder::run` into
`build()` + `run()`, which is exactly what `Builder::run` does anyway, so that
the setup closure can see when `build()` finished.

Two things are deliberately **outside** every one of these numbers. Process
creation, image and DLL loading, and CRT startup all happen before the first
line of `run()`, so they are missing from `startup_ms` itself rather than
unattributed within it. And first paint happens after it — see
[The webview bundle](#the-webview-bundle).

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

The `BootTimings` *fields* still deliberately do not sum to `boot`. The
remainder — everything else `Kernel::boot` does, which is the backup and
retention work — is now named on the same log line rather than left as a gap:
`restore_us` (applying a staged restore), `pre_migration_backup_us`,
`daily_backup_us` and `audit_prune_us`. Those four are logged rather than
stored in `BootTimings` because they are diagnostic: each is normally under a
few milliseconds, and each has one launch on which it is not. `daily_backup_us`
is the one that has already been caught doing it — 13.02 s, see
[The daily backup](#the-daily-backup-13-seconds-once-a-day).

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

The 2026-08-08 release measurements sharpen the first point rather than
changing it: `kernel_boot_us` was 62–269 ms across 31 launches against both an
empty and a 107 MB database, against a `startup_ms` of 2.7–6.3 s. Kernel boot
is 2–6% of startup. The exception is the one launch per calendar day that
writes the daily snapshot.

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

- ~~Release-build startup measurement.~~ **Done**, twice — 534 ms on
  2026-08-07, 2.7–6.3 s on 2026-08-08, see Budgets above. What remains is the
  measurement that would reconcile them: **a launch on a genuinely idle
  desktop, and a launch on a second machine.** Neither has been taken. Until
  one is, why WebView2 creation costs 2–6 s here is open.
- ~~Phase timing for the part of startup *outside* the kernel.~~ **Done** —
  `startup_ms` now carries `tracing_init_us`, `plugins_registered_us`,
  `context_us`, `app_build_us`, `webview_us`, `data_dir_us`, `kernel_boot_us`,
  `modules_us` and `tables_us`, and `Kernel::boot` names its backup work. First
  paint is still not timed — that item is still open, further down this list.
- **Why `webview_us` is seconds.** It is now measured and it is 90–96% of
  startup, but it is a single number covering event-loop start, window
  creation, and the WebView2 environment and controller. Splitting *those*
  needs hooks into wry rather than into this app, so it is a real piece of work
  and not a follow-up edit. Worth doing before any further startup effort:
  everything else in `startup_ms` put together came to 0.2–0.8 s per launch.
- **Move the daily database snapshot off the boot path.** 13.02 s on the
  author's 107 MB database, once per calendar day — see
  [The daily backup](#the-daily-backup-13-seconds-once-a-day). Deliberately not
  done as part of the measurement work: it is a correctness question about
  restore, not a tweak.
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
