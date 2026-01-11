//! IMAP event handlers
//!
//! Focused handler methods for each IMAP event type, extracted from the event loop.

use crate::app::state::View;
use crate::mail::ImapCommand;
use crate::mail::types::{EmailBody, EmailFlags};

use super::super::App;

impl App {
    /// Handle IMAP Connected event
    pub(crate) fn handle_imap_connected(&mut self) {
        self.state.connection.connected = true;
        self.state.set_status("Connected");
        self.state.clear_error();
    }

    /// Handle IMAP SyncStarted event
    pub(crate) fn handle_imap_sync_started(&mut self) {
        self.state.status.loading = true;
        self.state.set_status("Syncing...");
    }

    /// Handle IMAP SyncComplete event
    pub(crate) async fn handle_imap_sync_complete(
        &mut self,
        new_count: usize,
        total: usize,
        full_sync: bool,
    ) {
        self.state.status.loading = false;
        self.state.connection.last_sync = Some(chrono::Utc::now().timestamp());
        self.reload_from_cache().await;
        tracing::debug!("After reload: {} emails in state", self.state.emails.len());

        // Extract contacts from synced emails
        if new_count > 0 || full_sync {
            self.extract_contacts_from_emails().await;
        }

        let msg = if full_sync {
            format!("Synced {} emails", total)
        } else if new_count == 0 {
            "Up to date".to_string()
        } else {
            format!("{} new emails", new_count)
        };
        self.state.set_status(msg);
        self.state.clear_error();

        // After first successful INBOX sync, prefetch common folders (for active account only)
        if !self.prefetch.folder_done && self.state.folder.current == "INBOX" {
            self.prefetch.folder_done = true;
            // Folder list fetch is now triggered in event_loop for all accounts
            // Just wait for it or schedule prefetch if we have the list
            let account = self.accounts.active();
            if !account.folder_list.is_empty() {
                self.schedule_folder_prefetch();
            } else {
                self.prefetch.folder_pending = true;
            }
        }
    }

    /// Handle IMAP NewMail event
    #[allow(unused_variables)]
    pub(crate) fn handle_imap_new_mail(&mut self, count: usize, account_index: usize) {
        let msg = if count == 1 {
            "New mail!".to_string()
        } else {
            format!("{} new emails!", count)
        };
        self.state.set_status(msg);

        // Send desktop notification
        #[cfg(feature = "notifications")]
        if let Some(handle) = self.accounts.get(account_index) {
            crate::notification::notify_new_mail(
                &self.config,
                &handle.config,
                count,
                None, // TODO: fetch subject preview for single emails
            );
        }
    }

    /// Handle IMAP BodyFetched event
    pub(crate) fn handle_imap_body_fetched(&mut self, uid: u32, body: EmailBody) {
        // Remove from in-flight tracking
        self.prefetch.in_flight.remove(&uid);

        // Update body for both Reader and Inbox preview
        match &self.state.view {
            View::Reader { uid: viewing_uid } if *viewing_uid == uid => {
                self.state.reader.set_body(Some(body));
                self.state.status.loading = false;
                self.state.clear_error();
            }
            View::Inbox => {
                // For inbox preview, check if this is the currently selected email
                if let Some(email) = self.state.current_email_from_thread()
                    && email.uid == uid
                {
                    self.state.reader.set_body(Some(body));
                    self.state.status.loading = false;
                }
            }
            _ => {}
        }
    }

    /// Handle IMAP BodyFetchFailed event
    pub(crate) fn handle_imap_body_fetch_failed(&mut self, uid: u32, error: String) {
        // Remove from in-flight tracking
        self.prefetch.in_flight.remove(&uid);

        // Check if this is the currently viewed/selected email
        let is_current = match &self.state.view {
            View::Reader { uid: viewing_uid } => *viewing_uid == uid,
            View::Inbox => self
                .state
                .current_email_from_thread()
                .is_some_and(|e| e.uid == uid),
            _ => false,
        };

        if is_current {
            self.state.status.loading = false;
            self.state
                .set_error(format!("Failed to fetch email: {}", error));
        }
    }

    /// Handle IMAP FlagUpdated event
    pub(crate) async fn handle_imap_flag_updated(&mut self, uid: u32, flags: EmailFlags) {
        // UI was already updated optimistically in toggle_read/toggle_star
        // Sync to server state in case of any mismatch
        if let Some(email) = self.state.emails.iter_mut().find(|e| e.uid == uid) {
            email.flags = flags;
        }
        // Update thread unread counts (threads now use indices, not clones)
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
        // Update unread count from cache (authoritative source)
        if let Ok(count) = self.cache.get_unread_count(&self.cache_key()).await {
            self.state.unread_count = count;
        }
        // Clear status/error to confirm success
        self.state.status.message.clear();
        self.state.clear_error();
    }

    /// Handle IMAP Deleted event
    pub(crate) async fn handle_imap_deleted(&mut self) {
        // UI was already updated optimistically in delete_selected()
        // Just update counts from cache (now reflects server state)
        let cache_key = self.cache_key();
        if let Ok(count) = self.cache.get_email_count(&cache_key).await {
            self.state.total_count = count;
        }
        if let Ok(count) = self.cache.get_unread_count(&cache_key).await {
            self.state.unread_count = count;
        }

        self.state.set_status("Email deleted");
    }

    /// Handle IMAP FolderList event
    pub(crate) async fn handle_imap_folder_list(&mut self, folders: Vec<String>) {
        self.state.folder.list = folders;
        self.state.status.loading = false;
        // Set INBOX as default if not set
        if self.state.folder.current.is_empty() {
            self.state.folder.current = "INBOX".to_string();
        }
        // Sync sidebar selection if sidebar is visible (folders just loaded)
        if self.state.folder.sidebar_visible {
            // Set selection to current folder
            if let Some(idx) = self
                .state
                .folder
                .list
                .iter()
                .position(|f| f == &self.state.folder.current)
            {
                self.state.folder.sidebar_selected = idx;
            }
        }
        // Trigger folder prefetch if pending (for conversation mode)
        if self.prefetch.folder_pending {
            self.prefetch.folder_pending = false;
            self.schedule_folder_prefetch();
        }

        // Spawn folder monitor for Sent folder if conversation mode is enabled
        if self.state.conversation_mode
            && let Some(sent_folder) = self.find_sent_folder()
        {
            let account_idx = self.accounts.active_index();
            if let Err(e) = self
                .accounts
                .spawn_folder_monitor(account_idx, &sent_folder)
                .await
            {
                tracing::warn!("Failed to spawn Sent folder monitor: {}", e);
            }
        }
    }

    /// Handle IMAP FolderSelected event
    pub(crate) fn handle_imap_folder_selected(&mut self, folder: String) {
        self.state.folder.current = folder.clone();
        self.state.set_status(format!("Switched to {}", folder));
        // Trigger sync for the new folder
        self.accounts
            .active()
            .imap_handle
            .cmd_tx
            .try_send(ImapCommand::Sync)
            .ok();
    }

    /// Handle IMAP PrefetchComplete event
    pub(crate) async fn handle_imap_prefetch_complete(&mut self, folder: String) {
        tracing::debug!("Prefetch complete for folder: {}", folder);
        // If Sent folder was prefetched and conversation mode is enabled,
        // reload to merge sent emails into INBOX threads
        if self.state.conversation_mode
            && self.state.folder.current == "INBOX"
            && folder.to_lowercase().contains("sent")
        {
            self.reload_from_cache().await;
        }
    }

    /// Handle IMAP Error event
    pub(crate) fn handle_imap_error(&mut self, error: String) {
        self.state.status.loading = false;
        self.state.connection.connected = false;
        self.state.set_error(error);
    }
}
