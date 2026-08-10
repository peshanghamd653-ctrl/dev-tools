//! Security center IPC: one command, the combined report `devos_security`
//! already assembles from its three independent checks.

use devos_security::SecurityReport;

/// No `AppState` needed — every check here is either a subprocess (git,
/// cargo/npm audit) or a plain filesystem walk, neither of which touches
/// the kernel's pool.
#[tauri::command]
pub async fn security_scan(project_path: String) -> Result<SecurityReport, String> {
    Ok(devos_security::scan(std::path::Path::new(&project_path)).await)
}
