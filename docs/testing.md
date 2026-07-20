# Testing Strategy

## What's covered today

| Crate / area | Test count | What's exercised |
|---|---|---|
| `devos-kernel` | 14 | workspace/project CRUD + invariants (last-workspace delete refused, duplicate project path rejected), settings upsert, event bus delivery, job success/failure persistence — all against a real `tempfile` SQLite DB |
| `devos-terminal` | 4 | real ConPTY spawn → write → read → exit round trip (Windows-only, `#[cfg(all(test, windows))]`), resize, missing-session error |
| `devos-git` | 11 | porcelain-v2 parsing (branch headers, ordinary/rename/untracked entries, paths with spaces) + a real `git init`→stage→commit→log integration workflow, branch switch/create, discard (tracked + untracked), staged-diff truncation |
| `devos-secrets` | 4 | set/get roundtrip + overwrite, list never exposes values (compile-time, no value field), wrong-key decrypt fails, delete |
| `devos-ai` | 10 | Claude SSE parsing (text deltas, tool_use accumulation across split JSON, interleaved text+tools, empty-input default), Ollama NDJSON parsing, conversation lifecycle + auto-titling |
| `src-tauri` (`tools.rs`) | 4 | reads/lists real files, **path-traversal and absolute-path rejection**, dependency-dir skipping in search, unknown-tool error |
| Frontend (`src/**/*.test.ts`) | 9 | Zustand UI store (palette toggle, tab dedup/close, active workspace), `cn()` class merging, unified-diff parser (hunk/add/del/ctx classification, line numbering, synthesized untracked-file diffs) |

Run everything: `cargo test --workspace` (Rust) and `pnpm test` (frontend,
Vitest + jsdom). `cargo fmt --all --check` and `cargo clippy --workspace
--all-targets -- -D warnings` gate style and lint; `pnpm lint` (ESLint
strict) and `pnpm typecheck` (`tsc --noEmit`) do the same for TypeScript.

## Conventions

- **No mocked databases.** Every Rust test that touches SQLite uses a real
  file in a `tempfile::tempdir()`. This caught a real bug during
  development (`git restore --staged` failing in a repo with no commits
  yet) that a mock would have hidden.
- **No mocked subprocesses where a real one is feasible.** The terminal
  test spawns a real `cmd.exe` through ConPTY and asserts on real output,
  including answering the ConPTY cursor-position probe the way xterm.js
  does in production — a mock pty would have hidden that entirely.
- **Security-relevant behavior gets a test that would fail if the guard
  were removed** — not just a test of the happy path. The tool-calling
  path-traversal test and the secrets wrong-key test are both written this
  way.
- **Tests live next to the code they test** (`#[cfg(test)] mod tests` in
  the same file for Rust; `Foo.test.ts` beside `Foo.ts` for TypeScript) —
  no separate test-tree to keep in sync.

## Known gaps (honest, not yet closed)

- **No component tests** for React pages yet (Testing Library is installed
  and configured, unused so far). Planned once page structure stabilizes
  enough that tests won't be rewritten on every UI iteration.
- **No end-to-end tests.** A `tauri-driver`-based smoke test (boot → add
  project → palette nav → basic terminal/git/AI interaction) is planned for
  M1's close-out but not yet written.
- **No live-API test for Claude/Ollama.** The SSE/NDJSON parsers are tested
  against real captured frame shapes, but nothing in CI calls a real
  provider (no key available in CI, and it would be flaky/costly). Manual
  verification happens by running the app.
- **No performance regression tests.** Startup time is logged and eyeballed
  per session, not asserted in CI. See [performance.md](performance.md).
