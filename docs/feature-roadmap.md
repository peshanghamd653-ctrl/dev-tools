# Feature Roadmap

Honest framing: the full DevOS vision is a multi-year product. Development
is organized in vertical milestones — each one ships something used daily,
never a broad shell of empty screens. Planned modules appear in the sidebar
as disabled entries with their milestone tag, so the roadmap is visible
in-app.

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
- Planned: vector half of retrieval (tree-sitter symbols + `sqlite-vec`
  embeddings layered on the same tables).

### M3 — Ops tools
- Docker module via `bollard` (containers, images, logs, compose)
- API client: REST/GraphQL/WS, collections, environments
- Database manager: SQLite/Postgres/MySQL; schema explorer, SQL editor
- Secret manager UI (the store exists; a dedicated management screen doesn't yet)

- ✅ **Notification Center** (pulled forward from M4): `Kernel::notify`
  persists + broadcasts; failed jobs auto-notify; index completions notify;
  topbar bell with unread badge, level dots, mark-read/mark-all-read. This
  is the reporting surface background agents will use.

### M4 — Watchers & deploys
- System monitoring (`sysinfo`), website monitor, screenshot → GitHub
  issue, deployments (Vercel first)

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
