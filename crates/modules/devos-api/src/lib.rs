//! API client module: send REST requests, save them into collections,
//! keep automatic history. GraphQL/WebSocket/environments layer on later.

mod repo;
mod send;

pub use repo::{delete_request, init, list_history, list_requests, record_history, save_request};
pub use send::send_request;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("request failed: {0}")]
    Request(String),
}

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ApiHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ApiRequestSpec {
    pub method: String,
    pub url: String,
    pub headers: Vec<ApiHeader>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse {
    pub status: u16,
    pub headers: Vec<ApiHeader>,
    pub body: String,
    pub truncated: bool,
    #[ts(type = "number")]
    pub duration_ms: i64,
    #[ts(type = "number")]
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SavedRequest {
    pub id: String,
    pub name: String,
    pub collection: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<ApiHeader>,
    pub body: Option<String>,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ApiHistoryEntry {
    pub id: String,
    pub method: String,
    pub url: String,
    pub status: u16,
    #[ts(type = "number")]
    pub duration_ms: i64,
    #[ts(type = "number")]
    pub sent_at: i64,
}

pub struct ApiModule;

impl Module for ApiModule {
    fn id(&self) -> &'static str {
        "api"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "api.open".into(),
            module: "api".into(),
            title: "Open API Client".into(),
            keywords: vec!["rest".into(), "http".into(), "request".into()],
            shortcut: Some("Ctrl+8".into()),
        }]);
    }
}
