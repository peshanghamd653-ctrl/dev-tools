# DevOS

A desktop developer operating center — one fast, keyboard-driven app for
projects, terminal, git, AI, and (over time) everything else in the daily
development loop. Built with Tauri v2 + Rust and React 19.

## Status

**M0 + M1 complete, M2 in progress.** App shell, terminal, git, and an AI
assistant with Claude/Ollama streaming and tool calling, on a modular Rust
kernel with a typed IPC boundary. See
[docs/feature-roadmap.md](docs/feature-roadmap.md) for what's shipped and
what's next.

## Development

Prerequisites: Rust (stable, MSVC on Windows), Node 20+, pnpm.

```sh
pnpm install
pnpm tauri dev      # run the desktop app
pnpm test           # frontend unit tests
cargo test          # kernel tests (also regenerates IPC bindings)
pnpm lint && pnpm typecheck
```

Key shortcuts: `Ctrl+K` command palette · `Ctrl+B` toggle sidebar ·
`Ctrl+1/2` navigate · `Ctrl+,` settings.

## Documentation

The project knowledge base lives in [docs/](docs/):
[vision](docs/vision.md) · [architecture](docs/architecture.md) ·
[tech stack](docs/tech-stack.md) · [coding guidelines](docs/coding-guidelines.md) ·
[feature roadmap](docs/feature-roadmap.md) · [design system](docs/design-system.md) ·
[database](docs/database.md) · [IPC contracts](docs/ipc-contracts.md) ·
[plugin API](docs/plugin-api.md) · [security](docs/security.md) ·
[performance](docs/performance.md) · [AI & agents](docs/agents.md) ·
[release process](docs/release-process.md) · [testing](docs/testing.md) ·
[architecture decisions](docs/adr/).
