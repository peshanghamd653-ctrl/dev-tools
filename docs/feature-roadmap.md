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

Deferred to M1.5: project templates, repository cloning, file explorer.

### M2 — AI depth (in progress)
- ✅ AI commit-message generation (staged diff → conventional-commit message)
- ✅ Project-aware chat (git status/branch injected as system context, with
  a UI chip to attach/detach)
- ✅ **Tool calling**: Claude agentic loop (`run_agent`) with a read-only
  tool set (`read_file`, `list_dir`, `find_files`), a canonicalize-and-contain
  path guard against traversal, and an explicit per-conversation grant the
  user must turn on — off by default. Live tool activity shown in the chat.
- Planned: `edit_file`/`run_command` tools with per-call approval dialogs
  (not just a standing grant); project indexing + RAG (tree-sitter symbols +
  `sqlite-vec` embeddings); long-term memory; first background agent
  (build-failure watcher subscribing to terminal/job events).

### M3 — Ops tools
- Docker module via `bollard` (containers, images, logs, compose)
- API client: REST/GraphQL/WS, collections, environments
- Database manager: SQLite/Postgres/MySQL; schema explorer, SQL editor
- Secret manager UI (the store exists; a dedicated management screen doesn't yet)

### M4 — Watchers & deploys
- System monitoring (`sysinfo`), notification center, website monitor,
  screenshot → GitHub issue, deployments (Vercel first)

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
