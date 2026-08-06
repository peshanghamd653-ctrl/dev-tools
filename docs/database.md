# Database

One SQLite database per install (`%APPDATA%/com.peshang.devos/devos.db`),
WAL mode, foreign keys on, opened via SQLx. All timestamps are Unix epoch
**milliseconds** (INTEGER) unless noted; ids are UUID v4 strings.

## Ownership & migration rules

- The kernel owns core tables via embedded SQL migrations
  (`crates/devos-kernel/migrations/`, run by `sqlx::migrate!` at boot).
- **Each module owns its tables** with a name prefix (`ai_*`, and `term_*`/
  `git_*`/`secrets` when those modules need persistence) and creates them
  itself. `devos-ai` and `devos-secrets` demonstrate the pattern today:
  `ai::repo::init(pool)` and `SecretStore::init(pool)` run idempotent
  `CREATE TABLE IF NOT EXISTS` at boot, independent of the kernel's
  migration files. No cross-module foreign keys — modules stay
  independently replaceable.
- Kernel migrations are forward-only, embedded at compile time.

## Core tables (kernel-owned)

| Table | Purpose | Notes |
|---|---|---|
| `workspaces` | Top-level contexts | boot invariant: ≥ 1 exists |
| `projects` | Folders registered in a workspace | `UNIQUE(workspace_id, path)`, FK cascade |
| `settings` | Key-value app settings | upsert via `ON CONFLICT` |
| `jobs` | Durable background jobs | `status: pending/running/succeeded/failed` |
| `notifications` | Notification center (bell in the topbar) | `read` flag; written by `Kernel::notify` and failed jobs |
| `audit_log` | Security-relevant actions | append-only, not yet written to |

## AI tables (`devos-ai`, implemented)

| Table | Purpose | Notes |
|---|---|---|
| `ai_conversations` | One row per chat | `provider`, `model`, auto-titled from the first user message |
| `ai_messages` | Turn history | FK cascade from `ai_conversations`; indexed on `(conversation_id, created_at)` |
| `ai_memory` | Long-term facts per project | keyed by normalized project path; 500 chars/entry, 100 entries/project |

## Secrets table (`devos-secrets`, implemented)

| Table | Purpose | Notes |
|---|---|---|
| `secrets` | Encrypted key-value store | `value` is a BLOB: 12-byte nonce + AES-256-GCM ciphertext. Master key lives in the OS keystore, never in this table. See [security.md](security.md). |

## Index tables (`devos-index`, implemented)

| Table | Purpose | Notes |
|---|---|---|
| `index_files` | Per-file index state | `PRIMARY KEY (project, file)`, mtime+size for incremental skip |
| `index_chunks` | FTS5 virtual table | `content` indexed; `project`/`file`/`start_line` UNINDEXED; bm25 + snippet() at query time |

## API tables (`devos-api`, implemented)

| Table | Purpose | Notes |
|---|---|---|
| `api_requests` | Saved requests | collection is a plain grouping string; headers stored as JSON |
| `api_history` | Sent-request log | pruned to the newest 100 on every insert |

## Database manager tables (`devos-db`, implemented)

| Table | Purpose | Notes |
|---|---|---|
| `db_connections` | Saved connections for the database manager | `driver` is `sqlite` on every row today — the column exists so Postgres/MySQL slot in behind the same DTOs ([ADR-0007](adr/0007-sqlite-only-database-manager-first.md)). `path` is canonicalized on connect. **No credentials stored**: SQLite needs none, and when server drivers land the credential goes in `secrets` and is referenced by id, never inlined here. |

Created idempotently at boot, same `CREATE TABLE IF NOT EXISTS` pattern as
`api_*`. Note that this is a table *about* databases inside DevOS's own
database — the user's own SQLite files are opened as separate connections
and are never touched by kernel migrations.

## Planned per milestone

- **M2 (remainder)** `sqlite-vec` virtual table for embeddings alongside
  `index_chunks` · agent definitions/runs · `term_sessions` if session
  metadata needs to survive a full app restart (today sessions are
  in-memory only, tracked via `TerminalManager`)
- **M3 (remainder)** `api_environments` (variables) · the credential-bearing
  shape of `db_connections`. The table itself now exists, but only in its
  SQLite form (name + path); server drivers add a `secret_id` referencing
  `secrets` plus host/port/user/database columns when they land
  ([ADR-0007](adr/0007-sqlite-only-database-manager-first.md))
- **M4** `monitors`, `monitor_checks` (uptime/perf history) · `deployments`
- **M5** `plugins` (installed, version, permission grants) · `snippets`,
  `docs_pages`

Backups: automatic pre-migration copy + daily rotating copy of the DB file
— planned for M4, not yet implemented.
