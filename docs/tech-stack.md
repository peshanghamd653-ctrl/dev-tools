# Tech Stack

Versions as pinned in `Cargo.toml` / `package.json` at the time of writing.
Update this file when a dependency is deliberately upgraded — it should
always reflect what's actually in the lockfiles, not aspiration.

## Desktop shell

| Piece | Choice | Why |
|---|---|---|
| Runtime | Rust (stable, MSVC toolchain on Windows) | Performance + memory safety for a long-running desktop process. |
| App framework | Tauri v2 | Native webview (no bundled Chromium), small binaries, typed IPC. |
| Async runtime | Tokio (`rt-multi-thread`) | Everything in the kernel is async; pty/process work runs on blocking threads. |
| Serialization | Serde + serde_json | Standard; also drives ts-rs type export. |

## Persistence

| Piece | Choice | Why |
|---|---|---|
| Database | SQLite, WAL mode | Single-file, zero-ops, sufficient for a single-user desktop app. |
| Driver | SQLx (`sqlite`, async) | Compile-time-checked queries available; async-native. |
| Secrets | `keyring` (OS keystore) + `aes-gcm` | Master key never touches disk in plaintext; values encrypted at rest. |

## AI

| Piece | Choice | Why |
|---|---|---|
| Cloud provider | Claude (Anthropic Messages API) | Default provider — best coding models, user-confirmed priority. |
| Cloud provider | Gemini (Generative Language API, SSE) | Free tier — the only cloud provider usable without a billing account. Flash models only. Streams plain chat; tool calling is not yet adapted to Gemini's function-calling shape. |
| Local provider | Ollama (`/api/chat`, NDJSON) | Offline-first, no key required, also used for embeddings. Drives the agentic tool loop, same as Claude. |
| HTTP client | `reqwest` (streaming) | SSE (Claude, Gemini) and NDJSON (Ollama) all consumed as byte streams. |
| Tool calling | Anthropic `tool_use` blocks | Native to the Messages API; no extra framework needed. |

## Frontend

| Piece | Choice | Why |
|---|---|---|
| UI library | React 19 | Latest, used with the compiler-friendly patterns (no manual memoization). |
| Language | TypeScript (strict, `noUncheckedIndexedAccess`) | Matches the Rust side's rigor. |
| Bundler | Vite 7 | Fast HMR, first-class Tauri support. |
| Routing | TanStack Router (file-based, code-split) | Type-safe routes, automatic per-route chunking for the startup budget. |
| Server state | TanStack Query | Cache + invalidation driven by kernel events. |
| Client state | Zustand (+ `persist` middleware where needed) | Minimal boilerplate, no context-provider tree. |
| Forms | React Hook Form + Zod | Typed validation at the form boundary. |
| Styling | Tailwind CSS v4 + shadcn/ui (`new-york` style) | Utility-first, owns-the-code component model (no opaque UI package). |
| Terminal | xterm.js + `@xterm/addon-fit` | Industry standard; pairs with `portable-pty` on the Rust side. |
| Markdown | `react-markdown` + `remark-gfm` | Renders AI assistant replies. |
| Icons | lucide-react | Consistent icon set across the shell. |
| Animation | Motion (installed, not yet used) | Reserved for shell polish once core flows stabilize. |

## Type bridge

| Piece | Choice | Why |
|---|---|---|
| Rust → TS types | ts-rs | Every IPC DTO is defined once in Rust; `pnpm gen:types` (= `cargo test --workspace export_bindings`) regenerates `src/shared/ipc/bindings/`. See [ADR-0002](adr/0002-ts-rs-over-specta-for-ipc-types.md). |

## Tooling

| Piece | Choice |
|---|---|
| Package manager | pnpm (workspace) |
| Linting | ESLint (flat config, typescript-eslint strict) + `cargo clippy -D warnings` |
| Formatting | Prettier + `cargo fmt` |
| Testing | Vitest + Testing Library (frontend) · `cargo test` (Rust, incl. `tempfile`-based integration tests) |
| CI | GitHub Actions — see [release-process.md](release-process.md) |
