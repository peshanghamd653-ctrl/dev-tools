# Security Model

## Threat model (desktop, single user)

Protects against: secrets leaking via the DB file, backups, or logs;
malicious/compromised plugins; injection through webview content. Out of
scope: an attacker with full control of the user's OS account.

## Secrets (foundation in M3, design fixed now)

- Master key generated once, stored in **Windows Credential Manager**
  (via the `keyring` crate; Keychain/libsecret on macOS/Linux).
- Secret values encrypted **AES-256-GCM** (unique nonce per value) and stored
  in the `secrets` table. The DB file alone is useless without the OS keystore.
- Redaction layer: list endpoints return names/metadata only; values are
  fetched individually, on explicit action, and never logged. Secrets are
  never interpolated into frontend-visible strings.

## IPC & webview

- Strict CSP in `tauri.conf.json` (`default-src 'self'`; connect-src limited
  to the IPC endpoint). No remote code loading — the app is fully offline-capable.
- Tauri capabilities are minimal: `core:default`, `opener` (+ reveal-in-dir).
  Each future module adds only the permissions it needs.
- IPC commands validate inputs at the boundary (e.g. `project_add` verifies
  the directory exists) and business rules in the kernel (e.g. last-workspace
  delete refusal).

## Plugins (M5)

WASM sandbox; capability-gated host functions; manifest-declared permissions
surfaced at install; own-table DB access only; network allowlists.
See `07-plugin-api.md`.

## AI safety rails

- Mutating AI tool calls require explicit approval (see `08-ai-architecture.md`).
- Prompts/requests to cloud providers never include secret values; the
  redaction layer sits between the secret store and every consumer.

## Audit & recovery

- `audit_log` (append-only) records security-relevant actions: secret access,
  workspace deletion, plugin installs, AI tool executions.
- Automatic DB backup before every migration; daily rotating backups (M4).
- Crash recovery: SQLite WAL + jobs table means interrupted work is visible
  (status `running` at boot → marked stale) rather than silently lost.
