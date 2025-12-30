//! Command mode operations (execute, test, confirm, cancel)

use crate::command::{
    CommandResult, ParsedCommand, PendingCommand, available_commands, parse_command,
};
use crate::credentials::CredentialStore;
use crate::ui::app::{AddAccountData, AddAccountStep, ModalState, View};

use super::super::App;

impl App {
    pub(super) fn execute_command(&mut self) {
        let input = match &self.state.modal {
            ModalState::Command { input, .. } => input.trim().to_string(),
            _ => return,
        };

        if input.is_empty() {
            self.exit_command_mode();
            return;
        }

        match parse_command(&input) {
            Some(ParsedCommand::Clear) => {
                // Request confirmation
                if let ModalState::Command {
                    pending, result, ..
                } = &mut self.state.modal
                {
                    *pending = Some(PendingCommand::Clear);
                    *result = Some(CommandResult::Success(
                        "Clear all cached emails? (y/N)".to_string(),
                    ));
                }
            }
            Some(ParsedCommand::Help) | Some(ParsedCommand::Keys) => {
                // Exit command mode and show the unified help view
                self.exit_command_mode();
                self.state.modal = ModalState::Help {
                    keybindings: self.bindings.all_bindings(),
                    commands: available_commands(),
                    scroll: 0,
                };
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
                if let ModalState::Command {
                    input: cmd_input,
                    result,
                    ..
                } = &mut self.state.modal
                {
                    *result = Some(CommandResult::Error(format!(
                        "Unknown command: {}. Type :help for available commands.",
                        input
                    )));
                    cmd_input.clear();
                }
            }
        }
    }

    pub(super) fn test_credentials(&mut self) {
        let creds = CredentialStore::new(&self.state.connection.account_name);
        let info = creds.debug_info();

        let mut result_str = String::new();
        result_str.push_str(&format!(
            "Keyring: {}\n",
            if info.keyring_available {
                "OK"
            } else {
                "unavailable"
            }
        ));
        result_str.push_str(&format!(
            "Env var: {}\n",
            if info.env_var_set { "set" } else { "not set" }
        ));
        result_str.push_str(&format!("File: {}\n", info.file_path.display()));
        result_str.push_str(&format!(
            "File exists: {}",
            if info.file_exists { "yes" } else { "no" }
        ));

        if let ModalState::Command { input, result, .. } = &mut self.state.modal {
            *result = Some(CommandResult::Success(result_str));
            input.clear();
        }
    }

    pub(super) fn start_add_account_wizard(&mut self) {
        self.exit_command_mode();
        self.state.view = View::AddAccount {
            step: AddAccountStep::ChooseAuthMethod,
            data: AddAccountData::default(),
        };
    }

    pub(super) async fn confirm_pending_command(&mut self) {
        // Extract pending command if there is one
        let pending = match &mut self.state.modal {
            ModalState::Command { pending, .. } => pending.take(),
            _ => None,
        };

        if let Some(pending_cmd) = pending {
            match pending_cmd {
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
        if let ModalState::Command {
            pending, result, ..
        } = &mut self.state.modal
        {
            *pending = None;
            *result = Some(CommandResult::Success("Cancelled".to_string()));
        }
        self.exit_command_mode();
    }

    pub(crate) fn exit_command_mode(&mut self) {
        self.state.modal = ModalState::None;
    }
}
