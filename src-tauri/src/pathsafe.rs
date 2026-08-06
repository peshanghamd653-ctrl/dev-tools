//! The single path-containment guard. Every surface that resolves a
//! user- or model-supplied relative path against a project root (AI tools,
//! file explorer) goes through here, so the security property cannot drift
//! between features.

use std::fs::Metadata;
use std::path::{Path, PathBuf};

/// True for a symlink, a directory junction, or any other reparse point.
///
/// Directory walkers must not descend through these and writers must not
/// write through them: a link inside the project can point anywhere on disk,
/// and following one puts content the user never granted in front of the
/// model (or puts model-authored bytes outside the project).
///
/// `FileType::is_symlink()` alone is **not** enough on Windows — std reports
/// it only for `IO_REPARSE_TAG_SYMLINK`, so a `mklink /J` junction (which any
/// unprivileged user can create, e.g. inside a cloned repo) is reported as an
/// ordinary directory. The `FILE_ATTRIBUTE_REPARSE_POINT` bit covers every
/// tag, junctions included.
///
/// Callers must pass metadata obtained **without** following links:
/// [`std::fs::symlink_metadata`], or [`std::fs::DirEntry::metadata`], which
/// documents that it does not traverse symlinks.
pub fn is_reparse_point(meta: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        /// `winnt.h`. Inlined rather than pulled from a crate — one constant
        /// is not worth a dependency in the containment module.
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

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

/// Create a directory link at `link` pointing at `target`, returning whether
/// it worked. On Windows this is a **junction** (`mklink /J`) — the one link
/// type an unprivileged process can create, and therefore the shape a hostile
/// cloned repo would actually ship. On Unix it is an ordinary symlink.
///
/// Test-only, but it lives here rather than in one test module so every
/// walker's test can build the same adversarial tree.
#[cfg(test)]
pub(crate) fn make_dir_link(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard behind SEC-004. Written against a real junction because the
    /// obvious implementation (`FileType::is_symlink()`) passes every test
    /// built on Unix symlinks and still misses this one on Windows.
    #[test]
    fn reparse_points_are_recognized_including_junctions() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(dir.path().join("plain")).unwrap();
        std::fs::write(dir.path().join("plain/file.txt"), "x").unwrap();

        // An ordinary directory and an ordinary file are not links.
        for path in ["plain", "plain/file.txt"] {
            let meta = std::fs::symlink_metadata(dir.path().join(path)).unwrap();
            assert!(!is_reparse_point(&meta), "{path} is not a link");
        }

        let link = dir.path().join("link");
        assert!(
            make_dir_link(&link, &outside),
            "creating a junction/symlink must not need privileges"
        );
        let meta = std::fs::symlink_metadata(&link).unwrap();
        assert!(
            is_reparse_point(&meta),
            "a junction must be recognized as a link — on Windows it reports \
             is_symlink() == false and is_dir() == true"
        );
    }

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

    /// Canonicalization already collapses a link, so reading *through* one is
    /// refused. Pinned because `find_files` and the indexer relied on this
    /// being true of every surface, and it was not true of theirs (SEC-004).
    #[test]
    fn rejects_reads_through_a_link_that_leaves_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "ssh-key").unwrap();

        assert!(make_dir_link(&root.join("escape"), &outside));
        assert!(resolve_in_root(&root, "escape/secret.txt").is_err());
        assert!(resolve_in_root(&root, "escape").is_err());
    }
}
