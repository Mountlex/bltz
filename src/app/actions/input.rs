//! Text input handling (chars, backspace)

use std::collections::HashSet;
use std::time::Instant;

use crate::ui::app::{ComposerField, ModalState, View};

use super::super::App;

impl App {
    pub(crate) async fn handle_char(&mut self, c: char) {
        // Handle contacts edit input
        if matches!(self.state.view, View::Contacts) && self.state.contacts.editing.is_some() {
            self.contacts_edit_char(c);
            return;
        }

        // Handle command input
        if let ModalState::Command { input, pending, .. } = &mut self.state.modal {
            if pending.is_none() {
                input.push(c);
            }
            return;
        }

        // Handle search input
        if self.state.modal.is_search() {
            // Only reset selection when transitioning from empty to non-empty search
            let was_empty = self.state.search.query.is_empty();
            self.state.search.query.push(c);
            // Instant header search (body FTS runs after debounce)
            self.state.update_search_cache_hybrid(HashSet::new());
            // Schedule body FTS search after debounce delay
            self.last_search_input = Some(Instant::now());
            if was_empty {
                self.state.thread.selected = 0;
                self.state.thread.selected_in_thread = 0;
            }
            return;
        }

        // Handle add account wizard input
        if let View::AddAccount { step, data } = &mut self.state.view {
            use crate::ui::app::AddAccountStep;
            match step {
                AddAccountStep::EnterEmail => data.email.push(c),
                AddAccountStep::EnterPassword => data.password.push(c),
                AddAccountStep::EnterImapServer => data.imap_server.push(c),
                AddAccountStep::EnterSmtpServer => data.smtp_server.push(c),
                _ => {}
            }
            return;
        }

        // Handle composer input
        if let View::Composer {
            ref mut email,
            field,
        } = self.state.view
        {
            match field {
                ComposerField::To => {
                    email.to.push(c);
                }
                ComposerField::Cc => {
                    email.cc.push(c);
                }
                ComposerField::Subject => email.subject.push(c),
                ComposerField::Body => email.body.push(c),
            }
        }

        // Update autocomplete after typing in To or Cc field
        if let View::Composer { field, .. } = self.state.view
            && (field == ComposerField::To || field == ComposerField::Cc) {
                self.update_autocomplete().await;
            }
    }

    pub(crate) async fn handle_backspace(&mut self) {
        // Handle contacts edit backspace
        if matches!(self.state.view, View::Contacts) && self.state.contacts.editing.is_some() {
            self.contacts_edit_backspace();
            return;
        }

        // Handle command backspace
        if let ModalState::Command { input, .. } = &mut self.state.modal {
            input.pop();
            return;
        }

        // Handle search backspace
        if self.state.modal.is_search() {
            self.state.search.query.pop();
            // Instant header search (body FTS runs after debounce)
            self.state.update_search_cache_hybrid(HashSet::new());
            // Schedule body FTS search after debounce delay
            self.last_search_input = Some(Instant::now());
            // Reset selection when search changes
            self.state.thread.selected = 0;
            self.state.thread.selected_in_thread = 0;
            return;
        }

        // Handle add account wizard backspace
        if let View::AddAccount { step, data } = &mut self.state.view {
            use crate::ui::app::AddAccountStep;
            match step {
                AddAccountStep::EnterEmail => {
                    data.email.pop();
                }
                AddAccountStep::EnterPassword => {
                    data.password.pop();
                }
                AddAccountStep::EnterImapServer => {
                    data.imap_server.pop();
                }
                AddAccountStep::EnterSmtpServer => {
                    data.smtp_server.pop();
                }
                _ => {}
            }
            return;
        }

        // Handle composer backspace
        if let View::Composer {
            ref mut email,
            field,
        } = self.state.view
        {
            match field {
                ComposerField::To => {
                    email.to.pop();
                }
                ComposerField::Cc => {
                    email.cc.pop();
                }
                ComposerField::Subject => {
                    email.subject.pop();
                }
                ComposerField::Body => {
                    email.body.pop();
                }
            }
        }

        // Update autocomplete after backspace in To or Cc field
        if let View::Composer { field, .. } = self.state.view
            && (field == ComposerField::To || field == ComposerField::Cc) {
                self.update_autocomplete().await;
            }
    }
}
