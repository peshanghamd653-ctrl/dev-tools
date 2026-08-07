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

/// What came back from asking.
///
/// This used to be `Result<bool, String>`, where every refusal that was not a
/// button press arrived as prose. That was survivable while the only consumer
/// was the model — "denied" is "denied" to a retry loop — but the audit log
/// has to record *why*, and a user who pressed Deny and a user who walked away
/// from the machine for three minutes are not the same event. Recovering that
/// distinction by matching words in an error string is the mistake
/// `DbErrorDto` was introduced to undo on the database side; this is the same
/// fix applied before it can become the same bug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalOutcome {
    Approved,
    /// The user pressed Deny.
    Declined,
    /// Nobody answered within [`APPROVAL_TIMEOUT`]. Carries the timeout so the
    /// audit entry can say "no answer within 180s" without a second constant
    /// somewhere else drifting away from this one.
    TimedOut {
        after_secs: u64,
    },
    /// The request never reached anyone, or the channel died mid-wait. Fails
    /// closed like every other non-approval.
    Unreachable(String),
}

/// How mutating tools ask for consent. Trait so tests can stub it.
#[async_trait::async_trait]
pub trait ApprovalGate: Send + Sync {
    /// Resolves to [`ApprovalOutcome::Approved`] only if the user explicitly
    /// approved this call. Every other variant means the call must not run.
    async fn request(&self, name: &str, input: &Value) -> ApprovalOutcome;
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
    async fn request(&self, name: &str, input: &Value) -> ApprovalOutcome {
        let id = uuid::Uuid::new_v4().to_string();
        let rx = self.registry.register(id.clone());
        if let Err(e) = self.channel.send(AiDelta::ApprovalRequest {
            id: id.clone(),
            name: name.to_string(),
            input: input.to_string(),
        }) {
            self.registry.forget(&id);
            return ApprovalOutcome::Unreachable(format!(
                "could not reach the UI for approval: {e}"
            ));
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(true)) => ApprovalOutcome::Approved,
            Ok(Ok(false)) => ApprovalOutcome::Declined,
            Ok(Err(_)) => {
                self.registry.forget(&id);
                ApprovalOutcome::Unreachable("approval channel closed".into())
            }
            Err(_) => {
                self.registry.forget(&id);
                ApprovalOutcome::TimedOut {
                    after_secs: self.timeout.as_secs(),
                }
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

    /// A gate wired to a real channel, with the id of whatever it just asked
    /// about — the registry key is generated inside `request`, so a test that
    /// wants to answer has to read the frame the UI would have read.
    fn test_gate(timeout: Duration) -> (std::sync::Arc<ApprovalRegistry>, ChannelApprovalGate) {
        let registry = std::sync::Arc::new(ApprovalRegistry::default());
        let gate = ChannelApprovalGate {
            registry: registry.clone(),
            channel: Channel::new(|_| Ok(())),
            timeout,
        };
        (registry, gate)
    }

    /// The distinction the audit log is built on: a pressed Deny and an
    /// unattended machine are different outcomes, carrying different data.
    /// Before this was a typed enum both arrived as `Err(String)` and the only
    /// way to tell them apart was to match words in the message.
    #[tokio::test]
    async fn the_gate_reports_approval_refusal_and_silence_as_different_things() {
        let (registry, gate) = test_gate(APPROVAL_TIMEOUT);

        // Approve: answer from another task once the request is registered.
        let answering = registry.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let id = answering
                    .pending
                    .lock()
                    .expect("approval registry lock poisoned")
                    .keys()
                    .next()
                    .cloned();
                match id {
                    Some(id) => {
                        answering.resolve(&id, true);
                        return;
                    }
                    None => tokio::task::yield_now().await,
                }
            }
        });
        assert_eq!(
            gate.request("write_file", &serde_json::json!({"path": "a.rs"}))
                .await,
            ApprovalOutcome::Approved
        );

        // Deny.
        let (registry, gate) = test_gate(APPROVAL_TIMEOUT);
        let refusing = registry.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let id = refusing
                    .pending
                    .lock()
                    .expect("approval registry lock poisoned")
                    .keys()
                    .next()
                    .cloned();
                match id {
                    Some(id) => {
                        refusing.resolve(&id, false);
                        return;
                    }
                    None => tokio::task::yield_now().await,
                }
            }
        });
        assert_eq!(
            gate.request("run_command", &serde_json::json!({"command": "rm -rf ."}))
                .await,
            ApprovalOutcome::Declined
        );
    }

    /// Nobody answers. The outcome carries the timeout it waited, so the audit
    /// entry says "no answer within 180s" from the same constant the wait used
    /// rather than from a number retyped somewhere else. A short timeout here
    /// because this asserts the *shape*, not the duration.
    #[tokio::test]
    async fn silence_times_out_carrying_the_wait_and_forgets_the_request() {
        let (registry, gate) = test_gate(Duration::from_secs(1));
        let outcome = gate
            .request("edit_file", &serde_json::json!({"path": "a.rs"}))
            .await;

        assert_eq!(outcome, ApprovalOutcome::TimedOut { after_secs: 1 });
        assert!(
            registry
                .pending
                .lock()
                .expect("approval registry lock poisoned")
                .is_empty(),
            "a timed-out request must not stay resolvable"
        );
    }
}
