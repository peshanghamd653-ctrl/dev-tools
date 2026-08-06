# Security Model

## Threat model (desktop, single user)

Protects against: secrets leaking via the DB file, backups, or logs;
a malicious/compromised plugin (future); an AI tool call reading outside
the granted project; an unintended destructive write through the SQL
editor; injection through webview content. Out of scope: an attacker with
full control of the user's OS account.

## Secrets — implemented

- Master key generated once, stored in the **OS keystore** (Windows
  Credential Manager via the `keyring` crate; Keychain/libsecret on
  macOS/Linux — same crate, feature-gated per platform).
- Secret values encrypted **AES-256-GCM** (fresh random nonce per write,
  stored as the first 12 bytes of the blob) into the `secrets` table. The
  DB file alone is useless without the OS keystore — verified by a test
  that decryption with the wrong key fails (`wrong_key_cannot_decrypt`).
- **Redaction at the type level**: `SecretStore::list()` returns
  `SecretMeta { name, updated_at }` — there is no field to leak a value
  through. Only `SecretStore::get(name)` returns plaintext, and it's called
  in exactly one place today: building the Claude API request. Values are
  never logged or included in emitted events.
- Currently stored: `anthropic-api-key` (entered once via the AI assistant's
  key-setup card).

## AI tool-calling — implemented (M2)

The riskiest new surface as of M2: giving an LLM read access to files.

- **Off by default, two grant levels.** The tools list sent to Claude is
  empty unless the user toggles the "Tools" chip on (read-only level).
  A second chip adds `edit_file`/`write_file`/`run_command` — and revoking
  the read level automatically revokes the write level too.
- **Per-call approval for anything mutating** (ADR-0005): even with the
  write grant on, every individual `edit_file`/`write_file`/`run_command`
  call pauses the agent loop, shows an approval card in the chat with the
  full arguments (the command line, the exact old/new strings), and only
  proceeds on an explicit Approve. Deny — or 180 seconds of silence — turns
  the call into an error result the model must work around. The pending
  approval lives in `ApprovalRegistry` (`src-tauri/src/approvals.rs`),
  resolved by the `ai_tool_respond` command; a response for an unknown or
  already-resolved id is a no-op.
- **Write-tool semantics limit blast radius**: `edit_file` requires the
  target string to occur exactly once (ambiguity is an error, so the model
  can't mass-replace by accident); `write_file` refuses to overwrite an
  existing file; `run_command` runs in the project root with a 60s timeout
  and capped output, `stdin` closed.
- **Path containment**: every tool path is joined to the project root,
  canonicalized, and checked with `starts_with(root)` before any filesystem
  read. `../` traversal and absolute paths outside the project are rejected
  — covered by `rejects_path_traversal_and_absolute_paths` in
  `src-tauri/src/tools.rs`.
- **Dependency directories are skipped** in `find_files` (`.git`,
  `node_modules`, `target`, `dist`, `.venv`, `__pycache__`) so the model
  doesn't burn context or leak vendored secrets accidentally checked into
  those trees.
- **Output is bounded**: file reads cap at 256 KB, tool output at 64 KB,
  directory listings at 500 entries, recursive search at depth 12 / 200
  results — all enforced before the content reaches the model or the UI.
- **Every tool call is visible** in the chat UI in real time (name,
  arguments, success/failure) — no silent tool use.

## Database query execution — implemented (M3)

The database manager runs user-authored SQL against a user-chosen SQLite
file. The gate is two-tier, deliberately the same shape as the AI
tool-grant model above: a standing toggle for the dangerous class, plus a
mechanism that doesn't depend on DevOS having classified correctly.

- **Writes are opt-in, off by default.** Statements are classified by
  leading keyword after comment stripping — `SELECT`/`WITH`/`EXPLAIN`/
  `PRAGMA` are reads, anything else is a write. A write is refused unless
  the caller passes `allowWrite: true`, which is driven by a UI toggle
  that starts off. Reading a table and dropping it are not the same
  gesture.
- **Classification is not trusted on its own.** The read path also sets
  `PRAGMA query_only = ON` on the connection it executes against, so
  SQLite itself refuses the write even if the keyword check were fooled.
  The parser being wrong is a bug; the parser being wrong *and* the
  database accepting the write is the incident. Note this specific defence
  is SQLite-only and would have to be re-established as a read-only
  transaction if Postgres lands — see
  [ADR-0007](adr/0007-sqlite-only-database-manager-first.md).
- **Identifiers are quoted, not interpolated.** `db_table_rows` quotes the
  table name by doubling embedded quotes rather than string-formatting a
  caller-supplied identifier into the statement.
- **Results are bounded**: 500 rows, with an explicit `truncated` flag in
  `QueryResult` so the UI says the set was cut rather than presenting a
  partial result as complete. The grid is read-only — there is no
  edit-cell path to audit, because editing rows isn't implemented.
- **Not reachable from AI tool calling.** Connections are user-initiated
  only; there is no database tool in the model's tool set at any grant
  level, so the blast radius of a prompt injection does not extend to the
  user's data files.
- **No credentials stored.** `db_connections` holds a canonicalized file
  path and nothing else, because SQLite needs nothing else. Server
  drivers, when added, put the credential in `secrets` and reference it by
  id.

## IPC & webview

- Strict CSP in `tauri.conf.json` (`default-src 'self'`; connect-src limited
  to the IPC endpoint). No remote code loading — the app is fully offline-capable.
- Tauri capabilities are minimal and additive: `core:default`, `opener`
  (+ `reveal-item-in-dir`). Each module adds only the permissions it needs.
- IPC commands validate inputs at the boundary (e.g. `project_add` verifies
  the directory exists) and business rules in the domain layer (e.g.
  last-workspace delete refusal, empty-commit-message rejection).

## Plugins (planned, M5)

WASM sandbox; capability-gated host functions; manifest-declared permissions
surfaced at install; own-table DB access only; network allowlists.
See [plugin-api.md](plugin-api.md) — the AI tool-calling design above is a
working preview of the same gated-capability shape.

## Audit & recovery

- `audit_log` table exists (kernel migration) but nothing writes to it yet
  — planned to record secret access, workspace deletion, plugin installs,
  and AI tool executions once the volume of security-relevant actions
  justifies it.
- Automatic DB backup before migrations and daily rotating backups are
  planned for M4, not yet implemented.
- Crash recovery: SQLite WAL + the `jobs` table means interrupted work is
  visible (a `running` row at boot would indicate a crash mid-job) rather
  than silently lost — no automatic stale-job reconciliation exists yet.
