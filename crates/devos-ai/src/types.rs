use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub conversation_id: String,
    /// "user" | "assistant"
    pub role: String,
    pub content: String,
    #[ts(type = "number")]
    pub created_at: i64,
}

/// Frames streamed to the frontend while a reply is being generated.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AiDelta {
    Text {
        text: String,
    },
    /// The model invoked a tool; `input` is the JSON arguments, serialized.
    ToolCall {
        id: String,
        name: String,
        input: String,
    },
    /// A mutating tool is waiting for the user's explicit consent.
    /// Answered via the `ai_tool_respond` command with this `id`.
    ApprovalRequest {
        id: String,
        name: String,
        input: String,
    },
    /// A tool finished; `summary` is a short preview of the result.
    ToolResult {
        id: String,
        ok: bool,
        summary: String,
    },
    Done,
    Error {
        message: String,
    },
}

/// One turn of the conversation, provider-neutral.
#[derive(Debug, Clone, Serialize)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
}

/// One saved long-term memory fact, scoped to a project. Always visible and
/// deletable in the UI — memory is transparent, not magic.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub id: String,
    pub project: String,
    pub content: String,
    #[ts(type = "number")]
    pub created_at: i64,
}
