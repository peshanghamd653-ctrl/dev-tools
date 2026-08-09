mod ai_commands;
mod api_commands;
mod approvals;
mod audit_commands;
mod backup_commands;
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
mod startup_error;
mod state;
mod system_commands;
mod term_commands;
mod test_runner;
mod tools;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

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
///   * It does **not** move the WebView2 profile, and that is worth knowing
///     because the note's whole value is being exhaustive. `localStorage`
///     stays where the webview keeps it, so a redirected data directory still
///     carries over the persisted AI *read*-tool grant (`devos-ai`), the theme
///     preference, and UI state. Someone pointing this at a sandbox expecting
///     a clean boot gets a clean database and a grant they made earlier.
///     Nothing escalates — the write grant is deliberately session-scoped and
///     is not persisted at all — but a "fresh" profile is not fresh.
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

/// Where `startup_ms` actually goes.
///
/// `startup_ms` spans the top of [`run`] to "devos kernel ready". Until this
/// existed it was a single scalar with `boot_ms` inside it and nothing else
/// named, so it could say *that* startup regressed but never *where* — and on
/// a real database the unnamed part turned out to be most of it. The phases
/// below partition the whole span. Each is an `Instant::now()` and an
/// `elapsed()`, matching what `Kernel::boot` already does: a few nanoseconds,
/// no allocation, no feature flag, always on.
///
/// The phase that is not obvious is `webview_us`, and it is the large one.
/// Tauri creates the windows declared in `tauri.conf.json` — and with each
/// one, on Windows, a WebView2 environment and controller — inside its *own*
/// `setup`, immediately **before** it calls the closure given to
/// [`tauri::Builder::setup`]:
///
/// ```text
/// fn setup(app) {                                  // tauri::app::setup
///     for window_config in app.config().app.windows { ... build()?; }   // WebView2
///     app.manager.assets.setup(app);
///     (user_setup)(app)?;                          // <- this crate's closure
/// }
/// ```
///
/// and that runs from the event loop's `Ready` event, i.e. inside `App::run`,
/// not inside `App::build`. So the gap between `build` returning and this
/// crate's closure being entered is event-loop start plus window and webview
/// creation — synchronous, blocking, and charged to `startup_ms` in full.
/// Splitting `Builder::run` into `build` + `run` is what makes that gap
/// visible; it is otherwise identical to what `Builder::run` does.
///
/// What is still *not* covered: everything before the first line of [`run`] —
/// process creation, loading the binary and its DLLs, CRT startup. `Instant`
/// cannot see behind its own first tick, so that time is missing from
/// `startup_ms` itself, not merely unattributed within it.
#[derive(Clone, Copy)]
struct StartupPhases {
    tracing_init_us: u64,
    plugins_registered_us: u64,
    context_us: u64,
    app_build_us: u64,
    /// When `App::build` returned. The setup closure subtracts this from
    /// "now" to get `webview_us`.
    built_at: Instant,
}

impl StartupPhases {
    /// Zeroed timings, as if the build had just finished.
    ///
    /// Structurally unreachable: the record is written before `App::run` is
    /// called and the setup closure only runs from inside it. It exists so
    /// that a missing record degrades to zeroed numbers rather than a panic —
    /// a panic in the setup hook is a process that exits 101 with no window
    /// and no message, which is the failure `startup_error` exists to
    /// prevent, and no timing number is worth it.
    fn unrecorded() -> Self {
        Self {
            tracing_init_us: 0,
            plugins_registered_us: 0,
            context_us: 0,
            app_build_us: 0,
            built_at: Instant::now(),
        }
    }
}

pub fn run() {
    let start = Instant::now();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let tracing_init_us = start.elapsed().as_micros() as u64;

    // Written once, after `build()` returns, and read from inside the setup
    // closure — which has to be handed to the builder before those numbers
    // exist. A `OnceLock` rather than a `Mutex<Option<_>>` because it is
    // written exactly once and read after, and the type should say so.
    let recorded: Arc<OnceLock<StartupPhases>> = Arc::new(OnceLock::new());
    let setup_phases = recorded.clone();

    let phase = Instant::now();
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // Native folder picker, used by "Add project". Only `dialog:allow-open`
        // is granted in capabilities — no save dialogs, no message boxes.
        .plugin(tauri_plugin_dialog::init())
        // Self-update. The update *manifest* is signed with a minisign key
        // whose public half lives in tauri.conf.json — that signature, not
        // TLS, is what makes an update trustworthy: a compromised release
        // host still cannot hand this app a payload it will install. The
        // private half must never enter the repository; it belongs in the
        // release workflow's secrets. Note this is entirely separate from
        // Authenticode code signing, which is what stops SmartScreen warning
        // on first install — the two solve different problems and one does
        // not substitute for the other.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            // Everything tauri does between `build()` returning and this line:
            // starting the event loop, and creating the configured window
            // together with its WebView2 environment and controller. See
            // [`StartupPhases`].
            let phases = setup_phases
                .get()
                .copied()
                .unwrap_or_else(StartupPhases::unrecorded);
            let webview_us = phases.built_at.elapsed().as_micros() as u64;

            let phase = Instant::now();
            let raw_override = std::env::var(DATA_DIR_ENV).ok();
            let source = match effective_override(raw_override.as_deref()) {
                Some(_) => DATA_DIR_ENV,
                None => "app_data_dir",
            };
            // Every fallible step below reports through `startup_error::fatal`
            // rather than `?`. Returning an error here is not a quiet failure:
            // Tauri turns it into a panic, and a windows-subsystem binary has
            // no console, so the user gets an exit code and nothing else.
            let data_dir = match data_dir(raw_override.as_deref(), || app.path().app_data_dir()) {
                Ok(dir) => dir,
                Err(e) => startup_error::fatal(
                    app.handle(),
                    "locating the application data directory",
                    &e,
                    None,
                ),
            };
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
            if let Err(e) = app
                .asset_protocol_scope()
                .allow_directory(&screenshots, false)
            {
                startup_error::fatal(
                    app.handle(),
                    &format!("granting access to {}", screenshots.display()),
                    &e,
                    Some(&data_dir),
                );
            }

            let data_dir_us = phase.elapsed().as_micros() as u64;

            // The realistic failure point, and the one that produced the
            // silent exit this handling exists for: `Kernel::boot` opens the
            // database and runs migrations.
            let phase = Instant::now();
            let mut kernel = match tauri::async_runtime::block_on(Kernel::boot(&db_path)) {
                Ok(k) => k,
                Err(e) => {
                    startup_error::fatal(app.handle(), "opening the database", &e, Some(&data_dir))
                }
            };
            let kernel_boot_us = phase.elapsed().as_micros() as u64;

            let phase = Instant::now();
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
            let modules_us = phase.elapsed().as_micros() as u64;
            let kernel = Arc::new(kernel);

            // Each module's `CREATE TABLE IF NOT EXISTS` pass. Timed
            // separately rather than as one lump because they are the phases
            // that touch the largest tables — `devos_index` owns
            // `index_chunks` and `index_embeddings`, which are the bulk of a
            // real database — and "table init got slower as the index grew"
            // is precisely the shape of regression a single total would hide.
            let phase = Instant::now();
            let secrets = tauri::async_runtime::block_on(SecretStore::init(kernel.pool.clone()))?;
            let secrets_us = phase.elapsed().as_micros() as u64;

            let phase = Instant::now();
            tauri::async_runtime::block_on(devos_ai::repo::init(&kernel.pool))
                .map_err(|e| format!("ai tables: {e}"))?;
            let ai_tables_us = phase.elapsed().as_micros() as u64;

            let phase = Instant::now();
            tauri::async_runtime::block_on(devos_index::init(&kernel.pool))
                .map_err(|e| format!("index tables: {e}"))?;
            let index_tables_us = phase.elapsed().as_micros() as u64;

            let phase = Instant::now();
            tauri::async_runtime::block_on(devos_api::init(&kernel.pool))
                .map_err(|e| format!("api tables: {e}"))?;
            let api_tables_us = phase.elapsed().as_micros() as u64;

            let phase = Instant::now();
            tauri::async_runtime::block_on(devos_db::init(&kernel.pool))
                .map_err(|e| format!("db tables: {e}"))?;
            let db_tables_us = phase.elapsed().as_micros() as u64;

            let phase = Instant::now();
            tauri::async_runtime::block_on(devos_monitor::init(&kernel.pool))
                .map_err(|e| format!("monitor tables: {e}"))?;
            let monitor_tables_us = phase.elapsed().as_micros() as u64;

            let phase = Instant::now();
            tauri::async_runtime::block_on(devos_snippets::init(&kernel.pool))
                .map_err(|e| format!("snippet tables: {e}"))?;
            let snippet_tables_us = phase.elapsed().as_micros() as u64;

            let phase = Instant::now();
            tauri::async_runtime::block_on(devos_system::init(&kernel.pool))
                .map_err(|e| format!("system tables: {e}"))?;
            let system_tables_us = phase.elapsed().as_micros() as u64;

            tracing::info!(
                secrets_us,
                ai_tables_us,
                index_tables_us,
                api_tables_us,
                db_tables_us,
                monitor_tables_us,
                snippet_tables_us,
                system_tables_us,
                "module tables initialised"
            );
            let tables_us = secrets_us
                + ai_tables_us
                + index_tables_us
                + api_tables_us
                + db_tables_us
                + monitor_tables_us
                + snippet_tables_us
                + system_tables_us;

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
            // One line that adds up. The fields before `webview_us` happen on
            // the way in to tauri's event loop; the ones after happen inside
            // this closure. Their sum is `startup_ms` minus the handful of
            // microseconds spent between phases. See [`StartupPhases`] for
            // what each one spans and for what is deliberately outside all of
            // them.
            tracing::info!(
                startup_ms,
                tracing_init_us = phases.tracing_init_us,
                plugins_registered_us = phases.plugins_registered_us,
                context_us = phases.context_us,
                app_build_us = phases.app_build_us,
                webview_us,
                data_dir_us,
                kernel_boot_us,
                modules_us,
                tables_us,
                "devos kernel ready"
            );
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

            // The performance profiler's sampler: records CPU/memory every
            // 30s for the history chart. Built here (not inside `AppState`
            // below) so the same `Arc<SystemProbe>` backs both the scheduler
            // and the live `system_snapshot` command — a second probe would
            // start its own CPU-delta baseline and report 0% until its own
            // minimum interval had passed.
            let system_probe = Arc::new(SystemProbe::new());
            tauri::async_runtime::spawn(devos_system::run_scheduler(
                kernel.clone(),
                system_probe.clone(),
            ));

            // The daily backup, moved out of `Kernel::boot` — see
            // `Kernel::run_daily_backup` for why it was moved and why it is
            // safe to run detached from here specifically. It holds the same
            // `Arc<Kernel>` every other background task on this line does, so
            // the pool it needs cannot close out from under it during ordinary
            // operation, and on process exit it is cancelled together with
            // every other spawned task rather than outliving a closed pool —
            // which is the failure this cannot repeat: a bare `tokio::spawn`
            // holding a bare `SqlitePool` inside `boot` itself, tried first,
            // kept the connection alive past a clean shutdown and recreated
            // the `-wal`/`-shm` sidecars the shutdown had just deleted.
            {
                let kernel = kernel.clone();
                tauri::async_runtime::spawn(async move {
                    kernel.run_daily_backup().await;
                });
            }

            app.manage(AppState {
                kernel,
                terminal,
                db: Arc::new(DbManager::new()),
                secrets,
                ai: Arc::new(AiRegistry::new()),
                approvals: Arc::new(approvals::ApprovalRegistry::default()),
                system: system_probe,
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
            audit_commands::audit_log,
            backup_commands::backups_list,
            backup_commands::backup_restore_stage,
            backup_commands::backup_restore_cancel,
            backup_commands::backup_restart,
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
            index_commands::index_find_symbols,
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
            api_commands::api_environments,
            api_commands::api_environment_create,
            api_commands::api_environment_update,
            api_commands::api_environment_delete,
            api_commands::api_environment_set_active,
            db_commands::db_connections,
            db_commands::db_connect,
            db_commands::db_connection_delete,
            db_commands::db_schema,
            db_commands::db_query,
            db_commands::db_table_rows,
            system_commands::system_snapshot,
            system_commands::system_history,
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
        ]);
    let plugins_registered_us = phase.elapsed().as_micros() as u64;

    // `generate_context!` is mostly compile-time, but it still assembles the
    // config, the embedded asset map and the icons at run time. Timed on its
    // own so "the bundle got bigger" and "the builder got slower" cannot be
    // mistaken for each other.
    let phase = Instant::now();
    let context = tauri::generate_context!();
    let context_us = phase.elapsed().as_micros() as u64;

    // `Builder::run` is exactly `build(context)?.run(|_, _| {})`. Splitting it
    // is what lets the setup closure see when `build` finished, and therefore
    // how long tauri spent creating the window and its webview before calling
    // it. Nothing else changes: the same error is raised at the same point,
    // with the same message.
    let phase = Instant::now();
    let app = builder
        .build(context)
        .expect("error while running tauri application");
    let app_build_us = phase.elapsed().as_micros() as u64;

    // Last thing before the event loop, so `built_at` is the true start of
    // the window-and-webview phase the setup closure reports.
    let _ = recorded.set(StartupPhases {
        tracing_init_us,
        plugins_registered_us,
        context_us,
        app_build_us,
        built_at: Instant::now(),
    });

    app.run(|_, _| {});
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
