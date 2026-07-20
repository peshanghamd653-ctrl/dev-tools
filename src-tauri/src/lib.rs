mod ai_commands;
mod approvals;
mod commands;
mod core_module;
mod git_commands;
mod index_commands;
mod state;
mod term_commands;
mod tools;

use std::sync::Arc;

use devos_ai::AiRegistry;
use devos_kernel::Kernel;
use devos_secrets::SecretStore;
use devos_terminal::TerminalManager;
use tauri::{Emitter, Manager};
use tokio::sync::broadcast::error::RecvError;

use state::AppState;

pub fn run() {
    let start = std::time::Instant::now();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let db_path = app.path().app_data_dir()?.join("devos.db");
            let mut kernel = tauri::async_runtime::block_on(Kernel::boot(&db_path))?;
            kernel.register_module(&core_module::CoreModule);
            kernel.register_module(&devos_terminal::TerminalModule);
            kernel.register_module(&devos_git::GitModule);
            kernel.register_module(&devos_ai::AiModule);
            kernel.register_module(&devos_index::IndexModule);
            let kernel = Arc::new(kernel);

            let secrets = tauri::async_runtime::block_on(SecretStore::init(kernel.pool.clone()))?;
            tauri::async_runtime::block_on(devos_ai::repo::init(&kernel.pool))
                .map_err(|e| format!("ai tables: {e}"))?;
            tauri::async_runtime::block_on(devos_index::init(&kernel.pool))
                .map_err(|e| format!("index tables: {e}"))?;

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
            app.manage(AppState {
                kernel,
                terminal: Arc::new(TerminalManager::new()),
                secrets,
                ai: Arc::new(AiRegistry::new()),
                approvals: Arc::new(approvals::ApprovalRegistry::default()),
                startup_ms,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
