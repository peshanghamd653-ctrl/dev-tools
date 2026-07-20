# Performance

## Budgets

| Metric | Target | Measured (debug build, this session) |
|---|---|---|
| Cold start → kernel ready | < 1000 ms | 854–1461 ms across runs (see note below) |
| Base RAM | < 200 MB | not yet profiled |
| Interaction | 60 fps | not yet profiled |

The kernel logs `startup_ms` on every boot (`tracing::info!(startup_ms, ...
"devos kernel ready")`), visible in Settings → About via the `app_info`
command. This is measured from process start to the kernel + all modules
being registered — before the webview necessarily finishes its own paint.

**Important caveat**: all measurements to date are from `pnpm tauri dev`
**debug builds**, which include unoptimized compilation and a dev-only file
watcher. The four consecutive runs in this session (994, 873, 854, 877 ms)
are consistent and under budget even in debug — but a release build
(`cargo build --release`, LTO + strip, per `Cargo.toml`'s `[profile.release]`)
has not yet been measured and should be, before this budget is declared
"met" rather than "on track."

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

- Virtualized lists (TanStack Virtual) — not yet needed since no list
  (git history, terminal scrollback is xterm-native) exceeds ~100 rows in
  practice yet. Add when git history or job lists grow unbounded.
- RAM profiling and a documented baseline.
- Release-build startup measurement (see caveat above).
- Apache ECharts — listed in the original stack, not yet pulled in since
  no page charts anything yet; load lazily, per-page, when it is.
