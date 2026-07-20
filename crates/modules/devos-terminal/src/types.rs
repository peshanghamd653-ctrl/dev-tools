use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct TermSessionInfo {
    pub id: String,
    pub title: String,
    pub shell: String,
    pub cwd: String,
    #[ts(type = "number")]
    pub created_at: i64,
    pub exited: bool,
}

/// Frames streamed to the frontend on a per-session Tauri channel.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TermEvent {
    /// Raw bytes from the pty (may split UTF-8 codepoints; feed to xterm as bytes).
    Output {
        bytes: Vec<u8>,
    },
    Exit {
        code: Option<u32>,
    },
}
