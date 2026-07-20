//! Read-only project tools for the AI agent loop.
//!
//! Every path is resolved against the project root and canonicalized; a
//! result outside the root is rejected, so `../` and absolute paths cannot
//! escape the project the user granted access to.

use std::path::{Path, PathBuf};

use devos_ai::{ToolDef, ToolExecutor};
use serde_json::{json, Value};

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_LIST_ENTRIES: usize = 500;
const MAX_FIND_RESULTS: usize = 200;
const MAX_WALK_DEPTH: usize = 12;
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
    ]
}

pub struct ProjectTools {
    root: PathBuf,
}

impl ProjectTools {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Join + canonicalize + verify containment.
    fn resolve(&self, relative: &str) -> Result<PathBuf, String> {
        let root = std::fs::canonicalize(&self.root)
            .map_err(|_| "project root does not exist".to_string())?;
        let target = self.root.join(relative);
        let canonical =
            std::fs::canonicalize(&target).map_err(|_| format!("path not found: {relative}"))?;
        if !canonical.starts_with(&root) {
            return Err(format!("path escapes the project: {relative}"));
        }
        Ok(canonical)
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
            other => Err(format!("unknown tool: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, ProjectTools) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("README.md"), "# hello\n").unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/junk")).unwrap();
        std::fs::write(dir.path().join("node_modules/junk/x.js"), "skip me").unwrap();
        let tools = ProjectTools::new(dir.path().to_path_buf());
        (dir, tools)
    }

    #[tokio::test]
    async fn reads_files_and_lists_dirs() {
        let (_dir, tools) = fixture();
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
        let (_dir, tools) = fixture();
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
        let (_dir, tools) = fixture();
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
        let (_dir, tools) = fixture();
        assert!(tools
            .execute("delete_everything", &json!({}))
            .await
            .is_err());
    }
}
