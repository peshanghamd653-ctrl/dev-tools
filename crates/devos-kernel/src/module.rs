use crate::commands::CommandRegistry;
use crate::events::EventBus;

/// What a module receives at registration time. Modules contribute commands
/// and subscribe to events; they never reach into other modules.
pub struct ModuleCtx<'a> {
    pub commands: &'a CommandRegistry,
    pub events: &'a EventBus,
}

/// The contract every DevOS module implements. This same contract is the
/// foundation of the future plugin API, so core modules and plugins are not
/// separate species.
pub trait Module: Send + Sync {
    fn id(&self) -> &'static str;

    /// Called once at kernel boot. Must be fast; heavy work belongs in jobs.
    fn register(&self, ctx: &ModuleCtx<'_>);
}
