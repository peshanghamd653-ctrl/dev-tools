# DevOS

A desktop developer operating center — one fast, keyboard-driven app for
projects, terminal, git, AI, and (over time) everything else in the daily
development loop. Built with Tauri v2 + Rust and React 19.

## Status

**M0 — Foundation.** Working app shell (sidebar, command palette, workspaces,
projects, settings) on a modular Rust kernel with a typed IPC boundary.
See [docs/02-roadmap.md](docs/02-roadmap.md) for what lands next.

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

The full design suite lives in [docs/](docs/): architecture, roadmap,
wireframes, folder structure, database schema, IPC contracts, plugin API,
AI architecture, security model, and quality/operations strategy.
