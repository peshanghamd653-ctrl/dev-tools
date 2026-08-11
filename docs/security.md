# Security Model

## Threat model (desktop, single user)

Protects against: secrets leaking via the DB file, backups, or logs;
a malicious/compromised plugin (future); an AI tool call reading outside
the granted project; an unintended destructive write through the SQL
editor; unattended outbound requests from a scheduled monitor; injection
through webview content. Out of scope: an attacker with full control of
the user's OS account.

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
  through. Only `SecretStore::get(name)` returns plaintext, and every call
  site is a request builder resolving one provider's credential immediately
  before use. Values are never logged or included in emitted events.
- **The management UI can't reveal a value, by construction** (M4). The
  secret manager is a section on the Settings page over the existing
  `secret_set` / `secret_list` / `secret_delete` commands — no new backend.
  It lists stored **names**, adds or overwrites a value, and deletes behind
  a confirmation. There is no reveal button and there cannot be one: a
  stored value never crosses the IPC boundary in any direction except
  inward, so on the frontend side there is nothing to reveal. This is the
  type-level redaction above surfacing in the UI — `SecretMeta` carries no
  value field, so the absence is enforced by the compiler, not by a screen
  choosing not to render something. Values are write-only from the user's
  point of view: set it, overwrite it, delete it, never read it back. The
  UI says so on the page rather than leaving people hunting for a control
  that was never left out by accident.
- Currently stored: `anthropic-api-key` and `gemini-api-key` (each entered
  via the AI assistant's key-setup card), `vercel_token` (M4) and
  `github_token` (M4). All are ordinary secrets — same master key, same
  AES-256-GCM, same names-only listing — read only when the relevant request
  is built. No module touches the store itself; the command layer resolves
  the credential and passes it in.
- **Which secret a provider uses is decided in one place**
  (`key_secret_for` in `ai_commands.rs`) rather than at each call site. That
  matters more than it looks: before Gemini there was a single hardcoded
  lookup, and adding a second provider by copying it is how one provider
  ends up sending another's key to a third party. Ollama maps to `None` —
  it is local and needs no credential, and `None` keeps "needs no key"
  distinguishable from "key not configured yet".
- `gemini-api-key` travels as an `x-goog-api-key` **header**, not the `?key=`
  query parameter Google also documents. Both authenticate; only one keeps
  the credential out of URLs, and URLs reach logs, proxies and error
  messages. A test asserts the key appears in the request headers and *not*
  in the request line.
- **`github_token` is the most privileged credential DevOS holds**, and it
  deserves saying plainly: it backs `issue_create`, the only thing this app
  writes to the outside world, and a filed issue is public and irreversible.
  ([ADR-0009](adr/0009-deployments-read-only-no-write-actions.md) explains
  why the *Vercel* module performs no writes; that rationale is scoped to
  deployments and does not describe the whole outbound surface.) What bounds
  it:
  - The base URL is a compile-time constant at the command layer. The
    injectable `base_url` exists so tests run against a local server and is
    not reachable from IPC, so the token cannot be aimed elsewhere.
  - It is never logged — `devos-issue` and `issue_commands.rs` contain no
    `tracing`/`println` at all — and no error variant carries it; they carry
    a status, a truncated body, or a transport string. `reqwest` strips
    `Authorization` on a cross-host redirect.
  - `owner`/`name` are allowlisted to `[A-Za-z0-9._-]` with explicit `..`
    rejection before path interpolation.
  - The body is reviewed verbatim, and editable, before anything is sent.
  - Recommended token shape: a fine-grained PAT scoped to one repository
    with `issues:write` and nothing else. DevOS cannot enforce that — it is
    the narrowest credential the feature can be given, so it is worth
    giving.
- **Screenshots are a fourth at-rest channel**, alongside the DB, backups and
  logs named at the top of this document. `issue_capture` writes the raw,
  unredacted, full-resolution desktop to `<data-dir>/screenshots` as a plain
  PNG with default ACLs. Redaction is destructive in the *export* (verified:
  it is painted into the same bitmap `toBlob` reads back, and the flow
  refuses to fall back to the source file if flattening fails) — but the
  source capture is what the annotator loads, so it exists on disk in the
  meantime and is what a file-level backup or sync client would pick up.
  Retention is deliberately short and the directory is documented here
  rather than left for someone to find while investigating a leak.

## AI tool-calling — implemented (M2)

The riskiest new surface as of M2: giving an LLM read access to files.

- **Off by default, two grant levels.** The tools list sent to the model
  (Claude or Ollama — the two providers whose conversations can drive the
  agent loop) is empty unless the user toggles the "Tools" chip on (read-only
  level).
  A second chip adds `edit_file`/`write_file`/`run_command`/`run_tests`/
  `run_lint`/`git_commit`/`git_create_branch` — and revoking the read level
  automatically revokes the write level too. `git_diff` stays in the read
  tier alongside `read_file` and `search_code`: it inspects repository state
  and changes nothing.
- **The write grant is session-scoped.** It is deliberately not persisted,
  and a grant left on disk by an older build is forced off on rehydration.
  "Off by default" has to mean off at every launch, not off on first run —
  the user who granted shell access three weeks ago is not the user opening
  the app today. Per-call approval still guards each action, but the grant
  is what puts those tools in the model's list at all, and a tool that is
  never offered is the one thing a prompt injection cannot reach for. The
  read grant does persist: read tools are side-effect-free by construction
  and the chip states the grant on screen throughout.
- **Per-call approval for anything mutating** (ADR-0005), enforced by a
  `MUTATING_TOOLS` list checked *before* tool dispatch rather than inside
  one match arm. That ordering is the fix for a real hole: `save_memory`
  had been added to the read tier and inherited no gate, so indirect prompt
  injection from any file the model read could silently write durable,
  authoritative-looking text into the system prompt of **every future
  conversation** for that project. It is now approval-gated, and the read
  tier has an approval channel for exactly that reason. A gate-less
  executor refuses every mutating tool rather than falling through.
  Approving, not the grant tier, is what closes this — `save_memory` stays
  in the read grant, because "may remember a fact" and "may touch my
  filesystem and shell" are different permissions and conflating them is
  what ADR-0005 exists to prevent.
- Each call pauses the agent loop and shows an approval card with the full
  arguments. Long arguments are **folded, never silently truncated**: the
  card states how many characters are hidden and expands to the complete
  text, because approving a command whose tail scrolled off screen is not
  approval. Deny — or 180 seconds of silence — turns the call into an error
  the model must work around. Pending approvals live in `ApprovalRegistry`
  (`src-tauri/src/approvals.rs`), resolved by `ai_tool_respond`; a response
  for an unknown or already-resolved id is a no-op.
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
- **Links are not followed out of the project**, which canonicalization
  alone did not achieve. Two gaps were found and closed:
  - `find_files` and the indexer walked *through* symlinks and junctions,
    because `path.is_dir()` follows them. `read_file` correctly refused a
    path through a link, but `find_files` still listed it, and after
    `index_project` the file *contents* from outside the root landed in
    `index_chunks` where `search_code` fed snippets to the model — both in
    the read tier, both unapproved. Both walkers now use non-traversing
    metadata and skip any reparse point.
  - `write_file` wrote *through* a pre-existing symlink. It canonicalized
    only the parent and re-attached the raw final component, and
    `Path::exists()` follows links, so a **dangling** link reported absent
    and the write followed it out of the root — while the approval card
    innocently read `New file: docs/notes.md`.

  Two Windows details make the naive fixes insufficient, and both are worth
  knowing before touching this code: `FileType::is_symlink()` is only true
  for `IO_REPARSE_TAG_SYMLINK`, so a `mklink /J` junction reads as an
  ordinary directory and the check must test `FILE_ATTRIBUTE_REPARSE_POINT`
  instead; and `create_new(true)` is not enough on its own, because
  `CreateFileW` follows reparse points unless opened with
  `FILE_FLAG_OPEN_REPARSE_POINT`. Each guard is pinned by a test that
  builds a real junction and was confirmed to fail without the fix.
- **Dependency directories are skipped** in `find_files` (`.git`,
  `node_modules`, `target`, `dist`, `.venv`, `__pycache__`) so the model
  doesn't burn context or leak vendored secrets accidentally checked into
  those trees.
- **Output is bounded**: file reads cap at 256 KB, tool output at 64 KB,
  directory listings at 500 entries, recursive search at depth 12 / 200
  results — all enforced before the content reaches the model or the UI.
- **Secrets are redacted before a tool result reaches the model.** Every
  successful tool call's output passes through `devos-redact` — one choke
  point after the dispatch `match`, not a per-arm judgment call, the same
  shape that fixed `save_memory`'s gap above — which pattern-matches known
  credential shapes (`sk-ant-…`, `AKIA…`, GitHub/Slack/Google tokens, JWTs,
  PEM private-key blocks, and a generic `SOME_TOKEN=…`/`SOME_SECRET=…`
  catch-all) and replaces each with `[REDACTED:<kind>]`. `read_file` on a
  stray `.env`, a `git_diff` that adds a config line, `run_command` output
  that echoes an env var — all pass through the same filter, so a new tool
  that returns raw content is covered automatically rather than needing to
  remember to wire it in. This is shape-matching, not a secret scanner with
  provenance analysis: it will miss a bespoke internal token with no
  recognizable prefix, and it will occasionally redact a config value that
  happens to match a shape. The second failure direction is deliberate —
  over-redacting a harmless value costs a little context; under-redacting a
  real key costs the credential.
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
- **Classification is not trusted on its own** — and the first attempt at
  backing it up was itself insufficient, which is worth recording. A
  security review on 2026-08-06 demonstrated that `PRAGMA query_only = ON`
  is **not** a real guard: sqlx-sqlite executes every statement in a
  `;`-separated string, so `SELECT 1; PRAGMA query_only(0); UPDATE …`
  classified as a read from its first keyword and then turned the guard off
  from inside the query it was guarding. It landed silently, since the read
  path returns only rows. Two guards were added:
  - **One statement per call.** Anything with a separator outside a string
    literal, quoted identifier, or comment is rejected. This is a far
    narrower parse than classifying SQL, and it closes the demonstrated
    bypass.
  - **The read path runs on a connection opened `SQLITE_OPEN_READONLY`**
    (`DbManager::read_pool_for`). An open mode cannot be undone by a
    statement, so this is the guard actually carrying the guarantee.
    `query_only` is still set, but only as a backstop for the rare fallback
    to a read-write handle (a live WAL database whose `-shm` does not yet
    exist cannot be opened read-only). That fallback is degraded, not
    unguarded — the other two guards still apply.

  The lesson generalizes: a guard that the guarded input can address by
  name is not a guard. Note the read-only-connection defence is SQLite-
  specific; Postgres would need a read-only transaction instead — see
  [ADR-0007](adr/0007-sqlite-only-database-manager-first.md).
- **PRAGMA classification is an allowlist**, not a test for `=`. The
  original heuristic treated any PRAGMA without `=` as a read, which let
  through every parenthesised setter — `journal_mode(wal)`,
  `wal_checkpoint(TRUNCATE)` and `writable_schema(1)` all execute happily
  under `query_only`. Only named introspection pragmas are reads now.
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

## Website monitoring — implemented (M4)

The monitor fetches URLs the user supplies. The API client already does
that, but the monitor is the first feature that does it **unattended, on a
timer** — nobody is watching the request when it goes out, and it goes out
again every interval until the monitor is deleted. That changes what a bad
input costs.

- **The URL is validated on write.** `monitor_create` requires the URL to
  parse and its scheme to be `http` or `https`. `file://` and every other
  scheme are refused, so a monitor cannot be aimed at the local filesystem
  or at another protocol handler.
- **The interval has a floor.** `interval_secs` is clamped to 60 seconds.
  A mistyped `1` would otherwise turn DevOS into a small load generator
  pointed at somebody else's server, running for as long as the app is
  open. The floor makes that impossible rather than merely discouraged.
- **Each check is bounded**: a 15-second timeout and a limited redirect
  policy, so a hung target or a redirect loop can't occupy the scheduler.
- **Response bodies are never stored.** `monitor_checks` records the status
  code, the ok flag, the duration, and any transport error string —
  nothing else. A monitored page may carry session data, personal data, or
  an error page full of internals; no feature needs the body, so it never
  reaches the database. The module checks that a site *answers*, not that
  it answers correctly; content assertions are deferred (see the
  [roadmap](feature-roadmap.md)).
- **Not reachable from AI tool calling.** Monitors are user-created only;
  there is no monitor tool at any grant level. A prompt injection cannot
  register an outbound request that DevOS will then repeat on a timer.

System metrics (`devos-system`) are read-only and local: `sysinfo` values,
never persisted, never sent anywhere, and no command in that module takes
an argument.

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

## Audit log — implemented

Every guarantee above is enforced *while something happens*, and visible only
then. A tool call renders in the chat, a refused write renders in the SQL
editor, a restore raises a notification — and all of it dies with the
conversation, the tab, or the cleared bell. `audit_log` is the half that
survives, and the question it exists to answer is "what happened to my
machine, and did I agree to it?"

**What is recorded.** A closed set, defined as a Rust enum (`AuditEvent` in
`crates/devos-kernel/src/audit.rs`) so a module cannot invent an entry type
and nothing can be added without deciding it belongs in the security record:

- **AI tool approvals and denials** — the highest-value entry. Which tool,
  what it was aimed at, whether it ran, and for a refusal *which kind of
  refusal*: an explicit Deny and a 180-second timeout produce the same error
  for the model and very different stories for whoever reads this later. That
  distinction is a typed `ApprovalOutcome`, not a string the reader parses.
  The row is written at the single consent choke point in `ProjectTools::
  approve` — the same place the gate is enforced — so a mutating tool added
  later cannot be forgotten here without also being ungated.
- **Secret set and delete** — the **name** only.
- **Writes through the SQL editor** — that a write ran, against which
  connection, and how many rows moved.
- **Issues filed** — `owner/name#number`. DevOS's only outward-facing write,
  and the only recorded action whose effect is public and irreversible.
- **Restores applied or refused at boot** — the durable half of the
  notification `Kernel::boot` already raises, including the name of the
  preserved database.

**What is deliberately not recorded**, because logging everything produces a
table nobody reads and a privacy problem of its own:

- **Secret values.** `AuditEvent::SecretSet` has no field one could travel in,
  the same type-level redaction as `SecretMeta` — the compiler enforces it,
  not a screen choosing not to render something. This table is plaintext in
  the same database the AES-256-GCM encryption exists to survive, so it is the
  last place a value may appear. A test asserts the value string is absent
  from every column of every row, not merely from the field the writer chose.
- **SQL statements.** A statement is where the *data* is: `INSERT INTO users
  VALUES ('<a token>')` classifies as a write and carries whatever the user's
  database carries. "A write ran against Local notes and moved 400 rows" is
  the security fact; reproducing it verbatim is the editor's history's job.
- **Issue bodies.** Free-form prose assembled from an annotated screenshot,
  routinely quoting logs and config — and already durable on GitHub. Copying
  it here would duplicate arbitrary text into a table with no delete button.
- **File contents written by `edit_file`/`write_file`.** The path is the
  action; the bytes are the payload. Recording them would put arbitrary file
  content — including the `.env` the model was asked to create — into this
  table.
- **`save_memory`'s text.** That a memory was written is recorded; the text is
  not. It is already durable, already listed in the Memory panel *with* a
  delete button, and is free-form model output.
- **Read-only tool calls** (`read_file`, `list_dir`, `find_files`,
  `search_code`). These are the volume. They are side-effect-free by
  construction and containment-checked before they run, and a row per read
  would be a browsing history of the user's own source tree that buries the
  entries someone came looking for.
- **Ordinary CRUD** — workspaces, projects, snippets, monitors, saved
  requests. Recoverable, non-security-relevant, and already visible in its own
  screen.
- **Refused SQL writes.** Nothing happened; the refusal is surfaced in the
  editor at the time. A guard doing its job every time somebody forgets the
  write toggle is noise, not history.

**One honest exception to "action, not payload": `run_command` records the
command line.** For every other tool the argument is a target and the payload
is separate, but here the command *is* the action — "run_command was approved"
without it does not answer the question this table exists for. The residual
risk is real and worth stating rather than shrugging at: a command that embeds
a credential (`curl -H "Authorization: Bearer …"`) lands in the audit log in
plaintext. What bounds it: the text is truncated at 160 characters with an
explicit marker, and it is the exact text the user was shown in the approval
card and consented to — so it is not data the log introduces to the machine,
only data it keeps for longer than the chat does.

**Writing an entry never breaks the action it records.** `Kernel::audit`
returns `()`; a failed insert is logged and skipped, the same contract a
failed backup has against a boot. An audit log that can veto the action it is
describing is a liability, not a control. Tested by dropping the table out
from under a live tool call and asserting the file is still written.

**Retention is 90 days, and visible.** Age rather than a row cap, because a
cap lets one busy afternoon evict a year of history. No second size-based axis
on top, because every recorded event needs a human gesture — an approval click
or a 180-second wait, a Run, a Save, a restart — so the write rate is bounded
by a person and the window can be promised without a silent truncation behind
it. Pruning runs from `Kernel::boot`, best effort. The Settings viewer prints
the window, the total row count and the date the record reaches back to, and
says when the list on screen is a slice of a bigger table — neither truncation
is left to be inferred. Full reasoning in [database.md](database.md).

**The log cannot be edited from the app.** `audit_log` (read) is the only IPC
command; there is no write, delete or clear, and there is no clear button in
the viewer. Rows are appended by the code paths performing the audited actions
and removed only by the age-based prune, which takes rows by age and cannot be
aimed at a particular entry — so nothing reachable from the webview, including
a prompt injection that reaches it, can write its own alibi.

The viewer is a Settings section rather than a fourteenth sidebar item: it is
a "look when something went wrong" surface, and the secrets, backups and
restore controls it records are already on that page.

This is precondition 3 of the five in
[ADR-0010](adr/0010-wasmi-interpreter-for-plugin-runtime.md) for a plugin
runtime shipping in-process. The remaining four are unaffected — and note the
plugin sandbox's per-call journal (`Sandbox::take_journal()`) is *not* wired
here, because the crate is still not registered with the app; whatever
registers it has to drain that journal into `AuditEvent` variants that do not
exist yet.

## Recovery

- Automatic DB backup before migrations and daily rotating backups are
  implemented — see [database.md](database.md).
- Crash recovery: SQLite WAL + the `jobs` table means interrupted work is
  visible (a `running` row at boot would indicate a crash mid-job) rather
  than silently lost. `JobRunner::reconcile_stale`, called once at
  `Kernel::boot` before anything can submit a new job, closes this: every
  `running` row it finds is the previous process's, gets marked `failed`
  ("interrupted by a crash"), and produces a notification the same way any
  other job failure does.
