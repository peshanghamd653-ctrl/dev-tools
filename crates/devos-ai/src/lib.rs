//! AI layer: provider abstraction (Claude + Ollama first), streaming chat,
//! and conversation persistence. Cloud keys come from the encrypted secret
//! store — never from settings or env files.

pub mod providers;
pub mod repo;
pub mod types;

pub use providers::claude::run_agent;
pub use providers::{
    AiError, AiProvider, AiRegistry, AiResult, StreamRequest, ToolDef, ToolExecutor,
};
pub use types::{AiDelta, ChatMessage, ChatTurn, Conversation, MemoryEntry};

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

pub struct AiModule;

impl Module for AiModule {
    fn id(&self) -> &'static str {
        "ai"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "ai.open".into(),
            module: "ai".into(),
            title: "Open AI Assistant".into(),
            keywords: vec!["chat".into(), "claude".into(), "assistant".into()],
            shortcut: Some("Ctrl+5".into()),
        }]);
    }
}
