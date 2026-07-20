# DevOS Architecture

DevOS is a desktop developer operating center: one application that replaces
the daily rotation of terminal, git client, Docker Desktop, API client,
database browser, AI chat, and deployment dashboards.

## System overview

```
┌──────────────────────────── DevOS (Tauri v2) ────────────────────────────┐
│                                                                          │
│  WebView (React 19)                    Rust host process                 │
│  ┌────────────────────────┐            ┌───────────────────────────────┐ │
│  │ app shell              │  typed IPC │ devos-desktop (Tauri shell)   │ │
│  │  sidebar · tabs        │◄──────────►│  IPC commands · event bridge  │ │
│  │  command palette       │  commands  │                               │ │
│  │ features/* (lazy)      │  + events  │ devos-kernel                  │ │
│  │  dashboard · projects  │            │  module registry              │ │
│  │  settings · (M1: git,  │            │  command registry             │ │
│  │  terminal, ai)         │            │  event bus (broadcast)        │ │
│  │ shared/*               │            │  job runner (durable)         │ │
│  │  ipc client · ui kit   │            │  SQLx pool + migrations       │ │
│  │  stores · hooks        │            │                               │ │
│  └────────────────────────┘            │ crates/modules/* (M1+)        │ │
│                                        │  git · terminal · ai · docker │ │
│                                        └──────────────┬────────────────┘ │
│                                                       │                  │
│                                                SQLite (WAL)              │
└──────────────────────────────────────────────────────────────────────────┘
```

## Layering (clean architecture, pragmatic)

| Layer | Location | Rule |
|---|---|---|
| Domain + persistence | `devos-kernel` (`repo`, `types`) | No Tauri, no UI concepts. Pure Rust, fully testable. |
| Application services | `devos-kernel` (`kernel`, `jobs`, `events`, `commands`) | Orchestration; owns lifecycle. |
| Interface adapters | `src-tauri` (`commands.rs`) | Thin: validate → call repo → emit event. No business logic. |
| Presentation | `src/` React | Talks only to `shared/ipc/client.ts`; server state via TanStack Query. |

## Module contract

Every capability (core or future plugin) implements `devos_kernel::module::Module`:

```rust
pub trait Module: Send + Sync {
    fn id(&self) -> &'static str;
    fn register(&self, ctx: &ModuleCtx<'_>); // contribute commands, subscribe to events
}
```

Rules that keep the system modular:

- **Modules never import each other.** Cross-module effects go through the
  event bus (`EventBus`, a `tokio::sync::broadcast` channel).
- **Every module owns its migrations and tables** (prefix convention:
  `git_*`, `ai_*`, …). The kernel runs them.
- **Heavy work is a job** (`JobRunner::submit`): recorded in SQLite, status
  transitions broadcast as `KernelEvent::JobUpdated`. Nothing blocks boot or
  IPC threads.
- **The frontend mirrors the boundary**: one folder per feature under
  `src/features/`, no cross-feature imports; shared code lives in `src/shared/`.

## Event & command flow

- **Commands (webview → kernel):** Tauri `invoke` with types generated from
  Rust via ts-rs (`pnpm gen:types`). Wrapped once in `src/shared/ipc/client.ts`.
- **Events (kernel → webview):** every `KernelEvent` is forwarded on a single
  Tauri channel `devos://event`; `useKernelEventBridge` translates them into
  TanStack Query invalidations and toasts.
- **Command palette:** the frontend merges its navigation commands with
  backend-contributed `CommandDescriptor`s from the kernel's `CommandRegistry`.

## Deliberate deviations from the original brief (tradeoffs)

1. **No in-process message queue / full CQRS.** A typed command layer + broadcast
   event bus + SQLite-backed jobs deliver the same decoupling with far less
   machinery. CQRS is reserved for genuinely divergent read/write models
   (code indexing in M2).
2. **Git via the `git` CLI**, not libgit2: credential helpers, hooks, and LFS
   behave exactly as the user's git does. `gitoxide` may later accelerate
   hot paths (status/diff) behind the same module interface.
3. **Embedded browser with full DevTools is deferred.** Tauri uses WebView2;
   a preview webview is feasible, Chrome-grade DevTools inside the app is not.
4. **Plugin UI is contribution-based, not arbitrary JS.** Sandboxed logic will
   run as WASM (M5); UI integration happens through declarative contributions
   (commands, panels), because in-process JS cannot be sandboxed safely.
5. **ts-rs over specta**: simpler, generates plain `.ts` type files as part of
   `cargo test`, no runtime coupling.

## Performance budgets (enforced, not aspirational)

- Cold start to interactive shell: **< 1 s** (kernel logs `startup_ms`;
  visible in Settings → About).
- Base RAM: < 200 MB.
- Every route is lazy (`autoCodeSplitting`); module code loads on first visit.
- All kernel work is async (Tokio); the UI thread never blocks on IPC.
