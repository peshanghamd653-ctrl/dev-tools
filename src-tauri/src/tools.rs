//! Read-only project tools for the AI agent loop.
//!
//! Every path is resolved against the project root and canonicalized; a
//! result outside the root is rejected, so `../` and absolute paths cannot
//! escape the project the user granted access to.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use devos_ai::{ToolDef, ToolExecutor};
use serde_json::{json, Value};

use crate::approvals::ApprovalGate;

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_LIST_ENTRIES: usize = 500;
const MAX_FIND_RESULTS: usize = 200;
const MAX_WALK_DEPTH: usize = 12;
const MAX_COMMAND_OUTPUT: usize = 32 * 1024;
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".venv",
    "__pycache__",
];

pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "read_file".into(),
            description: "Read a text file from the active project. Path is relative to the project root.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative file path, e.g. src/main.rs" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "list_dir".into(),
            description: "List entries of a directory in the active project. Directories end with '/'.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative directory path; omit for the project root" }
                }
            }),
        },
        ToolDef {
            name: "find_files".into(),
            description: "Find files in the project whose relative path contains the query (case-insensitive).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Substring to match against relative paths" }
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "search_code".into(),
            description: "Full-text search the project's indexed content (ranked, with file:line and snippets). Use this to find where something is implemented or mentioned. If the project is not indexed yet the result says so.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Words or code fragments to search for" }
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "save_memory".into(),
            description: "Save a short durable fact about this project to DevOS memory (e.g. conventions, decisions, preferences the user asks you to remember). It will be included in future conversations. The user can see and delete every entry. Max 500 characters.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "One concise fact worth remembering" }
                },
                "required": ["content"]
            }),
        },
    ]
}

/// Mutating tools, offered only when the user grants the second (write)
/// capability level. Every call still goes through the approval gate
/// individually — the grant makes the tools *exist*, approval makes each
/// call *run* (ADR-0005).
pub fn write_tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "edit_file".into(),
            description: "Replace an exact string in a project file. old_string must occur exactly once. Requires user approval per call.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative file path" },
                    "old_string": { "type": "string", "description": "Exact text to replace (must be unique in the file)" },
                    "new_string": { "type": "string", "description": "Replacement text" }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
        ToolDef {
            name: "write_file".into(),
            description: "Create a NEW file in the project (fails if it already exists; use edit_file to change existing files). Requires user approval per call.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative file path; parent directory must exist" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDef {
            name: "run_command".into(),
            description: "Run a shell command in the project root (60s timeout, output captured). Requires user approval per call.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The command line to run" }
                },
                "required": ["command"]
            }),
        },
    ]
}

pub struct ProjectTools {
    root: PathBuf,
    /// Kernel pool, for `search_code` against the FTS index.
    pool: sqlx::SqlitePool,
    /// Present only when the user granted write/execute capability.
    gate: Option<Arc<dyn ApprovalGate>>,
    command_timeout: Duration,
}

impl ProjectTools {
    pub fn new(root: PathBuf, pool: sqlx::SqlitePool) -> Self {
        Self {
            root,
            pool,
            gate: None,
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
        }
    }

    pub fn with_write_access(
        root: PathBuf,
        pool: sqlx::SqlitePool,
        gate: Arc<dyn ApprovalGate>,
    ) -> Self {
        Self {
            root,
            pool,
            gate: Some(gate),
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
        }
    }

    #[cfg(test)]
    fn with_command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    /// Join + canonicalize + verify containment (shared guard).
    fn resolve(&self, relative: &str) -> Result<PathBuf, String> {
        crate::pathsafe::resolve_in_root(&self.root, relative)
    }

    fn read_file(&self, input: &Value) -> Result<String, String> {
        let relative = input["path"].as_str().ok_or("missing 'path'")?;
        let path = self.resolve(relative)?;
        let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_file() {
            return Err(format!("not a file: {relative}"));
        }
        if meta.len() > MAX_FILE_BYTES {
            return Err(format!("file too large ({} bytes): {relative}", meta.len()));
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        if bytes.contains(&0) {
            return Err(format!("binary file: {relative}"));
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        Ok(if text.len() > MAX_OUTPUT_BYTES {
            format!(
                "{}\n… truncated …",
                &text[..floor_char(&text, MAX_OUTPUT_BYTES)]
            )
        } else {
            text
        })
    }

    fn list_dir(&self, input: &Value) -> Result<String, String> {
        let relative = input["path"].as_str().unwrap_or(".");
        let path = self.resolve(relative)?;
        if !path.is_dir() {
            return Err(format!("not a directory: {relative}"));
        }
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() {
                dirs.push(format!("{name}/"));
            } else {
                files.push(name);
            }
            if dirs.len() + files.len() >= MAX_LIST_ENTRIES {
                break;
            }
        }
        dirs.sort();
        files.sort();
        dirs.extend(files);
        if dirs.is_empty() {
            Ok("(empty directory)".into())
        } else {
            Ok(dirs.join("\n"))
        }
    }

    fn find_files(&self, input: &Value) -> Result<String, String> {
        let query = input["query"]
            .as_str()
            .ok_or("missing 'query'")?
            .to_lowercase();
        if query.trim().is_empty() {
            return Err("query is empty".into());
        }
        let mut matches = Vec::new();
        walk(&self.root, &self.root, &query, 0, &mut matches);
        if matches.is_empty() {
            Ok(format!("no files matching \"{query}\""))
        } else {
            Ok(matches.join("\n"))
        }
    }

    async fn save_memory(&self, input: &Value) -> Result<String, String> {
        let content = input["content"].as_str().ok_or("missing 'content'")?;
        let project = devos_index::project_key(&self.root.to_string_lossy());
        let entry = devos_ai::repo::memory_add(&self.pool, &project, content)
            .await
            .map_err(|e| e.to_string())?;
        Ok(format!("saved to project memory: {}", entry.content))
    }

    async fn search_code(&self, input: &Value) -> Result<String, String> {
        let query = input["query"].as_str().ok_or("missing 'query'")?;
        let root = self.root.to_string_lossy().into_owned();
        let stats = devos_index::stats(&self.pool, &root)
            .await
            .map_err(|e| e.to_string())?;
        if stats.files == 0 {
            return Ok(
                "The project is not indexed yet. Ask the user to run \"Index Project for AI \
                 Search\" from the command palette or the Projects page, then search again."
                    .into(),
            );
        }
        let hits = devos_index::search(&self.pool, &root, query, 12)
            .await
            .map_err(|e| e.to_string())?;
        if hits.is_empty() {
            return Ok(format!("no matches for \"{query}\""));
        }
        Ok(hits
            .iter()
            .map(|h| {
                format!(
                    "{}:{}\n  {}",
                    h.file,
                    h.start_line,
                    h.snippet.replace('\n', " ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// Containment check for paths that may not exist yet: no `..`/absolute
    /// components allowed, parent directory must exist inside the root.
    fn resolve_for_write(&self, relative: &str) -> Result<PathBuf, String> {
        let rel_path = Path::new(relative);
        if rel_path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(format!("path escapes the project: {relative}"));
        }
        let root = std::fs::canonicalize(&self.root)
            .map_err(|_| "project root does not exist".to_string())?;
        let target = self.root.join(rel_path);
        let parent = target
            .parent()
            .ok_or_else(|| format!("invalid path: {relative}"))?;
        let canonical_parent = std::fs::canonicalize(parent)
            .map_err(|_| format!("parent directory not found: {relative}"))?;
        if !canonical_parent.starts_with(&root) {
            return Err(format!("path escapes the project: {relative}"));
        }
        let file_name = target
            .file_name()
            .ok_or_else(|| format!("invalid path: {relative}"))?;
        Ok(canonical_parent.join(file_name))
    }

    fn edit_file(&self, input: &Value) -> Result<String, String> {
        let relative = input["path"].as_str().ok_or("missing 'path'")?;
        let old_string = input["old_string"].as_str().ok_or("missing 'old_string'")?;
        let new_string = input["new_string"].as_str().ok_or("missing 'new_string'")?;
        if old_string.is_empty() {
            return Err("old_string is empty — use write_file for new files".into());
        }
        let path = self.resolve(relative)?;
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("could not read {relative}: {e}"))?;
        let occurrences = content.matches(old_string).count();
        if occurrences == 0 {
            return Err(format!("old_string not found in {relative}"));
        }
        if occurrences > 1 {
            return Err(format!(
                "old_string occurs {occurrences} times in {relative}; provide a longer, unique snippet"
            ));
        }
        let updated = content.replacen(old_string, new_string, 1);
        std::fs::write(&path, &updated).map_err(|e| e.to_string())?;
        Ok(format!(
            "edited {relative}: -{} +{} chars",
            old_string.len(),
            new_string.len()
        ))
    }

    fn write_file(&self, input: &Value) -> Result<String, String> {
        let relative = input["path"].as_str().ok_or("missing 'path'")?;
        let content = input["content"].as_str().ok_or("missing 'content'")?;
        let path = self.resolve_for_write(relative)?;
        if path.exists() {
            return Err(format!(
                "{relative} already exists — use edit_file to change it"
            ));
        }
        std::fs::write(&path, content).map_err(|e| e.to_string())?;
        Ok(format!("created {relative} ({} bytes)", content.len()))
    }

    async fn run_command(&self, input: &Value) -> Result<String, String> {
        let command = input["command"].as_str().ok_or("missing 'command'")?;
        if command.trim().is_empty() {
            return Err("command is empty".into());
        }

        let mut cmd = if cfg!(windows) {
            let mut c = tokio::process::Command::new("cmd");
            c.args(["/C", command]);
            c
        } else {
            let mut c = tokio::process::Command::new("sh");
            c.args(["-c", command]);
            c
        };
        cmd.current_dir(&self.root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

        let output = tokio::time::timeout(self.command_timeout, cmd.output())
            .await
            .map_err(|_| {
                format!(
                    "command timed out after {}s",
                    self.command_timeout.as_secs()
                )
            })?
            .map_err(|e| e.to_string())?;

        let mut report = String::new();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stdout.trim().is_empty() {
            report.push_str(stdout.trim_end());
        }
        if !stderr.trim().is_empty() {
            if !report.is_empty() {
                report.push_str("\n--- stderr ---\n");
            }
            report.push_str(stderr.trim_end());
        }
        if report.len() > MAX_COMMAND_OUTPUT {
            report.truncate(floor_char(&report, MAX_COMMAND_OUTPUT));
            report.push_str("\n… output truncated …");
        }
        let code = output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "unknown".into());
        if report.is_empty() {
            Ok(format!("(no output) exit code {code}"))
        } else {
            Ok(format!("{report}\n(exit code {code})"))
        }
    }
}

fn walk(root: &Path, dir: &Path, query: &str, depth: usize, matches: &mut Vec<String>) {
    if depth > MAX_WALK_DEPTH || matches.len() >= MAX_FIND_RESULTS {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if matches.len() >= MAX_FIND_RESULTS {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk(root, &path, query, depth + 1, matches);
            }
        } else if let Ok(relative) = path.strip_prefix(root) {
            let relative = relative.to_string_lossy().replace('\\', "/");
            if relative.to_lowercase().contains(query) {
                matches.push(relative);
            }
        }
    }
}

fn floor_char(s: &str, max: usize) -> usize {
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

#[async_trait::async_trait]
impl ToolExecutor for ProjectTools {
    async fn execute(&self, name: &str, input: &Value) -> Result<String, String> {
        match name {
            "read_file" => self.read_file(input),
            "list_dir" => self.list_dir(input),
            "find_files" => self.find_files(input),
            "search_code" => self.search_code(input).await,
            "save_memory" => self.save_memory(input).await,
            "edit_file" | "write_file" | "run_command" => {
                let gate = self
                    .gate
                    .as_ref()
                    .ok_or("write access has not been granted for this conversation")?;
                if !gate.request(name, input).await? {
                    return Err("denied by user".into());
                }
                match name {
                    "edit_file" => self.edit_file(input),
                    "write_file" => self.write_file(input),
                    _ => self.run_command(input).await,
                }
            }
            other => Err(format!("unknown tool: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool(dir: &Path) -> sqlx::SqlitePool {
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(dir.join("tools-test.db"))
            .create_if_missing(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        devos_index::init(&pool).await.unwrap();
        pool
    }

    async fn fixture() -> (tempfile::TempDir, ProjectTools) {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(project.join("README.md"), "# hello\n").unwrap();
        std::fs::create_dir_all(project.join("node_modules/junk")).unwrap();
        std::fs::write(project.join("node_modules/junk/x.js"), "skip me").unwrap();
        let pool = test_pool(dir.path()).await;
        let tools = ProjectTools::new(project, pool);
        (dir, tools)
    }

    #[tokio::test]
    async fn reads_files_and_lists_dirs() {
        let (_dir, tools) = fixture().await;
        let content = tools
            .execute("read_file", &json!({"path": "src/main.rs"}))
            .await
            .unwrap();
        assert!(content.contains("fn main"));

        let listing = tools.execute("list_dir", &json!({})).await.unwrap();
        assert!(listing.contains("src/"));
        assert!(listing.contains("README.md"));
    }

    #[tokio::test]
    async fn rejects_path_traversal_and_absolute_paths() {
        let (_dir, tools) = fixture().await;
        let escape = tools
            .execute("read_file", &json!({"path": "../../Windows/win.ini"}))
            .await;
        assert!(escape.is_err(), "traversal must be rejected: {escape:?}");

        let absolute = tools
            .execute("read_file", &json!({"path": "C:/Windows/win.ini"}))
            .await;
        assert!(absolute.is_err(), "absolute path must be rejected");
    }

    #[tokio::test]
    async fn find_skips_dependency_dirs() {
        let (_dir, tools) = fixture().await;
        let found = tools
            .execute("find_files", &json!({"query": "main"}))
            .await
            .unwrap();
        assert!(found.contains("src/main.rs"));

        let skipped = tools
            .execute("find_files", &json!({"query": "x.js"}))
            .await
            .unwrap();
        assert!(
            skipped.contains("no files matching"),
            "node_modules must be skipped: {skipped}"
        );
    }

    #[tokio::test]
    async fn unknown_tool_errors() {
        let (_dir, tools) = fixture().await;
        assert!(tools
            .execute("delete_everything", &json!({}))
            .await
            .is_err());
    }

    // ---- write/execute tools ----

    struct StubGate {
        approve: bool,
        calls: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl ApprovalGate for StubGate {
        async fn request(&self, name: &str, _input: &Value) -> Result<bool, String> {
            self.calls.lock().unwrap().push(name.to_string());
            Ok(self.approve)
        }
    }

    async fn write_fixture(approve: bool) -> (tempfile::TempDir, ProjectTools, Arc<StubGate>) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "fn one() {}\nfn two() {}\n").unwrap();
        let gate = Arc::new(StubGate {
            approve,
            calls: std::sync::Mutex::new(Vec::new()),
        });
        let pool = test_pool(dir.path()).await;
        let tools = ProjectTools::with_write_access(dir.path().to_path_buf(), pool, gate.clone());
        (dir, tools, gate)
    }

    #[tokio::test]
    async fn edit_file_requires_approval_and_unique_match() {
        let (dir, tools, gate) = write_fixture(true).await;
        let result = tools
            .execute(
                "edit_file",
                &json!({"path": "src/lib.rs", "old_string": "fn one()", "new_string": "fn renamed()"}),
            )
            .await
            .unwrap();
        assert!(result.contains("edited"));
        assert_eq!(gate.calls.lock().unwrap().as_slice(), ["edit_file"]);
        let content = std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("fn renamed()"));

        // Ambiguous old_string is rejected before touching the file.
        let ambiguous = tools
            .execute(
                "edit_file",
                &json!({"path": "src/lib.rs", "old_string": "fn ", "new_string": "x"}),
            )
            .await;
        assert!(ambiguous.unwrap_err().contains("2 times"));
    }

    #[tokio::test]
    async fn denied_call_leaves_file_untouched() {
        let (dir, tools, _gate) = write_fixture(false).await;
        let result = tools
            .execute(
                "edit_file",
                &json!({"path": "src/lib.rs", "old_string": "fn one()", "new_string": "gone"}),
            )
            .await;
        assert_eq!(result.unwrap_err(), "denied by user");
        let content = std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(content.contains("fn one()"), "file must be unchanged");
    }

    #[tokio::test]
    async fn write_tools_absent_without_grant() {
        let (_dir, tools) = fixture().await; // ProjectTools::new — no gate
        let result = tools
            .execute("write_file", &json!({"path": "x.txt", "content": "hi"}))
            .await;
        assert!(result.unwrap_err().contains("not been granted"));
    }

    #[tokio::test]
    async fn write_file_creates_once_and_stays_contained() {
        let (dir, tools, _gate) = write_fixture(true).await;
        tools
            .execute(
                "write_file",
                &json!({"path": "src/new.rs", "content": "// new\n"}),
            )
            .await
            .unwrap();
        assert!(dir.path().join("src/new.rs").exists());

        let again = tools
            .execute("write_file", &json!({"path": "src/new.rs", "content": "x"}))
            .await;
        assert!(again.unwrap_err().contains("already exists"));

        let escape = tools
            .execute(
                "write_file",
                &json!({"path": "../outside.txt", "content": "x"}),
            )
            .await;
        assert!(escape.unwrap_err().contains("escapes the project"));
        let absolute = tools
            .execute(
                "write_file",
                &json!({"path": "C:/outside.txt", "content": "x"}),
            )
            .await;
        assert!(absolute.is_err());
    }

    #[tokio::test]
    async fn run_command_captures_output_and_times_out() {
        let (_dir, tools, _gate) = write_fixture(true).await;
        let result = tools
            .execute("run_command", &json!({"command": "echo devos_ok"}))
            .await
            .unwrap();
        assert!(result.contains("devos_ok"));
        assert!(result.contains("exit code 0"));

        let (dir2, _, gate2) = write_fixture(true).await;
        let pool2 = test_pool(dir2.path()).await;
        let slow_tools = ProjectTools::with_write_access(dir2.path().to_path_buf(), pool2, gate2)
            .with_command_timeout(Duration::from_secs(1));
        let sleep_cmd = if cfg!(windows) {
            "ping -n 10 127.0.0.1 > NUL"
        } else {
            "sleep 10"
        };
        let timed_out = slow_tools
            .execute("run_command", &json!({"command": sleep_cmd}))
            .await;
        assert!(timed_out.unwrap_err().contains("timed out"));
    }

    #[tokio::test]
    async fn save_memory_persists_via_repo() {
        let (dir, tools) = fixture().await;
        devos_ai::repo::init(&tools.pool).await.unwrap();
        let saved = tools
            .execute(
                "save_memory",
                &json!({"content": "  uses pnpm workspaces  "}),
            )
            .await
            .unwrap();
        assert!(saved.contains("uses pnpm workspaces"));

        let project = devos_index::project_key(&dir.path().join("project").to_string_lossy());
        let entries = devos_ai::repo::memory_list(&tools.pool, &project)
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "uses pnpm workspaces");
    }

    #[tokio::test]
    async fn search_code_reports_unindexed_then_finds_content() {
        let (dir, tools) = fixture().await;
        let unindexed = tools
            .execute("search_code", &json!({"query": "hello"}))
            .await
            .unwrap();
        assert!(unindexed.contains("not indexed"));

        let project = dir.path().join("project");
        devos_index::reindex_project(&tools.pool, &project.to_string_lossy())
            .await
            .unwrap();
        let found = tools
            .execute("search_code", &json!({"query": "hello"}))
            .await
            .unwrap();
        assert!(found.contains("README.md:1"), "got: {found}");
    }
}
