use tokio::sync::broadcast;

use crate::types::KernelEvent;

/// In-process publish/subscribe bus. Modules emit events instead of calling
/// each other; the desktop shell forwards every event to the webview.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<KernelEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Emitting with no subscribers is not an error.
    pub fn emit(&self, event: KernelEvent) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<KernelEvent> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}
