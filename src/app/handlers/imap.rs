//! IMAP event handlers
//!
//! Focused handler methods for each IMAP event type, extracted from the event loop.
//!
//! ## Startup Sequence (Conversation Mode)
//!
//! When conversation mode is enabled, the app needs to sync both INBOX and
//! Sent folder before displaying the merged conversation view. The sequence is:
//!
//! 1. IMAP actor connects → `Connected` event → request folder list
//! 2. IMAP actor syncs INBOX → `SyncComplete` event → mark inbox_synced
//! 3. Folder list received → `FolderList` event → spawn Sent folder monitor
//! 4. Sent folder monitor syncs → `SyncComplete` (from monitor) → mark sent_folder_synced
//! 5. All requirements met → `try_initial_load()` → display conversation view
//!
//! Without conversation mode, step 4 is skipped and the view is displayed
//! as soon as INBOX sync completes and folder list is received.

use crate::app::state::View;
use crate::mail::types::{EmailBody, EmailFlags};
use crate::mail::{ImapCommand, ImapError};

use super::super::App;

impl App {
    /// Handle IMAP Connected event
    ///
    /// On connection, immediately request the folder list so we can:
    /// 1. Display available folders in the sidebar
    /// 2. Spawn the Sent folder monitor for conversation mode
    pub(crate) fn handle_imap_connected(&mut self) {
        self.state.connection.connected = true;
        self.state.set_status("Connected");
        self.state.clear_error();

        // Request folder list immediately on connection (not after sync)
        // This allows us to spawn the Sent folder monitor early
        let account = self.accounts.active();
        if account.folder_list.is_empty() {
            account
                .imap_handle
                .cmd_tx
                .try_send(ImapCommand::ListFolders)
                .ok();
        }
    }

    /// Handle IMAP SyncStarted event
    pub(crate) fn handle_imap_sync_started(&mut self) {
        self.state.status.loading = true;
        self.state.set_status("Syncing...");
    }

    /// Handle IMAP SyncComplete event (main INBOX actor)
    ///
    /// This is called when the main IMAP actor completes syncing the current folder.
    /// For startup sequence, we track inbox_synced and check if we can do initial load.
    pub(crate) async fn handle_imap_sync_complete(
        &mut self,
        new_count: usize,
        total: usize,
        full_sync: bool,
    ) {
        self.state.status.loading = false;
        self.state.connection.last_sync = Some(chrono::Utc::now().timestamp());

        // Track startup state for INBOX sync
        if self.state.folder.current == "INBOX" && !self.startup.inbox_synced {
            self.startup.inbox_synced = true;
            tracing::debug!("Startup: INBOX synced");
        }

        // Try initial load if startup requirements are met
        // Otherwise, reload from cache for subsequent syncs
        if self.try_initial_load().await {
            tracing::debug!("Startup: initial load complete");
        } else if self.startup.initial_load_done {
            // Not startup - regular sync, reload normally
            self.reload_from_cache().await;
        }
        // If startup not done yet, we wait for other requirements

        tracing::debug!("After sync: {} emails in state", self.state.emails.len());

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
    ///
    /// In conversation mode, emails from different folders (INBOX, Sent) are merged,
    /// but UIDs are only unique within a folder. We must match by both UID and folder
    /// to avoid displaying the wrong email body.
    pub(crate) fn handle_imap_body_fetched(&mut self, uid: u32, folder: &str, body: EmailBody) {
        // Remove from in-flight tracking
        self.prefetch.in_flight.remove(&uid);

        // Helper to check if an email's folder matches the fetched body's folder.
        // If email has no folder set, assume it's from the current folder.
        let folder_matches = |email: &crate::mail::types::EmailHeader| {
            email
                .folder
                .as_ref()
                .map(|f| f == folder)
                .unwrap_or_else(|| folder == self.state.folder.current)
        };

        // Update body for both Reader and Inbox preview
        match &self.state.view {
            View::Reader { uid: viewing_uid } if *viewing_uid == uid => {
                // Find the email being viewed and verify its folder matches the fetched body.
                // In conversation mode, there might be multiple emails with the same UID
                // from different folders, so we must check the folder.
                let email_being_viewed = self
                    .state
                    .emails
                    .iter()
                    .find(|e| e.uid == *viewing_uid && folder_matches(e));

                if email_being_viewed.is_some() {
                    self.state.reader.set_body(Some(body));
                    self.state.status.loading = false;
                    self.state.clear_error();
                    self.dirty = true;
                }
            }
            View::Inbox => {
                // For inbox preview, check if this is the currently selected email
                if let Some(email) = self.state.current_email_from_thread()
                    && email.uid == uid
                    && folder_matches(email)
                {
                    self.state.reader.set_body(Some(body));
                    self.state.status.loading = false;
                    self.dirty = true;
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
    ///
    /// This is called when the server returns the list of available folders.
    /// For conversation mode startup, we immediately spawn the Sent folder monitor
    /// so it can sync while we wait.
    pub(crate) async fn handle_imap_folder_list(&mut self, folders: Vec<String>) {
        self.state.folder.list = folders;
        self.state.status.loading = false;

        // Set INBOX as default if not set
        if self.state.folder.current.is_empty() {
            self.state.folder.current = "INBOX".to_string();
        }

        // Sync sidebar selection if sidebar is visible (folders just loaded)
        if self.state.folder.sidebar_visible
            && let Some(idx) = self
                .state
                .folder
                .list
                .iter()
                .position(|f| f == &self.state.folder.current)
        {
            self.state.folder.sidebar_selected = idx;
        }

        // Trigger folder prefetch if pending
        if self.prefetch.folder_pending {
            self.prefetch.folder_pending = false;
            self.schedule_folder_prefetch();
        }

        // Track startup state
        if !self.startup.folder_list_received {
            self.startup.folder_list_received = true;
            tracing::debug!(
                "Startup: folder list received ({} folders)",
                self.state.folder.list.len()
            );
        }

        // Spawn folder monitor for Sent folder if conversation mode is enabled
        if self.state.conversation_mode {
            if let Some(sent_folder) = self.find_sent_folder() {
                let account_idx = self.accounts.active_index();
                match self
                    .accounts
                    .spawn_folder_monitor(account_idx, &sent_folder)
                    .await
                {
                    Ok(true) => {
                        tracing::debug!(
                            "Startup: spawned Sent folder monitor for '{}'",
                            sent_folder
                        );
                    }
                    Ok(false) => {
                        // Already monitoring - mark as synced (monitor handles its own sync)
                        tracing::debug!("Startup: Sent folder monitor already exists");
                    }
                    Err(e) => {
                        tracing::warn!("Failed to spawn Sent folder monitor: {}", e);
                        // If we can't monitor Sent, mark as synced to avoid blocking startup
                        self.startup.sent_folder_synced = true;
                    }
                }
            } else {
                // No Sent folder found - mark as synced to avoid blocking startup
                tracing::debug!("Startup: no Sent folder found, skipping monitor");
                self.startup.sent_folder_synced = true;
            }
        } else {
            // Not in conversation mode - no need to wait for Sent folder
            self.startup.sent_folder_synced = true;
        }

        // Try initial load if startup requirements are met
        if self.try_initial_load().await {
            tracing::debug!("Startup: initial load complete (after folder list)");
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
        // reload to merge sent emails into INBOX threads (but only after startup)
        if self.startup.initial_load_done
            && self.state.conversation_mode
            && self.state.folder.current == "INBOX"
            && folder.to_lowercase().contains("sent")
        {
            self.reload_from_cache().await;
        }
    }

    /// Try to perform the initial cache load if all startup requirements are met
    ///
    /// Returns true if the initial load was performed, false if requirements not yet met.
    ///
    /// ## Requirements
    /// - INBOX must be synced
    /// - Folder list must be received
    /// - In conversation mode: Sent folder must also be synced
    pub(crate) async fn try_initial_load(&mut self) -> bool {
        if !self.startup.is_ready(self.state.conversation_mode) {
            tracing::debug!(
                "Startup not ready: inbox={}, folders={}, sent={}, done={}",
                self.startup.inbox_synced,
                self.startup.folder_list_received,
                self.startup.sent_folder_synced,
                self.startup.initial_load_done
            );
            return false;
        }

        tracing::info!(
            "Startup requirements met, performing initial load (conversation_mode={})",
            self.state.conversation_mode
        );

        self.startup.initial_load_done = true;
        self.reload_from_cache().await;

        // Schedule prefetch for the first email
        self.schedule_prefetch().await;

        true
    }

    /// Handle IMAP Error event
    pub(crate) fn handle_imap_error(&mut self, error: ImapError) {
        self.state.status.loading = false;

        // Mark disconnected for connection-related errors
        let is_connection_error = matches!(
            error,
            ImapError::ConnectionFailed(_)
                | ImapError::TlsFailed(_)
                | ImapError::Disconnected
                | ImapError::Timeout
                | ImapError::MaxRetriesExceeded
        );

        if is_connection_error {
            self.state.connection.connected = false;
        }

        // Use Display impl for user-friendly error message
        self.state.set_error(error.to_string());
    }
}
