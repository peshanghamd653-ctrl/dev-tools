use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GitRepoInfo {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
}

/// One entry from `git status --porcelain=v2 -z`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GitFileEntry {
    pub path: String,
    /// Original path for renames.
    pub orig_path: Option<String>,
    /// Index (staged) status letter: M A D R C . etc.
    pub staged: String,
    /// Worktree (unstaged) status letter.
    pub unstaged: String,
    pub untracked: bool,
    pub conflicted: bool,
}

impl GitFileEntry {
    pub fn has_staged_changes(&self) -> bool {
        !self.untracked && !self.conflicted && self.staged != "."
    }

    pub fn has_unstaged_changes(&self) -> bool {
        self.untracked || self.conflicted || self.unstaged != "."
    }
}

/// Combined payload for the git page: one IPC round trip per refresh.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub info: GitRepoInfo,
    pub entries: Vec<GitFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GitBranch {
    pub name: String,
    pub current: bool,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct GitCommit {
    pub id: String,
    pub short_id: String,
    pub author: String,
    /// Unix seconds.
    #[ts(type = "number")]
    pub time: i64,
    pub summary: String,
}
