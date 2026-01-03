//! Main event loop and IMAP event processing

use anyhow::Result;
use crossterm::event;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::{Duration, Instant};

use crate::constants::{DELETION_DELAY_SECS, SEARCH_DEBOUNCE_MS};
use crate::input::{InputResult, handle_input};
use crate::mail::types::EmailFlags;
use crate::mail::{
    ImapCommand, ImapEvent, folder_cache_key, group_into_threads, merge_into_threads,
};
use crate::ui::app::{ModalState, View};

use super::{App, EMAIL_PAGE_SIZE};

impl App {
    pub(crate) async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        // Schedule prefetch for the first email (will be processed after debounce)
        self.schedule_prefetch().await;

        loop {
            // Clear expired errors before render
            self.state.clear_error_if_expired();

            // Process pending deletions that have exceeded the grace period
            self.process_pending_deletions().await;

            // Render
            terminal.draw(|f| crate::ui::render(f, &self.state))?;

            // Process any pending prefetch if debounce delay has passed
            self.process_pending_prefetch().await;

            // Process debounced search (body FTS) if timeout has passed
            if let Some(last_input) = self.last_search_input
                && last_input.elapsed() >= Duration::from_millis(SEARCH_DEBOUNCE_MS)
            {
                self.execute_search().await;
                self.last_search_input = None;
            }

            // Handle input (adaptive timeout: faster when loading or pending prefetch)
            let poll_timeout = if self.state.status.loading || self.pending_prefetch.is_some() {
                50
            } else {
                150
            };
            if event::poll(Duration::from_millis(poll_timeout))? {
                let evt = event::read()?;
                match handle_input(evt, &self.state, &self.bindings) {
                    InputResult::Quit => break,
                    InputResult::Action(action) => self.handle_action(action).await?,
                    InputResult::Char(c) => self.handle_char(c).await,
                    InputResult::Backspace => self.handle_backspace().await,
                    InputResult::Continue => {}
                }
            }

            // Process IMAP events from the actor (non-blocking)
            self.process_imap_events().await;

            // Process AI events from the actor (non-blocking)
            self.process_ai_events();

            // Load more emails if user is near the bottom of the list
            if self.state.needs_more_emails() {
                self.load_more_emails().await;
            }
        }

        Ok(())
    }

    /// Process events from all IMAP actors
    pub(crate) async fn process_imap_events(&mut self) {
        let active_index = self.accounts.active_index();

        for account_event in self.accounts.poll_events() {
            let is_active = account_event.account_index == active_index;
            tracing::debug!(
                "Received IMAP event from account {}: {:?}",
                account_event.account_index,
                account_event.event
            );

            match account_event.event {
                ImapEvent::Connected => {
                    tracing::info!(
                        "Account {} connected to IMAP server",
                        account_event.account_index
                    );
                    if is_active {
                        self.state.connection.connected = true;
                        self.state.set_status("Connected");
                        self.state.clear_error();
                    }
                }
                ImapEvent::SyncStarted => {
                    if is_active {
                        self.state.status.loading = true;
                        self.state.set_status("Syncing...");
                    }
                }
                ImapEvent::SyncComplete {
                    new_count,
                    total,
                    full_sync,
                } => {
                    tracing::info!(
                        "Account {} sync complete: new={}, total={}, full={}",
                        account_event.account_index,
                        new_count,
                        total,
                        full_sync
                    );

                    if is_active {
                        self.state.status.loading = false;
                        self.state.connection.last_sync = Some(chrono::Utc::now().timestamp());
                        self.reload_from_cache().await;
                        tracing::debug!(
                            "After reload: {} emails in state",
                            self.state.emails.len()
                        );

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

                        // After first successful INBOX sync, prefetch common folders
                        if !self.prefetch_done && self.state.folder.current == "INBOX" {
                            self.prefetch_done = true;
                            // Request folder list first (needed for prefetch)
                            if self.state.folder.list.is_empty() {
                                self.folder_prefetch_pending = true;
                                self.accounts
                                    .send_command(ImapCommand::ListFolders)
                                    .await
                                    .ok();
                            } else {
                                self.schedule_folder_prefetch();
                            }
                        }
                    }
                }
                ImapEvent::NewMail { count } => {
                    if is_active {
                        let msg = if count == 1 {
                            "New mail!".to_string()
                        } else {
                            format!("{} new emails!", count)
                        };
                        self.state.set_status(msg);
                    }

                    // Send desktop notification
                    if let Some(handle) = self.accounts.get(account_event.account_index) {
                        crate::notification::notify_new_mail(
                            &self.config,
                            &handle.config,
                            count,
                            None, // TODO: fetch subject preview for single emails
                        );
                    }
                }
                ImapEvent::BodyFetched { uid, body } => {
                    if is_active {
                        // Remove from in-flight tracking
                        self.in_flight_fetches.remove(&uid);

                        // Update body for both Reader and Inbox preview
                        match &self.state.view {
                            View::Reader { uid: viewing_uid } if *viewing_uid == uid => {
                                self.state.reader.body = Some(body);
                                self.state.status.loading = false;
                                self.state.clear_error();
                            }
                            View::Inbox => {
                                // For inbox preview, check if this is the currently selected email
                                if let Some(email) = self.state.current_email_from_thread()
                                    && email.uid == uid
                                {
                                    self.state.reader.body = Some(body);
                                    self.state.status.loading = false;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                ImapEvent::BodyFetchFailed { uid, error } => {
                    if is_active {
                        // Remove from in-flight tracking
                        self.in_flight_fetches.remove(&uid);

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
                }
                ImapEvent::FlagUpdated { uid, flags } => {
                    if is_active {
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
                                    .filter(|&&idx| {
                                        !self.state.emails[idx].flags.contains(EmailFlags::SEEN)
                                    })
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
                }
                ImapEvent::Deleted { uid: _ } => {
                    if is_active {
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
                }
                ImapEvent::FolderList { folders } => {
                    if is_active {
                        self.state.folder.list = folders;
                        self.state.status.loading = false;
                        // Set INBOX as default if not set
                        if self.state.folder.current.is_empty() {
                            self.state.folder.current = "INBOX".to_string();
                        }
                        // Auto-open folder picker if it was pending
                        if self.state.folder.picker_pending {
                            self.state.folder.picker_pending = false;
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
                        // Trigger folder prefetch if pending (for conversation mode)
                        if self.folder_prefetch_pending {
                            self.folder_prefetch_pending = false;
                            self.schedule_folder_prefetch();
                        }
                    }
                }
                ImapEvent::FolderSelected { folder } => {
                    if is_active {
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
                }
                ImapEvent::PrefetchComplete { folder } => {
                    tracing::debug!("Prefetch complete for folder: {}", folder);
                    // If Sent folder was prefetched and conversation mode is enabled,
                    // reload to merge sent emails into INBOX threads
                    if is_active
                        && self.state.conversation_mode
                        && self.state.folder.current == "INBOX"
                        && folder.to_lowercase().contains("sent")
                    {
                        self.reload_from_cache().await;
                    }
                }
                ImapEvent::Error(e) => {
                    if is_active {
                        self.state.status.loading = false;
                        self.state.connection.connected = false;
                        self.state.set_error(e);
                    }
                }
            }
        }

        // Update other accounts info for status bar after processing events
        self.refresh_other_accounts_info();
    }

    /// Process events from the AI actor (non-blocking)
    pub(crate) fn process_ai_events(&mut self) {
        use crate::ai::AiEvent;
        use crate::ui::app::PolishPreview;

        let Some(ref mut ai) = self.ai_actor else {
            return;
        };

        while let Ok(event) = ai.event_rx.try_recv() {
            match event {
                AiEvent::EmailSummary { uid, summary } => {
                    self.state.reader.summary_loading = false;
                    self.state.reader.cached_summary = Some((uid, summary));
                    self.state.set_status("Summary ready");
                }
                AiEvent::ThreadSummary { thread_id, summary } => {
                    self.state.reader.summary_loading = false;
                    self.state.reader.cached_thread_summary = Some((thread_id, summary));
                    self.state.set_status("Thread summary ready");
                }
                AiEvent::Polished { original, polished } => {
                    self.state.polish.preview = Some(PolishPreview {
                        original,
                        polished,
                        loading: false,
                    });
                    self.state
                        .set_status("Polish ready - Enter to accept, Esc to reject");
                }
                AiEvent::Error(e) => {
                    self.state.reader.summary_loading = false;
                    if let Some(ref mut preview) = self.state.polish.preview {
                        preview.loading = false;
                    }
                    self.state.set_error(e);
                }
            }
        }
    }

    /// Get the folder-specific cache key for the current account and folder
    pub(crate) fn cache_key(&self) -> String {
        folder_cache_key(self.account_id(), &self.state.folder.current)
    }

    /// Get the cache key for a specific email (uses email's folder if set, else current folder)
    pub(crate) fn email_cache_key(&self, email: &crate::mail::types::EmailHeader) -> String {
        let folder = email
            .folder
            .as_deref()
            .unwrap_or(&self.state.folder.current);
        folder_cache_key(self.account_id(), folder)
    }

    /// Reload state from cache (resets to first page using keyset pagination)
    pub(crate) async fn reload_from_cache(&mut self) {
        let cache_key = self.cache_key();
        // First page: no cursor (None)
        if let Ok(mut emails) = self
            .cache
            .get_emails_before_cursor(&cache_key, None, EMAIL_PAGE_SIZE)
            .await
        {
            // If conversation mode is enabled and we're in INBOX, merge sent emails
            if self.state.conversation_mode
                && self.state.folder.current == "INBOX"
                && let Some(sent_folder) = self.find_sent_folder()
            {
                let sent_cache_key = folder_cache_key(self.account_id(), &sent_folder);
                if let Ok(sent_emails) = self
                    .cache
                    .get_emails_before_cursor(&sent_cache_key, None, EMAIL_PAGE_SIZE)
                    .await
                {
                    // Merge sent emails (they have folder field set so can be distinguished)
                    emails.extend(sent_emails);
                    // Sort by date descending for consistent ordering
                    emails.sort_by(|a, b| b.date.cmp(&a.date));
                }
            }

            self.state.pagination.emails_loaded = emails.len();
            self.state.pagination.all_loaded = emails.len() < EMAIL_PAGE_SIZE;
            // Update pagination cursor to oldest email's (date, uid) for deterministic ordering
            self.state.pagination.cursor = emails.last().map(|e| (e.date, e.uid));
            // Assign emails first, then build threads from reference (avoids clone)
            self.state.emails = emails;
            self.state.thread.threads = group_into_threads(&self.state.emails);
            // Invalidate search cache since threads changed
            self.state.invalidate_search_cache();
        }
        if let Ok(count) = self.cache.get_email_count(&cache_key).await {
            self.state.total_count = count;
        }
        if let Ok(count) = self.cache.get_unread_count(&cache_key).await {
            self.state.unread_count = count;
        }
        // Clamp selection to visible threads (respects search/starred filter)
        self.state.clamp_selection_to_visible();

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
    }

    /// Find the Sent folder name from the available folder list
    fn find_sent_folder(&self) -> Option<String> {
        // Common sent folder patterns
        const SENT_PATTERNS: &[&str] = &["sent", "sent mail", "sent items", "[gmail]/sent"];

        for folder in &self.state.folder.list {
            let lower = folder.to_lowercase();
            for pattern in SENT_PATTERNS {
                if lower.contains(pattern) {
                    return Some(folder.clone());
                }
            }
        }
        None
    }

    /// Load more emails from cache (keyset pagination - O(1) instead of O(offset))
    pub(crate) async fn load_more_emails(&mut self) {
        if self.state.pagination.all_loaded {
            return;
        }

        let cache_key = self.cache_key();
        // Use cursor-based pagination: get emails older than the last loaded email
        if let Ok(more_emails) = self
            .cache
            .get_emails_before_cursor(&cache_key, self.state.pagination.cursor, EMAIL_PAGE_SIZE)
            .await
        {
            if more_emails.is_empty() {
                self.state.pagination.all_loaded = true;
                return;
            }

            let loaded_count = more_emails.len();
            self.state.pagination.emails_loaded += loaded_count;
            self.state.pagination.all_loaded = loaded_count < EMAIL_PAGE_SIZE;
            // Update cursor to new oldest email's (date, uid) for deterministic ordering
            self.state.pagination.cursor = more_emails.last().map(|e| (e.date, e.uid));

            // Try incremental merge first (much faster for pagination)
            let start_idx = self.state.emails.len();
            self.state.emails.extend(more_emails);

            // Attempt incremental merge - falls back to full rebuild if needed
            if !merge_into_threads(
                &mut self.state.thread.threads,
                &self.state.emails,
                start_idx,
            ) {
                // Incremental merge not possible - do full rebuild
                self.state.thread.threads = group_into_threads(&self.state.emails);
            }

            // Invalidate search cache since threads changed
            self.state.invalidate_search_cache();
        }
    }

    /// Schedule background prefetch of common folders for faster switching
    fn schedule_folder_prefetch(&self) {
        // Common folder patterns to prefetch (handles various naming conventions)
        const PREFETCH_PATTERNS: &[&str] = &["sent", "drafts", "trash", "spam", "archive", "junk"];

        let current = self.state.folder.current.to_lowercase();

        for pattern in PREFETCH_PATTERNS {
            // Find matching folder in the folder list (case-insensitive)
            if let Some(folder) = self
                .state
                .folder
                .list
                .iter()
                .find(|f| f.to_lowercase().contains(pattern))
                .cloned()
            {
                // Skip if it's the current folder
                if folder.to_lowercase() == current {
                    continue;
                }

                // Send prefetch command (non-blocking)
                let _ = self
                    .accounts
                    .active()
                    .imap_handle
                    .cmd_tx
                    .try_send(ImapCommand::PrefetchFolder { folder });
            }
        }

        tracing::debug!("Scheduled folder prefetch for common folders");
    }

    /// Process pending deletions that have exceeded the grace period
    async fn process_pending_deletions(&mut self) {
        use crate::app::undo::UndoableAction;

        let now = Instant::now();

        // Find deletions that should be executed
        let mut to_execute = Vec::new();
        self.pending_deletions.retain(|pd| {
            if now.duration_since(pd.initiated_at).as_secs() >= DELETION_DELAY_SECS {
                to_execute.push((pd.uid, pd.account_id.clone(), pd.folder.clone()));
                false // Remove from pending
            } else {
                true // Keep in pending
            }
        });

        // Execute the deletions
        for (uid, account_id, folder) in to_execute {
            // Only delete if still on correct account/folder
            if account_id == self.account_id() && folder == self.state.folder.current {
                self.accounts
                    .send_command(ImapCommand::Delete { uid })
                    .await
                    .ok();
            }
        }

        // Clean up undo entries for deletions that have been executed
        self.undo_stack.retain(|entry| match &entry.action {
            UndoableAction::Delete { initiated_at, .. } => {
                now.duration_since(*initiated_at).as_secs() < DELETION_DELAY_SECS
            }
            _ => true,
        });
    }
}
