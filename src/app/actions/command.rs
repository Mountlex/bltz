//! Command mode operations (execute, test, confirm, cancel)

use crate::command::{
    available_commands, parse_command, CommandResult, ParsedCommand, PendingCommand,
};
use crate::credentials::CredentialStore;
use crate::ui::app::{AddAccountData, AddAccountStep, ModalState, View};

use super::super::App;

impl App {
    pub(super) fn execute_command(&mut self) {
        let input = self.state.command_input.trim().to_string();

        if input.is_empty() {
            self.exit_command_mode();
            return;
        }

        match parse_command(&input) {
            Some(ParsedCommand::Clear) => {
                // Request confirmation
                self.state.pending_confirmation = Some(PendingCommand::Clear);
                self.state.command_result = Some(CommandResult::Success(
                    "Clear all cached emails? (y/N)".to_string(),
                ));
            }
            Some(ParsedCommand::Help) => {
                self.state.command_result = Some(CommandResult::ShowHelp(available_commands()));
                self.state.command_input.clear();
            }
            Some(ParsedCommand::Keys) => {
                let keybindings = self.bindings.all_bindings();
                self.state.command_result = Some(CommandResult::ShowKeys(keybindings));
                self.state.command_input.clear();
            }
            Some(ParsedCommand::Quit) => {
                // Will be handled by the event loop checking for quit
                self.exit_command_mode();
            }
            Some(ParsedCommand::TestCredentials) => {
                self.test_credentials();
            }
            Some(ParsedCommand::AddAccount) => {
                self.start_add_account_wizard();
            }
            None => {
                self.state.command_result = Some(CommandResult::Error(format!(
                    "Unknown command: {}. Type :help for available commands.",
                    input
                )));
                self.state.command_input.clear();
            }
        }
    }

    pub(super) fn test_credentials(&mut self) {
        let creds = CredentialStore::new(&self.state.account_name);
        let info = creds.debug_info();

        let mut result = String::new();
        result.push_str(&format!(
            "Keyring: {}\n",
            if info.keyring_available {
                "OK"
            } else {
                "unavailable"
            }
        ));
        result.push_str(&format!(
            "Env var: {}\n",
            if info.env_var_set { "set" } else { "not set" }
        ));
        result.push_str(&format!("File: {}\n", info.file_path.display()));
        result.push_str(&format!(
            "File exists: {}",
            if info.file_exists { "yes" } else { "no" }
        ));

        self.state.command_result = Some(CommandResult::Success(result));
        self.state.command_input.clear();
    }

    pub(super) fn start_add_account_wizard(&mut self) {
        self.exit_command_mode();
        self.state.view = View::AddAccount {
            step: AddAccountStep::ChooseAuthMethod,
            data: AddAccountData::default(),
        };
    }

    pub(super) async fn confirm_pending_command(&mut self) {
        if let Some(pending) = self.state.pending_confirmation.take() {
            match pending {
                PendingCommand::Clear => {
                    // Clear cache for the current folder only
                    match self.cache.clear_all(&self.cache_key()).await {
                        Ok(_) => {
                            // Reload from (now empty) cache
                            self.reload_from_cache().await;
                            self.state.set_status("Cache cleared. Press R to re-sync.");
                        }
                        Err(e) => {
                            self.state
                                .set_error(format!("Failed to clear cache: {}", e));
                        }
                    }
                }
            }
            self.exit_command_mode();
        }
    }

    pub(super) fn cancel_pending_command(&mut self) {
        self.state.pending_confirmation = None;
        self.state.command_result = Some(CommandResult::Success("Cancelled".to_string()));
        self.exit_command_mode();
    }

    pub(crate) fn exit_command_mode(&mut self) {
        self.state.modal = ModalState::None;
        self.state.command_input.clear();
        self.state.command_result = None;
        self.state.pending_confirmation = None;
    }
}
