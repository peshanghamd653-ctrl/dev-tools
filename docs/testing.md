# Testing Strategy

## What's covered today

| Crate / area | Test count | What's exercised |
|---|---|---|
| `devos-kernel` | 19 | workspace/project CRUD + invariants (last-workspace delete refused, duplicate project path rejected), settings upsert, event bus delivery, job success/failure persistence, notification persistence + unread counts — all against a real `tempfile` SQLite DB |
| `devos-terminal` | 10 | real ConPTY spawn → write → read → exit round trip (Windows-only, `#[cfg(all(test, windows))]`), resize, missing-session error, OSC 133 scanning across chunk boundaries, tail ring buffer, ANSI stripping |
| `devos-git` | 11 | porcelain-v2 parsing (branch headers, ordinary/rename/untracked entries, paths with spaces) + a real `git init`→stage→commit→log integration workflow, branch switch/create, discard (tracked + untracked), staged-diff truncation |
| `devos-secrets` | 4 | set/get roundtrip + overwrite, list never exposes values (compile-time, no value field), wrong-key decrypt fails, delete |
| `devos-ai` | 12 | Claude SSE parsing (text deltas, tool_use accumulation across split JSON, interleaved text+tools, empty-input default), Ollama NDJSON parsing, conversation lifecycle + auto-titling, memory add/list/cap |
| `devos-index` | 6 | FTS5 indexing over a real temp tree, incremental reindex by mtime/size, skip-dir handling, query sanitization, bm25-ranked search hits |
| `devos-docker` | 5 | container/image mapping from Engine API shapes, port formatting, `Unavailable` vs `Api` error classification (so the UI can degrade rather than error) |
| `devos-api` | 10 | full request round trip against a **real one-shot local TCP server** (asserting both the parsed response and the raw bytes the server received), invalid method/URL, connection-refused, saved-request CRUD, history recording + pruning to 100 |
| `devos-db` | 21 | statement classification past comments/case, **`PRAGMA query_only` stopping a write disguised as a read** (`WITH … INSERT`), write blocked without consent and landing with it, every SQLite value type incl. NULL → `None`, 500-row cap + `truncated`, identifier quoting, schema introspection (PK/NOT NULL/DEFAULT, views, broken view → `-1`), pool cache + refusal to create a missing file |
| `src-tauri` | 14 | AI tools read/list real files, **path-traversal and absolute-path rejection**, dependency-dir skipping in search, unknown-tool error, approval-gate resolve/deny/timeout |
| Frontend (`src/**/*.test.ts`) | 9 | Zustand UI store (palette toggle, tab dedup/close, active workspace), `cn()` class merging, unified-diff parser (hunk/add/del/ctx classification, line numbering, synthesized untracked-file diffs) |

Rust totals **112** via `cargo test --workspace`. Per-crate counts include the
generated `export_bindings_*` tests ts-rs emits for each exported DTO — those
assert the TypeScript bindings are current, not runtime behavior, so the
behavioral count per crate is lower than the number above.

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
