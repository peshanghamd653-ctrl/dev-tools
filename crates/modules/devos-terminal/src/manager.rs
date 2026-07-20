use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc;

use crate::types::{TermEvent, TermSessionInfo};

#[derive(Debug, thiserror::Error)]
pub enum TermError {
    #[error("terminal session not found: {0}")]
    NotFound(String),
    #[error("pty error: {0}")]
    Pty(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type TermResult<T> = Result<T, TermError>;

pub struct CreateSessionOptions {
    pub shell: Option<String>,
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
}

/// Returned from [`TerminalManager::create`]: session metadata plus the
/// event stream the caller forwards to the UI.
pub struct SessionHandle {
    pub info: TermSessionInfo,
    pub events: mpsc::UnboundedReceiver<TermEvent>,
}

struct Session {
    info: Mutex<TermSessionInfo>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

/// Owns every live pty. Sessions survive UI route changes because they live
/// here, not in the webview.
#[derive(Default)]
pub struct TerminalManager {
    sessions: Arc<Mutex<HashMap<String, Arc<Session>>>>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&self, opts: CreateSessionOptions) -> TermResult<SessionHandle> {
        let shell = opts
            .shell
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(default_shell);
        let cwd = opts
            .cwd
            .filter(|c| !c.trim().is_empty())
            .unwrap_or_else(default_cwd);

        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: opts.rows.max(2),
                cols: opts.cols.max(2),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| TermError::Pty(e.to_string()))?;

        let mut cmd = CommandBuilder::new(&shell);
        cmd.cwd(&cwd);
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| TermError::Pty(e.to_string()))?;
        // The slave side belongs to the child now.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| TermError::Pty(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| TermError::Pty(e.to_string()))?;
        let killer = child.clone_killer();

        let id = uuid::Uuid::new_v4().to_string();
        let info = TermSessionInfo {
            id: id.clone(),
            title: shell_title(&shell),
            shell,
            cwd,
            created_at: chrono::Utc::now().timestamp_millis(),
            exited: false,
        };

        let (tx, rx) = mpsc::unbounded_channel();

        // Blocking pty reads happen on a dedicated OS thread; events flow out
        // through the unbounded channel (send is sync-safe).
        let out_tx = tx.clone();
        std::thread::Builder::new()
            .name(format!("pty-read-{id}"))
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if out_tx
                                .send(TermEvent::Output {
                                    bytes: buf[..n].to_vec(),
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            })?;

        let session = Arc::new(Session {
            info: Mutex::new(info.clone()),
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            killer: Mutex::new(killer),
        });
        self.sessions
            .lock()
            .expect("terminal sessions lock poisoned")
            .insert(id.clone(), session.clone());

        // Waiter thread: report the exit code and mark the session exited.
        let exit_tx = tx;
        let wait_id = id.clone();
        let sessions = Arc::clone(&self.sessions);
        std::thread::Builder::new()
            .name(format!("pty-wait-{id}"))
            .spawn(move || {
                let code = child.wait().ok().map(|status| status.exit_code());
                // The reader runs on its own thread; give ConPTY a moment to
                // flush the final frames so Exit is ordered after the output
                // it belongs to.
                std::thread::sleep(std::time::Duration::from_millis(250));
                if let Some(session) = sessions
                    .lock()
                    .expect("terminal sessions lock poisoned")
                    .get(&wait_id)
                {
                    session
                        .info
                        .lock()
                        .expect("session info lock poisoned")
                        .exited = true;
                }
                let _ = exit_tx.send(TermEvent::Exit { code });
                tracing::info!(session = %wait_id, ?code, "terminal session exited");
            })?;

        Ok(SessionHandle { info, events: rx })
    }

    pub fn write(&self, id: &str, data: &[u8]) -> TermResult<()> {
        let session = self.get(id)?;
        let mut writer = session.writer.lock().expect("session writer lock poisoned");
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> TermResult<()> {
        let session = self.get(id)?;
        let master = session.master.lock().expect("session master lock poisoned");
        let result = master.resize(PtySize {
            rows: rows.max(2),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        });
        result.map_err(|e| TermError::Pty(e.to_string()))
    }

    /// Kill the child (if still running) and forget the session.
    pub fn kill(&self, id: &str) -> TermResult<()> {
        let session = self
            .sessions
            .lock()
            .expect("terminal sessions lock poisoned")
            .remove(id)
            .ok_or_else(|| TermError::NotFound(id.to_string()))?;
        let _ = session
            .killer
            .lock()
            .expect("session killer lock poisoned")
            .kill();
        Ok(())
    }

    pub fn list(&self) -> Vec<TermSessionInfo> {
        let mut infos: Vec<TermSessionInfo> = self
            .sessions
            .lock()
            .expect("terminal sessions lock poisoned")
            .values()
            .map(|s| s.info.lock().expect("session info lock poisoned").clone())
            .collect();
        infos.sort_by_key(|info| info.created_at);
        infos
    }

    fn get(&self, id: &str) -> TermResult<Arc<Session>> {
        self.sessions
            .lock()
            .expect("terminal sessions lock poisoned")
            .get(id)
            .cloned()
            .ok_or_else(|| TermError::NotFound(id.to_string()))
    }
}

fn shell_title(shell: &str) -> String {
    std::path::Path::new(shell)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| shell.to_string())
}

#[cfg(windows)]
fn default_shell() -> String {
    // Prefer PowerShell 7 when installed; fall back to Windows PowerShell.
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            if dir.join("pwsh.exe").is_file() {
                return "pwsh.exe".into();
            }
        }
    }
    "powershell.exe".into()
}

#[cfg(not(windows))]
fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
}

#[cfg(windows)]
fn default_cwd() -> String {
    std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into())
}

#[cfg(not(windows))]
fn default_cwd() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn spawns_streams_and_exits() {
        let manager = TerminalManager::new();
        let handle = manager
            .create(CreateSessionOptions {
                shell: Some("cmd.exe".into()),
                cwd: None,
                cols: 80,
                rows: 24,
            })
            .expect("create session");
        let id = handle.info.id.clone();
        assert_eq!(manager.list().len(), 1);

        manager
            .write(&id, b"echo hello_devos\r\nexit\r\n")
            .expect("write to pty");

        let mut events = handle.events;
        let mut output = Vec::new();
        let mut exit_code = None;
        let deadline = tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(event) = events.recv().await {
                match event {
                    TermEvent::Output { bytes } => {
                        // ConPTY probes the terminal with a Device Status
                        // Report (ESC[6n) and stalls until it gets a cursor
                        // position back. xterm.js answers this in the real
                        // app; emulate it here.
                        if bytes.windows(4).any(|w| w == b"\x1b[6n") {
                            manager.write(&id, b"\x1b[1;1R").expect("answer DSR");
                        }
                        output.extend_from_slice(&bytes);
                    }
                    TermEvent::Exit { code } => {
                        exit_code = Some(code);
                        break;
                    }
                }
            }
        })
        .await;

        if deadline.is_err() {
            panic!(
                "terminal did not exit within 20s; output so far: {:?}",
                String::from_utf8_lossy(&output)
            );
        }
        // Any frames that raced the exit notification are already queued.
        while let Ok(TermEvent::Output { bytes }) = events.try_recv() {
            output.extend_from_slice(&bytes);
        }
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("hello_devos"), "missing echo output: {text}");
        assert!(exit_code.is_some(), "no exit event received");
        assert!(manager.list()[0].exited, "session not marked exited");

        manager.kill(&id).expect("kill removes session");
        assert!(manager.list().is_empty());
    }

    #[tokio::test]
    async fn resize_and_missing_session_errors() {
        let manager = TerminalManager::new();
        let handle = manager
            .create(CreateSessionOptions {
                shell: Some("cmd.exe".into()),
                cwd: None,
                cols: 80,
                rows: 24,
            })
            .expect("create session");
        manager.resize(&handle.info.id, 120, 40).expect("resize");
        assert!(manager.write("nope", b"x").is_err());
        manager.kill(&handle.info.id).expect("kill");
    }
}
