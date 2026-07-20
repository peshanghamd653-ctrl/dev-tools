use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc;

use crate::integration::{encoded_prompt_hook, supports_integration, CommandFailure, OscScanner};
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
    /// Inject the OSC 133 prompt hook (PowerShell family only). The user
    /// can disable this via the `terminal.integration` setting.
    pub shell_integration: bool,
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
    /// Last `TAIL_CAP_BYTES` of raw output.
    buffer: Arc<Mutex<Vec<u8>>>,
}

const TAIL_CAP_BYTES: usize = 32 * 1024;

/// Owns every live pty. Sessions survive UI route changes because they live
/// here, not in the webview.
pub struct TerminalManager {
    sessions: Arc<Mutex<HashMap<String, Arc<Session>>>>,
    failure_tx: mpsc::UnboundedSender<CommandFailure>,
    failure_rx: Mutex<Option<mpsc::UnboundedReceiver<CommandFailure>>>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        let (failure_tx, failure_rx) = mpsc::unbounded_channel();
        Self {
            sessions: Arc::default(),
            failure_tx,
            failure_rx: Mutex::new(Some(failure_rx)),
        }
    }
}

impl TerminalManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// The stream of non-zero command exits detected via OSC 133 markers.
    /// Yields once; the desktop shell consumes it at startup.
    pub fn take_failure_receiver(&self) -> Option<mpsc::UnboundedReceiver<CommandFailure>> {
        self.failure_rx
            .lock()
            .expect("failure receiver lock poisoned")
            .take()
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
        if opts.shell_integration && supports_integration(&shell) {
            cmd.args([
                "-NoLogo",
                "-NoExit",
                "-EncodedCommand",
                &encoded_prompt_hook(),
            ]);
        }
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

        // Recent-output ring buffer: powers AI diagnosis and (later) the
        // build-failure watcher. Owned here so it survives webview reloads.
        let buffer = Arc::new(Mutex::new(Vec::<u8>::new()));

        // Blocking pty reads happen on a dedicated OS thread; events flow out
        // through the unbounded channel (send is sync-safe).
        let out_tx = tx.clone();
        let read_buffer = Arc::clone(&buffer);
        let failure_tx = self.failure_tx.clone();
        let reader_session_id = id.clone();
        std::thread::Builder::new()
            .name(format!("pty-read-{id}"))
            .spawn(move || {
                let mut buf = [0u8; 8192];
                let mut scanner = OscScanner::default();
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            {
                                let mut tail =
                                    read_buffer.lock().expect("tail buffer lock poisoned");
                                tail.extend_from_slice(&buf[..n]);
                                if tail.len() > TAIL_CAP_BYTES {
                                    let excess = tail.len() - TAIL_CAP_BYTES;
                                    tail.drain(..excess);
                                }
                            }
                            for code in scanner.feed(&buf[..n]) {
                                if code != 0 {
                                    let _ = failure_tx.send(CommandFailure {
                                        session_id: reader_session_id.clone(),
                                        exit_code: code,
                                    });
                                }
                            }
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
            buffer,
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

    /// Recent output as plain text: ANSI/OSC sequences and carriage returns
    /// stripped, ready to hand to an AI or a log view.
    pub fn tail(&self, id: &str) -> TermResult<String> {
        let session = self.get(id)?;
        let bytes = session
            .buffer
            .lock()
            .expect("tail buffer lock poisoned")
            .clone();
        Ok(strip_ansi(&String::from_utf8_lossy(&bytes)))
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

/// Remove ANSI escape sequences (CSI, OSC, two-char escapes) and carriage
/// returns, leaving readable text.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                // CSI: ESC [ â€¦paramsâ€¦ final-byte (@ through ~)
                Some('[') => {
                    chars.next();
                    for n in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&n) {
                            break;
                        }
                    }
                }
                // OSC: ESC ] â€¦payloadâ€¦ (BEL or ESC \)
                Some(']') => {
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '\u{07}' {
                            break;
                        }
                        if n == '\u{1b}' {
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                // Two-character escape (ESC + one char).
                _ => {
                    chars.next();
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
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
                shell_integration: false,
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

        // The backend tail mirrors the stream, with escapes stripped.
        let tail = manager.tail(&id).expect("tail");
        assert!(tail.contains("hello_devos"), "tail missing output: {tail}");
        assert!(!tail.contains('\u{1b}'), "tail must be ANSI-free");

        manager.kill(&id).expect("kill removes session");
        assert!(manager.list().is_empty());
    }

    /// End-to-end proof of the watcher chain: PowerShell + injected prompt
    /// hook → OSC 133 marker through ConPTY → scanner → CommandFailure.
    #[tokio::test]
    async fn powershell_integration_reports_failed_commands() {
        let manager = Arc::new(TerminalManager::new());
        let mut failures = manager.take_failure_receiver().expect("first take");
        assert!(manager.take_failure_receiver().is_none(), "yields once");

        let handle = manager
            .create(CreateSessionOptions {
                shell: Some("powershell.exe".into()),
                cwd: None,
                cols: 120,
                rows: 30,
                shell_integration: true,
            })
            .expect("create powershell session");
        let id = handle.info.id.clone();

        // Emulate xterm: answer ConPTY's cursor-position probes so output
        // keeps flowing (same handshake as the cmd.exe test).
        let responder_manager = Arc::clone(&manager);
        let responder_id = id.clone();
        let mut events = handle.events;
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                if let TermEvent::Output { bytes } = event {
                    if bytes.windows(4).any(|w| w == b"\x1b[6n") {
                        let _ = responder_manager.write(&responder_id, b"\x1b[1;1R");
                    }
                }
            }
        });

        // Give PowerShell a moment to boot, then run a failing command.
        tokio::time::sleep(Duration::from_secs(3)).await;
        manager
            .write(&id, b"cmd /c exit 5\r")
            .expect("write command");

        let failure = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let failure = failures.recv().await.expect("failure channel open");
                // The hook may emit a spurious exit-1 while PowerShell warms
                // up; wait for the code we actually produced.
                if failure.exit_code == 5 {
                    return failure;
                }
            }
        })
        .await
        .expect("command failure detected within 30s");
        assert_eq!(failure.session_id, id);
        assert_eq!(failure.exit_code, 5);

        manager.kill(&id).expect("kill session");
    }

    #[test]
    fn strip_ansi_removes_csi_osc_and_carriage_returns() {
        assert_eq!(strip_ansi("\u{1b}[32mgreen\u{1b}[0m"), "green");
        assert_eq!(strip_ansi("\u{1b}]0;window title\u{07}text"), "text");
        assert_eq!(strip_ansi("\u{1b}]133;D;1\u{1b}\\after"), "after");
        assert_eq!(strip_ansi("line1\r\nline2\r"), "line1\nline2");
        assert_eq!(strip_ansi("\u{1b}Mplain"), "plain");
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
                shell_integration: false,
            })
            .expect("create session");
        manager.resize(&handle.info.id, 120, 40).expect("resize");
        assert!(manager.write("nope", b"x").is_err());
        manager.kill(&handle.info.id).expect("kill");
    }
}
