//! command registry
//!
//! commands can be registered from anywhere and are executed by the engine.
//! the actual console shell comes later, but the registry exists from day one.
//!
//! # usage
//!
//! implement the [`Command`] trait for any type and register it with
//! the [`CommandRegistry`]. commands receive a list of string arguments
//! and return a result string or error.

use std::collections::HashMap;

/// a command that can be executed by the engine.
///
/// implement this trait to create a custom command.
/// commands must be [`Send`] and [`Sync`] since they may be
/// executed from any thread.
pub trait Command: Send + Sync {
    /// execute the command with the given arguments
    ///
    /// # Errors
    /// returns an error string if the command fails to execute.
    fn execute(&self, args: &[String]) -> Result<String, String>;

    /// get a brief description of the command
    fn description(&self) -> &str;
}

/// registry of all available commands.
///
/// stores commands by name and provides execution and listing interfaces.
/// the registry is initialized with built-in commands like `help` and `version`.
pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn Command>>,
}

impl CommandRegistry {
    /// create a new empty command registry
    #[must_use]
    pub fn new() -> Self {
        let mut registry = Self {
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
    ///
    /// # Errors
    /// returns an error if the command is not registered or if execution fails.
    pub fn execute(&self, name: &str, args: &[String]) -> Result<String, String> {
        self.commands.get(name).map_or_else(
            || Err(format!("unknown command: {name}")),
            |cmd| cmd.execute(args),
        )
    }

    /// list all registered commands
    #[must_use]
    pub fn list_commands(&self) -> Vec<&str> {
        self.commands
            .keys()
            .map(std::string::String::as_str)
            .collect()
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
        Self {
            registry_snapshot: registry
                .list_commands()
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        }
    }
}

impl Command for HelpCommand {
    fn execute(&self, _args: &[String]) -> Result<String, String> {
        Ok(format!("available commands: {:?}", self.registry_snapshot))
    }

    fn description(&self) -> &'static str {
        "show available commands"
    }
}

/// version command
struct VersionCommand;

impl Command for VersionCommand {
    fn execute(&self, _args: &[String]) -> Result<String, String> {
        Ok(env!("CARGO_PKG_VERSION").to_string())
    }

    fn description(&self) -> &'static str {
        "show engine version"
    }
}
