mod ai_commands;
mod api_commands;
mod approvals;
mod commands;
mod core_module;
mod db_commands;
mod deploy_commands;
mod docker_commands;
mod fs_commands;
mod git_commands;
mod index_commands;
mod issue_commands;
mod monitor_commands;
mod pathsafe;
mod snippet_commands;
mod state;
mod system_commands;
mod term_commands;
mod tools;

use std::path::PathBuf;
use std::sync::Arc;

use devos_ai::AiRegistry;
use devos_db::DbManager;
use devos_kernel::Kernel;
use devos_secrets::SecretStore;
use devos_system::SystemProbe;
use devos_terminal::TerminalManager;
use tauri::{Emitter, Manager};
use tokio::sync::broadcast::error::RecvError;

use state::AppState;

/// Environment override for the directory that holds `devos.db`.
///
/// This is a **test seam, not a feature.** It exists so the e2e suite (and a
/// CI run, which has no `%APPDATA%` worth polluting) can boot a throwaway
/// database instead of the developer's real one. Nothing in the UI sets it,
/// reads it back, or persists it.
///
/// Security note, since an environment variable that relocates application
/// data deserves one rather than a shrug:
///
///   * It does **not** move secret *values*. API keys are AES-256-GCM
///     ciphertext in this database and the master key lives in the OS keystore
///     (`devos-secrets`), so a redirected database is inert on its own.
///   * It does move everything secrets-*adjacent*: secret names, saved API
///     requests and their history, database connection entries, indexed file
///     content, notifications — and **screenshots**, which are the most
///     sensitive thing this app writes, since a desktop capture routinely
///     contains `.env` contents or a token sitting in a terminal. Pointing
///     this at a synced or shared folder would put that data somewhere it
///     was never meant to go. Screenshots follow deliberately: leaving them
///     in the real app-data directory while everything else moved would
///     surprise someone in exactly the place surprise is least affordable.
///   * It grants no privilege that isn't already held. Setting a variable in
///     this process's environment requires being the user the app runs as, and
///     that user can already read the default database directly. The footgun
///     is misconfiguration (a stray value in a shortcut or a shell profile
///     silently splitting the app's history in two), not escalation — which is
///     why an unset variable must resolve to exactly today's path, and why
///     the chosen path is logged at startup.
const DATA_DIR_ENV: &str = "DEVOS_DATA_DIR";

/// The override, if it actually says anything.
///
/// Blank counts as unset: `DEVOS_DATA_DIR=` left in a script must not resolve
/// to `""` and drop the database in whatever the working directory happens to
/// be — for a GUI app launched from Explorer that is `C:\Windows\System32`.
/// Shared by [`data_dir`] and the startup log so the two can never disagree
/// about whether an override was in force.
fn effective_override(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// Resolve the directory holding `devos.db`.
///
/// `override_value` is the raw `DEVOS_DATA_DIR` value; `default` produces the
/// normal `app_data_dir()`. Split this way — value in, closure for the default
/// — so the resolution rules are unit-testable without mutating the process
/// environment (which is global, and would race the other tests in this
/// binary).
///
/// When the override is absent or blank this returns `default()` untouched and
/// performs no filesystem work, so the shipped default path is byte-for-byte
/// what it was before this seam existed.
fn data_dir(
    override_value: Option<&str>,
    default: impl FnOnce() -> tauri::Result<PathBuf>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let Some(raw) = effective_override(override_value) else {
        // No `create_dir_all` here on purpose: `devos_kernel::db::connect`
        // already creates the parent of the database file, and this branch is
        // meant to be indistinguishable from the code it replaced.
        return Ok(default()?);
    };

    let dir = PathBuf::from(raw);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("{DATA_DIR_ENV}=\"{raw}\": cannot create data directory: {e}"))?;
    Ok(dir)
}

pub fn run() {
    let start = std::time::Instant::now();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // Native folder picker, used by "Add project". Only `dialog:allow-open`
        // is granted in capabilities — no save dialogs, no message boxes.
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let raw_override = std::env::var(DATA_DIR_ENV).ok();
            let source = match effective_override(raw_override.as_deref()) {
                Some(_) => DATA_DIR_ENV,
                None => "app_data_dir",
            };
            let data_dir = data_dir(raw_override.as_deref(), || app.path().app_data_dir())?;
            let db_path = data_dir.join("devos.db");

            // Logged *before* boot, and unconditionally: "which database am I
            // looking at" is the first question a confusing e2e failure or a
            // mysteriously empty workspace list raises, and a boot that dies in
            // migrations should still have answered it.
            tracing::info!(
                db_path = %db_path.display(),
                source,
                "devos database selected"
            );

            // Screenshots follow the data directory rather than always landing
            // in the real app-data dir. A desktop capture is the most sensitive
            // thing DevOS writes to disk — it routinely contains `.env`
            // contents or a token sitting in a terminal — so someone who
            // redirects application data to a sandbox or an encrypted volume
            // and finds screenshots of their desktop left behind in %APPDATA%
            // has been surprised in the one place surprise is least affordable.
            //
            // The asset-protocol scope in tauri.conf.json is static and
            // resolves `$APPDATA` itself, so it cannot describe an env
            // override. Granting the resolved directory here is what keeps the
            // annotator able to load the capture it just took; without it the
            // webview refuses the file and the feature fails at image load.
            let screenshots = data_dir.join("screenshots");
            app.asset_protocol_scope()
                .allow_directory(&screenshots, false)
                .map_err(|e| format!("asset scope for {}: {e}", screenshots.display()))?;

            let mut kernel = tauri::async_runtime::block_on(Kernel::boot(&db_path))?;
            kernel.register_module(&core_module::CoreModule);
            kernel.register_module(&devos_terminal::TerminalModule);
            kernel.register_module(&devos_git::GitModule);
            kernel.register_module(&devos_ai::AiModule);
            kernel.register_module(&devos_index::IndexModule);
            kernel.register_module(&devos_docker::DockerModule);
            kernel.register_module(&devos_api::ApiModule);
            kernel.register_module(&devos_db::DbModule);
            kernel.register_module(&devos_system::SystemModule);
            kernel.register_module(&devos_monitor::MonitorModule);
            kernel.register_module(&devos_deploy::DeployModule);
            kernel.register_module(&devos_issue::IssueModule);
            kernel.register_module(&devos_snippets::SnippetsModule);
            let kernel = Arc::new(kernel);

            let secrets = tauri::async_runtime::block_on(SecretStore::init(kernel.pool.clone()))?;
            tauri::async_runtime::block_on(devos_ai::repo::init(&kernel.pool))
                .map_err(|e| format!("ai tables: {e}"))?;
            tauri::async_runtime::block_on(devos_index::init(&kernel.pool))
                .map_err(|e| format!("index tables: {e}"))?;
            tauri::async_runtime::block_on(devos_api::init(&kernel.pool))
                .map_err(|e| format!("api tables: {e}"))?;
            tauri::async_runtime::block_on(devos_db::init(&kernel.pool))
                .map_err(|e| format!("db tables: {e}"))?;
            tauri::async_runtime::block_on(devos_monitor::init(&kernel.pool))
                .map_err(|e| format!("monitor tables: {e}"))?;
            tauri::async_runtime::block_on(devos_snippets::init(&kernel.pool))
                .map_err(|e| format!("snippet tables: {e}"))?;

            // Forward every kernel event to the webview on one channel.
            let mut rx = kernel.events.subscribe();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(event) => {
                            let _ = handle.emit("devos://event", &event);
                        }
                        Err(RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "event forwarder lagged");
                        }
                        Err(RecvError::Closed) => break,
                    }
                }
            });

            let startup_ms = start.elapsed().as_millis() as i64;
            tracing::info!(startup_ms, "devos kernel ready");
            let terminal = Arc::new(TerminalManager::new());

            // The failure watcher: OSC 133 markers from shell integration
            // become persistent notifications, throttled per session so a
            // rapid-fire failing loop can't flood the bell.
            if let Some(mut failures) = terminal.take_failure_receiver() {
                let watch_kernel = kernel.clone();
                let watch_terminal = terminal.clone();
                tauri::async_runtime::spawn(async move {
                    let mut last_notified: std::collections::HashMap<String, std::time::Instant> =
                        std::collections::HashMap::new();
                    while let Some(failure) = failures.recv().await {
                        let now = std::time::Instant::now();
                        let throttled = last_notified
                            .get(&failure.session_id)
                            .is_some_and(|t| now.duration_since(*t).as_secs() < 30);
                        if throttled {
                            continue;
                        }
                        last_notified.insert(failure.session_id.clone(), now);
                        let tail = watch_terminal.tail(&failure.session_id).unwrap_or_default();
                        let snippet: String = tail
                            .trim_end()
                            .chars()
                            .rev()
                            .take(400)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        let _ = watch_kernel
                            .notify(
                                "terminal",
                                "warning",
                                &format!("Command failed (exit {})", failure.exit_code),
                                Some(snippet.trim()),
                            )
                            .await;
                    }
                });
            }

            // The uptime watcher: checks monitors whose interval has elapsed
            // and notifies only when a site's reachability actually changes
            // (see `devos_monitor::alert_for`). Spawned here rather than by
            // the module so the crate stays runtime-agnostic.
            tauri::async_runtime::spawn(devos_monitor::run_scheduler(kernel.clone()));

            app.manage(AppState {
                kernel,
                terminal,
                db: Arc::new(DbManager::new()),
                secrets,
                ai: Arc::new(AiRegistry::new()),
                approvals: Arc::new(approvals::ApprovalRegistry::default()),
                system: Arc::new(SystemProbe::new()),
                startup_ms,
                db_path: db_path.display().to_string(),
                data_dir,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::workspaces_list,
            commands::workspace_create,
            commands::workspace_rename,
            commands::workspace_delete,
            commands::projects_list,
            commands::project_add,
            commands::project_remove,
            commands::settings_get,
            commands::settings_set,
            commands::commands_list,
            commands::jobs_recent,
            commands::notifications_list,
            commands::notifications_unread_count,
            commands::notification_mark_read,
            commands::notifications_mark_all_read,
            term_commands::term_create,
            term_commands::term_write,
            term_commands::term_resize,
            term_commands::term_kill,
            term_commands::term_list,
            term_commands::term_tail,
            git_commands::git_status,
            git_commands::git_stage,
            git_commands::git_unstage,
            git_commands::git_discard,
            git_commands::git_commit,
            git_commands::git_log,
            git_commands::git_branches,
            git_commands::git_switch,
            git_commands::git_diff,
            git_commands::git_push,
            git_commands::git_pull,
            ai_commands::secret_set,
            ai_commands::secret_list,
            ai_commands::secret_delete,
            ai_commands::ai_conversations_list,
            ai_commands::ai_conversation_create,
            ai_commands::ai_conversation_delete,
            ai_commands::ai_messages,
            ai_commands::ai_ollama_models,
            ai_commands::ai_send,
            ai_commands::ai_commit_message,
            ai_commands::ai_tool_respond,
            ai_commands::ai_memory_list,
            ai_commands::ai_memory_add,
            ai_commands::ai_memory_delete,
            index_commands::index_project,
            index_commands::index_stats,
            index_commands::index_search,
            fs_commands::fs_list_dir,
            fs_commands::fs_read_file,
            fs_commands::fs_find,
            docker_commands::docker_ping,
            docker_commands::docker_containers,
            docker_commands::docker_images,
            docker_commands::docker_start,
            docker_commands::docker_stop,
            docker_commands::docker_restart,
            docker_commands::docker_logs,
            api_commands::api_send,
            api_commands::api_save,
            api_commands::api_requests,
            api_commands::api_request_delete,
            api_commands::api_history,
            db_commands::db_connections,
            db_commands::db_connect,
            db_commands::db_connection_delete,
            db_commands::db_schema,
            db_commands::db_query,
            db_commands::db_table_rows,
            system_commands::system_snapshot,
            monitor_commands::monitors_list,
            monitor_commands::monitor_create,
            monitor_commands::monitor_delete,
            monitor_commands::monitor_toggle,
            monitor_commands::monitor_check_now,
            deploy_commands::deploy_configured,
            deploy_commands::deploy_projects,
            deploy_commands::deploy_list,
            issue_commands::issue_configured,
            issue_commands::issue_capture,
            issue_commands::issue_targets,
            issue_commands::issue_create,
            issue_commands::issue_copy_image,
            snippet_commands::snippets_list,
            snippet_commands::snippets_search,
            snippet_commands::snippet_save,
            snippet_commands::snippet_delete,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod data_dir_tests {
    use super::*;

    /// The guarantee that matters most: with the variable unset, the resolved
    /// directory is `app_data_dir()` and nothing else. If this ever fails, an
    /// existing user's database silently moved.
    #[test]
    fn unset_override_returns_app_data_dir_unchanged() {
        let expected = PathBuf::from(r"C:\Users\someone\AppData\Roaming\com.peshang.devos");
        let resolved = data_dir(None, || Ok(expected.clone())).unwrap();
        assert_eq!(resolved, expected);
    }

    /// `DEVOS_DATA_DIR=` (or a value that is all whitespace) is a mistake, not
    /// a request to use the process's working directory.
    #[test]
    fn blank_override_is_treated_as_unset() {
        let expected = PathBuf::from(r"C:\real\app\data");
        for blank in ["", "   ", "\t"] {
            let resolved = data_dir(Some(blank), || Ok(expected.clone())).unwrap();
            assert_eq!(resolved, expected, "{blank:?} should not override");
        }
        assert_eq!(effective_override(Some("  ")), None);
        assert_eq!(effective_override(None), None);
    }

    /// A set override wins, is created if missing, and — asserted by the
    /// closure that panics rather than by an equality check — the default is
    /// never even computed, so no `%APPDATA%` lookup happens on a machine that
    /// deliberately has none.
    #[test]
    fn override_replaces_default_and_creates_the_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nested").join("data");
        assert!(!target.exists());

        let resolved = data_dir(Some(target.to_str().unwrap()), || {
            unreachable!("app_data_dir() must not be consulted when the override is set")
        })
        .unwrap();

        assert_eq!(resolved, target);
        assert!(
            target.is_dir(),
            "override directory should have been created"
        );
    }

    /// Surrounding whitespace is trimmed — quoting an env var in a shell
    /// script is a common way to acquire a trailing space.
    #[test]
    fn override_is_trimmed() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("padded");
        let padded = format!("  {}  ", target.display());
        let resolved = data_dir(Some(&padded), || unreachable!()).unwrap();
        assert_eq!(resolved, target);
    }

    /// An unusable override fails loudly at startup instead of falling back to
    /// the real database — silently writing to `%APPDATA%` when the caller
    /// asked for isolation is the one outcome a test seam must never produce.
    #[test]
    fn unusable_override_is_an_error_not_a_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        // A regular file where a directory was asked for.
        let file = tmp.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        let target = file.join("child");

        let err = data_dir(Some(target.to_str().unwrap()), || unreachable!()).unwrap_err();
        assert!(
            err.to_string().contains(DATA_DIR_ENV),
            "error should name the variable, got: {err}"
        );
    }
}
