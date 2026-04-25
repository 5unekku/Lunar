//! command registry
//!
//! commands can be registered from anywhere and are executed by the engine.
//! the actual console shell comes later, but the registry exists from day one.

use std::collections::HashMap;

/// a command that can be executed by the engine
pub trait Command: Send + Sync {
    /// execute the command with the given arguments
    fn execute(&self, args: &[String]) -> Result<String, String>;

    /// get a brief description of the command
    fn description(&self) -> &str;
}

/// registry of all available commands
pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn Command>>,
}

impl CommandRegistry {
    /// create a new empty command registry
    pub fn new() -> Self {
        let mut registry = CommandRegistry {
            commands: HashMap::new(),
        };

        // register built-in commands
        registry.register("help".to_string(), Box::new(HelpCommand::new(&registry)));
        registry.register("version".to_string(), Box::new(VersionCommand));

        registry
    }

    /// register a new command
    pub fn register(&mut self, name: String, command: Box<dyn Command>) {
        self.commands.insert(name, command);
    }

    /// execute a command by name
    pub fn execute(&self, name: &str, args: &[String]) -> Result<String, String> {
        match self.commands.get(name) {
            Some(command) => command.execute(args),
            None => Err(format!("unknown command: {}", name)),
        }
    }

    /// list all registered commands
    pub fn list_commands(&self) -> Vec<&str> {
        self.commands.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// help command
struct HelpCommand {
    registry_snapshot: Vec<String>,
}

impl HelpCommand {
    fn new(registry: &CommandRegistry) -> Self {
        HelpCommand {
            registry_snapshot: registry
                .list_commands()
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

impl Command for HelpCommand {
    fn execute(&self, _args: &[String]) -> Result<String, String> {
        Ok(format!("available commands: {:?}", self.registry_snapshot))
    }

    fn description(&self) -> &str {
        "show available commands"
    }
}

/// version command
struct VersionCommand;

impl Command for VersionCommand {
    fn execute(&self, _args: &[String]) -> Result<String, String> {
        Ok(env!("CARGO_PKG_VERSION").to_string())
    }

    fn description(&self) -> &str {
        "show engine version"
    }
}
