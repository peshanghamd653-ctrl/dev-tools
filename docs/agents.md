# AI & Agents

Provider priority (user-confirmed): **Claude first, Ollama second**
(local/offline, also used for embeddings once indexing lands). OpenAI and
Gemini slot in behind the same trait later with no special treatment.

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
- `OllamaProvider` — `/api/chat`, NDJSON streaming, also exposes
  `list_models()` for the model picker.
- `AiRegistry` holds both and resolves by id; `complete_once()` is a
  one-shot helper (used by AI commit-message generation) that drops the
  streaming receiver immediately — providers tolerate a closed channel.
- API keys come from `devos-secrets`, never from settings or env files.

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

## Tool calling — the agent loop (implemented, Claude only)

```
stream_once(messages, tools) → text delta(s) + tool_use block(s)
        │
        ▼ (if any tool_use blocks)
  for each call: emit ToolCall → executor.execute() → emit ToolResult
        │
        ▼
  append assistant turn (text + tool_use) and a user turn (tool_result)
        │
        └──► loop (max 10 rounds; the final round withholds tools,
              forcing a concluding answer instead of another call)
```

- `ToolExecutor` is a trait (`devos-ai::providers::ToolExecutor`); the
  desktop layer implements it as `ProjectTools` (`src-tauri/src/tools.rs`),
  scoped to one canonicalized project root.
- Tool set today: `read_file`, `list_dir`, `find_files`, `search_code`,
  `save_memory` (read level) plus `edit_file`, `write_file`, `run_command`
  (write level). See [security.md](security.md) for containment and
  approval guarantees. `search_code` queries the FTS5 project index
  (bm25-ranked, file:line + snippet); if the project isn't indexed it tells
  the model to ask the user to index it rather than failing. `save_memory`
  sits at the read level deliberately: it writes only to DevOS's own
  memory store, which is fully visible and deletable in the Memory dialog
  — it cannot touch project files.
- **Nothing runs without an explicit grant.** The frontend only includes
  `toolsEnabled` / `writeToolsEnabled` in the `ai_send` call when the user
  has turned on the corresponding chips; otherwise the backend never even
  builds those tool defs, and Claude has nothing to call.
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

## Background agents — first watcher implemented

The pieces an agent needs all exist: durable jobs (`JobRunner`), the event
bus, the gated tool executor, and a persistent reporting surface
(`Kernel::notify` → Notification Center bell).

The **build-failure watcher is live** for PowerShell sessions:

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

## Project indexing / RAG — lexical half implemented

`devos-index` provides the lexical side of the documented hybrid-retrieval
plan: FTS5 chunks (50 lines, 1-based start lines) indexed incrementally by
mtime/size, pruned on delete, searched with bm25 ranking and snippets.
Reindexing runs through the kernel `JobRunner` — the first real background
job — so progress and completion surface via standard `jobUpdated` events.

Still planned: the vector half — tree-sitter symbol extraction + chunk
embeddings (`sqlite-vec`, Ollama or API embeddings) layered onto the same
tables, then hybrid lexical+vector retrieval with cited answers.
