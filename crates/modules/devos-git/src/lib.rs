//! Git module: repository operations through the user's own `git` CLI, so
//! credentials, hooks, and LFS behave exactly like their terminal.

mod cli;
mod ops;
mod types;

pub use cli::{GitError, GitResult};
pub use ops::{
    branches, commit, diff_file, discard, log, pull, push, repo_info, stage, staged_diff, status,
    switch, unstage,
};
pub use types::{GitBranch, GitCommit, GitFileEntry, GitRepoInfo, GitStatus};

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

pub struct GitModule;

impl Module for GitModule {
    fn id(&self) -> &'static str {
        "git"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "git.open".into(),
            module: "git".into(),
            title: "Open Git".into(),
            keywords: vec!["commit".into(), "diff".into(), "branch".into()],
            shortcut: Some("Ctrl+4".into()),
        }]);
    }
}
