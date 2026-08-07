# Testing Strategy

## What's covered today

| Crate / area | Test count | What's exercised |
|---|---|---|
| `devos-kernel` | 62 | workspace/project CRUD + invariants (last-workspace delete refused, duplicate project path rejected), settings upsert, event bus delivery, job success/failure persistence, notification persistence + unread counts, **backups capturing a just-committed row where a naive `fs::copy` loses it**, daily rotation + retention, boot surviving an unwritable backup dir, migrations applying exactly once across boots, module registration doing no per-module DB work, and **restore**: a candidate refused when it is not SQLite / truncated / fails `integrity_check` / is some other app's database, the replaced database preserved before the swap, sidecars not surviving it, each of the four interrupted states resolving to either a complete restore or none — never half — and the **audit log**: each event landing with the right actor/action/detail, a denial recording *which* refusal it was, **a secret write recording the name and provably not the value** (every column of every row concatenated and searched), retention dropping exactly what is past the window at ±1ms of the boundary, and a failing audit insert not propagating to the action it records — all against a real `tempfile` SQLite DB |
| `devos-terminal` | 10 | real ConPTY spawn → write → read → exit round trip (Windows-only, `#[cfg(all(test, windows))]`), resize, missing-session error, OSC 133 scanning across chunk boundaries, tail ring buffer, ANSI stripping |
| `devos-git` | 11 | porcelain-v2 parsing (branch headers, ordinary/rename/untracked entries, paths with spaces) + a real `git init`→stage→commit→log integration workflow, branch switch/create, discard (tracked + untracked), staged-diff truncation |
| `devos-secrets` | 4 | set/get roundtrip + overwrite, list never exposes values (compile-time, no value field), wrong-key decrypt fails, delete |
| `devos-ai` | 19 | Claude SSE parsing (text deltas, tool_use accumulation across split JSON, interleaved text+tools, empty-input default), Ollama NDJSON parsing, conversation lifecycle + auto-titling, memory add/list/cap, and the Gemini adapter: **the assistant role renamed to `model`** (which the API accepts as-is while degrading the reply, so nothing else would catch it), the system prompt travelling as `systemInstruction`, chunks carrying no text (safety-only, finish-only, usage-only) yielding nothing rather than failing, a malformed frame not aborting a reply already arriving, 429 keeping its status, and a round trip asserting on the raw bytes received that **the key is in the `x-goog-api-key` header and not in the request line** |
| `devos-index` | 39 | FTS5 indexing over a real temp tree, **not descending into junctions that leave the project** (verified failing before the fix: it indexed 3 files instead of 2, with the outside file searchable), incremental reindex by mtime/size, skip-dir handling, query sanitization, bm25-ranked search hits, cosine similarity (identical/orthogonal/degenerate → `0.0`, never `NaN`), vector BLOB roundtrip, reciprocal-rank fusion of two known rankings, **search staying correct when the embeddings backend is entirely unavailable**, unchanged files never re-embedded, and a later run backfilling what a dead backend missed |
| `devos-docker` | 5 | container/image mapping from Engine API shapes, port formatting, `Unavailable` vs `Api` error classification (so the UI can degrade rather than error) |
| `devos-api` | 10 | full request round trip against a **real one-shot local TCP server** (asserting both the parsed response and the raw bytes the server received), invalid method/URL, connection-refused, saved-request CRUD, history recording + pruning to 100 |
| `devos-db` | 30 | statement classification past comments/case, **a smuggled `; PRAGMA query_only(0); UPDATE …` failing to disable the read guard** (the SEC-001 regression), a read-only connection refusing writes outright, separator detection inside literals/identifiers/comments, setter-vs-introspection PRAGMA classification, write blocked without consent and landing with it, every SQLite value type incl. NULL → `None`, 500-row cap + `truncated`, identifier quoting, schema introspection (PK/NOT NULL/DEFAULT, views, broken view → `-1`), pool cache + refusal to create a missing file, and the error->DTO mapping (one kind per variant, the serialized `{kind, message}` shape pinned, and **SQLite's own read-only refusal mapped to `writeBlocked` via its numeric result code** so no prose is load-bearing and the UI never depends on which guard caught the write) |
| `devos-system` | 6 | snapshot reports plausible hardware (cores/memory/uptime; disks deliberately *not* asserted non-empty), top processes capped and CPU-ordered, and **CPU usage is not stuck at zero across two snapshots** — the failure mode a fresh probe per call would produce |
| `devos-monitor` | 24 | monitor CRUD, blank name and **non-http scheme rejection** (`file://`, `ftp://`, `javascript:`, `data:`), interval floor clamping, real HTTP checks against a hermetic one-shot TCP server (200 / 500 / connection-refused), **alerts fire only on state transitions** (ok→fail and fail→ok notify; same-state pairs stay silent), first-check-fails-alerts semantics, uptime/latency aggregation incl. the zero-check case, pruning, and deleting a monitor taking its history with it |
| `devos-deploy` | 16 | Vercel project/deployment parsing against a hermetic one-shot server, the `uid`→`id` / `created`→`createdAt` / `meta.githubCommitMessage`→`commitMessage` field mapping, **a deployment with no `meta` yielding `null` rather than dropping the row**, `Authorization: Bearer` asserted on the raw bytes the server received, 401/403 → `Auth` (distinct from a generic API error), body truncation, and a blank token making no request at all |
| `devos-issue` | 37 | git-remote parsing across every URL form GitHub emits (https/ssh/scp/`git://`, userinfo including the `x-access-token:<pat>@` form CI clones leave behind, port segments, `.git` and trailing slashes, case-folded dedupe across fetch/push lines) with **non-GitHub hosts rejected by exact match** so Enterprise/GitLab/Bitbucket never produce a bogus target; issue creation asserted on the raw bytes received (bearer token, User-Agent, pinned API version, JSON body); 401/403 → `Auth`, **404 → `NotFound` distinct from a generic API error**, 422 preserved, junk-201 → `Decode`; blank/unsafe inputs rejected against a closed port so no-network is positive proof; and screenshot pruning down to a **single** capture, with the retention test deriving its expectations from the constant so changing it changes what is asserted rather than passing vacuously |
| `devos-snippets` | 11 | snippet CRUD against a real temp SQLite file, insert-vs-update decided by the draft's id with **an edit not rewriting `created_at`**, blank titles refused on create *and* on edit, a deleted id refusing both a second delete and a resurrecting save, search matched per field (title/body/tag/language) case-insensitively and **inside a word** — the property FTS5 would not have — plus the negatives: a term in no snippet, two words that are not adjacent, and `%`/`_` treated as characters rather than wildcards |
| `devos-plugin` | 45 | WASM sandbox limits proven rather than assumed: fuel halting an infinite loop and resetting per call, a host-side `ResourceLimiter` capping linear memory using the *host's* ceiling rather than the module's declared maximum, an out-of-range guest `(ptr, len)` becoming an error instead of a host read, and — the structural one — a module importing `devos.http_fetch` **failing to instantiate** without the `net` permission, so the capability is absent rather than refused. A second review then broke two of those claims and the fixes are pinned here too: a 146-byte module can no longer commit ~976 MiB through unbounded *table* declarations before a single instruction runs, host calls now charge fuel for the work they are asked to do (a log-spam loop went from 1.6M calls reading 101.7 GiB in 52s to ~4,400 reading ~290 MB in milliseconds), and **every member of `NEEDS_APPROVAL` is proven refused by a deny-all gate** — the previous tests asserted the list's contents while the runtime never consulted it. Fixtures are checked-in `.wat` compiled at test time, so there is no opaque binary to review |
| `src-tauri` | 41 | AI tools read/list real files, **path-traversal and absolute-path rejection**, dependency-dir skipping in search, unknown-tool error, approval-gate resolve/deny/timeout, **every tool in `MUTATING_TOOLS` refused on denial** (asserting nothing landed — no edit, no created file, no `run_command` marker, no memory row) with a length check so adding an ungated mutating tool fails the test, `save_memory` persisting only after approval and with the exact text shown, `write_file` refusing to write through a real dangling junction, and the audit path: an approved mutating call landing a row while **read-only tools write none**, the row recording the action and not the payload, the gate reporting approval / refusal / silence as three different values rather than three different strings, and an unwritable audit log not breaking the call it was meant to record |
| Frontend (`src/**/*.test.{ts,tsx}`) | 362 | Zustand UI store, `cn()` merging, unified-diff parser, uptime/percent formatting, the **shared byte formatter** (`shared/lib/format.ts`) covering the GB/TB range the old per-page copies truncated, **write-blocked classification** read off the `DbErrorDto.kind` discriminant, with an unrecognised rejection deliberately *not* reaching the write-blocked card, monitor state derivation + sparkline ordering, deploy state/URL/error classification, secret-name handling, snippet tag normalisation and match-field derivation — plus **component tests** for seven pages: the SQL write toggle defaulting to read-only, keeping one accessible name in both states, and its blocked-write card, Docker's "daemon off" state kept distinct from an ordinary API error, all four monitor status chips, request-spec construction, out-of-shell rendering, lazy directory loading asking for the full nested path, and snippets copying the *body* to the clipboard while a refused clipboard says so instead of claiming success |

Rust totals **370** via `cargo test --workspace`. Per-crate counts include the
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

  The `if:` guard is confirmed working: both pushes to GitHub on 2026-08-08
  reported this job as `skipped`, which is exactly what a push run should do.
  That proves the trigger logic, not the job — everything above about the
  runner stays unverified until a pull request exists to run it on.
  See [release-process.md](release-process.md).
- **The bindings drift check now runs on GitHub, and passes.** The gap
  itself is closed: the `rust` job now diffs `src/shared/ipc/bindings/`
  straight after `cargo test --workspace` regenerates it, and fails with the
  diff plus the fix (`pnpm gen:types`, then commit) rather than a bare exit
  code. Two details make it work rather than annoy — `--intent-to-add`, since
  plain `git diff` exits 0 for a *new* binding that was never committed
  (the drift most worth catching), and `--ignore-cr-at-eol` with
  `core.autocrlf=input`, because with `autocrlf=false` and a CRLF working
  tree a plain diff reports every file as rewritten, i.e. a red build on
  every PR for a reason nobody can act on.

  This entry used to predict that the first real run would be red, on the
  theory that the committed bindings had probably already drifted. **That
  prediction was wrong.** The first push to GitHub (2026-08-08) ran the whole
  `rust` job green — fmt, clippy, tests and this check — so the CRLF and
  `--intent-to-add` handling that was only ever exercised against throwaway
  repos holds on a real runner too. Worth recording as evidence rather than
  quietly deleting: the local rehearsal was accurate, and the pessimism was
  not.
- **Rust advisory scanning is gated and green on GitHub.** The gap itself is
  closed: the `audit` job in
  `.github/workflows/ci.yml` covered npm only, so nothing checked the half of
  the graph holding the encryption, the secret store, the SQL engine and every
  network client. It now runs `cargo deny check advisories` against a
  `deny.toml` at the repo root, on PRs, pushes and the weekly schedule. Run the
  identical gate locally with `cargo deny check advisories -W unmaintained`
  (`cargo install cargo-deny --locked`, or a prebuilt binary — the compile is
  ~14 minutes). cargo-deny rather than cargo-audit because a lock file is a
  feature/target *union* and not a build: 731 crates are locked here, 46 crate
  names are never compiled on any target, and `cargo audit` reports 3
  vulnerabilities of which **zero** reach the Windows binary — `rsa 0.9.10`
  (RUSTSEC-2023-0071) sits behind `sqlx-mysql`, which the workspace never
  enables, and `quick-xml 0.30.0` (RUSTSEC-2026-0194/0195) behind `xcb`, the
  X11 backend of `xcap`. cargo-deny resolves the graph through `cargo metadata`
  and never sees any of them, so none of it needs an ignore entry; `deny.toml`
  is empty of ignores and explains the bar for adding one. Vulnerabilities and
  unsoundness fail; unmaintained crates warn, because today's five are `unic-*`
  crates arriving via `urlpattern` → `tauri-utils` with no upgrade available,
  and blocking on somebody else's roadmap is how a required check gets
  un-required. Everything the gate does not block on is still printed by the
  informational step into the job summary.

  This entry also used to predict a red first run, over RUSTSEC-2026-0221 (an
  unsoundness in `event-listener 5.4.1` reached through `sqlx-core`). That was
  true when written and was fixed before the repository was ever pushed —
  `cargo update -p event-listener` to the patched 5.4.2 — so the first real run
  was green.
- **License checking is gated and green on GitHub.** The same
  `audit` job now also runs `cargo deny check licenses` against a curated
  `[licenses] allow` list in `deny.toml`. This is an attribution gate, not a
  security one: it exists so a dependency arriving under a license nobody has
  looked at becomes a commit-time decision instead of a discovery made after
  installers are in the wild. It is not vacuous — removing a single entry from
  the allow list was verified to turn it red.

  Note it did not run on the *first* push: the npm audit step ahead of it
  failed, and every later step in the job was skipped. A job that goes red
  early reports nothing about the checks behind it, which is worth remembering
  before reading a failed run as evidence about anything but the first
  failure.
  **What it deliberately does not check:** whether
  [THIRD-PARTY-NOTICES.md](../THIRD-PARTY-NOTICES.md) is up to date. A new
  license failing the gate and stale notices are different failures, and only
  the first is mechanically detectable — regenerating with `pnpm gen:notices`
  is a contributor responsibility documented in CONTRIBUTING.md.
- **Orphan bindings are structurally uncatchable** by regenerate-and-diff:
  deleting a Rust DTO leaves its `.ts` file behind and nothing regenerates
  over it, so the check sees no difference. Only an inventory comparison
  would find those.
- **Startup timing is a tripwire, not a regression test.** Boot is measured
  and asserted, but only against a loose ceiling: a single cold boot on this
  machine spans ~20x (69 ms quiet → 1375 ms under load), so no wall-clock
  threshold can catch a 10x regression without flaking. The load-bearing
  assertions are structural instead — migrations apply exactly once across
  boots, and module registration does no per-module DB work. See
  [performance.md](performance.md).
- **An unrecognised database rejection loses its classification, by choice.**
  `db_*` commands return `Result<T, DbErrorDto>` and `isWriteBlocked`
  (`src/features/db/errors.ts`) switches on `kind`, so the coupling to the
  backend's *wording* is gone. What replaces it is a coupling to the *shape*:
  a rejection that isn't that object — a panic, a serialization failure, a
  packaged frontend older than the backend it boots against — renders as a
  generic error rather than the write-blocked card. That is the intended
  direction. Guessing from the message could only ever produce a false
  positive, and a card that offers to switch writes on for an unrelated
  failure is worse than a plain error showing the same text. Drift inside a
  single build fails `pnpm typecheck` rather than degrading silently
  (`KNOWN_KINDS` is a total `Record` over the generated union), so the only
  live window is a version-skewed frontend, and both branches are tested.
