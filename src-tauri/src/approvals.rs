//! Per-call approval for mutating AI tools.
//!
//! Flow: the executor hits a mutating tool → sends an `ApprovalRequest`
//! frame on the conversation's delta channel and parks on a oneshot → the
//! user clicks Approve/Deny in the chat UI → the `ai_tool_respond` command
//! resolves the oneshot → execution proceeds or returns "denied".
//!
//! The registry is global (commands must find pending requests by id); the
//! delta channel is per-send, so the gate that pairs them is constructed
//! inside `ai_send`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use devos_ai::AiDelta;
use serde_json::Value;
use tauri::ipc::Channel;
use tokio::sync::oneshot;

/// How long a pending approval waits before counting as denied.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Default)]
pub struct ApprovalRegistry {
    pending: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl ApprovalRegistry {
    pub fn register(&self, id: String) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .expect("approval registry lock poisoned")
            .insert(id, tx);
        rx
    }

    /// Returns false if the id is unknown (already resolved or timed out).
    pub fn resolve(&self, id: &str, approved: bool) -> bool {
        match self
            .pending
            .lock()
            .expect("approval registry lock poisoned")
            .remove(id)
        {
            Some(tx) => tx.send(approved).is_ok(),
            None => false,
        }
    }

    fn forget(&self, id: &str) {
        self.pending
            .lock()
            .expect("approval registry lock poisoned")
            .remove(id);
    }
}

/// How mutating tools ask for consent. Trait so tests can stub it.
#[async_trait::async_trait]
pub trait ApprovalGate: Send + Sync {
    /// Resolves to `true` only if the user explicitly approved this call.
    async fn request(&self, name: &str, input: &Value) -> Result<bool, String>;
}

/// Production gate: emits an `ApprovalRequest` frame and waits on the registry.
pub struct ChannelApprovalGate {
    registry: std::sync::Arc<ApprovalRegistry>,
    channel: Channel<AiDelta>,
    timeout: Duration,
}

impl ChannelApprovalGate {
    pub fn new(registry: std::sync::Arc<ApprovalRegistry>, channel: Channel<AiDelta>) -> Self {
        Self {
            registry,
            channel,
            timeout: APPROVAL_TIMEOUT,
        }
    }
}

#[async_trait::async_trait]
impl ApprovalGate for ChannelApprovalGate {
    async fn request(&self, name: &str, input: &Value) -> Result<bool, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let rx = self.registry.register(id.clone());
        self.channel
            .send(AiDelta::ApprovalRequest {
                id: id.clone(),
                name: name.to_string(),
                input: input.to_string(),
            })
            .map_err(|e| format!("could not reach the UI for approval: {e}"))?;

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(approved)) => Ok(approved),
            Ok(Err(_)) => {
                self.registry.forget(&id);
                Err("approval channel closed".into())
            }
            Err(_) => {
                self.registry.forget(&id);
                Err("approval request timed out".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolve_delivers_to_waiter() {
        let registry = ApprovalRegistry::default();
        let rx = registry.register("req-1".into());
        assert!(registry.resolve("req-1", true));
        assert_eq!(rx.await, Ok(true));
    }

    #[tokio::test]
    async fn unknown_or_double_resolve_is_rejected() {
        let registry = ApprovalRegistry::default();
        let _rx = registry.register("req-2".into());
        assert!(!registry.resolve("nope", true));
        assert!(registry.resolve("req-2", false));
        assert!(!registry.resolve("req-2", true), "second resolve must fail");
    }
}
