//! API client module: send REST requests, save them into collections, keep
//! automatic history, and resolve `{{VAR}}` placeholders against a named
//! environment. GraphQL/WebSocket/OpenAPI import layer on later.

mod repo;
mod send;
mod substitute;

pub use repo::{
    active_environment, create_environment, delete_environment, delete_request, init,
    list_environments, list_history, list_requests, migrate_secrets, record_history, save_request,
    set_active_environment, update_environment,
};
pub use send::send_request;
pub use substitute::{substitute, substitute_spec};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("secret store error: {0}")]
    Secret(#[from] devos_secrets::SecretError),
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

/// One entry in an environment's variable set.
///
/// `id` is a stable identifier, independent of `key` — it is what
/// [`SecretStore`](devos_secrets::SecretStore) vault entries for `secret`
/// variables are keyed by (see [`ApiEnvironment`]'s doc comment), so
/// renaming `key` never orphans a vault entry. Assigned once, on first
/// save; a var arriving with an empty `id` (a brand-new row from the
/// editor) gets one generated for it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ApiEnvVar {
    #[serde(default)]
    pub id: String,
    pub key: String,
    pub value: String,
    pub secret: bool,
}

/// A named set of variables (`API_URL`, `TOKEN`, …) that [`substitute_spec`]
/// resolves `{{KEY}}` placeholders against. At most one environment is
/// `active` at a time; `api_send` resolves against whichever one is.
///
/// **`ApiEnvVar::secret` is backed by the secrets vault (SEC-105).** A
/// variable marked secret has its value encrypted via
/// `devos_secrets::SecretStore` under `api-env-var:{ApiEnvVar::id}` — the
/// `api_environments.vars` JSON blob stores an empty string for it, never
/// the plaintext. `repo::hydrate` fills the real value back in on every
/// read (`list_environments`, `active_environment`, and the environment
/// returned from `create_environment`/`update_environment`), so the editor
/// and `api_send`'s `{{VAR}}` substitution see it exactly as before —
/// only at-rest storage changed. A variable that is not secret is still
/// plain JSON, same as `api_requests.headers`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ApiEnvironment {
    pub id: String,
    pub name: String,
    pub vars: Vec<ApiEnvVar>,
    pub active: bool,
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
