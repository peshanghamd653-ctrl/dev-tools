# Security Model

## Threat model (desktop, single user)

Protects against: secrets leaking via the DB file, backups, or logs;
a malicious/compromised plugin (future); an AI tool call reading outside
the granted project; injection through webview content. Out of scope: an
attacker with full control of the user's OS account.

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

- **Off by default, per-conversation.** The tools list sent to Claude is
  empty unless the user has explicitly toggled the "Tools" chip on for that
  chat. There is no implicit or persistent-across-sessions grant.
- **Read-only tool set**: `read_file`, `list_dir`, `find_files`. No write,
  delete, or execute capability exists yet — `edit_file`/`run_command` are
  planned for a later milestone, explicitly gated separately (see
  [feature-roadmap.md](feature-roadmap.md)).
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
