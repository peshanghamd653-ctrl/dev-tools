//! The single path-containment guard. Every surface that resolves a
//! user- or model-supplied relative path against a project root (AI tools,
//! file explorer) goes through here, so the security property cannot drift
//! between features.

use std::path::{Path, PathBuf};

/// Join `relative` to `root`, canonicalize, and verify the result stays
/// inside the canonicalized root. Rejects `..` traversal and absolute paths.
pub fn resolve_in_root(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let canonical_root =
        std::fs::canonicalize(root).map_err(|_| "project root does not exist".to_string())?;
    let target = root.join(relative);
    let canonical =
        std::fs::canonicalize(&target).map_err(|_| format!("path not found: {relative}"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(format!("path escapes the project: {relative}"));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_and_rejects() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/ok.txt"), "x").unwrap();

        let resolved = resolve_in_root(dir.path(), "src/ok.txt").unwrap();
        assert!(resolved.ends_with("ok.txt"));

        assert!(resolve_in_root(dir.path(), "../..").is_err());
        assert!(resolve_in_root(dir.path(), "C:/Windows/win.ini").is_err());
        assert!(resolve_in_root(dir.path(), "src/missing.txt").is_err());
    }
}
