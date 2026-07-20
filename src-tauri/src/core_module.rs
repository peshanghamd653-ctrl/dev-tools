use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

/// Built-in workspace/project commands exposed to the command palette.
/// Exists to exercise the same contribution path plugins will use.
pub struct CoreModule;

impl Module for CoreModule {
    fn id(&self) -> &'static str {
        "core"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![
            CommandDescriptor {
                id: "core.workspace.create".into(),
                module: "core".into(),
                title: "Create Workspace".into(),
                keywords: vec!["workspace".into(), "new".into()],
                shortcut: None,
            },
            CommandDescriptor {
                id: "core.project.add".into(),
                module: "core".into(),
                title: "Add Project".into(),
                keywords: vec!["project".into(), "open".into(), "add".into()],
                shortcut: None,
            },
        ]);
    }
}
