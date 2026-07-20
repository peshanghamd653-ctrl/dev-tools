use std::sync::RwLock;

use crate::types::CommandDescriptor;

/// Registry of palette-invokable commands contributed by modules.
/// The frontend merges this list with its own navigation commands.
#[derive(Default)]
pub struct CommandRegistry {
    items: RwLock<Vec<CommandDescriptor>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, mut commands: Vec<CommandDescriptor>) {
        self.items
            .write()
            .expect("command registry lock poisoned")
            .append(&mut commands);
    }

    pub fn list(&self) -> Vec<CommandDescriptor> {
        self.items
            .read()
            .expect("command registry lock poisoned")
            .clone()
    }
}
