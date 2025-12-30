//! Command types and parsing for vim-style command mode

/// A pending command that requires user confirmation before execution
#[derive(Debug, Clone)]
pub enum PendingCommand {
    Clear,
}

use crate::input::KeybindingEntry;

/// Result of command execution
#[derive(Debug, Clone)]
pub enum CommandResult {
    Success(String),
    Error(String),
    ShowHelp(Vec<CommandHelp>),
    ShowKeys(Vec<KeybindingEntry>),
}

/// Help information for a command
#[derive(Debug, Clone)]
pub struct CommandHelp {
    pub name: &'static str,
    pub description: &'static str,
}

/// Parsed command from user input
#[derive(Debug, Clone)]
pub enum ParsedCommand {
    Clear,
    Help,
    Keys,
    Quit,
    TestCredentials,
    AddAccount,
}

/// Parse a command string into a ParsedCommand
pub fn parse_command(input: &str) -> Option<ParsedCommand> {
    let trimmed = input.trim();
    match trimmed {
        "clear" => Some(ParsedCommand::Clear),
        "help" | "h" | "?" => Some(ParsedCommand::Help),
        "keys" | "keybindings" | "bindings" => Some(ParsedCommand::Keys),
        "q" | "quit" => Some(ParsedCommand::Quit),
        "testcreds" | "test-creds" => Some(ParsedCommand::TestCredentials),
        "addaccount" | "add-account" => Some(ParsedCommand::AddAccount),
        _ => None,
    }
}

/// Get all available commands for help display
pub fn available_commands() -> Vec<CommandHelp> {
    vec![
        CommandHelp {
            name: "addaccount",
            description: "Add a new email account",
        },
        CommandHelp {
            name: "clear",
            description: "Wipe local email cache (requires confirmation)",
        },
        CommandHelp {
            name: "help",
            description: "Show this help message",
        },
        CommandHelp {
            name: "keys",
            description: "Show all keybindings",
        },
        CommandHelp {
            name: "quit",
            description: "Exit the application",
        },
        CommandHelp {
            name: "testcreds",
            description: "Test credential storage backend",
        },
    ]
}
