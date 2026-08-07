# DevOS

A keyboard-driven desktop **developer operating center**: terminal, git, an AI
assistant with real tools, and ten more modules behind one command palette,
built on Tauri v2 (Rust + React 19). It is one app for the daily development
loop rather than a best-in-class replacement for any single tool it touches —
the sections below say exactly where each module stops.

> **Status: v0.1.0, Windows-only, and built by one person for one person.**
> The project's own definition of 1.0 is "the author's actual daily driver
> with no missing core-loop feature" — it is not there yet. Nothing here is
> supported software, and several modules are deliberately a thin slice of
> the tool they resemble.

## Windows only, today

DevOS runs on **Windows 10/11 x64 and nowhere else**. This is a scope
decision, not an architectural one — the kernel and every module crate are
OS-neutral Rust — but the OS-specific adapters only exist for Windows:

- Terminal sessions are ConPTY, and the terminal tests only compile under
  `#[cfg(all(test, windows))]`.
- The automatic build-failure watcher injects an OSC 133 prompt hook into
  **PowerShell** specifically.
- Path containment guards handle Windows reparse points (junctions), which is
  a different check from POSIX symlinks.
- The dependency advisory gate (`deny.toml`) resolves only the
  `x86_64-pc-windows-msvc` target.
- The end-to-end suite uses `tauri-driver`, which has no macOS support.

macOS and Linux are additive work — keyring backend, pty backend, capture
backend — not a rewrite. They are not started, not tested, and not scheduled.

## What's in it

Thirteen modules. What each one does, and where it stops:

| Module | What works | What it does not do |
|---|---|---|
| **Terminal** (`Ctrl+3`) | ConPTY sessions that live in the Rust process and survive route changes; tabs, split view, exit detection; a 32 KB output ring buffer feeding a one-click "diagnose this failure" hand-off to the AI chat; non-zero exits raise a notification | The prompt hook that detects those exits is PowerShell-only. Other shells get the terminal, not the watcher |
| **Git** (`Ctrl+4`) | status / stage / unstage / discard / commit / log / branches / switch + create / diff / push / pull, through the `git` CLI's porcelain v2 output ([ADR-0001](docs/adr/0001-shell-out-to-git-cli.md)) | No merge, rebase, stash, tags, remote management, or conflict resolution. Push and pull use whatever credentials your system `git` already has |
| **AI assistant** (`Ctrl+5`) | Streaming chat against Claude, Gemini, or a local Ollama; persisted conversations; project-aware context; commit-message generation from the staged diff; long-term per-project memory; hybrid code search (BM25 + embeddings + tree-sitter symbols, fused by reciprocal rank) | **Tool calling is Claude-only** — Gemini and Ollama stream plain chat, and the desktop layer gates tools on the provider rather than silently dropping the grant. Embeddings need Ollama running; without it, search degrades to lexical instead of failing |
| **Files** (`Ctrl+6`) | Lazy directory tree, mono preview with line numbers, filename search, full-text content search — every path through the shared containment guard | **Read-only.** There is no create, rename, move, or delete |
| **Docker** (`Ctrl+7`) | Containers (state, ports, start / stop / restart, last-200-line logs) and images over the Engine API named pipe; a real "Docker isn't running" state with reconnect polling | No volumes, compose, live stats, or image pull/remove |
| **API client** (`Ctrl+8`) | REST requests with a header and body editor, response viewer (status, timing, size, pretty JSON, headers), saved requests grouped into collections, automatic history capped at 100 | **No environments and no variables** — every request is literal. No GraphQL, WebSockets, auth helpers, or code generation |
| **Database** (`Ctrl+9`) | **SQLite only.** Named connections, schema explorer (tables, views, columns, file size), SQL editor, read-only result grid capped at 500 rows with an explicit truncation flag. Writes are refused unless a toggle that starts **off** is turned on | No Postgres or MySQL — the `driver` column exists, the sqlx driver features are deliberately not enabled ([ADR-0007](docs/adr/0007-sqlite-only-database-manager-first.md)). No query history, saved queries, ER diagrams, export, or row editing |
| **System metrics** (dashboard) | CPU and cores, memory and swap, uptime, per-disk capacity, top processes by CPU | A live readout with no loop behind it. **Nothing is persisted**, so there is no history, no charting, and no alerting on a threshold |
| **Monitors** (`Ctrl+0`) | Named HTTP uptime checks with per-monitor intervals, 24h uptime percentage and average response time, the newest 30 checks, manual re-check, notifications on ok↔fail transitions | **Only runs while DevOS is open** ([ADR-0008](docs/adr/0008-in-process-watchers-notify-on-transitions.md)). Checks that a site answers, not that it answers *correctly*. Alerts land in the in-app Notification Center only — no email, webhook, or Slack |
| **Deployments** (`Ctrl+Shift+D`) | **Read-only Vercel visibility**: projects and their recent deployments (state, target, URL, commit message, timestamp) | No triggering, promoting, rolling back, or deleting — deliberately ([ADR-0009](docs/adr/0009-deployments-read-only-no-write-actions.md)). Vercel only; no Netlify, Fly, Railway, or Cloudflare |
| **Screenshot → GitHub issue** (`Ctrl+Shift+S`) | Capture the primary monitor, annotate and redact, review and edit the exact generated issue body, file it via the GitHub REST API | **Does not attach the image.** GitHub documents no API for that, so the image is handed off through the clipboard for you to paste — one manual step, by choice |
| **Snippets** (`Ctrl+Shift+N`) | A searchable library of reusable fragments; substring search across title, body, tag and language; copy-to-clipboard as the primary action | Substring `LIKE`, not FTS5 — deliberate, so `Query` matches inside `useQuery` |
| **Audit log** (Settings) | An append-only record of AI tool approvals and denials, secret set/delete by name, SQL writes, filed issues, and database restores. 90-day retention, with the window, total row count and reach-back date printed in the viewer | Not editable, clearable, or exportable from the app — that is the point. Read-only IPC, age-based pruning only |

Around them: workspaces and projects, a `Ctrl+K` command palette that merges
frontend navigation with backend-contributed commands, a notification center,
three themes plus a System option that follows `prefers-color-scheme`, and
automatic pre-migration and daily database backups with a staged restore that
applies on the next boot — validating the candidate first and preserving the
database it displaces, so a mistaken restore is itself recoverable.

**Not built, and honestly flagged as such:** the plugin runtime is a working
`wasmi` spike in `crates/devos-plugin` that is *not registered with the app* —
[ADR-0010](docs/adr/0010-wasmi-interpreter-for-plugin-runtime.md) lists what
would have to be true first. There is no plugin marketplace, no project
templates, no repository cloning, and no embedded DevTools.

## Install

### From a release

Download from the [Releases](../../releases) page:

- `DevOS_<version>_x64-setup.exe` — NSIS installer, **~5.9 MB**
- `DevOS_<version>_x64_en-US.msi` — MSI installer, **~8.4 MB**

Both are x64 Windows. DevOS uses the system WebView2 runtime rather than
bundling Chromium, which is where the small download comes from; WebView2 ships
with Windows 11 and current Windows 10, and the bundle config leaves Tauri's
default bootstrapper in place to fetch it if it is absent.

**The installers are not code-signed yet**, so Windows SmartScreen will warn on
first run. Code signing and the in-app updater are being wired up separately —
see [docs/release-process.md](docs/release-process.md).

### From source

Prerequisites:

- **Rust** stable, MSVC toolchain (CI uses `dtolnay/rust-toolchain@stable`)
- **Node** 20+ and **pnpm** (CI builds with Node 24 and pnpm 11)
- **Microsoft C++ Build Tools** and the Windows SDK — Tauri's standard Windows
  prerequisites
- **`git` on `PATH`** — the Git module shells out to it rather than embedding
  libgit2

```sh
pnpm install
pnpm tauri dev      # run the app against the Vite dev server
pnpm tauri build    # produce the MSI and NSIS installers in target/release/bundle
```

Optional at runtime: **Docker Desktop** for the Docker module, **Ollama** for
local models and embeddings. Both degrade to a clear "not running" state rather
than an error when absent.

## AI providers and keys

Three providers, all **bring-your-own-key or bring-your-own-model**. DevOS
proxies nothing, has no account, and sends no telemetry.

| Provider | Credential | Notes |
|---|---|---|
| **Claude** (Anthropic) | Your API key, entered in the app | The default. The only provider with tool calling |
| **Gemini** (Google) | Your API key, entered in the app | Chat only. The key travels as an `x-goog-api-key` header, never in a URL |
| **Ollama** | None | Local, offline. Base URL configurable; defaults to `http://localhost:11434` |

**How keys are stored.** A master key is generated once and kept in the **OS
keystore** (Windows Credential Manager). Individual secrets are encrypted with
**AES-256-GCM**, fresh nonce per write, into the local SQLite database — so the
database file on its own is useless without the keystore entry. Values are
**write-only from the UI's point of view**: they never cross the IPC boundary
outward, so the secret manager in Settings can list names, overwrite and delete,
and there is no reveal button because there is nothing to reveal.

**The AI has filesystem and shell tools, and they are off by default.** Reading
files requires an explicit per-conversation grant; editing files, writing files,
and running commands require a second grant that is *session-scoped* and forced
off at every launch, and every individual call then pauses on an approval card
showing the full arguments. Read [docs/security.md](docs/security.md) before
turning the second one on.

## Where your data lives

One SQLite database per install at `%APPDATA%\com.peshang.devos\devos.db` (WAL
mode), plus rotating backups and a `screenshots` directory beside it. Override
the root with the `DEVOS_DATA_DIR` environment variable. Nothing is uploaded
anywhere; the only outbound traffic is to the provider or service a feature is
explicitly talking to.

## Performance

Measured on the author's machine (2026-08-07, installed release build), not a
benchmark rig: **534 ms** cold start to kernel-ready, against a 1000 ms budget.
Method, caveats and what the number does *not* cover are in
[docs/performance.md](docs/performance.md) — notably, it stops before the
webview's first paint.

## Tests

**370 Rust tests** (`cargo test --workspace`) and **362 frontend tests**
(`pnpm test`), plus a 7-test WebdriverIO smoke suite (`pnpm e2e`). The Rust
count includes the `export_bindings_*` tests ts-rs emits per exported DTO, so
the behavioral count is lower than the total.

The conventions matter more than the count: no mocked databases, no mocked
subprocesses where a real one is feasible, and security-relevant behavior gets
a test that fails if the guard is removed. [docs/testing.md](docs/testing.md)
has the per-crate breakdown and a "known gaps" section that is kept current.

## Documentation

The knowledge base in [docs/](docs/) is written to be honest about what is
missing, and it is the primary source for everything above:

[vision](docs/vision.md) · [architecture](docs/architecture.md) ·
[tech stack](docs/tech-stack.md) · [coding guidelines](docs/coding-guidelines.md) ·
[feature roadmap](docs/feature-roadmap.md) · [design system](docs/design-system.md) ·
[database](docs/database.md) · [IPC contracts](docs/ipc-contracts.md) ·
[plugin API](docs/plugin-api.md) · [security](docs/security.md) ·
[performance](docs/performance.md) · [AI & agents](docs/agents.md) ·
[release process](docs/release-process.md) · [testing](docs/testing.md) ·
[architecture decisions](docs/adr/)

Start with [feature-roadmap.md](docs/feature-roadmap.md) for what has shipped
per milestone and what was deferred within each module, and
[docs/adr/](docs/adr/) for why the non-obvious calls went the way they did.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the gate commands, the generated-
bindings rule, and the testing conventions. Security reports go through
[SECURITY.md](SECURITY.md), not a public issue.

## License

[MIT](LICENSE) © 2026 peshang
