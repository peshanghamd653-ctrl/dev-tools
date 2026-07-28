//! File-explorer IPC commands. Read-only; every path goes through the
//! shared containment guard in pathsafe.rs.

use std::io::Read;
use std::path::PathBuf;

use devos_kernel::types::{FilePreview, FsEntry};

use crate::pathsafe::resolve_in_root;

const MAX_DIR_ENTRIES: usize = 1000;
const MAX_PREVIEW_BYTES: u64 = 512 * 1024;
const MAX_FIND_RESULTS: usize = 200;

#[tauri::command]
pub async fn fs_list_dir(project_path: String, relative: String) -> Result<Vec<FsEntry>, String> {
    let root = PathBuf::from(&project_path);
    let dir = if relative.is_empty() || relative == "." {
        std::fs::canonicalize(&root).map_err(|_| "project root does not exist".to_string())?
    } else {
        resolve_in_root(&root, &relative)?
    };
    if !dir.is_dir() {
        return Err(format!("not a directory: {relative}"));
    }

    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        let item = FsEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir: meta.is_dir(),
            size: meta.len() as i64,
        };
        if item.is_dir {
            dirs.push(item);
        } else {
            files.push(item);
        }
        if dirs.len() + files.len() >= MAX_DIR_ENTRIES {
            break;
        }
    }
    dirs.sort_by_key(|entry| entry.name.to_lowercase());
    files.sort_by_key(|entry| entry.name.to_lowercase());
    dirs.extend(files);
    Ok(dirs)
}

#[tauri::command]
pub async fn fs_read_file(project_path: String, relative: String) -> Result<FilePreview, String> {
    let root = PathBuf::from(&project_path);
    let path = resolve_in_root(&root, &relative)?;
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(format!("not a file: {relative}"));
    }
    let size = meta.len();

    let mut handle = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity(MAX_PREVIEW_BYTES.min(size) as usize);
    handle
        .by_ref()
        .take(MAX_PREVIEW_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;

    // Binary sniff on what we read (NUL byte heuristic).
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return Ok(FilePreview {
            content: String::new(),
            truncated: false,
            binary: true,
            size: size as i64,
        });
    }

    Ok(FilePreview {
        content: String::from_utf8_lossy(&bytes).into_owned(),
        truncated: size > MAX_PREVIEW_BYTES,
        binary: false,
        size: size as i64,
    })
}

/// Filename search: same walk rules as the indexer (skips node_modules etc.).
#[tauri::command]
pub async fn fs_find(project_path: String, query: String) -> Result<Vec<String>, String> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let root = PathBuf::from(&project_path);
    if !root.is_dir() {
        return Err(format!("not a directory: {project_path}"));
    }
    Ok(devos_index::project_files(&root)
        .into_iter()
        .filter(|path| path.to_lowercase().contains(&needle))
        .take(MAX_FIND_RESULTS)
        .collect())
}
