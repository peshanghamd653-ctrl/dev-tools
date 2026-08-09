# AI & Agents

Provider priority (user-confirmed): **Claude first, Ollama second**
(local/offline, also used for embeddings). **Gemini** was added third, for
one reason worth stating: its free tier makes it the only cloud provider a
user can try without a billing account — Claude needs one, and Ollama needs
hardware. OpenAI slots in behind the same trait when there is a reason.

## Provider abstraction (`devos-ai`, implemented)

```rust
#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn stream_chat(&self, req: StreamRequest<'_>, tx: &UnboundedSender<AiDelta>) -> AiResult<String>;
}
```

- `ClaudeProvider` — Anthropic Messages API, SSE streaming, a stateful
  `SseParser` that emits text deltas immediately and accumulates `tool_use`
  blocks (whose JSON arguments can arrive fragmented across multiple
  `input_json_delta` frames — the parser buffers per block index).
- `GeminiProvider` — Generative Language API, `streamGenerateContent?alt=sse`.
  Three things differ from Claude and each is a quiet-failure trap: the
  assistant role is called **`model`** (sending `assistant` is *accepted*
  and degrades the reply rather than erroring), the system prompt is
  `systemInstruction` rather than a message, and the key goes in an
  `x-goog-api-key` header rather than `?key=` so it stays out of URLs. Only
  flash-class models are offered — the pro models are the ones a free key
  cannot usefully drive, so listing them would mostly produce quota errors.
- `OllamaProvider` — `/api/chat`, NDJSON streaming, also exposes
  `list_models()` for the model picker.
- `AiRegistry` holds all three and resolves by id; `complete_once()` is a
  one-shot helper (used by AI commit-message generation) that drops the
  streaming receiver immediately — providers tolerate a closed channel.
- API keys come from `devos-secrets`, never from settings or env files, and
  the provider→secret mapping lives in one function so a new provider cannot
  inherit another's credential by copy-paste.
- **Tool calling drives Claude and Ollama; Gemini still streams plain chat.**
  Each provider that has it is a separate agent loop — `claude::run_agent`
  and `ollama::run_agent` — not one generic implementation, because the two
  APIs disagree on how a call is represented (Claude streams a tool's
  arguments incrementally as SSE `input_json_delta` fragments; Ollama buffers
  the call server-side and emits `message.tool_calls` complete in one NDJSON
  frame) and on how the result is threaded back in (Claude's `tool_result`
  content block vs. Ollama's `role: "tool"` message). Gemini has function
  calling but nothing here has adapted its shape to either loop yet; the
  desktop layer's gate — `matches!(provider, "claude" | "ollama")` in
  `ai_commands.rs` — is what stops a Gemini conversation from accepting a
  tools grant it would silently drop.

## Conversation persistence (implemented)

`ai_conversations` / `ai_messages` (see [database.md](database.md)).
Conversations auto-title from the first user message (truncated at 48
chars). Every send appends the user turn, replays full history to the
provider, then appends the assistant turn once streaming completes —
nothing is persisted until the full reply is in hand, so a failed stream
doesn't leave a half-written assistant message.

## Project-aware context (implemented)

When a project is attached (default: on, toggle in the chat UI), `ai_send`
builds a system prompt from `devos-git::status()` — current branch, changed
file count, and up to 20 changed filenames — before calling the provider.
This is intentionally cheap (no file contents) and always-on when attached;
it's separate from tool calling, which is opt-in per conversation.

## Tool calling — the agent loop (implemented for Claude and Ollama)

```
stream_once(messages, tools) → text delta(s) + tool call(s)
        │
        ▼ (if any tool calls)
  for each call: emit ToolCall → executor.execute() → emit ToolResult
        │
        ▼
  append the assistant's turn and one result turn per call, in
  whichever shape the provider's continuation format expects
        │
        └──► loop (max 10 rounds; the final round withholds tools,
              forcing a concluding answer instead of another call)
```

Same shape, two independent implementations. `claude::run_agent` echoes back
`{"type": "tool_use", ...}` content blocks and one `tool_result` block per
call; `ollama::run_agent` echoes an assistant message carrying `tool_calls` in
the OpenAI-style `{type: "function", function: {name, arguments}}` envelope
and one `{"role": "tool", "content": ...}` message per result — the
convention Ollama's own documentation models tool calling on, verified here
only against a hermetic test server rather than a live Ollama install (see
the doc comment on `ollama::run_agent` for what that verification does and
does not cover). Ollama also assigns tool calls no id at all, unlike
Anthropic's `tool_use` blocks — the ids on its `AiDelta::ToolCall` /
`ToolResult` frames are generated client-side purely to correlate the two for
the frontend's approval UI, and never round-trip back to Ollama.

- `ToolExecutor` is a trait (`devos-ai::providers::ToolExecutor`); the
  desktop layer implements it as `ProjectTools` (`src-tauri/src/tools.rs`),
  scoped to one canonicalized project root.
- Tool set today: `read_file`, `list_dir`, `find_files`, `search_code`,
  `save_memory`, `git_diff` (read level) plus `edit_file`, `write_file`,
  `run_command`, `run_tests`, `run_lint`, `git_commit`, `git_create_branch`
  (write level). See [security.md](security.md) for containment and
  approval guarantees. `search_code` queries the FTS5 project index
  (bm25-ranked, file:line + snippet); if the project isn't indexed it tells
  the model to ask the user to index it rather than failing. `save_memory`
  sits at the read level deliberately: it writes only to DevOS's own
  memory store, which is fully visible and deletable in the Memory dialog
  — it cannot touch project files.

  `git_diff` also sits at the read level — it shells out to `devos_git::
  staged_diff` (the same call `ai_commit_message` already made, same 24 KB
  cap) and changes nothing, so it needs neither the write grant nor
  approval, the same tier as `read_file`. An empty diff distinguishes
  "nothing staged, but N files changed" from "nothing staged, and the
  working tree is genuinely clean" — collapsing those two into one silent
  empty string would let a model (and the user reading its summary)
  conclude nothing had changed when the truer answer is that nothing had
  been staged yet.

  `git_commit` and `git_create_branch` are the write-level counterparts.
  Unlike `run_tests`/`run_lint` neither resolves anything on the model's
  behalf — the commit message and the branch name are exactly what the
  model sent, shown to the approval gate as-is, the same direct pattern
  `edit_file`/`write_file` already use. `git_commit` does not stage
  anything itself (`run_command` with `git add`, or the Git page, does
  that); if nothing is staged the underlying `git commit` fails and that
  failure is what the model sees, not a false success.

  `run_tests` and `run_lint` take no argument from the model at all —
  `src-tauri/src/test_runner.rs` detects the project's command (test:
  Cargo.toml → cargo test, package.json's `test` script → npm/pnpm/yarn,
  pyproject.toml/pytest.ini/setup.cfg → pytest, go.mod → go test; lint:
  Cargo.toml → `cargo clippy --all-targets -- -D warnings` — the same
  command this project's own CI runs, chosen because plain clippy's
  per-target "generated N warnings (K duplicate)" output cannot be summed
  without double-counting — package.json's `lint` script → npm/pnpm/yarn,
  go.mod → `go vet`; no Python lint detector, since unlike pytest there is
  no single dominant convention to pick between ruff/flake8/pylint) and
  resolves it *before* the approval gate runs, so the card shows the actual
  command rather than an empty `{}` the model cannot usefully fill in. More
  than one setup at the same root — this repository's own is exactly that
  case for both tools, Cargo.toml and package.json both at the root — is
  refused rather than guessed at, naming both and pointing at `run_command`
  for the specific one wanted. The result is a parsed summary (pass/fail
  counts for tests, a clean/problem-count for lint — both verified against
  real `cargo test`/`cargo clippy`/`vitest`/`eslint` output this codebase
  produces, including output captured from real failures) ahead of the raw
  text, so a fix-test-lint loop does not have to re-derive the outcome from
  a wall of text on every round.
- **Nothing runs without an explicit grant.** The frontend only includes
  `toolsEnabled` / `writeToolsEnabled` in the `ai_send` call when the user
  has turned on the corresponding chips; otherwise the backend never even
  builds those tool defs, and the model has nothing to call. The toggle
  buttons themselves are gated by `providerSupportsTools()`
  (`src/features/ai/providers.ts`), which the backend's own gate in
  `ai_commands.rs` has to agree with by hand — there is no shared source of
  truth across the Rust/TypeScript boundary, so a test on each side pins the
  same two provider names.
- **Mutating calls additionally pause on per-call approval**: the executor's
  `ApprovalGate` emits an `AiDelta::ApprovalRequest` frame, the chat shows
  an approval card, and the `ai_tool_respond` command resolves the parked
  oneshot. The gate is a trait, so tests exercise the full approve/deny
  path with a stub instead of a UI.
- Live activity streams to the UI as `AiDelta::ToolCall` / `ToolResult`
  frames, rendered as a running list above the assistant's reply.

## Long-term memory — implemented

Per-project facts in `ai_memory` (capped: 500 chars/entry, 100 entries per
project). Injected into the project system prompt on every send when the
project is attached. Three surfaces, all showing the same data:

- the model's `save_memory` tool ("remember that we use pnpm"),
- the Memory dialog in the chat UI (list, manual add, delete),
- the system prompt block "Saved project memory".

Deliberately transparent: there is no hidden summarization or automatic
distillation — every remembered fact was either saved by the model in a
visible tool call or typed by the user, and each is one click from deletion.

## Background watchers — two implemented

The pieces an agent needs all exist: durable jobs (`JobRunner`), the event
bus, the gated tool executor, and a persistent reporting surface
(`Kernel::notify` → Notification Center bell). Two watchers use them today.
Neither is an agent in the tool-calling sense: both observe and report,
and neither changes anything.

### Build-failure watcher — event-driven (M2)

Live for PowerShell sessions:

```
prompt hook (OSC 133;D;<exit-code>, injected via -EncodedCommand,
chains the user's own prompt)          →  ConPTY output stream
→ cross-chunk OscScanner in the pty reader thread
→ CommandFailure channel out of TerminalManager
→ desktop consumer (30s/session throttle)
→ Kernel::notify("terminal", "warning", …, output snippet)  →  bell
```

Detection is deterministic (real exit codes, no output-pattern heuristics)
and free; **AI diagnosis stays one click away** (the terminal's sparkle
button feeds the same ring buffer into a chat) rather than auto-running on
every failure — a deliberate cost/noise decision. Opt out entirely with
setting `terminal.integration=off`. cmd.exe and other shells are not
instrumented (no reliable prompt hook); they keep manual diagnosis only.
The watcher never applies changes.

### Monitor scheduler — time-driven (M4)

The website monitor (`devos-monitor`) is the second watcher, and the first
that runs on a clock instead of reacting to something the user did:

```
tokio task started at boot  →  tick every ~15s
→ select enabled monitors whose newest check is older than interval_secs
→ HTTP check (15s timeout, limited redirects) → insert into monitor_checks
→ compare with the previous check's ok flag
→ transition only:  ok→fail  Kernel::notify(level "warning")
                    fail→ok  Kernel::notify(level "info")   →  bell
```

Transition-based notification is the whole design: a site that stays down
produces one warning, not one per check, so the bell stays worth reading —
including for the terminal watcher, which shares it. The cost is that a
still-broken monitor is silent after that first warning; `/monitors` is the
surface that always shows current state.

**Checks only run while DevOS is open.** There is no daemon and no OS
scheduled task, so closing the app stops monitoring, `monitor_checks` has
holes wherever it was closed, and an outage that starts *and* ends while
it was closed is never noticed — the stored state was `ok` before and the
first check after launch is `ok`, so there is no transition. That
limitation, why it was accepted, and what would have to change to remove
it are in
[ADR-0008](adr/0008-in-process-watchers-notify-on-transitions.md).

Like the terminal watcher, this one only reports: it records a result and
raises a notification. It takes no remedial action, and it does not call
the AI — detection is deterministic and free, and there is little for a
model to add to a status code and a transport error string.

## Project indexing / RAG — hybrid retrieval implemented

`devos-index` provides the lexical side of the documented hybrid-retrieval
plan: FTS5 chunks (50 lines, 1-based start lines) indexed incrementally by
mtime/size, pruned on delete, searched with bm25 ranking and snippets.
Reindexing runs through the kernel `JobRunner` — the first real background
job — so progress and completion surface via standard `jobUpdated` events.

The vector half now exists too, behind the same `index_search` entry point —
no command changed shape. Chunk embeddings come from **Ollama**
(`nomic-embed-text` by default, reusing the AI module's `ai.ollama.url`), are
stored as `f32` BLOBs in `index_embeddings`, and are merged with the bm25
ranking by **reciprocal-rank fusion** so two rankings combine without tuning
score scales against each other.

**`sqlite-vec` was evaluated and rejected**, and the blocker was
architectural rather than a Windows packaging problem: sqlx loads SQLite
extensions only at connect time, the pool is built once in the kernel where
`devos-index` has no say, and sqlx deliberately re-disables extension loading
after connecting — so there is no runtime escape hatch. Brute-force cosine
over BLOBs is fast enough at one project's scale, needs no native artifact,
and the BLOB layout deliberately matches what `sqlite-vec` expects, so
swapping it in later is a query change rather than a re-index.

**Degradation is the important property**: most users will not run Ollama.
A project with no stored vectors never contacts it at all (a cheap `EXISTS`
check precedes any network call), indexing costs exactly one refused
connection rather than one per chunk, and search returns its lexical results
unchanged. `index.embeddings=off` disables the whole path.

**Tree-sitter symbol extraction** completes the picture. Rust, TypeScript
and TSX grammars (JS/JSX ride the TSX parser) pull declarations into
`index_symbols`, which `search` fuses as a third RRF leg. The effect worth
naming: bm25 rewards a short file that mentions a name five times in
comments over the file that actually declares it, and the symbol leg
corrects that ordering. A file with no grammar is never parsed and indexes
lexically exactly as before; a file whose syntax is broken degrades to the
same path rather than failing the run.

Deliberately *not* done: chunking on symbol boundaries. It would re-key
every `index_chunks` and `index_embeddings` row and hand the embedder
whatever size the author wrote — a 400-line component becomes one chunk
that silently overflows `nomic-embed-text`'s input window, while a
one-line getter becomes its own. Fixed 50-line windows happen to suit the
model that actually runs here.
