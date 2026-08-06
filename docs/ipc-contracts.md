# IPC Contracts

## Principles

- **One boundary file.** The webview calls the backend only through
  `src/shared/ipc/client.ts`. Features import `ipc`, never `invoke`
  directly.
- **Generated types.** DTOs are defined once in Rust (`devos-kernel`,
  `devos-ai`, `devos-git`, `devos-terminal` — each crate's `types.rs`, serde
  `camelCase`) and exported to `src/shared/ipc/bindings/` by ts-rs
  (`pnpm gen:types` = `cargo test --workspace export_bindings`). A
  hand-written payload shape in the frontend is a code-review reject.
- **Errors cross IPC as strings** (`Result<T, String>`), produced from each
  domain error type's `Display`. The UI surfaces them via toasts or inline
  form errors — they're messages for humans, not codes to branch on.
  Structured error codes become necessary once plugins exist (see
  [plugin-api.md](plugin-api.md)); not needed yet.
- **Events, not polling**, for kernel state changes. Mutating commands emit
  a `KernelEvent`; the desktop shell forwards every event on one channel,
  `devos://event`; `useKernelEventBridge` maps events → TanStack Query
  invalidations and toasts. (Git status is the one deliberate polling
  exception — see [performance.md](performance.md).)

## Streaming (implemented pattern: terminal, AI)

High-volume or long-lived output does **not** buffer and return from the
command. A Tauri `Channel<T>` is created on the frontend and passed as a
command argument; the backend `.send()`s frames to it directly as they
arrive, and the command's `Result` return value is reserved for the final
outcome once streaming completes.

```ts
// frontend
const channel = new Channel<TermEvent>();
channel.onmessage = (event) => { /* handle each frame */ };
const info = await ipc.termCreate({ cols, rows }, channel);
```

```rust
// backend
#[tauri::command]
async fn term_create(/* … */, on_output: Channel<TermEvent>) -> Result<TermSessionInfo, String> {
    // spawn a task that forwards manager events onto `on_output`
}
```

Used by:
- **Terminal** (`term_create` → `TermEvent::Output`/`Exit` frames)
- **AI chat** (`ai_send` → `AiDelta::Text`/`ToolCall`/`ToolResult`/`Done`/`Error` frames)

## Command catalog

### Core (M0)

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
| `notifications_list` | — | `NotificationDto[]` (last 50) | — |
| `notifications_unread_count` | — | `number` | — |
| `notification_mark_read` | `id` | `()` | — |
| `notifications_mark_all_read` | — | `()` | — |

### Terminal (M1)

| Command | Args | Returns |
|---|---|---|
| `term_create` | `shell?, cwd?, cols, rows, onOutput: Channel<TermEvent>` | `TermSessionInfo` |
| `term_write` | `id, data` | `()` |
| `term_resize` | `id, cols, rows` | `()` |
| `term_kill` | `id` | `()` |
| `term_list` | — | `TermSessionInfo[]` |
| `term_tail` | `id` | `string` (recent output, ANSI-stripped) |

### Git (M1)

| Command | Args | Returns |
|---|---|---|
| `git_status` | `path` | `GitStatus { info, entries }` |
| `git_stage` / `git_unstage` | `path, files[]` | `()` |
| `git_discard` | `path, file, untracked` | `()` |
| `git_commit` | `path, message` | `string` (commit id) |
| `git_log` | `path, limit` | `GitCommit[]` |
| `git_branches` | `path` | `GitBranch[]` |
| `git_switch` | `path, branch, create` | `()` |
| `git_diff` | `path, file, staged, untracked` | `string` (unified diff) |
| `git_push` / `git_pull` | `path` | `string` (CLI output) |

### AI + secrets (M1–M2)

| Command | Args | Returns | Notes |
|---|---|---|---|
| `secret_set` | `name, value` | `()` | value never returned by any other command |
| `secret_list` | — | `string[]` | names only |
| `secret_delete` | `name` | `()` | |
| `ai_conversations_list` | — | `Conversation[]` | |
| `ai_conversation_create` | `provider, model` | `Conversation` | |
| `ai_conversation_delete` | `id` | `()` | |
| `ai_messages` | `conversationId` | `ChatMessage[]` | |
| `ai_ollama_models` | — | `string[]` | queries local Ollama `/api/tags` |
| `ai_send` | `conversationId, content, projectPath?, toolsEnabled?, writeToolsEnabled?, onDelta: Channel<AiDelta>` | `ChatMessage` | runs the tool-calling agent loop when `toolsEnabled && projectPath && provider == "claude"`; `writeToolsEnabled` adds the mutating tool defs |
| `ai_tool_respond` | `id, approved` | `bool` (false if the id was unknown/expired) | resolves a pending per-call approval |
| `ai_memory_list` | `projectPath` | `MemoryEntry[]` | project key normalized backend-side |
| `ai_memory_add` | `projectPath, content` | `MemoryEntry` | caps: 500 chars, 100 entries/project |
| `ai_memory_delete` | `id` | `()` | |
| `ai_commit_message` | `path, provider, model` | `string` | one-shot, no streaming; reads the staged diff |

### Index (M2)

| Command | Args | Returns | Notes |
|---|---|---|---|
| `index_project` | `path` | `string` (job id) | runs as a kernel job; completion arrives via `jobUpdated` |
| `index_stats` | `path` | `IndexStats` | files/chunks/lastIndexed for the project |
| `index_search` | `path, query` | `IndexHit[]` (≤30) | FTS content search for the file explorer |

### Files (M1.5)

| Command | Args | Returns | Notes |
|---|---|---|---|
| `fs_list_dir` | `projectPath, relative` | `FsEntry[]` (dirs first, ≤1000) | path-contained via pathsafe |
| `fs_read_file` | `projectPath, relative` | `FilePreview` | 512 KB cap, binary sniffing |
| `fs_find` | `projectPath, query` | `string[]` (≤200 relative paths) | same walk rules as the indexer |

### Docker (M3)

| Command | Args | Returns | Notes |
|---|---|---|---|
| `docker_ping` | — | `string` (version) | error prefixed `unavailable:` when the daemon is down |
| `docker_containers` | — | `DockerContainer[]` (all, incl. stopped) | |
| `docker_images` | — | `DockerImage[]` | dangling tags filtered |
| `docker_start` / `docker_stop` / `docker_restart` | `id` | `()` | 10s stop timeout |
| `docker_logs` | `id` | `string` | last 200 lines, 256 KB cap |

### API client (M3)

| Command | Args | Returns | Notes |
|---|---|---|---|
| `api_send` | `spec: ApiRequestSpec` | `ApiResponse` | 30s timeout, 1 MB body cap; records history |
| `api_save` | `name, collection, spec` | `SavedRequest` | blank collection → "Default" |
| `api_requests` | — | `SavedRequest[]` | ordered by collection, name |
| `api_request_delete` | `id` | `()` | |
| `api_history` | — | `ApiHistoryEntry[]` (last 50) | table pruned to 100 |

### Database (M3)

SQLite only — see [ADR-0007](adr/0007-sqlite-only-database-manager-first.md).

| Command | Args | Returns | Notes |
|---|---|---|---|
| `db_connections` | — | `DbConnection[]` | `driver` is `sqlite` on every row today |
| `db_connect` | `name, path` | `DbConnection` | canonicalizes the path before storing; no credentials involved |
| `db_connection_delete` | `id` | `()` | |
| `db_schema` | `id` | `DbSchema` | `DbTable[]` (`kind`: `table`/`view`, `rowCount`, `DbColumn[]`), plus the file's `sizeBytes` |
| `db_query` | `id, sql, allowWrite` | `QueryResult` | write statements refused unless `allowWrite`; the read path runs under `PRAGMA query_only = ON`. See [security.md](security.md) |
| `db_table_rows` | `id, table, limit` | `QueryResult` | identifier quoted (embedded quotes doubled), never formatted into SQL |

`QueryResult` is `{ columns, rows: (string \| null)[][], rowCount,
rowsAffected, durationMs, truncated, readOnly }` — every cell crosses as a
string so no driver's native type set leaks into the contract. Rows are
capped at 500; `truncated` says so explicitly rather than the UI implying
a complete result set.

### System (M4)

| Command | Args | Returns | Notes |
|---|---|---|---|
| `system_snapshot` | — | `SystemSnapshot` | live read, nothing persisted |

`SystemSnapshot` is `{ cpuUsage, cpuCores, memTotal, memUsed, swapTotal,
swapUsed, uptimeSecs, disks: DiskInfo[], topProcesses: ProcessInfo[] }`,
with `DiskInfo { name, mount, total, available }` and `ProcessInfo { pid,
name, cpuUsage, memory }`. Byte counts cross raw and are formatted in the
UI — the DTO carries numbers, not "3.4 GB" strings. The command reads a
single long-lived `SystemProbe` held in app state rather than constructing
one per call: CPU usage is a delta between two samples, so a fresh probe
would report 0% every time.

### Monitors (M4)

| Command | Args | Returns | Notes |
|---|---|---|---|
| `monitors_list` | — | `MonitorStatus[]` | |
| `monitor_create` | `name, url, intervalSecs` | `Monitor` | URL must parse with an `http`/`https` scheme; `intervalSecs` clamped to a 60s floor. See [security.md](security.md) |
| `monitor_delete` | `id` | `()` | |
| `monitor_toggle` | `id, enabled` | `Monitor` | disabled monitors are skipped by the scheduler |
| `monitor_check_now` | `id` | `MonitorCheck` | one check immediately, off-schedule; recorded like any other |

`MonitorStatus` is `{ monitor, lastCheck, uptimePct, avgMs, recent }` —
`uptimePct` and `avgMs` are over the last 24h, `recent` is the newest 30
checks, and `lastCheck` is null until a monitor has been checked once.
Most checks are produced not by these commands but by the
background scheduler (~15s tick), which reaches the UI the same way any
other background work does: `Kernel::notify` → `notificationAdded`, and
only on an ok↔fail transition. See [agents.md](agents.md) and
[ADR-0008](adr/0008-in-process-watchers-notify-on-transitions.md).

## Event catalog

`KernelEvent` is a tagged union `{ kind, data? }`:
`workspacesChanged` · `projectsChanged{workspaceId}` · `settingsChanged{key}`
· `jobUpdated{job}` · `notificationAdded{notification: NotificationDto}`.

`TermEvent` (per-session channel): `output{bytes}` · `exit{code}`.

`AiDelta` (per-send channel): `text{text}` · `toolCall{id,name,input}` ·
`approvalRequest{id,name,input}` · `toolResult{id,ok,summary}` · `done` ·
`error{message}`.
