//! Action handlers for user input
//!
//! This module is split into focused submodules:
//! - `navigation`: Movement and scrolling
//! - `prefetch`: Email body prefetching
//! - `email`: Email operations (open, delete, flags)
//! - `compose`: Email composition and sending
//! - `input`: Text input handling
//! - `command`: Command mode operations
//! - `wizard`: Add account wizard
//! - `contacts`: Contact management
//! - `ai`: AI-powered features (summarization, polish)

mod ai;
mod command;
mod compose;
mod contacts;
mod email;
mod input;
mod navigation;
mod prefetch;
mod undo;
mod wizard;

use anyhow::Result;

use crate::input::Action;
use crate::mail::ImapCommand;
use crate::ui::app::{ModalState, View};

use super::App;

impl App {
    pub(crate) async fn handle_action(&mut self, action: Action) -> Result<()> {
        match action {
            // Navigation
            Action::Up => {
                if self.state.modal.is_help() {
                    self.help_scroll_up();
                } else {
                    self.move_up();
                    self.schedule_prefetch().await;
                }
            }
            Action::Down => {
                if self.state.modal.is_help() {
                    self.help_scroll_down();
                } else {
                    self.move_down();
                    self.schedule_prefetch().await;
                }
            }
            Action::Left => self.move_left().await,
            Action::Right => self.move_right(),
            Action::Top => {
                self.move_to_top();
                self.schedule_prefetch().await;
            }
            Action::Bottom => {
                self.move_to_bottom();
                self.schedule_prefetch().await;
            }
            Action::PageUp => {
                self.move_page(-10);
                self.schedule_prefetch().await;
            }
            Action::PageDown => {
                self.move_page(10);
                self.schedule_prefetch().await;
            }

            // Email actions
            Action::Open => self.open_selected().await,
            Action::Back => self.go_back().await,
            Action::Quit => {} // Handled in event loop
            Action::Delete => self.delete_selected().await,
            Action::ToggleRead => self.toggle_read().await,
            Action::ToggleStar => self.toggle_star().await,
            Action::ToggleThread => {
                self.toggle_thread();
                self.schedule_prefetch().await;
            }
            Action::Refresh => {
                self.accounts.send_command(ImapCommand::Sync).await.ok();
            }
            Action::NextAccount => {
                self.switch_to_next_account().await;
            }
            Action::PrevAccount => {
                self.switch_to_prev_account().await;
            }

            // Starred view toggle
            Action::ViewStarred => {
                if matches!(self.state.view, View::Inbox) && !self.state.modal.is_active() {
                    self.state.toggle_view_mode();
                    if self.state.is_starred_view() {
                        self.state.set_status("Showing starred emails");
                    } else {
                        self.state.set_status("Showing all emails");
                    }
                }
            }

            // Search
            Action::Search => {
                if matches!(self.state.view, View::Inbox) && !self.state.modal.is_active() {
                    self.state.modal = ModalState::Search;
                } else if !matches!(self.state.view, View::Inbox) {
                    self.state
                        .set_error("Search is only available in inbox view");
                }
            }

            // Folder picker
            Action::FolderPicker => {
                if matches!(self.state.view, View::Inbox) {
                    self.toggle_folder_picker().await;
                }
            }

            // Command mode
            Action::Command => {
                if matches!(self.state.view, View::Inbox) && !self.state.modal.is_active() {
                    self.state.modal = ModalState::Command {
                        input: String::new(),
                        result: None,
                        pending: None,
                    };
                }
            }
            Action::ExecuteCommand => {
                self.execute_command();
            }
            Action::ConfirmCommand => {
                self.confirm_pending_command().await;
            }
            Action::CancelCommand => {
                self.cancel_pending_command();
            }

            // Composer
            Action::Reply => self.start_reply().await,
            Action::ReplyAll => self.start_reply_all().await,
            Action::Forward => self.start_forward().await,
            Action::Compose => self.start_compose(),
            Action::NextField => self.next_composer_field(),
            Action::PrevField => self.prev_composer_field(),
            Action::Send => self.send_email().await,
            Action::Cancel => self.cancel_compose(),
            Action::CycleSendAccount => self.cycle_send_account(),

            // Add account wizard
            Action::WizardNext => self.wizard_next().await,
            Action::WizardBack => self.wizard_back(),
            Action::WizardConfirm => self.wizard_confirm().await,

            // Undo
            Action::Undo => self.undo().await,

            // Contacts
            Action::OpenContacts => {
                if matches!(self.state.view, View::Inbox) && !self.state.modal.is_active() {
                    self.open_contacts().await;
                }
            }
            Action::EditContact => {
                if matches!(self.state.view, View::Contacts) {
                    self.contacts_start_edit();
                }
            }

            // Autocomplete (handled in input handler, but included for completeness)
            Action::AutocompleteUp => self.autocomplete_up(),
            Action::AutocompleteDown => self.autocomplete_down(),
            Action::AutocompleteSelect => self.autocomplete_select(),
            Action::AutocompleteClose => self.autocomplete_close(),

            // AI features
            Action::ToggleSummary => self.toggle_summary().await,
            Action::SummarizeThread => self.summarize_thread().await,
            Action::Polish => self.start_polish().await,
            Action::AcceptPolish => self.accept_polish(),
            Action::RejectPolish => self.reject_polish(),

            // Preview
            Action::ToggleHeaderExpand => {
                self.state.reader.headers_expanded = !self.state.reader.headers_expanded;
            }

            // Help
            Action::Help => {
                self.toggle_help();
            }

            // View modes
            Action::ToggleConversationMode => {
                self.toggle_conversation_mode().await;
            }
        }
        Ok(())
    }

    /// Toggle conversation mode (show sent emails in inbox threads)
    async fn toggle_conversation_mode(&mut self) {
        if !matches!(self.state.view, View::Inbox) {
            return;
        }

        self.state.conversation_mode = !self.state.conversation_mode;

        // Clear reader/prefetch state since email list will change
        self.state.reader.body = None;
        self.last_prefetch_uid = None;
        self.pending_prefetch = None;
        self.in_flight_fetches.clear();

        self.reload_from_cache().await;

        // Trigger prefetch for newly selected email
        self.schedule_prefetch().await;

        if self.state.conversation_mode {
            self.state.set_status("Conversation view enabled");
        } else {
            self.state.set_status("Conversation view disabled");
        }
    }

    fn toggle_help(&mut self) {
        use crate::command::available_commands;

        if self.state.modal.is_help() {
            self.state.modal = ModalState::None;
        } else if !self.state.modal.is_active() {
            self.state.modal = ModalState::Help {
                keybindings: self.bindings.all_bindings(),
                commands: available_commands(),
                scroll: 0,
            };
        }
    }

    pub(crate) fn help_scroll_down(&mut self) {
        if let ModalState::Help {
            scroll,
            keybindings,
            commands,
        } = &mut self.state.modal
        {
            // Calculate max scroll based on content height
            // Each keybinding is 1 line, plus category headers (2 lines each)
            // Commands section has 1 header + entries
            let mut categories = 0;
            let mut last_category = "";
            for kb in keybindings.iter() {
                if kb.category != last_category {
                    categories += 1;
                    last_category = kb.category;
                }
            }
            let content_lines = keybindings.len() + categories * 2 + commands.len() + 2;
            let max_scroll = content_lines.saturating_sub(10); // Approx visible area

            if *scroll < max_scroll {
                *scroll = scroll.saturating_add(1);
            }
        }
    }

    pub(crate) fn help_scroll_up(&mut self) {
        if let ModalState::Help { scroll, .. } = &mut self.state.modal {
            *scroll = scroll.saturating_sub(1);
        }
    }
}
