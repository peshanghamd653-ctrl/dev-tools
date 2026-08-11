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
| `audit_log` | Security-relevant actions | append-only; written by `devos_kernel::audit`. `actor` (`user`/`ai`/`system`), `action` (stable dotted id, outcome included — `ai.tool.denied`), `detail` (one line naming what was acted on), `created_at`. Pruned by age at boot, see below |

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
| `index_symbols` | Declarations found by tree-sitter | `project`/`file`/`name`/`kind`/`start_line`; consulted as a third ranking leg so a declaration outranks a comment mentioning it. Rows follow their file and are dropped with its chunks |
| `index_meta` | Per-project index markers | `PRIMARY KEY (project, key)`. Holds `symbols.version`, which backfills symbols into an index built before they existed — the mtime/size skip would otherwise mean they never appear. Lives here rather than kernel `settings` because the index must work against a pool that has no `settings` table |
| `index_embeddings` | Chunk vectors | `PRIMARY KEY (project, file, start_line)` so they key to chunks and inherit the mtime/size skip; `f32` BLOBs in `sqlite-vec`'s little-endian layout, plus `model` and `dim` so a model switch re-embeds instead of mixing vector spaces |

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

## Monitor tables (`devos-monitor`, implemented)

| Table | Purpose | Notes |
|---|---|---|
| `monitors` | One row per website monitor | `name`, `url`, `interval_secs`, `enabled`, `created_at`. The URL is validated on write (scheme must be `http`/`https`) and the interval clamped to a 60s floor — see [security.md](security.md). User-created only; nothing else writes here. |
| `monitor_checks` | Result of every check, scheduled or manual | `status` (HTTP code, absent when the request never completed), `ok`, `duration_ms`, `error` (transport error string), `checked_at`; `monitor_id` carries the owning monitor's id. Indexed on `(monitor_id, checked_at)` — every read is "this monitor, recently". Pruned to 7 days. **Response bodies are never stored** ([security.md](security.md)). |

Created idempotently at boot, same `CREATE TABLE IF NOT EXISTS` pattern as
`api_*` and `db_connections`. This is the first module whose tables are
written by a background scheduler rather than only by user action — see
[agents.md](agents.md) and
[ADR-0008](adr/0008-in-process-watchers-notify-on-transitions.md). Since
the scheduler only runs while the app is open, `monitor_checks` has gaps
wherever DevOS was closed, and the 24h uptime percentage computed from it
is uptime *across the checks that ran*, not true uptime.

`devos-system` has no tables at all: system metrics are read live from
`sysinfo` per call and never persisted, so there is no history to query.

## Snippet tables (`devos-snippets`, implemented)

| Table | Purpose | Notes |
|---|---|---|
| `snippets` | One row per saved fragment | `title`, `language`, `body`, `tags`, `created_at`, `updated_at`. `tags` is comma-delimited rather than JSON: normalization strips the separator from every tag, so the join/split round trip is lossless — unlike the free-form header *values* that make `api_requests` need JSON. `body` is stored byte-for-byte; indentation and a trailing newline are content here, not noise. |

Created idempotently at boot, the same pattern as `api_*`, `db_connections`
and `monitors`. **There is no index and no FTS5 shadow table**, deliberately:
search is a `LIKE` scan over the four text columns, because the content is
code and FTS5's word tokenizer would not match `Query` inside `useQuery`. See
[ipc-contracts.md](ipc-contracts.md) for the full reasoning.

## Planned per milestone

- **M2 (remainder)** agent definitions/runs · `term_sessions` if session
  metadata needs to survive a full app restart (today sessions are
  in-memory only, tracked via `TerminalManager`). Embeddings now exist as
  `index_embeddings` (see above) — `sqlite-vec` was evaluated and rejected,
  so no virtual table is planned.
- **M3 (remainder)** `api_environments` (variables) · the credential-bearing
  shape of `db_connections`. The table itself now exists, but only in its
  SQLite form (name + path); server drivers add a `secret_id` referencing
  `secrets` plus host/port/user/database columns when they land
  ([ADR-0007](adr/0007-sqlite-only-database-manager-first.md))
- **M4 (remainder)** `deployments`. `monitors` and `monitor_checks` now
  exist (above); the deploy half of the milestone isn't built, so its table
  isn't either
- **M5** `plugins` (installed, version, permission grants) · `docs_pages`.
  `snippets` now exists (above). The `plugins` table stays planned rather
  than built: [ADR-0010](adr/0010-wasmi-interpreter-for-plugin-runtime.md)
  concluded the runtime should not ship in-process yet, and persisting
  permission grants for something that does not run would be premature.

## Audit log — implemented

`audit_log` is the durable half of things that are otherwise only visible
while they happen. Every tool call renders in the chat, every refused write
renders in the SQL editor, every restore raises a notification — and none of
that survives a deleted conversation or a cleared bell. Rows are appended
through `devos_kernel::audit::record`, never by a module writing its own SQL.

**The columns are M0's, unchanged.** `actor`/`action`/`detail`/`created_at`
is the classic audit tuple and it fits, so no migration was needed:

| Column | Holds |
|---|---|
| `actor` | `user`, `ai`, or `system` — "did I do this, or did the model do it with my permission?" is most of what someone opens this table to answer |
| `action` | A stable dotted identifier, with the **outcome as part of it**: `ai.tool.approved` vs `ai.tool.denied`. So "show me every refusal" is a prefix match, not a prose search |
| `detail` | One line naming what was acted on, plus the reason when there is one. Truncated at 160 characters with an explicit `… (truncated)` marker |
| `created_at` | Unix ms, like every other timestamp here |

The vocabulary is a Rust enum (`AuditEvent`) rather than free-form strings, so
a call site cannot invent an `action` nobody can query, and a variant like
`SecretSet { name }` has **no field a secret value could travel in** — the
redaction is a compile-time property, not a format string somebody remembered
to get right. See [security.md](security.md) for what is recorded and, more
importantly, what deliberately is not.

**Listed and pruned by `id`, not `created_at`.** The column is `INTEGER
PRIMARY KEY AUTOINCREMENT`, so it *is* insertion order; it breaks the tie two
events in the same millisecond would leave undefined, and it reads straight
off the primary key — which is why this table carries no second index.

**Retention: 90 days, pruned at boot.** An append-only table that grows
forever is a real problem on a long-lived install, and both ways out are bad
in different ways — so the choice is stated rather than implied:

- An **age** window is used because it answers the question people ask ("what
  happened to my machine last month?") and cannot discard something still
  inside its own stated window. A row cap would let one busy afternoon evict a
  year of history, which is the precise failure this table exists to prevent.
- **No second, size-based axis** on top of it, because every recorded event
  needs a human gesture — an approval click (or a 180-second wait), a Run in
  the SQL editor, a Save on a secret, a restart to apply a restore. The write
  rate is bounded by a person, not by a loop, so 90 days is a promise that can
  be kept without a silent truncation behind it.
- **Neither truncation is silent.** The Settings viewer prints the window, the
  total row count, and the date the record actually reaches back to — and says
  so when the list on screen is a slice of a bigger table.

Pruning runs from `Kernel::boot`, best effort: a boot that died pruning the
audit log would be a boot the audit log broke.

**There is no write, edit, delete or clear command.** `audit_log` (read) is
the only IPC surface. Rows are appended by the code paths that perform the
audited actions and removed only by the age-based prune, so nothing reachable
from the webview — or from a prompt injection that reaches the webview — can
edit its own trail.

## Backups — implemented

Automatic, silent, and run from `Kernel::boot` (`devos-kernel/src/backup.rs`).
Files land in `backups/` beside the live DB, named so that lexical order is
chronological order:

| Kind | Name | When |
|---|---|---|
| Pre-migration | `devos-premigration-YYYYMMDD-HHMMSS-vNNNN.db` | only when migrations have run before *and* an embedded migration is pending — not on first run, not on an up-to-date boot |
| Daily | `devos-daily-YYYY-MM-DD.db` | at most one per calendar day, newest `DAILY_RETENTION = 7` kept |
| Replaced by a restore | `devos-replaced-YYYYMMDD-HHMMSS.db` | written immediately before an in-app restore replaces the live database; **never pruned**, because it is the only copy of what the restore displaced |

**The WAL problem is the reason this is not a file copy.** In WAL mode
(ADR-0004) copying `devos.db` alone yields a backup missing the most recent
committed transactions. Snapshots therefore use `VACUUM INTO`, which runs in
a read transaction, includes uncheckpointed WAL frames, produces a
self-contained file, and never mutates or checkpoints the live database. A
`PRAGMA wal_checkpoint(TRUNCATE)` + file-set copy path exists as a fallback
for SQLite older than 3.27; the linked version is asserted in a test so the
assumption fails loudly rather than degrading in silence. There is also a
test proving a naive `fs::copy` loses a just-committed row — the failure
this design exists to prevent.

**A backup failure never breaks boot**: both entry points return `None` on
error and log a warning. It no longer stops there — `Kernel::boot` and
`Kernel::run_daily_backup` call the `Result`-returning helpers directly
(`try_pre_migration_backup`/`try_daily_backup`, `pub(crate)` in
`crates/devos-kernel/src/backup.rs`) so a failure can be reported before it
is collapsed to `None`, and record a warning notification the same way a
restore outcome already does. "Silently failing for three months" is closed;
`Kernel::run_daily_backup`'s path is covered end to end by
`a_failing_daily_backup_produces_a_notification` in `backup.rs`.

## Restore — implemented

Restoring cannot overwrite `devos.db` while the pool is open, so it does not
try. It happens in two halves:

1. **Stage** (`backup_restore_stage`, app running) validates the chosen file,
   copies it to `restore-pending/staged.db` beside the database, then writes
   `restore-pending/RESTORE.json` **via a rename**. That marker is the commit
   point, so a request is never half-made. Nothing about the live database
   changes, and `backup_restore_cancel` undoes it completely.
2. **Apply** (`backup::apply_pending_restore`) runs as the first statement of
   `Kernel::boot`, before `db::connect` — the only moment the file can be
   swapped. It re-validates, preserves, deletes the sidecars, and renames the
   staged file into place. It never fails the boot: a refusal leaves the
   existing database in use and is reported as a notification.

**The current database is always preserved first**, as
`backups/devos-replaced-<timestamp>.db`, written with `VACUUM INTO` for the
same WAL reason as every other snapshot — the swap is about to delete
`devos.db-wal`, and a plain copy would preserve a database missing exactly the
commits someone is most likely to want back. That file is listed like any
other backup and can itself be restored, so a mistaken restore is recoverable.
If preservation fails the restore does not happen at all.

**Sidecars are deleted before the rename, never after.** A `devos.db-wal` left
beside a restored database is replayed on the next open: at best it fails a
checksum, at worst it silently reapplies frames from the database that was just
replaced. `-shm` is removed first, so if another process still holds the
database the failure lands on the rebuildable file rather than on someone
else's write-ahead log. (Windows unmaps `-shm` asynchronously after a pool
closes, so removal retries briefly — without that, whether a restore worked was
a coin flip.)

**Interruption is decided by two files.** Marker absent → nothing applies, and
any staged debris is swept. Marker present with a staged file → apply. Marker
present with *no* staged file → the swap already landed, because the staged
file stops existing at the instant it becomes the database; the restore is
complete and only the marker is cleared. A marker that will not parse is read
as no marker. There is no state in which a restore half-applies.

**A candidate is refused rather than installed** when it is not a SQLite file,
is truncated (checked arithmetically against the page size and page count in
its header, because SQLite will often open a truncated file without
complaining), fails `PRAGMA integrity_check` on a read-only connection, or has
no `_sqlx_migrations` table — a healthy database belonging to some other
application would otherwise restore "successfully" with the real one already
moved aside.

Restoring an older file is safe with respect to schema: migrations run
immediately afterwards in the same boot, and the pre-migration snapshot fires
first, so the restored file is itself backed up before it is brought forward.
