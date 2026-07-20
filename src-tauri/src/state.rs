use std::sync::Arc;

use devos_ai::AiRegistry;
use devos_kernel::Kernel;
use devos_secrets::SecretStore;
use devos_terminal::TerminalManager;

pub struct AppState {
    pub kernel: Arc<Kernel>,
    pub terminal: Arc<TerminalManager>,
    pub secrets: SecretStore,
    pub ai: Arc<AiRegistry>,
    /// Milliseconds from process start to kernel ready, for the startup budget.
    pub startup_ms: i64,
}
