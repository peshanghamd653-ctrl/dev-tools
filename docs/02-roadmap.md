# Roadmap, Phases & Complexity

Honest framing: the full DevOS vision is a multi-year product. Development is
organized in vertical milestones — each one ships something used daily, never
a broad shell of empty screens. Planned modules appear in the sidebar as
disabled entries with their milestone tag, so the roadmap is visible in-app.

## Milestones

### M0 — Foundation ✅ (this repository state)
- Rust + pnpm + cargo workspace, Tauri v2 boots
- `devos-kernel`: module registry, command registry, event bus, durable jobs,
  SQLx (WAL) + migrations, tested
- App shell: sidebar, workspace switcher, command palette (Ctrl+K), keyboard
  shortcuts, dark-first theme, dashboard, projects, settings
- Typed IPC with generated bindings; kernel events bridged to the UI
- CI: fmt/clippy/tests, eslint/tsc/vitest/build
- Complexity: **M** · risk: low (done)

### M1 — Daily-driver core ✅ (completed 2026-07-20; file explorer + clone/templates deferred to M1.5)
- **Terminal**: xterm.js + `portable-pty` module crate; multiple/split
  terminals, persistent sessions, streaming over IPC channels. (L)
- **Git**: status/stage/commit/branch/history/diff viewer via `git` CLI
  porcelain v2; module crate + feature UI. (L)
- **Projects+**: clone repository, project templates. (M)
- **AI assistant (chat)**: `devos-ai` crate with `AiProvider` trait; Claude API
  + Ollama first (user-confirmed priority), streaming into the chat UI;
  keys in the encrypted secret store. (L)
- **File explorer**: tree + fast search + preview. (M)

### M2 — AI depth
- Project indexing: tree-sitter symbols + chunk embeddings in SQLite
  (`sqlite-vec`); Ollama or API embeddings. (L)
- RAG answers with citations; tool calling (read/edit files, run commands)
  gated by an approval UI. (L)
- Commit-message / PR-description generation. (S)
- Long-term memory per project; first background agent (build-failure watcher). (M)

### M3 — Ops tools
- Docker module via `bollard` (containers, images, logs, compose). (L)
- API client: REST/GraphQL/WS, collections, environments. (L)
- Database manager: SQLite/Postgres/MySQL; schema explorer, SQL editor. (XL)
- Secret manager UI on the M0 encryption foundation. (M)

### M4 — Watchers & deploys
- System monitoring (`sysinfo`), notification center, website monitor,
  screenshot → GitHub issue, deployments (Vercel first). (L overall)

### M5 — Extensibility & polish
- WASM plugin runtime (Extism-style) + contribution manifests, plugin SDK,
  marketplace scaffold, docs/wiki module, snippets, theme system. (XL)

## Complexity legend
S ≈ a day · M ≈ days · L ≈ 1–2 weeks · XL ≈ several weeks (single developer,
AI-assisted).

## Sequencing rationale
Terminal + Git first (confirmed priority) because they anchor daily use; AI
chat lands in the same milestone since provider plumbing is independent of
the git/terminal work. Everything later builds on seams that already exist in
M0 (modules, jobs, events, secrets).
