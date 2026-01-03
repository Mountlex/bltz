//! Contact management actions

use crate::app::state::{ComposerField, ContactEditState, View};
use crate::mail::types::ComposeEmail;

use super::super::App;

impl App {
    /// Open the contacts view
    pub(crate) async fn open_contacts(&mut self) {
        match self.contacts.get_all().await {
            Ok(contacts) => {
                self.state.contacts.list = contacts;
                self.state.contacts.selected = 0;
                self.state.contacts.scroll_offset = 0;
                self.state.contacts.editing = None;
                self.state.view = View::Contacts;
            }
            Err(e) => {
                self.state
                    .set_error(format!("Failed to load contacts: {}", e));
            }
        }
    }

    /// Move selection up in contacts view
    pub(crate) fn contacts_move_up(&mut self) {
        if self.state.contacts.selected > 0 {
            self.state.contacts.selected -= 1;
        }
    }

    /// Move selection down in contacts view
    pub(crate) fn contacts_move_down(&mut self) {
        if self.state.contacts.selected < self.state.contacts.list.len().saturating_sub(1) {
            self.state.contacts.selected += 1;
        }
    }

    /// Delete the selected contact
    pub(crate) async fn contacts_delete(&mut self) {
        if let Some(contact) = self.state.contacts.list.get(self.state.contacts.selected) {
            let id = contact.id;
            if let Err(e) = self.contacts.delete(id).await {
                self.state
                    .set_error(format!("Failed to delete contact: {}", e));
                return;
            }
            // Remove from list
            self.state
                .contacts
                .list
                .remove(self.state.contacts.selected);
            // Adjust selection
            if self.state.contacts.selected >= self.state.contacts.list.len() {
                self.state.contacts.selected = self.state.contacts.list.len().saturating_sub(1);
            }
            self.state.set_status("Contact deleted");
        }
    }

    /// Start editing the selected contact's name
    pub(crate) fn contacts_start_edit(&mut self) {
        if let Some(contact) = self.state.contacts.list.get(self.state.contacts.selected) {
            self.state.contacts.editing = Some(ContactEditState {
                contact_id: contact.id,
                name: contact.name.clone().unwrap_or_default(),
            });
        }
    }

    /// Save the contact name edit
    pub(crate) async fn contacts_save_edit(&mut self) {
        if let Some(edit) = self.state.contacts.editing.take() {
            if let Err(e) = self.contacts.update_name(edit.contact_id, &edit.name).await {
                self.state
                    .set_error(format!("Failed to update contact: {}", e));
                return;
            }
            // Refresh the list
            if let Ok(contacts) = self.contacts.get_all().await {
                self.state.contacts.list = contacts;
            }
            self.state.set_status("Contact updated");
        }
    }

    /// Cancel the contact edit
    pub(crate) fn contacts_cancel_edit(&mut self) {
        self.state.contacts.editing = None;
    }

    /// Open composer with the selected contact as recipient
    pub(crate) fn contacts_compose_to(&mut self) {
        if let Some(contact) = self.state.contacts.list.get(self.state.contacts.selected) {
            let mut email = ComposeEmail::new();
            email.to = contact.email.clone();
            self.state.view = View::Composer {
                email,
                field: ComposerField::Subject, // Skip To since it's filled
            };
        }
    }

    /// Handle character input in contacts edit mode
    pub(crate) fn contacts_edit_char(&mut self, c: char) {
        if let Some(ref mut edit) = self.state.contacts.editing {
            edit.name.push(c);
        }
    }

    /// Handle backspace in contacts edit mode
    pub(crate) fn contacts_edit_backspace(&mut self) {
        if let Some(ref mut edit) = self.state.contacts.editing {
            edit.name.pop();
        }
    }
}
