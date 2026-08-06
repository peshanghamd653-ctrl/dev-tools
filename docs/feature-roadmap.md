# Feature Roadmap

Honest framing: the full DevOS vision is a multi-year product. Development
is organized in vertical milestones — each one ships something used daily,
never a broad shell of empty screens. Planned modules can appear in the
sidebar as disabled entries with their milestone tag, so the roadmap is
visible in-app — that list is empty as of M4, since everything it once
advertised has shipped, and the section hides itself rather than rendering
an empty header.

## Milestones

### M0 — Foundation ✅
- Rust + pnpm + cargo workspace, Tauri v2 boots
- `devos-kernel`: module registry, command registry, event bus, durable
  jobs, SQLx (WAL) + migrations, tested
- App shell: sidebar, workspace switcher, command palette (Ctrl+K), keyboard
  shortcuts, dark-first theme, dashboard, projects, settings
- Typed IPC with generated bindings; kernel events bridged to the UI
- CI: fmt/clippy/tests, eslint/tsc/vitest/build

### M1 — Daily-driver core ✅
- **Terminal**: ConPTY sessions (`portable-pty`) streamed to xterm.js;
  sessions live in the Rust process so they survive route changes; tabs,
  split view, exit detection.
- **Git**: status/stage/commit/branch/history/diff via `git` CLI
  porcelain v2; diff viewer; push/pull; branch switch/create.
- **AI assistant (chat)**: `devos-ai` with `AiProvider` trait; Claude +
  Ollama, streaming, conversation persistence, markdown rendering.
- **Secrets**: `devos-secrets` — OS-keystore master key + AES-256-GCM,
  redaction-by-type at the IPC boundary.

M1.5 status: ✅ **file explorer** (`/files`, Ctrl+6 — lazy directory tree,
mono preview with line numbers, filename search + FTS content search, all
paths through the shared `pathsafe` containment guard). Still deferred:
project templates, repository cloning.

### M2 — AI depth (in progress)
- ✅ AI commit-message generation (staged diff → conventional-commit message)
- ✅ Project-aware chat (git status/branch injected as system context, with
  a UI chip to attach/detach)
- ✅ **Tool calling**: Claude agentic loop (`run_agent`) with a read-only
  tool set (`read_file`, `list_dir`, `find_files`), a canonicalize-and-contain
  path guard against traversal, and an explicit per-conversation grant the
  user must turn on — off by default. Live tool activity shown in the chat.
- ✅ **Write/execute tools with per-call approval**: `edit_file` (unique
  exact-match replace), `write_file` (new files only), `run_command`
  (project-root shell, 60s timeout) behind a second grant chip; every
  individual call pauses the agent loop on an approval card in the chat
  (Approve/Deny, 180s timeout = deny). See [security.md](security.md).
- ✅ **Project index (lexical half of RAG)**: `devos-index` module — FTS5
  full-text index over project files, built incrementally (mtime/size) as
  the kernel's first real background job, with a bm25-ranked `search_code`
  AI tool returning file:line + snippets. Triggered from the Projects page
  or the palette ("Index Project for AI Search").
- ✅ **Long-term project memory**: `ai_memory` table, `save_memory` AI tool
  (read level — writes only to DevOS's own store), facts injected into the
  project system prompt, and a Memory dialog in the chat (list/add/delete —
  transparent, never magic).
- ✅ **Terminal AI diagnosis**: the terminal keeps a 32 KB backend ring
  buffer of output (ANSI-stripped on read); a toolbar button sends the tail
  into a fresh AI chat with a "diagnose and propose a fix" prompt. This is
  the manual core of the build-failure watcher — the automatic version
  needs OSC 133 shell integration for command boundaries, and will build on
  this same buffer.
- ✅ **Automatic failure watcher**: PowerShell sessions get an OSC 133
  prompt hook (injected via `-EncodedCommand`, chaining the user's existing
  prompt); the pty reader scans for `133;D;<code>` markers; non-zero exits
  become Notification Center entries with an output snippet, throttled to
  one per session per 30s. Opt out with setting `terminal.integration=off`.
  AI diagnosis stays one click away (the sparkle button) rather than
  auto-running — detection is free, diagnosis costs tokens.
- ✅ **Vector half of retrieval** (added 2026-08-06): chunk embeddings via
  Ollama stored as `f32` BLOBs in `index_embeddings`, fused with bm25 by
  reciprocal-rank fusion behind the unchanged `index_search` command. A
  project with no stored vectors never contacts Ollama, and search degrades
  to lexical-only rather than failing when it is absent. `sqlite-vec` was
  evaluated and rejected — sqlx loads extensions only at connect time and
  re-disables loading afterwards, so a module crate cannot reach it. See
  [agents.md](agents.md).
- Still planned: **tree-sitter symbol extraction**, the other half of this
  line item. Not started.

### M3 — Ops tools ✅
- ✅ **Docker module** (`/docker`, Ctrl+7): containers (state, ports,
  start/stop/restart, last-200-line logs dialog) and images via `bollard`
  over the Engine API named pipe; graceful "Docker isn't running" state
  with auto-reconnect polling. Deferred within the module: volumes,
  compose, live stats, image pull/remove.
- ✅ **API client** (`/api`, Ctrl+8): REST requests with headers/body
  editor, response viewer (status, timing, size, pretty JSON, headers),
  saved requests grouped into collections, automatic history (last 100,
  auto-pruned). Custom HTTP token methods allowed. Deferred within the
  module: GraphQL helpers, WebSockets, environments/variables, auth
  helpers, code generation.
- ✅ **Database manager** (`/database`, Ctrl+9): **SQLite only.** Named
  connections storing a canonicalized file path, schema explorer
  (tables/views with columns, plus the file's size on disk), SQL editor,
  and a read-only result grid capped at 500 rows with an explicit
  `truncated` flag. Writes are refused unless a toggle that is **off by
  default** is turned on, and the read path additionally sets
  runs on a read-only connection and accepts one statement per call, so the
  engine rather than the classifier carries the guarantee — see
  [security.md](security.md). Postgres and MySQL are deferred: the
  `driver` column exists so they slot in behind the same DTOs, but their
  sqlx driver features were deliberately not enabled — see
  [ADR-0007](adr/0007-sqlite-only-database-manager-first.md). Also deferred
  within the module: query history, saved queries, ER diagrams, CSV/JSON
  export, and row editing (the grid inspects and queries; it does not edit
  cells).

- ✅ **Notification Center** (pulled forward from M4): `Kernel::notify`
  persists + broadcasts; failed jobs auto-notify; index completions notify;
  topbar bell with unread badge, level dots, mark-read/mark-all-read. This
  is the reporting surface background agents will use.

M3 status: ✅ — all three ops modules shipped (Docker, API client, database
manager), plus the Notification Center pulled forward from M4. Carried
forward rather than quietly dropped:

- **Secret manager UI** — ✅ **shipped in M4** (Settings page), on top of
  the `secret_*` commands that already existed. Listed here because it was
  carried rather than dropped; see the M4 entry below.
- **Postgres/MySQL drivers** and the credential flow they need
  (ADR-0007).
- **Per-module deferrals** listed above: Docker volumes/compose/live
  stats/image pull+remove · API GraphQL, WebSockets, environments and
  variables, auth helpers, code generation · database query history, saved
  queries, ER diagrams, CSV/JSON export, row editing.

### M4 — Watchers & deploys (in progress)
- ✅ **System monitoring** (`devos-system`, via `sysinfo`): no page of its
  own — it renders as a strip of metric cards on the Dashboard. One
  command, `system_snapshot`: CPU usage and core count, memory and swap
  used/total, uptime, per-disk total/available, and the top processes by
  CPU. Byte counts cross the IPC boundary raw and are formatted in the UI.
  A single long-lived `SystemProbe` is held in app state because CPU usage
  is a delta between two samples — a fresh probe per call would report 0%
  forever. **Nothing is persisted**: metrics are live-only, so there is no
  history, no charting over time, and no alerting on a threshold.
- ✅ **Website monitor** (`/monitors`, Ctrl+0): named HTTP monitors with a
  per-monitor interval, stored in `monitors` / `monitor_checks` (see
  [database.md](database.md)); 24h uptime percentage and average response
  time, the newest 30 checks, enable/disable, and a manual "check now". A
  background tokio task started at boot ticks every ~15s, checks monitors
  whose newest check is older than their interval, and notifies **only on
  state transitions** — ok→fail as a warning, fail→ok as an info, through
  the Notification Center. **Monitoring only runs while DevOS is open**;
  that limitation and what it would take to remove it are in
  [ADR-0008](adr/0008-in-process-watchers-notify-on-transitions.md). URLs
  are restricted to `http`/`https` and intervals clamped to a 60s floor —
  see [security.md](security.md). Deferred within the module: alerting
  anywhere but the in-app Notification Center (no email, webhook, or
  Slack), status-page export, TLS-certificate-expiry checks,
  response-content assertions (it checks that a site answers, not that it
  answers *correctly*), and multi-region checking.
- ✅ **Deployments** (`/deploy`, Ctrl+Shift+D) — **read-only Vercel
  visibility.** `devos-deploy` lists projects and their recent deployments
  (state, target, URL, commit message, timestamp) straight from the Vercel
  API; the token lives in the encrypted secret store as `vercel_token` and
  the crate never reads that store itself — it takes `(token, base_url, …)`
  so `base_url` can be pointed at a local one-shot server in tests. Errors
  distinguish not-configured from auth-rejected (401/403) from a generic
  API failure. **No deploy triggering, promotion, rollback, or deletion** —
  deliberately, and that decision is
  [ADR-0009](adr/0009-deployments-read-only-no-write-actions.md); DevOS
  does not replace the Vercel dashboard. **Deviation from the M4 plan: no
  `deployments` table was created.** The plan listed one; deployment data
  turned out to be worth reading live per request the way the Docker module
  does, so nothing is persisted and nothing is cached. Also deferred:
  Vercel only — no Netlify, Fly, Railway, or Cloudflare.
- ✅ **Secret manager UI** (Settings page) — carried forward from M3 and
  now shipped. Lists stored secret **names**, adds or overwrites a value,
  deletes behind a confirmation, all over the existing `secret_set` /
  `secret_list` / `secret_delete` commands — no new backend. There is no
  reveal button and there cannot be one: values never cross the IPC
  boundary outward, so there is nothing to reveal. The UI says that plainly
  instead of looking like it forgot a feature — see
  [security.md](security.md).
- **Screenshot → GitHub issue** — not built. Researched 2026-08-06, and the
  research changed the intended shape, so recording it here rather than
  losing it:
  - **GitHub documents no API for attaching an image to an issue.** Release
    assets are documented and are a different thing. An undocumented
    endpoint (`uploads.github.com/user-attachments/assets`) was demonstrated
    working on 2026-08-03, but the only credential ever shown against it is
    the `gh` CLI's OAuth token — no source has tested a PAT. Reading another
    application's credential store is not an acceptable fallback for this
    project.
  - Dead ends, verified rather than assumed: base64 data URIs (GitHub's
    sanitizer strips the `src`), gists (files are stored as text), and
    linking `raw.githubusercontent.com` (camo cannot authenticate to
    private repos, so it breaks for exactly the users most likely to care).
    `github.com/user-attachments/assets/<uuid>` is the only URL form that
    renders regardless of repo visibility.
  - **Intended shape:** capture + annotate + file a context-rich issue, with
    the image handed off via the clipboard for the user to paste. Every
    network call is then documented, and it upgrades to real attachment
    later without rework. Same reasoning as
    [ADR-0009](adr/0009-deployments-read-only-no-write-actions.md): merging
    a documented half with an undocumented one makes the whole feature
    inherit the reliability of its worst component.
  - **Two security constraints, not afterthoughts.** A desktop screenshot
    routinely contains `.env` contents, and the terminal ring buffer is
    exactly where an exported `API_KEY=` lands — so redaction is required
    scope and terminal context must be opt-in and visible. Filing an issue
    would also be DevOS's **first outward-facing write**, so it needs the
    existing per-call approval gate, showing the full generated body
    verbatim — the user is approving text they did not write.
  - Capture crate: `xcap` (Apache-2.0, actively maintained, Windows window
    and region capture). The older `screenshots` crate is deprecated by its
    own author in favour of it.

M4 status: **partially done** — monitoring (both items), deployments, and
the secret manager UI shipped; **screenshot → GitHub issue was not
started**, so the milestone stays open. Two qualifications on what
"shipped" means here: of the monitoring pair only the website monitor is an
actual background watcher (system metrics are a live readout with no loop
behind them), and deployments is read-only visibility, not deployment
control. Also still outstanding under M4: automatic DB backups
(pre-migration + daily rotating), listed in [database.md](database.md) and
[security.md](security.md) and not yet implemented.

### M5 — Extensibility & polish
- WASM plugin runtime (Extism-style) + contribution manifests, plugin SDK,
  marketplace scaffold, docs/wiki module, snippets, theme system

## Complexity legend
S ≈ a day · M ≈ days · L ≈ 1–2 weeks · XL ≈ several weeks (single developer,
AI-assisted).

## Sequencing rationale
Terminal + Git shipped first because they anchor daily use; AI chat rode
alongside since provider plumbing is independent. Tool calling followed
immediately because it's the highest-leverage AI feature and the security
model (explicit grant, read-only, path-guarded) needed to be right before
anything more capable (file edits, command execution) is added. Everything
later builds on seams that already exist (modules, jobs, events, secrets).
