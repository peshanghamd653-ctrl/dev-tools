# Quality & Operations

## Testing strategy

| Layer | Tool | What | Status |
|---|---|---|---|
| Kernel unit/integration | `cargo test` (tempfile SQLite) | repo CRUD, invariants, event bus, job lifecycle | ✅ M0 |
| Type safety across IPC | ts-rs generated bindings + `tsc --noEmit` | drift between Rust DTOs and TS is a compile error | ✅ M0 |
| Frontend unit | Vitest (jsdom) | stores, utils | ✅ M0 |
| Frontend component | Vitest + Testing Library | palette, dialogs, pages | M1 |
| End-to-end | WebDriver (`tauri-driver`) smoke: boot → add project → palette nav | M1 |
| Lint gates | `cargo fmt --check`, `clippy -D warnings`, ESLint strict | enforced in CI | ✅ M0 |

Rule: every module lands with kernel tests for its repository layer and at
least one component test for its primary screen.

## Performance strategy

- Budgets: < 1 s cold start (kernel reports `startup_ms`, shown in Settings),
  < 200 MB base RAM, 60 fps interactions.
- Techniques in place: route-level code splitting (`autoCodeSplitting`),
  single event channel instead of polling, SQLite WAL, release profile with
  LTO + strip. Planned: virtualized lists (TanStack Virtual) the moment any
  list can exceed ~100 rows (git history, logs); ECharts lazy-loaded only on
  pages that chart.

## Deployment strategy

- Local: `pnpm tauri dev` (dev) / `pnpm tauri build` (NSIS installer on
  Windows; dmg/AppImage when cross-platform builds are enabled).
- CI (GitHub Actions): lint + typecheck + tests on every push; release
  workflow (tag → `tauri build` matrix → signed artifacts) added when the
  first release ships. Updates via `tauri-plugin-updater` + signing keys —
  scheduled with the first public build (M1 end).

## Risk assessment

| Risk | Impact | Mitigation |
|---|---|---|
| Scope explosion (the brief is a multi-year product) | stalled project | vertical milestones; disabled-module sidebar keeps ambition visible without fake UI |
| Windows-first drift breaking cross-platform | rework later | kernel is OS-neutral; OS-specific code isolated (keyring, pty); CI adds macOS/Linux targets at M1 |
| WebView2/Tauri constraint on embedded browser | feature gap vs. brief | scoped down deliberately (see architecture doc §deviations) |
| SQLx/ts-rs version drift | build breakage | workspace-pinned versions; generated bindings committed |
| AI provider API changes | broken chat | thin adapters behind one trait; contract tests per provider (M1) |

## Suggested improvements beyond the brief

1. **Structured IPC error codes** once plugins exist (today: display strings).
2. **Session restore** — reopen tabs, terminals, and scroll positions on boot.
3. **Global quick-switcher** (Ctrl+P) across projects/files/commands, fed by
   the M2 index.
4. **Workspace-scoped settings overrides** (kernel already keys settings; add
   scoping convention).
5. **Telemetry: local-only by default**, an opt-in diagnostics export instead
   of phone-home analytics.
6. **Feature flags table** for staged rollout of experimental modules.
