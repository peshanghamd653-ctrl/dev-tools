# AI Architecture

User-confirmed provider priority: **Claude API first, Ollama second** (local,
offline, also used for embeddings). OpenAI/Gemini follow through the same
abstraction with no special treatment.

## Provider abstraction (`devos-ai`, M1)

```rust
#[async_trait]
pub trait AiProvider: Send + Sync {
    fn id(&self) -> &'static str;                  // "claude", "ollama", …
    fn capabilities(&self) -> Capabilities;        // streaming, tools, vision, embeddings
    async fn chat(&self, req: ChatRequest, out: StreamSink) -> AiResult<ChatOutcome>;
    async fn embed(&self, texts: &[String]) -> AiResult<Vec<Embedding>>;
}
```

- `ChatRequest` is provider-neutral (messages, system, tools, budget).
  Adapters translate to Claude Messages API / OpenAI / Gemini / Ollama.
- Streaming uses the IPC streaming contract (`devos://ai/<conversationId>`);
  deltas are incremental tokens + tool-call frames.
- API keys live in the encrypted secret store, never in settings or env files.
- Model selection per conversation with a workspace default; offline mode
  falls back to Ollama automatically when no network is available.

## Tool calling (M2)

Tools are declared by modules (same contribution pattern as commands):
`read_file`, `edit_file`, `run_command`, `git_diff`, … Execution flow:

```
model → tool_call frame → approval gate (UI) → module executes → result → model
```

- **Every mutating tool call requires explicit user approval** in the UI;
  approvals can be granted per-conversation for read-only tools.
- All tool executions are written to `audit_log`.

## Project knowledge (M2)

- Indexer job (background, incremental): tree-sitter parses sources into
  symbols + chunks; embeddings stored in SQLite `sqlite-vec`; lexical index
  alongside for hybrid retrieval.
- RAG: retrieve → rerank → cite. Answers carry file:line citations the UI
  turns into links.
- Long-term memory: distilled facts per project (`ai_memory`), injected into
  system context, editable by the user (transparency over magic).

## Background agents (M2+)

An agent = prompt + tool allowlist + trigger (schedule or event) + budget,
executed as kernel jobs, reporting via `notificationAdded` events. First
agent: build-failure watcher (subscribes to terminal/job events, diagnoses
failures, proposes a fix as a notification — never auto-applies).
