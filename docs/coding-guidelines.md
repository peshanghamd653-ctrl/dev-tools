# Coding Guidelines

These are the conventions the codebase already follows — not aspirations.
When adding code, match the existing pattern in the nearest file before
inventing a new one.

## Rust

- **Repository pattern, no exceptions.** All SQL lives in `repo.rs` (or
  `ops.rs` for CLI-backed modules like `devos-git`). Commands and modules
  call functions, never `sqlx::query!` inline in a Tauri command.
- **IPC commands are thin.** A `#[tauri::command]` function validates input,
  calls exactly one domain function, maps the error, optionally emits an
  event. No branching business logic in `src-tauri/src/*_commands.rs`.
- **Errors are typed internally, stringly-typed at the IPC boundary.**
  Domain code returns `thiserror`-derived enums (`KernelError`, `GitError`,
  `AiError`, `SecretError`, `TermError`). Tauri commands `.map_err(|e|
  e.to_string())` — the frontend gets a human-readable message, not a code
  to branch on (see [architecture decision on this trade-off](adr/) if one
  is added later; today it's a deliberate simplicity choice, revisited if
  plugins need structured errors).
- **Every domain module gets `#[cfg(test)] mod tests` in the same file**,
  using `tempfile::tempdir()` for anything touching the filesystem or a
  SQLite file. No mocking the database — tests run against a real (temp)
  SQLite file or a real subprocess (git, ConPTY).
- **The `Module` trait is the extension point.** New capabilities implement
  `devos_kernel::module::Module` and get registered in `src-tauri/src/lib.rs`
  `setup()`. They never import another module's crate for anything but its
  public `Module`/type exports.
- **Comments explain *why*, not *what*.** A comment justifying a non-obvious
  constraint (e.g. why the terminal test answers a ConPTY cursor probe) is
  expected. A comment restating the function name is not.

## TypeScript / React

- **One folder per feature under `src/features/`.** Each has a `hooks.ts`
  (TanStack Query hooks) and page component(s). Features never import from
  each other directly — go through `shared/` or kernel events. (The one
  sanctioned exception so far: `GitPage` reads `useAiStore` to know which
  provider/model to request a commit message from — a UI-level convenience,
  not a data dependency.)
- **`shared/ipc/client.ts` is the only place `invoke`/`listen`/`Channel` are
  called.** Every new backend command gets one function in the `ipc` object
  here, typed against the generated binding.
- **Server state via TanStack Query, client state via Zustand.** If it's
  fetched from the backend, it's a query/mutation. If it's UI-only (palette
  open, active tab, a form draft), it's a store.
- **Zustand stores that should survive a restart use `persist`.** Stores
  that shouldn't (dialog open state) don't.
- **No manual `useMemo`/`useCallback` for the sake of it.** Only reach for
  them when profiling shows a real cost (e.g. the terminal's `ResizeObserver`
  callback, which genuinely needs a stable reference).
- **Every route is a separate file under `src/app/routes/`,** picked up by
  TanStack Router's file-based codegen (`routeTree.gen.ts` — generated,
  never hand-edited).

## Cross-cutting

- **Redaction at the type level, not by convention.** The secret store's
  `list()` returns `SecretMeta { name, updated_at }` — there is no code path
  that can accidentally leak a value through the listing endpoint, because
  the struct has no value field. Prefer this pattern over "just don't log
  it" wherever secrets or credentials are involved.
- **Every module ships tests with the code that introduces it**, not after.
  A PR adding a new IPC command without a domain-layer test for the logic
  behind it is incomplete.
- **Run the full gate before calling anything done:** `cargo fmt --all &&
  cargo clippy --workspace --all-targets -- -D warnings && cargo test
  --workspace`, then `pnpm typecheck && pnpm lint && pnpm test && pnpm exec
  vite build`. See [testing.md](testing.md) for what each layer covers.
