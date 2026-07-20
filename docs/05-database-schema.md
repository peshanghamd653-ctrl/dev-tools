# Database Schema

One SQLite database per install (`%APPDATA%/com.peshang.devos/devos.db`),
WAL mode, foreign keys on, opened via SQLx. All timestamps are Unix epoch
**milliseconds** (INTEGER); ids are UUID v4 strings.

## Core tables (M0 — `crates/devos-kernel/migrations/0001_core.sql`)

| Table | Purpose | Notes |
|---|---|---|
| `workspaces` | Top-level contexts | boot invariant: ≥ 1 exists |
| `projects` | Folders registered in a workspace | `UNIQUE(workspace_id, path)`, FK cascade |
| `settings` | Key-value app settings | upsert via `ON CONFLICT` |
| `jobs` | Durable background jobs | `status: pending/running/succeeded/failed` |
| `notifications` | Notification center backlog (UI in M4) | `read` flag |
| `audit_log` | Security-relevant actions | append-only |

## Ownership & migration rules

- The kernel owns core tables; **each module owns its tables** with a name
  prefix (`git_*`, `term_*`, `ai_*`, `api_*`, `mon_*`) and ships its own
  migrations. No cross-module foreign keys — reference by id and tolerate
  absence, so modules stay independently replaceable.
- Migrations are forward-only, embedded at compile time (`sqlx::migrate!`).

## Planned per milestone

- **M1** `term_sessions` (persistent terminal sessions) · `git_repo_state`
  cache · `ai_conversations`, `ai_messages` (chat history, provider, token
  counts) · `project_templates`
- **M2** `ai_memory` (long-term memory entries) · `index_files`,
  `index_chunks` (+ `sqlite-vec` virtual table for embeddings) · agent
  definitions/runs
- **M3** `secrets` (AES-GCM blobs; key in Windows Credential Manager — plaintext
  never touches the DB) · `api_collections`, `api_requests`, `api_environments`
  · `db_connections` (credentials referenced from `secrets`)
- **M4** `monitors`, `monitor_checks` (uptime/perf history) · `deployments`
- **M5** `plugins` (installed, version, permission grants) · `snippets`,
  `docs_pages`

Backups: automatic pre-migration copy + daily rotating copy of the DB file
(M4, listed in the security model).
