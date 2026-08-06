# Testing Strategy

## What's covered today

| Crate / area | Test count | What's exercised |
|---|---|---|
| `devos-kernel` | 32 | workspace/project CRUD + invariants (last-workspace delete refused, duplicate project path rejected), settings upsert, event bus delivery, job success/failure persistence, notification persistence + unread counts, **backups capturing a just-committed row where a naive `fs::copy` loses it**, daily rotation + retention, boot surviving an unwritable backup dir, migrations applying exactly once across boots, and module registration doing no per-module DB work — all against a real `tempfile` SQLite DB |
| `devos-terminal` | 10 | real ConPTY spawn → write → read → exit round trip (Windows-only, `#[cfg(all(test, windows))]`), resize, missing-session error, OSC 133 scanning across chunk boundaries, tail ring buffer, ANSI stripping |
| `devos-git` | 11 | porcelain-v2 parsing (branch headers, ordinary/rename/untracked entries, paths with spaces) + a real `git init`→stage→commit→log integration workflow, branch switch/create, discard (tracked + untracked), staged-diff truncation |
| `devos-secrets` | 4 | set/get roundtrip + overwrite, list never exposes values (compile-time, no value field), wrong-key decrypt fails, delete |
| `devos-ai` | 12 | Claude SSE parsing (text deltas, tool_use accumulation across split JSON, interleaved text+tools, empty-input default), Ollama NDJSON parsing, conversation lifecycle + auto-titling, memory add/list/cap |
| `devos-index` | 39 | FTS5 indexing over a real temp tree, **not descending into junctions that leave the project** (verified failing before the fix: it indexed 3 files instead of 2, with the outside file searchable), incremental reindex by mtime/size, skip-dir handling, query sanitization, bm25-ranked search hits, cosine similarity (identical/orthogonal/degenerate → `0.0`, never `NaN`), vector BLOB roundtrip, reciprocal-rank fusion of two known rankings, **search staying correct when the embeddings backend is entirely unavailable**, unchanged files never re-embedded, and a later run backfilling what a dead backend missed |
| `devos-docker` | 5 | container/image mapping from Engine API shapes, port formatting, `Unavailable` vs `Api` error classification (so the UI can degrade rather than error) |
| `devos-api` | 10 | full request round trip against a **real one-shot local TCP server** (asserting both the parsed response and the raw bytes the server received), invalid method/URL, connection-refused, saved-request CRUD, history recording + pruning to 100 |
| `devos-db` | 25 | statement classification past comments/case, **a smuggled `; PRAGMA query_only(0); UPDATE …` failing to disable the read guard** (the SEC-001 regression), a read-only connection refusing writes outright, separator detection inside literals/identifiers/comments, setter-vs-introspection PRAGMA classification, write blocked without consent and landing with it, every SQLite value type incl. NULL → `None`, 500-row cap + `truncated`, identifier quoting, schema introspection (PK/NOT NULL/DEFAULT, views, broken view → `-1`), pool cache + refusal to create a missing file |
| `devos-system` | 6 | snapshot reports plausible hardware (cores/memory/uptime; disks deliberately *not* asserted non-empty), top processes capped and CPU-ordered, and **CPU usage is not stuck at zero across two snapshots** — the failure mode a fresh probe per call would produce |
| `devos-monitor` | 24 | monitor CRUD, blank name and **non-http scheme rejection** (`file://`, `ftp://`, `javascript:`, `data:`), interval floor clamping, real HTTP checks against a hermetic one-shot TCP server (200 / 500 / connection-refused), **alerts fire only on state transitions** (ok→fail and fail→ok notify; same-state pairs stay silent), first-check-fails-alerts semantics, uptime/latency aggregation incl. the zero-check case, pruning, and deleting a monitor taking its history with it |
| `devos-deploy` | 16 | Vercel project/deployment parsing against a hermetic one-shot server, the `uid`→`id` / `created`→`createdAt` / `meta.githubCommitMessage`→`commitMessage` field mapping, **a deployment with no `meta` yielding `null` rather than dropping the row**, `Authorization: Bearer` asserted on the raw bytes the server received, 401/403 → `Auth` (distinct from a generic API error), body truncation, and a blank token making no request at all |
| `devos-issue` | 37 | git-remote parsing across every URL form GitHub emits (https/ssh/scp/`git://`, userinfo including the `x-access-token:<pat>@` form CI clones leave behind, port segments, `.git` and trailing slashes, case-folded dedupe across fetch/push lines) with **non-GitHub hosts rejected by exact match** so Enterprise/GitLab/Bitbucket never produce a bogus target; issue creation asserted on the raw bytes received (bearer token, User-Agent, pinned API version, JSON body); 401/403 → `Auth`, **404 → `NotFound` distinct from a generic API error**, 422 preserved, junk-201 → `Decode`; blank/unsafe inputs rejected against a closed port so no-network is positive proof; screenshot pruning to the newest 20 |
| `src-tauri` | 29 | AI tools read/list real files, **path-traversal and absolute-path rejection**, dependency-dir skipping in search, unknown-tool error, approval-gate resolve/deny/timeout, **every tool in `MUTATING_TOOLS` refused on denial** (asserting nothing landed — no edit, no created file, no `run_command` marker, no memory row) with a length check so adding an ungated mutating tool fails the test, `save_memory` persisting only after approval and with the exact text shown, and `write_file` refusing to write through a real dangling junction |
| Frontend (`src/**/*.test.{ts,tsx}`) | 274 | Zustand UI store, `cn()` merging, unified-diff parser, uptime/percent formatting, the **shared byte formatter** (`shared/lib/format.ts`) covering the GB/TB range the old per-page copies truncated, **write-blocked classification** pinned to the exact strings `DbError::WriteBlocked` and SQLite emit, monitor state derivation + sparkline ordering, deploy state/URL/error classification, secret-name handling — plus **component tests** for six pages: the SQL write toggle defaulting to read-only, keeping one accessible name in both states, and its blocked-write card, Docker's "daemon off" state kept distinct from an ordinary API error, all four monitor status chips, request-spec construction, out-of-shell rendering, and lazy directory loading asking for the full nested path |

Rust totals **260** via `cargo test --workspace`. Per-crate counts include the
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

A WebdriverIO + `tauri-driver` smoke suite in `e2e/`: 7 tests covering boot,
isolation and navigation. `@wdio/tauri-service` matches `msedgedriver` to the
installed WebView2 runtime automatically. It runs against the **external**
driver provider, so nothing test-only is compiled into the shipping binary.

The load-bearing assertion checks `window.__TAURI_INTERNALS__` exists *and*
that the version stat is populated from the `app_info` IPC command — proof the
Rust kernel answered, which no unit test can reach.

**Each run boots a throwaway database.** `wdio.conf.js` points
`DEVOS_DATA_DIR` (see `src-tauri/src/lib.rs`) at a fresh directory under the
OS temp dir before anything can start the app, so the suite exercises the
genuine first-run path and never opens
`%APPDATA%\com.peshang.devos\devos.db`. That is asserted, not assumed: one
test checks a `devos.db` really appeared under the temp directory *and* that
the running app reports first-run state (exactly one workspace, no projects).
The directory is removed in `onComplete`; runs killed before that get swept by
the next run. `DEVOS_E2E_KEEP_DATA=1` keeps it for inspection.

Limits, stated rather than implied: it runs the **debug** binary against the
Vite dev server, so bundling, `frontendDist` and the production CSP are
unverified; only the *database* is isolated — the WebView2 profile (and so
`localStorage`) and the OS keystore entry holding the secrets master key are
the machine's real ones; and it is Windows-only (`tauri-driver` does not
support macOS).

## Known gaps (honest, not yet closed)

- **No live-API test for Claude/Ollama.** The SSE/NDJSON parsers are tested
  against real captured frame shapes, but nothing in CI calls a real
  provider (no key available in CI, and it would be flaky/costly). Manual
  verification happens by running the app.
- **The e2e CI job has never run on a GitHub runner.** The old blocker —
  the app hardcoding its database to `app_data_dir()/devos.db`, leaving a CI
  run nowhere isolated to boot — is gone: `DEVOS_DATA_DIR` overrides that
  root, and `.github/workflows/ci.yml` now has an `e2e` job
  (`windows-latest`, Rust toolchain + `Swatinem/rust-cache`, a pinned
  `tauri-driver`, a debug `cargo build -p devos-desktop`, then `pnpm e2e`).
  It runs on **pull requests and `workflow_dispatch` only** — not on push,
  because every commit reaching `main` already passed it on its PR and a
  duplicate ~20-minute Windows build bills at 2x; not on the weekly schedule,
  which exists for advisories against unchanged lockfiles and would buy
  nothing here. What is unproven is the runner itself: WebView2 is present on
  the image and `msedgedriver` is downloaded to match, but no run has happened
  yet, and the app calls the OS keystore at startup for the secrets master
  key — plausible on a GitHub Windows runner, unverified. **Keep it out of
  branch protection until it has a green streak**; a required check that
  flakes gets un-required and then ignored, which is worse than this gap. The
  suite itself passes locally on repeated consecutive runs.
  See [release-process.md](release-process.md).
- **CI does not assert the ts-rs bindings are committed.** `cargo test
  --workspace` regenerates `src/shared/ipc/bindings/*.ts` as a side effect,
  so a stale committed binding passes CI silently. A
  `git diff --exit-code` check on that directory would close it.
- **Startup timing is a tripwire, not a regression test.** Boot is measured
  and asserted, but only against a loose ceiling: a single cold boot on this
  machine spans ~20x (69 ms quiet → 1375 ms under load), so no wall-clock
  threshold can catch a 10x regression without flaking. The load-bearing
  assertions are structural instead — migrations apply exactly once across
  boots, and module registration does no per-module DB work. See
  [performance.md](performance.md).
- **The write-blocked card still depends on the backend's error *wording*.**
  `isWriteBlocked` (`src/features/db/errors.ts`) is now one exported,
  directly-tested function with its matched strings as named constants and a
  comment naming `DbError::WriteBlocked` and the Rust file it lives in — but
  it is still sniffing a string, because Tauri commands flatten typed errors
  with `.map_err(|e| e.to_string())`. The real fix is a discriminant that
  survives the IPC boundary, which is a backend change.
