# Architecture

## System overview

```
┌──────────────────────────── DevOS (Tauri v2) ────────────────────────────┐
│                                                                          │
│  WebView (React 19)                    Rust host process                 │
│  ┌────────────────────────┐            ┌───────────────────────────────┐ │
│  │ app shell               │  typed IPC │ devos-desktop (Tauri shell)   │ │
│  │  sidebar · palette      │◄──────────►│  IPC commands · event bridge  │ │
│  │  workspace switcher     │  commands  │  tool executor (path-guarded) │ │
│  │ features/* (lazy)       │  + events  │                               │ │
│  │  dashboard · projects   │            │ devos-kernel                  │ │
│  │  terminal · git · ai    │            │  module registry              │ │
│  │  settings               │            │  command registry              │ │
│  │ shared/*                │            │  event bus (tokio broadcast)  │ │
│  │  ipc client · ui kit    │            │  job runner (durable)         │ │
│  │  stores · hooks         │            │  SQLx pool + migrations       │ │
│  └────────────────────────┘            │                               │ │
│                                        │ devos-secrets                 │ │
│                                        │  keyring master key + AES-GCM │ │
│                                        │                               │ │
│                                        │ devos-ai                      │ │
│                                        │  provider trait · agent loop  │ │
│                                        │                               │ │
│                                        │ modules/devos-terminal        │ │
│                                        │  portable-pty session manager │ │
│                                        │                               │ │
│                                        │ modules/devos-git             │ │
│                                        │  git CLI wrapper + parsers    │ │
│                                        └──────────────┬────────────────┘ │
│                                                       │                  │
│                                                SQLite (WAL)              │
└──────────────────────────────────────────────────────────────────────────┘
```

## Layering (clean architecture, pragmatic)

| Layer | Location | Rule |
|---|---|---|
| Domain + persistence | `devos-kernel`, `devos-ai`, `devos-git`, `devos-secrets` (`repo`/`ops`/`types`) | No Tauri, no UI concepts. Pure Rust, fully unit-testable. |
| Application services | `devos-kernel` (`kernel`, `jobs`, `events`, `commands`), `devos-ai` (agent loop) | Orchestration; owns lifecycle. |
| Interface adapters | `src-tauri/src/*_commands.rs` | Thin: validate → call domain → emit event. No business logic. |
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
  `git_*`, `ai_*`, `term_*`). `devos-ai` demonstrates this today —
  `ai::repo::init()` creates `ai_conversations`/`ai_messages` idempotently
  at boot, independent of the kernel's own migrations.
- **Heavy work is a job** (`JobRunner::submit`): recorded in SQLite, status
  transitions broadcast as `KernelEvent::JobUpdated`. Nothing blocks boot or
  IPC threads.
- **The frontend mirrors the boundary**: one folder per feature under
  `src/features/`, no cross-feature imports; shared code lives in `src/shared/`.

Live modules today: `core`, `terminal`, `git`, `ai`.

## Event & command flow

- **Commands (webview → kernel):** Tauri `invoke` with types generated from
  Rust via ts-rs (`pnpm gen:types`). Wrapped once in `src/shared/ipc/client.ts`.
  Full command/event catalog: [ipc-contracts.md](ipc-contracts.md). See
  [database.md](database.md) for the schema these commands operate on.
- **Events (kernel → webview):** every `KernelEvent` is forwarded on a single
  Tauri channel `devos://event`; `useKernelEventBridge` translates them into
  TanStack Query invalidations and toasts.
- **Streaming (terminal, AI):** long-lived or high-volume output does not
  return from the command. A Tauri `Channel` is passed into the command; the
  backend streams frames (`TermEvent`, `AiDelta`) directly to that channel.
  This is how the terminal's pty output and the AI assistant's token stream
  both work, and it keeps every such operation cancellable.
- **Command palette:** the frontend merges its navigation commands with
  backend-contributed `CommandDescriptor`s from the kernel's `CommandRegistry`.

## Deliberate deviations from the original brief

1. **No in-process message queue / full CQRS.** A typed command layer + broadcast
   event bus + SQLite-backed jobs deliver the same decoupling with far less
   machinery.
2. **Git via the `git` CLI**, not libgit2 — see [ADR-0001](adr/0001-shell-out-to-git-cli.md).
3. **Embedded browser with full DevTools is deferred.** Tauri uses WebView2;
   Chrome-grade DevTools inside the app is not realistic short-term.
4. **Plugin UI is contribution-based, not arbitrary JS** — see [ADR-0003](adr/0003-contribution-based-plugin-model.md).
5. **ts-rs over specta** — see [ADR-0002](adr/0002-ts-rs-over-specta-for-ipc-types.md).
6. **Terminal sessions live in the Rust process, not the webview** — see [ADR-0006](adr/0006-terminal-sessions-live-in-rust.md).

## Repository layout

See [tech-stack.md](tech-stack.md) for what each part is built with. Folder
map:

```
dev tools/
├─ crates/
│  ├─ devos-kernel/        # module registry, command/event bus, jobs, SQLx pool
│  ├─ devos-secrets/       # keyring master key + AES-256-GCM secret store
│  ├─ devos-ai/            # provider trait, Claude/Ollama adapters, agent loop
│  └─ modules/
│     ├─ devos-terminal/   # portable-pty session manager
│     └─ devos-git/        # git CLI wrapper + porcelain-v2 parser
├─ src-tauri/              # Tauri app: IPC commands, tool executor, window
├─ src/
│  ├─ app/                 # shell: routes, layout, palette, nav
│  ├─ features/<module>/   # hooks.ts + page components, one folder per feature
│  └─ shared/              # ipc client, ui kit (shadcn), stores, hooks
└─ docs/                   # this knowledge base
```
