# IPC Design & API Contracts

## Principles

- **One boundary file.** The webview calls the kernel only through
  `src/shared/ipc/client.ts`. Features import `ipc`, never `invoke`.
- **Generated types.** DTOs live in `crates/devos-kernel/src/types.rs`
  (serde `camelCase`) and are exported to `src/shared/ipc/bindings/` by ts-rs
  (`pnpm gen:types` = `cargo test -p devos-kernel export_bindings`).
  A hand-written payload shape in the frontend is a code-review reject.
- **Errors** cross IPC as strings (`Result<T, String>`), produced from
  `KernelError`'s `Display`. The UI surfaces them via toasts or inline form
  errors; they are messages for humans, not codes to branch on. (Structured
  error codes become necessary with plugins — see improvements list.)
- **Events, not polling.** Mutating commands emit `KernelEvent`; the desktop
  shell forwards every event on the single channel `devos://event`;
  `useKernelEventBridge` maps events → TanStack Query invalidations.

## Command catalog (M0)

| Command | Args | Returns | Emits |
|---|---|---|---|
| `app_info` | — | `AppInfo` | — |
| `workspaces_list` | — | `Workspace[]` | — |
| `workspace_create` | `name` | `Workspace` | `workspacesChanged` |
| `workspace_rename` | `id, name` | `Workspace` | `workspacesChanged` |
| `workspace_delete` | `id` | `()` (refuses last) | `workspacesChanged` |
| `projects_list` | `workspaceId` | `Project[]` | — |
| `project_add` | `workspaceId, name, path` | `Project` (validates dir exists) | `projectsChanged` |
| `project_remove` | `id, workspaceId` | `()` | `projectsChanged` |
| `settings_get` / `settings_set` | `key` / `key, value` | `string \| null` / `()` | `settingsChanged` |
| `commands_list` | — | `CommandDescriptor[]` | — |
| `jobs_recent` | — | `JobInfo[]` (last 50) | — |

## Event catalog

`KernelEvent` is a tagged union `{ kind, data? }`:
`workspacesChanged` · `projectsChanged{workspaceId}` · `settingsChanged{key}`
· `jobUpdated{job}` · `notificationAdded{level,title,body}`.

## Streaming (contract for M1, used by terminal/AI/git)

Long-running or streaming operations do not return their payload from the
command. Pattern:

1. `xyz_start(args) -> jobId` — kernel spawns the work via `JobRunner`.
2. Data streams as events on a scoped channel (`devos://term/<sessionId>`,
   `devos://ai/<conversationId>`), using Tauri channels for high-volume data.
3. `jobUpdated` carries terminal state; `xyz_cancel(jobId)` aborts.

This keeps every operation cancellable and the UI responsive regardless of
payload size.
