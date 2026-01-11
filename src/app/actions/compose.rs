//! Email composition actions (reply, forward, send)

use crate::app::state::{ComposerField, View};
use crate::config::AuthMethod;
use crate::credentials::CredentialStore;
use crate::mail::types::{ComposeEmail, EmailFlags, EmailHeader};
use crate::mail::{ImapCommand, SmtpClient};

use super::super::App;

impl App {
    /// Get the current email for compose operations (reply, forward).
    /// Returns (uid, email) or None if not available.
    fn get_current_email_for_compose(&self) -> Option<(u32, EmailHeader)> {
        match &self.state.view {
            View::Inbox => self
                .state
                .current_email_from_thread()
                .map(|e| (e.uid, e.clone())),
            View::Reader { uid } => self
                .state
                .emails
                .iter()
                .find(|e| e.uid == *uid)
                .map(|e| (*uid, e.clone())),
            _ => None,
        }
    }

    /// Get email body text for quoting in replies/forwards.
    /// Checks current_body first, then cache.
    async fn get_email_body_text(&self, uid: u32) -> String {
        if let Some(ref body) = self.state.reader.body {
            return body.display_text().to_string();
        }
        if let Ok(Some(body)) = self.cache.get_email_body(&self.cache_key(), uid).await {
            return body.display_text().to_string();
        }
        String::new()
    }

    pub(super) async fn start_reply(&mut self) {
        let (uid, email) = match self.get_current_email_for_compose() {
            Some(result) => result,
            None => {
                self.state.set_error("No email selected for reply");
                return;
            }
        };

        let body_text = self.get_email_body_text(uid).await;
        let mut reply = ComposeEmail::reply_to(&email, &body_text);
        reply.reply_to_uid = Some(uid); // Track original email for ANSWERED flag
        self.state.view = View::Composer {
            email: reply,
            field: ComposerField::Body,
        };
    }

    pub(super) async fn start_reply_all(&mut self) {
        let (uid, email) = match self.get_current_email_for_compose() {
            Some(result) => result,
            None => {
                self.state.set_error("No email selected for reply all");
                return;
            }
        };

        let body_text = self.get_email_body_text(uid).await;

        // Get our email address for filtering
        let my_email = self
            .accounts
            .get(self.state.connection.account_index)
            .map(|h| h.config.email.as_str())
            .unwrap_or("");

        let mut reply = ComposeEmail::reply_all(&email, &body_text, my_email);
        reply.reply_to_uid = Some(uid); // Track original email for ANSWERED flag
        self.state.view = View::Composer {
            email: reply,
            field: ComposerField::Body,
        };
    }

    pub(super) async fn start_forward(&mut self) {
        let (uid, email) = match self.get_current_email_for_compose() {
            Some(result) => result,
            None => {
                self.state.set_error("No email selected for forward");
                return;
            }
        };

        let body_text = self.get_email_body_text(uid).await;
        let forward = ComposeEmail::forward(&email, &body_text);
        self.state.view = View::Composer {
            email: forward,
            field: ComposerField::To, // Start at To since it's empty
        };
    }

    pub(super) fn start_compose(&mut self) {
        self.state.view = View::Composer {
            email: ComposeEmail::new(),
            field: ComposerField::To,
        };
    }

    pub(super) fn next_composer_field(&mut self) {
        if let View::Composer { ref mut field, .. } = self.state.view {
            *field = field.next();
        }
    }

    pub(super) fn prev_composer_field(&mut self) {
        if let View::Composer { ref mut field, .. } = self.state.view {
            *field = field.prev();
        }
    }

    pub(super) async fn send_email(&mut self) {
        if let View::Composer { ref email, .. } = self.state.view {
            if email.to.is_empty() {
                self.state.set_error("Recipient is required");
                return;
            }
            if email.subject.is_empty() {
                self.state.set_error("Subject is required");
                return;
            }

            let email = email.clone();
            self.do_send(email).await;
        }
    }

    pub(crate) async fn do_send(&mut self, email: ComposeEmail) {
        self.state.status.loading = true;
        self.state.set_status("Sending...");

        // Determine which account to send from
        let send_account_index = email
            .from_account_index
            .unwrap_or(self.accounts.active_index());
        let account = match self.accounts.get(send_account_index) {
            Some(h) => &h.config,
            None => {
                self.state.set_error("Invalid sending account");
                self.state.status.loading = false;
                return;
            }
        };

        // Create SMTP client for the sending account
        // Note: We create a fresh connection each time to support cross-account sending
        let credentials = CredentialStore::new(&account.email);

        // Get credentials based on auth method
        let password = match &account.auth {
            AuthMethod::Password => match credentials.get_smtp_password() {
                Ok(p) => p,
                Err(e) => {
                    self.state
                        .set_error(format!("Failed to get SMTP password: {}", e));
                    self.state.status.loading = false;
                    return;
                }
            },
            AuthMethod::OAuth2 { client_id, .. } => {
                // For OAuth2, get the stored refresh token and exchange for a fresh access token
                let refresh_token = match credentials.get_oauth2_refresh_token() {
                    Ok(token) => token,
                    Err(e) => {
                        self.state.set_error(format!(
                            "OAuth2 refresh token not found: {}. Please re-authenticate.",
                            e
                        ));
                        self.state.status.loading = false;
                        return;
                    }
                };

                match crate::oauth2::get_access_token(client_id, &refresh_token).await {
                    Ok(access_token) => access_token,
                    Err(e) => {
                        self.state.set_error(format!(
                            "Failed to refresh OAuth2 access token: {}. Please re-authenticate.",
                            e
                        ));
                        self.state.status.loading = false;
                        return;
                    }
                }
            }
        };

        let smtp = match SmtpClient::new_with_auth(
            &account.smtp,
            account.username_or_email(),
            &password,
            &account.email,
            account.display_name.as_deref(),
            &account.auth,
        )
        .await
        {
            Ok(client) => client,
            Err(e) => {
                self.state
                    .set_error(format!("Failed to connect to SMTP: {}", e));
                self.state.status.loading = false;
                return;
            }
        };

        match smtp.send(&email).await {
            Ok(_) => {
                // Add recipient to contacts
                self.contacts.add_or_update(&email.to, None).await.ok();

                // Set ANSWERED flag on original email if this was a reply
                if let Some(reply_to_uid) = email.reply_to_uid {
                    // Update local state immediately (optimistic update)
                    if let Some(original) =
                        self.state.emails.iter_mut().find(|e| e.uid == reply_to_uid)
                    {
                        original.flags.insert(EmailFlags::ANSWERED);
                    }

                    // Send IMAP command to server (use email's actual folder)
                    let folder = self.folder_for_uid(reply_to_uid);
                    self.accounts
                        .send_command(ImapCommand::SetFlag {
                            uid: reply_to_uid,
                            flag: EmailFlags::ANSWERED,
                            folder,
                        })
                        .await
                        .ok();
                }

                let account_name = self
                    .state
                    .connection
                    .account_names
                    .get(send_account_index)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                self.state
                    .set_status(format!("Email sent from {}", account_name));
                self.state.view = View::Inbox;
            }
            Err(e) => {
                self.state.set_error(format!("Failed to send: {}", e));
            }
        }

        self.state.status.loading = false;
    }

    pub(super) fn cancel_compose(&mut self) {
        if matches!(self.state.view, View::Composer { .. }) {
            self.state.view = View::Inbox;
        }
    }

    /// Cycle through accounts for sending in composer
    pub(super) fn cycle_send_account(&mut self) {
        if let View::Composer { ref mut email, .. } = self.state.view {
            let account_count = self.accounts.count();
            if account_count <= 1 {
                return; // Only one account, nothing to cycle
            }

            let current_index = email
                .from_account_index
                .unwrap_or(self.state.connection.account_index);
            let next_index = (current_index + 1) % account_count;
            email.from_account_index = Some(next_index);
        }
    }

    /// Update autocomplete suggestions based on To or Cc field content
    pub(crate) async fn update_autocomplete(&mut self) {
        if let View::Composer { ref email, field } = self.state.view {
            // Get the current field value
            let field_value = match field {
                ComposerField::To => &email.to,
                ComposerField::Cc => &email.cc,
                _ => {
                    self.state.autocomplete.visible = false;
                    self.state.autocomplete.suggestions.clear();
                    return;
                }
            };

            if field_value.is_empty() {
                self.state.autocomplete.visible = false;
                self.state.autocomplete.suggestions.clear();
                return;
            }

            // Get the text after the last comma (for multi-recipient support)
            let search_text = field_value
                .rfind(',')
                .map(|idx| field_value[idx + 1..].trim())
                .unwrap_or(field_value)
                .trim();

            if search_text.is_empty() {
                self.state.autocomplete.visible = false;
                self.state.autocomplete.suggestions.clear();
                return;
            }

            // Search contacts matching the current input
            match self.contacts.search(search_text).await {
                Ok(contacts) => {
                    self.state.autocomplete.suggestions = contacts;
                    self.state.autocomplete.visible =
                        !self.state.autocomplete.suggestions.is_empty();
                    self.state.autocomplete.selected = 0;
                }
                Err(_) => {
                    self.state.autocomplete.visible = false;
                }
            }
        }
    }

    /// Move autocomplete selection up
    pub(crate) fn autocomplete_up(&mut self) {
        if self.state.autocomplete.selected > 0 {
            self.state.autocomplete.selected -= 1;
        }
    }

    /// Move autocomplete selection down
    pub(crate) fn autocomplete_down(&mut self) {
        let max = self.state.autocomplete.suggestions.len().saturating_sub(1);
        if self.state.autocomplete.selected < max {
            self.state.autocomplete.selected += 1;
        }
    }

    /// Select the current autocomplete suggestion
    pub(crate) fn autocomplete_select(&mut self) {
        if let Some(contact) = self
            .state
            .autocomplete
            .suggestions
            .get(self.state.autocomplete.selected)
            && let View::Composer {
                ref mut email,
                field,
            } = self.state.view
        {
            // Determine which field to update
            let field_value = match field {
                ComposerField::To => &mut email.to,
                ComposerField::Cc => &mut email.cc,
                _ => {
                    self.state.autocomplete.visible = false;
                    self.state.autocomplete.suggestions.clear();
                    return;
                }
            };

            // For multi-recipient: find the last comma and replace text after it
            if let Some(comma_idx) = field_value.rfind(',') {
                // Keep everything up to and including the comma, add selected email
                let prefix = field_value[..=comma_idx].to_string();
                *field_value = format!("{} {}, ", prefix, contact.email);
            } else {
                // Single recipient - just replace with selected email
                *field_value = format!("{}, ", contact.email);
            }
        }
        self.state.autocomplete.visible = false;
        self.state.autocomplete.suggestions.clear();
    }

    /// Close autocomplete dropdown
    pub(crate) fn autocomplete_close(&mut self) {
        self.state.autocomplete.visible = false;
    }
}
