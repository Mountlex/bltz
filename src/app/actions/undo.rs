//! Undo action implementation

use crate::app::undo::UndoableAction;
use crate::mail::types::EmailFlags;
use crate::mail::{ImapCommand, group_into_threads};

use super::super::App;

impl App {
    /// Execute undo for the most recent undoable action
    pub(crate) async fn undo(&mut self) {
        // Pop the most recent undo entry
        let entry = match self.undo_stack.pop() {
            Some(e) => e,
            None => {
                self.state.set_status("Nothing to undo");
                return;
            }
        };

        // Verify we're on the same account/folder (undo is context-sensitive)
        if entry.account_id != self.account_id() || entry.folder != self.state.folder.current {
            self.state
                .set_error("Cannot undo action from different account/folder");
            return;
        }

        match entry.action {
            UndoableAction::ToggleRead { uid, was_seen } => {
                self.undo_toggle_read(uid, was_seen).await;
            }
            UndoableAction::ToggleStar { uid, was_flagged } => {
                self.undo_toggle_star(uid, was_flagged).await;
            }
            UndoableAction::Delete {
                email,
                thread_index,
                ..
            } => {
                self.undo_delete(*email, thread_index).await;
            }
        }
    }

    async fn undo_toggle_read(&mut self, uid: u32, was_seen: bool) {
        // Restore previous state in UI
        if let Some(email) = self.state.emails.iter_mut().find(|e| e.uid == uid) {
            if was_seen {
                email.flags.insert(EmailFlags::SEEN);
            } else {
                email.flags.remove(EmailFlags::SEEN);
            }
        }

        // Update thread unread counts (threads use indices, not clones)
        for thread in self.state.thread.threads.iter_mut() {
            if thread
                .email_indices
                .iter()
                .any(|&idx| self.state.emails[idx].uid == uid)
            {
                thread.unread_count = thread
                    .email_indices
                    .iter()
                    .filter(|&&idx| !self.state.emails[idx].flags.contains(EmailFlags::SEEN))
                    .count();
                break;
            }
        }

        // Update unread count
        if was_seen {
            self.state.unread_count = self.state.unread_count.saturating_sub(1);
        } else {
            self.state.unread_count += 1;
        }

        // Send IMAP command to sync server (use email's actual folder)
        let folder = self.folder_for_uid(uid);
        let cmd = if was_seen {
            ImapCommand::SetFlag {
                uid,
                flag: EmailFlags::SEEN,
                folder,
            }
        } else {
            ImapCommand::RemoveFlag {
                uid,
                flag: EmailFlags::SEEN,
                folder,
            }
        };
        self.accounts.send_command(cmd).await.ok();

        self.state.set_status("Undo: read status restored");
    }

    async fn undo_toggle_star(&mut self, uid: u32, was_flagged: bool) {
        // Restore previous state in UI
        // (threads use indices, so updating self.state.emails is sufficient)
        if let Some(email) = self.state.emails.iter_mut().find(|e| e.uid == uid) {
            if was_flagged {
                email.flags.insert(EmailFlags::FLAGGED);
            } else {
                email.flags.remove(EmailFlags::FLAGGED);
            }
        }

        // Send IMAP command to sync server (use email's actual folder)
        let folder = self.folder_for_uid(uid);
        let cmd = if was_flagged {
            ImapCommand::SetFlag {
                uid,
                flag: EmailFlags::FLAGGED,
                folder,
            }
        } else {
            ImapCommand::RemoveFlag {
                uid,
                flag: EmailFlags::FLAGGED,
                folder,
            }
        };
        self.accounts.send_command(cmd).await.ok();

        self.state.set_status("Undo: star status restored");
    }

    async fn undo_delete(&mut self, email: crate::mail::types::EmailHeader, thread_index: usize) {
        let uid = email.uid;

        // Cancel the pending deletion
        self.pending_deletions.retain(|pd| pd.uid != uid);

        // Restore email to state
        self.state.emails.push(email);
        self.state.emails.sort_by(|a, b| b.date.cmp(&a.date));
        self.state.thread.threads = group_into_threads(&self.state.emails);
        // Invalidate search cache since threads changed
        self.state.invalidate_search_cache();

        // Restore selection (or clamp to bounds)
        let visible_count = self.state.visible_thread_count();
        self.state.thread.selected = thread_index.min(visible_count.saturating_sub(1));
        self.state.thread.selected_in_thread = 0;

        self.state.set_status("Undo: email restored");
        // No IMAP command needed - deletion was never sent to server
    }
}
