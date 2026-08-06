use std::sync::Arc;

use devos_ai::AiRegistry;
use devos_db::DbManager;
use devos_kernel::Kernel;
use devos_secrets::SecretStore;
use devos_system::SystemProbe;
use devos_terminal::TerminalManager;

use crate::approvals::ApprovalRegistry;

pub struct AppState {
    pub kernel: Arc<Kernel>,
    pub terminal: Arc<TerminalManager>,
    /// Cached pools for the user's own databases (see `devos_db::DbManager`).
    pub db: Arc<DbManager>,
    pub secrets: SecretStore,
    pub ai: Arc<AiRegistry>,
    /// Pending per-call tool approvals (see approvals.rs).
    pub approvals: Arc<ApprovalRegistry>,
    /// Long-lived metrics source; rebuilding it per call pins CPU usage at
    /// zero (see `devos_system::SystemProbe`).
    pub system: Arc<SystemProbe>,
    /// Milliseconds from process start to kernel ready, for the startup budget.
    pub startup_ms: i64,
    /// The database file this process actually opened.
    ///
    /// Carried rather than re-derived: `DEVOS_DATA_DIR` can move it, and a
    /// second `app_data_dir()` call in a command would confidently report a
    /// path nothing ever opened — the worst kind of diagnostic, since it looks
    /// authoritative while pointing at the wrong file.
    pub db_path: String,
}
