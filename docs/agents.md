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
- Tool set today: `read_file`, `list_dir`, `find_files` — all read-only.
  See [security.md](security.md) for the containment guarantees.
- **Nothing runs without an explicit grant.** The frontend only includes
  `toolsEnabled: true` in the `ai_send` call when the user has turned on
  the "Tools" chip for that conversation; otherwise the backend never even
  builds the tool list, and Claude has nothing to call.
- Live activity streams to the UI as `AiDelta::ToolCall` / `ToolResult`
  frames, rendered as a running list above the assistant's reply.

## Background agents — planned (M2 remainder / M4)

Not yet built. Design intent, for when it lands: an agent = prompt + tool
allowlist + trigger (schedule or event) + budget, executed as a kernel job
(`JobRunner`), reporting via `KernelEvent::NotificationAdded`. First planned
agent: a build-failure watcher subscribing to terminal/job events, using the
same read-only tool set to diagnose a failure and propose a fix as a
notification — never auto-applying a change. This reuses every piece
already built (jobs, events, tool executor) rather than needing new
infrastructure.

## Project indexing / RAG — planned (M2 remainder)

tree-sitter symbol extraction + chunk embeddings in SQLite (`sqlite-vec`);
Ollama or API embeddings; hybrid lexical + vector retrieval; answers cited
with file:line links. Not started — `find_files`/`read_file` are a
stand-in today, sufficient for small-to-medium projects but not a
substitute for real retrieval at scale.
