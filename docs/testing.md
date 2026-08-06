# Testing Strategy

## What's covered today

| Crate / area | Test count | What's exercised |
|---|---|---|
| `devos-kernel` | 32 | workspace/project CRUD + invariants (last-workspace delete refused, duplicate project path rejected), settings upsert, event bus delivery, job success/failure persistence, notification persistence + unread counts, **backups capturing a just-committed row where a naive `fs::copy` loses it**, daily rotation + retention, boot surviving an unwritable backup dir, migrations applying exactly once across boots, and module registration doing no per-module DB work — all against a real `tempfile` SQLite DB |
| `devos-terminal` | 10 | real ConPTY spawn → write → read → exit round trip (Windows-only, `#[cfg(all(test, windows))]`), resize, missing-session error, OSC 133 scanning across chunk boundaries, tail ring buffer, ANSI stripping |
| `devos-git` | 11 | porcelain-v2 parsing (branch headers, ordinary/rename/untracked entries, paths with spaces) + a real `git init`→stage→commit→log integration workflow, branch switch/create, discard (tracked + untracked), staged-diff truncation |
| `devos-secrets` | 4 | set/get roundtrip + overwrite, list never exposes values (compile-time, no value field), wrong-key decrypt fails, delete |
| `devos-ai` | 12 | Claude SSE parsing (text deltas, tool_use accumulation across split JSON, interleaved text+tools, empty-input default), Ollama NDJSON parsing, conversation lifecycle + auto-titling, memory add/list/cap |
| `devos-index` | 21 | FTS5 indexing over a real temp tree, incremental reindex by mtime/size, skip-dir handling, query sanitization, bm25-ranked search hits, cosine similarity (identical/orthogonal/degenerate → `0.0`, never `NaN`), vector BLOB roundtrip, reciprocal-rank fusion of two known rankings, **search staying correct when the embeddings backend is entirely unavailable**, unchanged files never re-embedded, and a later run backfilling what a dead backend missed |
| `devos-docker` | 5 | container/image mapping from Engine API shapes, port formatting, `Unavailable` vs `Api` error classification (so the UI can degrade rather than error) |
| `devos-api` | 10 | full request round trip against a **real one-shot local TCP server** (asserting both the parsed response and the raw bytes the server received), invalid method/URL, connection-refused, saved-request CRUD, history recording + pruning to 100 |
| `devos-db` | 25 | statement classification past comments/case, **a smuggled `; PRAGMA query_only(0); UPDATE …` failing to disable the read guard** (the SEC-001 regression), a read-only connection refusing writes outright, separator detection inside literals/identifiers/comments, setter-vs-introspection PRAGMA classification, write blocked without consent and landing with it, every SQLite value type incl. NULL → `None`, 500-row cap + `truncated`, identifier quoting, schema introspection (PK/NOT NULL/DEFAULT, views, broken view → `-1`), pool cache + refusal to create a missing file |
| `devos-system` | 6 | snapshot reports plausible hardware (cores/memory/uptime; disks deliberately *not* asserted non-empty), top processes capped and CPU-ordered, and **CPU usage is not stuck at zero across two snapshots** — the failure mode a fresh probe per call would produce |
| `devos-monitor` | 24 | monitor CRUD, blank name and **non-http scheme rejection** (`file://`, `ftp://`, `javascript:`, `data:`), interval floor clamping, real HTTP checks against a hermetic one-shot TCP server (200 / 500 / connection-refused), **alerts fire only on state transitions** (ok→fail and fail→ok notify; same-state pairs stay silent), first-check-fails-alerts semantics, uptime/latency aggregation incl. the zero-check case, pruning, and deleting a monitor taking its history with it |
| `devos-deploy` | 16 | Vercel project/deployment parsing against a hermetic one-shot server, the `uid`→`id` / `created`→`createdAt` / `meta.githubCommitMessage`→`commitMessage` field mapping, **a deployment with no `meta` yielding `null` rather than dropping the row**, `Authorization: Bearer` asserted on the raw bytes the server received, 401/403 → `Auth` (distinct from a generic API error), body truncation, and a blank token making no request at all |
| `src-tauri` | 14 | AI tools read/list real files, **path-traversal and absolute-path rejection**, dependency-dir skipping in search, unknown-tool error, approval-gate resolve/deny/timeout |
| Frontend (`src/**/*.test.{ts,tsx}`) | 138 | Zustand UI store, `cn()` merging, unified-diff parser, byte/uptime/percent formatting, monitor state derivation + sparkline ordering, deploy state/URL/error classification, secret-name handling — plus **component tests** for six pages: the SQL write toggle defaulting to read-only and its blocked-write card, Docker's "daemon off" state kept distinct from an ordinary API error, all four monitor status chips, request-spec construction, out-of-shell rendering, and lazy directory loading asking for the full nested path |

Rust totals **190** via `cargo test --workspace`. Per-crate counts include the
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

## End-to-end (`pnpm e2e`)

A WebdriverIO + `tauri-driver` smoke suite in `e2e/`: 6 tests covering boot
and navigation. `@wdio/tauri-service` matches `msedgedriver` to the installed
WebView2 runtime automatically. It runs against the **external** driver
provider, so nothing test-only is compiled into the shipping binary.

The load-bearing assertion checks `window.__TAURI_INTERNALS__` exists *and*
that the version stat is populated from the `app_info` IPC command — proof the
Rust kernel answered, which no unit test can reach.

Limits, stated rather than implied: it runs the **debug** binary against the
Vite dev server, so bundling, `frontendDist` and the production CSP are
unverified; it boots the real `%APPDATA%` database rather than a temp one, so
it is not hermetic; it is Windows-only (`tauri-driver` does not support
macOS); and it is not wired into CI.

## Known gaps (honest, not yet closed)

- **No live-API test for Claude/Ollama.** The SSE/NDJSON parsers are tested
  against real captured frame shapes, but nothing in CI calls a real
  provider (no key available in CI, and it would be flaky/costly). Manual
  verification happens by running the app.
- **No CI at all**, so none of the above runs automatically — including
  `pnpm audit` and the e2e suite. Everything here is a local gate today.
- **Startup timing is a tripwire, not a regression test.** Boot is measured
  and asserted, but only against a loose ceiling: a single cold boot on this
  machine spans ~20x (69 ms quiet → 1375 ms under load), so no wall-clock
  threshold can catch a 10x regression without flaking. The load-bearing
  assertions are structural instead — migrations apply exactly once across
  boots, and module registration does no per-module DB work. See
  [performance.md](performance.md).
- **Testability debt found while writing the component tests**, worth fixing
  before it spreads: `inDesktopShell` is a module-level `const` snapshotted
  at import, so driving both paths needs a module mock; `formatSize` is
  duplicated verbatim in three pages and diverges from `system/format.ts`
  (it caps at MB, so a 500 GB disk renders as "512000.0 MB"); the SQL write
  toggle has no stable accessible name, only its own changing label; and
  `isWriteBlocked` classifies by string-sniffing the backend's error text,
  so rewording `DbError::WriteBlocked` silently downgrades the explanatory
  card to a raw dump.
