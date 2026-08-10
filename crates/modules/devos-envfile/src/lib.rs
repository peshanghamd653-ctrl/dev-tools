//! Reads and writes `.env`-style files directly — distinct from
//! `devos-secrets`' encrypted vault, which is deliberately write-only (it
//! can list names but never read a value back). A project's own `.env` is
//! plaintext on disk already, loaded by whatever tool runs the project; a
//! manager for it needs to show and edit real values, which is exactly what
//! the vault refuses to do on purpose. The two are complementary, not
//! competing: this is for values a running project needs, the vault is for
//! values only DevOS itself should ever see again.
//!
//! Deliberately simple parsing: `KEY=VALUE` per line, `#` comments, one
//! layer of matching quotes stripped on read and added on write when a
//! value needs it. No escape-sequence handling, no multiline values — the
//! same "handle the common case correctly, don't guess at the rest" choice
//! `devos-redact`'s env-style pattern and the toolbox's JSON-to-YAML
//! converter both made.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

const TEMPLATE_SUFFIXES: &[&str] = &["example", "sample", "template", "defaults", "dist"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct EnvEntry {
    pub key: String,
    pub value: String,
    #[ts(type = "number")]
    pub line: usize,
}

/// Same naming rule `devos-security`'s `.env`-gitignore check uses, so the
/// two features agree on what counts as a real env file rather than a
/// template meant to be committed.
pub fn is_env_file(name: &str) -> bool {
    if name == ".env" {
        return true;
    }
    match name.strip_prefix(".env.") {
        Some(suffix) => !TEMPLATE_SUFFIXES.contains(&suffix),
        None => false,
    }
}

/// Root-level only — unlike the security check's full recursive walk, this
/// is for "what can I edit right now," and a project's real `.env` files
/// overwhelmingly live at its root. Nested ones (a monorepo package's own
/// `.env`) are a known gap, not a silent one.
pub fn list_env_files(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_file = entry.metadata().map(|m| m.is_file()).unwrap_or(false);
            (is_file && is_env_file(&name)).then_some(name)
        })
        .collect();
    names.sort();
    names
}

pub fn read(path: &Path) -> std::io::Result<Vec<EnvEntry>> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    Ok(parse(&content))
}

fn parse(content: &str) -> Vec<EnvEntry> {
    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, raw_value) = trimmed.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            Some(EnvEntry {
                key: key.to_string(),
                value: unquote(raw_value.trim()),
                line: i + 1,
            })
        })
        .collect()
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

fn quote_if_needed(value: &str) -> String {
    let needs_quoting = value.is_empty()
        || value
            .chars()
            .any(|c| c.is_whitespace() || c == '#' || c == '"');
    if needs_quoting {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EnvFileError {
    #[error("a key cannot be empty")]
    EmptyKey,
    #[error("a key or value cannot contain a newline")]
    ContainsNewline,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn validate(key: &str, value: &str) -> Result<(), EnvFileError> {
    if key.trim().is_empty() {
        return Err(EnvFileError::EmptyKey);
    }
    if key.contains(['\n', '\r', '=']) || value.contains(['\n', '\r']) {
        return Err(EnvFileError::ContainsNewline);
    }
    Ok(())
}

/// Adds `key` if it isn't already set, replaces its value in place if it
/// is — the existing line's position is preserved so a diff of the file
/// shows only the value that actually changed.
pub fn set(path: &Path, key: &str, value: &str) -> Result<(), EnvFileError> {
    validate(key, value)?;
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let new_line = format!("{key}={}", quote_if_needed(value));
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let mut replaced = false;
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some((existing_key, _)) = trimmed.split_once('=') {
            if existing_key.trim() == key {
                *line = new_line.clone();
                replaced = true;
                break;
            }
        }
    }
    if !replaced {
        lines.push(new_line);
    }
    Ok(write_lines(path, &lines)?)
}

/// A no-op, not an error, when `key` isn't present — matches
/// `devos-kernel::backup::cancel_restore`'s "cancelling nothing is fine"
/// precedent.
pub fn remove(path: &Path, key: &str) -> std::io::Result<()> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let lines: Vec<String> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                return true;
            }
            match trimmed.split_once('=') {
                Some((existing_key, _)) => existing_key.trim() != key,
                None => true,
            }
        })
        .map(String::from)
        .collect();
    write_lines(path, &lines)
}

fn write_lines(path: &Path, lines: &[String]) -> std::io::Result<()> {
    let mut content = lines.join("\n");
    if !lines.is_empty() {
        content.push('\n');
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
}

/// Joins `file_name` onto `root` after checking it is one of the exact
/// names `list_env_files` would have returned — no path separators, no
/// `..`, nothing that could resolve outside `root`. Every write-side
/// command goes through this rather than trusting a caller-supplied path.
pub fn resolve(root: &Path, file_name: &str) -> Result<PathBuf, EnvFileError> {
    if !is_env_file(file_name) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{file_name} is not a recognized .env file name"),
        )
        .into());
    }
    Ok(root.join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_env_file_accepts_dotenv_and_local_variants() {
        assert!(is_env_file(".env"));
        assert!(is_env_file(".env.local"));
        assert!(is_env_file(".env.production"));
    }

    #[test]
    fn is_env_file_rejects_templates_and_unrelated_names() {
        assert!(!is_env_file(".env.example"));
        assert!(!is_env_file(".env.sample"));
        assert!(!is_env_file("env"));
        assert!(!is_env_file("config.env"));
    }

    #[test]
    fn parse_reads_key_value_pairs_and_skips_comments_and_blanks() {
        let content = "# comment\nPORT=3000\n\nNAME=\"quoted value\"\n";
        let entries = parse(content);
        assert_eq!(
            entries,
            vec![
                EnvEntry {
                    key: "PORT".into(),
                    value: "3000".into(),
                    line: 2
                },
                EnvEntry {
                    key: "NAME".into(),
                    value: "quoted value".into(),
                    line: 4
                },
            ]
        );
    }

    #[test]
    fn set_adds_a_new_key_to_an_empty_or_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        set(&path, "PORT", "3000").unwrap();
        assert_eq!(
            read(&path).unwrap(),
            vec![EnvEntry {
                key: "PORT".into(),
                value: "3000".into(),
                line: 1
            }]
        );
    }

    #[test]
    fn set_replaces_an_existing_key_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "A=1\nB=2\nC=3\n").unwrap();
        set(&path, "B", "changed").unwrap();
        let entries = read(&path).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1].key, "B");
        assert_eq!(entries[1].value, "changed");
        assert_eq!(entries[1].line, 2, "position in the file is preserved");
    }

    #[test]
    fn set_quotes_a_value_that_needs_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        set(&path, "MSG", "hello world").unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw, "MSG=\"hello world\"\n");
    }

    #[test]
    fn set_rejects_a_key_or_value_containing_a_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        assert!(set(&path, "A\nB", "1").is_err());
        assert!(set(&path, "A", "1\n2").is_err());
    }

    #[test]
    fn remove_deletes_the_matching_line_and_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "A=1\nB=2\n# note\nC=3\n").unwrap();
        remove(&path, "B").unwrap();
        let entries = read(&path).unwrap();
        assert_eq!(
            entries.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            vec!["A", "C"]
        );
    }

    #[test]
    fn remove_is_a_no_op_for_a_missing_key_or_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        assert!(remove(&path, "NOPE").is_ok());
        std::fs::write(&path, "A=1\n").unwrap();
        assert!(remove(&path, "NOPE").is_ok());
        assert_eq!(read(&path).unwrap().len(), 1);
    }

    #[test]
    fn list_env_files_finds_root_level_files_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "").unwrap();
        std::fs::write(dir.path().join(".env.local"), "").unwrap();
        std::fs::write(dir.path().join(".env.example"), "").unwrap();
        std::fs::create_dir_all(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested/.env"), "").unwrap();

        assert_eq!(
            list_env_files(dir.path()),
            vec![".env".to_string(), ".env.local".to_string()]
        );
    }

    #[test]
    fn resolve_rejects_a_name_that_is_not_a_real_env_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve(dir.path(), "../../etc/passwd").is_err());
        assert!(resolve(dir.path(), ".env.example").is_err());
        assert!(resolve(dir.path(), "config.env").is_err());
        assert!(resolve(dir.path(), ".env").is_ok());
    }
}
