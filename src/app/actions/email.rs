//! Email actions (open, delete, flags, folder operations)

use std::time::Instant;

use crate::app::undo::{PendingDeletion, UndoEntry, UndoableAction};
use crate::mail::types::EmailFlags;
use crate::mail::{ImapCommand, group_into_threads};
use crate::ui::app::{ModalState, View};

use super::super::App;

impl App {
    pub(super) async fn open_selected(&mut self) {
        // Handle folder selection from picker
        if self.state.modal.is_folder_picker() {
            self.select_folder().await;
            return;
        }

        // Handle contacts view - open composer with selected contact
        if matches!(self.state.view, View::Contacts) {
            if self.state.contacts.editing.is_some() {
                // In edit mode, Enter saves
                self.contacts_save_edit().await;
            } else {
                // Not editing, compose to contact
                self.contacts_compose_to();
            }
            return;
        }

        let email = match &self.state.view {
            View::Inbox => self.state.current_email_from_thread().cloned(),
            _ => None,
        };

        if let Some(email) = email {
            let uid = email.uid;
            let cache_key = self.email_cache_key(&email);

            // Mark as read
            if !email.is_seen() {
                self.accounts
                    .send_command(ImapCommand::SetFlag {
                        uid,
                        flag: EmailFlags::SEEN,
                    })
                    .await
                    .ok();
            }

            // Switch to reader view
            self.state.view = View::Reader { uid };
            self.state.reader.reset_scroll();
            self.state.reader.body = None;

            // Check local cache first (instant) - use email's folder cache key
            if let Ok(Some(body)) = self.cache.get_email_body(&cache_key, uid).await {
                self.state.reader.body = Some(body);
                return;
            }

            // Request body fetch (non-blocking - result comes via event)
            // Use email's folder (important for sent emails in conversation mode)
            let folder = email
                .folder
                .clone()
                .unwrap_or_else(|| self.state.folder.current.clone());
            self.state.status.loading = true;
            self.accounts
                .send_command(ImapCommand::FetchBody { uid, folder })
                .await
                .ok();
        }
    }

    pub(super) async fn go_back(&mut self) {
        // If a modal is open, close it first
        match self.state.modal {
            ModalState::FolderPicker => {
                self.state.modal = ModalState::None;
                return;
            }
            ModalState::Command { .. } => {
                self.exit_command_mode();
                return;
            }
            ModalState::Search => {
                self.state.modal = ModalState::None;
                // Keep search query so results stay filtered (Esc just closes input)
                return;
            }
            ModalState::Help { .. } => {
                self.state.modal = ModalState::None;
                return;
            }
            ModalState::None => {}
        }

        match &self.state.view {
            View::Inbox => {
                // Clear search if we're in inbox with no other place to go
                if !self.state.search.query.is_empty() {
                    self.state.clear_search();
                }
            }
            View::Reader { uid } => {
                let uid = *uid;
                self.state.view = View::Inbox;
                // Try to keep body from cache for smooth transition back to inbox preview
                // Use email's folder to get the correct cache key
                let cache_key = self
                    .state
                    .emails
                    .iter()
                    .find(|e| e.uid == uid)
                    .map(|e| self.email_cache_key(e))
                    .unwrap_or_else(|| self.cache_key());
                if let Ok(Some(body)) = self.cache.get_email_body(&cache_key, uid).await {
                    self.state.reader.body = Some(body);
                    self.last_prefetch_uid = Some(uid);
                } else {
                    self.state.reader.body = None;
                }
            }
            View::Composer { .. } => {
                self.state.view = View::Inbox;
                self.state.reader.body = None;
            }
            View::Contacts => {
                // If editing, cancel edit; otherwise go back to inbox
                if self.state.contacts.editing.is_some() {
                    self.contacts_cancel_edit();
                } else {
                    self.state.view = View::Inbox;
                }
            }
            View::AddAccount { .. } => {
                // Cancel wizard and go back to inbox
                self.state.view = View::Inbox;
            }
        }
    }

    pub(super) async fn toggle_folder_picker(&mut self) {
        if self.state.modal.is_folder_picker() {
            self.state.modal = ModalState::None;
        } else {
            // Request folder list if we don't have it
            if self.state.folder.list.is_empty() {
                if self.state.status.loading {
                    self.state.set_status("Loading folders...");
                } else {
                    self.state.status.loading = true;
                    self.state.set_status("Loading folders...");
                    self.state.folder.picker_pending = true;
                    self.accounts
                        .send_command(ImapCommand::ListFolders)
                        .await
                        .ok();
                }
                return;
            }
            self.state.modal = ModalState::FolderPicker;
            // Set selection to current folder
            if let Some(idx) = self
                .state
                .folder
                .list
                .iter()
                .position(|f| f == &self.state.folder.current)
            {
                self.state.folder.selected = idx;
            }
        }
    }

    pub(super) async fn select_folder(&mut self) {
        if let Some(folder) = self
            .state
            .folder
            .list
            .get(self.state.folder.selected)
            .cloned()
        {
            if folder != self.state.folder.current {
                self.state.status.loading = true;
                self.state.set_status(format!("Switching to {}...", folder));

                // Set current folder FIRST (cache_key() depends on this)
                self.state.folder.current = folder.clone();

                // IMMEDIATELY load from cache (shows cached data before network)
                self.reload_from_cache().await;

                // Reset selection state
                self.state.thread.selected = 0;
                self.state.thread.selected_in_thread = 0;
                self.state.reader.body = None;
                self.state.clear_search();
                self.in_flight_fetches.clear();
                self.last_prefetch_uid = None;
                self.pending_prefetch = None;

                // Request folder change and sync (updates cache with fresh data)
                self.accounts
                    .send_command(ImapCommand::SelectFolder { folder })
                    .await
                    .ok();
            }
            self.state.modal = ModalState::None;
        }
    }

    pub(super) async fn delete_selected(&mut self) {
        // Handle contacts view - delete contact
        if matches!(self.state.view, View::Contacts) {
            self.contacts_delete().await;
            return;
        }

        let uid = match &self.state.view {
            View::Inbox => self.state.current_email_from_thread().map(|e| e.uid),
            View::Reader { uid } => Some(*uid),
            _ => None,
        };

        if let Some(uid) = uid {
            // Capture email BEFORE removal for undo
            let email = self.state.emails.iter().find(|e| e.uid == uid).cloned();
            let thread_index = self.state.thread.selected;

            if let Some(email) = email {
                let now = Instant::now();

                // Create pending deletion instead of immediate delete
                let pending = PendingDeletion {
                    uid,
                    email: email.clone(),
                    initiated_at: now,
                    account_id: self.account_id().to_string(),
                    folder: self.state.folder.current.clone(),
                };
                self.pending_deletions.push(pending);

                // Push to undo stack
                self.undo_stack.push(UndoEntry {
                    action: UndoableAction::Delete {
                        email: Box::new(email),
                        initiated_at: now,
                        thread_index,
                    },
                    account_id: self.account_id().to_string(),
                    folder: self.state.folder.current.clone(),
                });

                // Optimistic UI update: remove from local state immediately
                self.state.emails.retain(|e| e.uid != uid);
                self.state.thread.threads = group_into_threads(&self.state.emails);
                // Invalidate search cache since threads changed
                self.state.invalidate_search_cache();

                // Adjust selection if out of bounds
                let visible_count = self.state.visible_threads().len();
                if visible_count == 0 {
                    self.state.thread.selected = 0;
                    self.state.thread.selected_in_thread = 0;
                } else if self.state.thread.selected >= visible_count {
                    self.state.thread.selected = visible_count - 1;
                    self.state.thread.selected_in_thread = 0;
                }

                // Clean up expanded threads that no longer exist
                let thread_ids: std::collections::HashSet<_> = self
                    .state
                    .thread
                    .threads
                    .iter()
                    .map(|t| t.id.clone())
                    .collect();
                self.state
                    .thread
                    .expanded
                    .retain(|id| thread_ids.contains(id));

                // Go back to inbox if in reader
                if matches!(self.state.view, View::Reader { .. }) {
                    self.state.view = View::Inbox;
                }

                self.state.set_status("Deleted (u to undo, 10s)");
                // Do NOT send ImapCommand::Delete yet - delayed execution
            }
        }
    }

    pub(super) async fn toggle_read(&mut self) {
        let uid = match &self.state.view {
            View::Inbox => self.state.current_email_from_thread().map(|e| e.uid),
            View::Reader { uid } => Some(*uid),
            _ => None,
        };

        if let Some(uid) = uid {
            let is_seen = self
                .state
                .emails
                .iter()
                .find(|e| e.uid == uid)
                .map(|e| e.is_seen())
                .unwrap_or(false);

            // Push undo entry BEFORE making changes
            self.undo_stack.push(UndoEntry {
                action: UndoableAction::ToggleRead {
                    uid,
                    was_seen: is_seen,
                },
                account_id: self.account_id().to_string(),
                folder: self.state.folder.current.clone(),
            });

            // OPTIMISTIC UPDATE: Apply flag change immediately to UI state
            if let Some(email) = self.state.emails.iter_mut().find(|e| e.uid == uid) {
                if is_seen {
                    email.flags.remove(EmailFlags::SEEN);
                } else {
                    email.flags.insert(EmailFlags::SEEN);
                }
            }

            // Update thread unread counts (threads use indices, not clones)
            for thread in self.state.thread.threads.iter_mut() {
                if thread
                    .email_indices
                    .iter()
                    .any(|&idx| self.state.emails[idx].uid == uid)
                {
                    // Recalculate thread unread count
                    thread.unread_count = thread
                        .email_indices
                        .iter()
                        .filter(|&&idx| !self.state.emails[idx].flags.contains(EmailFlags::SEEN))
                        .count();
                    break;
                }
            }

            // Update global unread count optimistically
            if is_seen {
                // Marking as unread - increment
                self.state.unread_count += 1;
            } else {
                // Marking as read - decrement
                self.state.unread_count = self.state.unread_count.saturating_sub(1);
            }

            // Show status feedback with undo hint
            self.state.set_status(if is_seen {
                "Marked unread (u to undo)"
            } else {
                "Marked read (u to undo)"
            });

            // Send background command to IMAP server
            if is_seen {
                self.accounts
                    .send_command(ImapCommand::RemoveFlag {
                        uid,
                        flag: EmailFlags::SEEN,
                    })
                    .await
                    .ok();
            } else {
                self.accounts
                    .send_command(ImapCommand::SetFlag {
                        uid,
                        flag: EmailFlags::SEEN,
                    })
                    .await
                    .ok();
            }
        }
    }

    pub(super) async fn toggle_star(&mut self) {
        let uid = match &self.state.view {
            View::Inbox => self.state.current_email_from_thread().map(|e| e.uid),
            View::Reader { uid } => Some(*uid),
            _ => None,
        };

        if let Some(uid) = uid {
            let is_flagged = self
                .state
                .emails
                .iter()
                .find(|e| e.uid == uid)
                .map(|e| e.is_flagged())
                .unwrap_or(false);

            // Push undo entry BEFORE making changes
            self.undo_stack.push(UndoEntry {
                action: UndoableAction::ToggleStar {
                    uid,
                    was_flagged: is_flagged,
                },
                account_id: self.account_id().to_string(),
                folder: self.state.folder.current.clone(),
            });

            // OPTIMISTIC UPDATE: Apply flag change immediately to UI state
            // (threads use indices, so updating self.state.emails is sufficient)
            if let Some(email) = self.state.emails.iter_mut().find(|e| e.uid == uid) {
                if is_flagged {
                    email.flags.remove(EmailFlags::FLAGGED);
                } else {
                    email.flags.insert(EmailFlags::FLAGGED);
                }
            }

            // Show status feedback with undo hint
            self.state.set_status(if is_flagged {
                "Unstarred (u to undo)"
            } else {
                "Starred (u to undo)"
            });

            // Send background command to IMAP server
            if is_flagged {
                self.accounts
                    .send_command(ImapCommand::RemoveFlag {
                        uid,
                        flag: EmailFlags::FLAGGED,
                    })
                    .await
                    .ok();
            } else {
                self.accounts
                    .send_command(ImapCommand::SetFlag {
                        uid,
                        flag: EmailFlags::FLAGGED,
                    })
                    .await
                    .ok();
            }
        }
    }
}
