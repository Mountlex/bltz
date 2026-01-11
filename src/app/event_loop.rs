//! Main event loop and IMAP event processing

use anyhow::Result;
use crossterm::event;
use std::time::{Duration, Instant};

use crate::constants::{DELETION_DELAY_SECS, SEARCH_DEBOUNCE_MS};
use crate::input::{InputResult, handle_input};
use crate::mail::{
    ImapCommand, ImapEvent, folder_cache_key, group_into_threads, merge_into_threads,
};

use super::render_thread::RenderThread;
use super::{App, EMAIL_PAGE_SIZE};

impl App {
    pub(crate) async fn event_loop(&mut self, render_thread: &RenderThread) -> Result<()> {
        // Schedule prefetch for the first email (will be processed after debounce)
        self.schedule_prefetch().await;

        loop {
            // Process IMAP events FIRST (non-blocking) - prioritize responsiveness
            if self.process_imap_events().await {
                self.dirty = true;
            }

            // Process AI events from the actor (non-blocking)
            if self.process_ai_events() {
                self.dirty = true;
            }

            // Clear expired errors
            if self.state.clear_error_if_expired() {
                self.dirty = true;
            }

            // Process pending deletions that have exceeded the grace period
            if self.process_pending_deletions().await {
                self.dirty = true;
            }

            // Render only when dirty (non-blocking - sends to render thread)
            if self.dirty {
                render_thread.render(self.state.clone());
                self.dirty = false;
            }

            // Process any pending prefetch if debounce delay has passed
            self.process_pending_prefetch().await;

            // Process debounced search (body FTS) if timeout has passed
            if let Some(last_input) = self.last_search_input
                && last_input.elapsed() >= Duration::from_millis(SEARCH_DEBOUNCE_MS)
            {
                self.execute_search().await;
                self.last_search_input = None;
                self.dirty = true;
            }

            // Handle input (adaptive timeout: faster when loading or pending prefetch)
            let poll_timeout = if self.state.status.loading || self.prefetch.pending.is_some() {
                50
            } else {
                150
            };
            if event::poll(Duration::from_millis(poll_timeout))? {
                let evt = event::read()?;
                // Any input event (including resize) requires re-render
                self.dirty = true;
                match handle_input(evt, &self.state, &self.bindings) {
                    InputResult::Quit => break,
                    InputResult::Action(action) => {
                        self.state.acknowledge_error();
                        self.handle_action(action).await?;
                    }
                    InputResult::Char(c) => {
                        self.state.acknowledge_error();
                        self.handle_char(c).await;
                    }
                    InputResult::Backspace => {
                        self.state.acknowledge_error();
                        self.handle_backspace().await;
                    }
                    InputResult::Continue => {}
                }
            }

            // Load more emails if user is near the bottom of the list
            if self.state.needs_more_emails() {
                self.load_more_emails().await;
                self.dirty = true;
            }
        }

        Ok(())
    }

    /// Process events from all IMAP actors. Returns true if any events were processed.
    pub(crate) async fn process_imap_events(&mut self) -> bool {
        let active_index = self.accounts.active_index();
        let events = self.accounts.poll_events();
        let had_events = !events.is_empty();

        for account_event in events {
            let is_active = account_event.account_index == active_index;
            tracing::debug!(
                "Received IMAP event from account {}: {:?}",
                account_event.account_index,
                account_event.event
            );

            // Handle folder monitor events separately (early filter)
            if let Some(ref folder) = account_event.folder {
                self.handle_folder_monitor_event(is_active, folder, &account_event.event)
                    .await;
                continue;
            }

            // Main actor events only below (folder is always None)
            match account_event.event {
                ImapEvent::Connected => {
                    tracing::info!(
                        "Account {} connected to IMAP server",
                        account_event.account_index
                    );
                    if is_active {
                        self.handle_imap_connected();
                    }
                }
                ImapEvent::SyncStarted => {
                    if is_active {
                        self.handle_imap_sync_started();
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
                    // Request folder list for any account that doesn't have one yet
                    if let Some(handle) = self.accounts.get(account_event.account_index)
                        && handle.folder_list.is_empty()
                    {
                        handle
                            .imap_handle
                            .cmd_tx
                            .try_send(ImapCommand::ListFolders)
                            .ok();
                    }
                    // Process UI updates only for active account
                    if is_active {
                        self.handle_imap_sync_complete(new_count, total, full_sync)
                            .await;
                    }
                }
                ImapEvent::NewMail { count } => {
                    if is_active {
                        self.handle_imap_new_mail(count, account_event.account_index);
                    } else {
                        // Send desktop notification for non-active accounts too
                        #[cfg(feature = "notifications")]
                        if let Some(handle) = self.accounts.get(account_event.account_index) {
                            crate::notification::notify_new_mail(
                                &self.config,
                                &handle.config,
                                count,
                                None,
                            );
                        }
                    }
                }
                ImapEvent::BodyFetched { uid, body } => {
                    if is_active {
                        self.handle_imap_body_fetched(uid, body);
                    }
                }
                ImapEvent::BodyFetchFailed { uid, error } => {
                    if is_active {
                        self.handle_imap_body_fetch_failed(uid, error);
                    }
                }
                ImapEvent::FlagUpdated { uid, flags } => {
                    if is_active {
                        self.handle_imap_flag_updated(uid, flags).await;
                    }
                }
                ImapEvent::Deleted { uid: _ } => {
                    if is_active {
                        self.handle_imap_deleted().await;
                    }
                }
                ImapEvent::FolderList { folders } => {
                    // Always store in the originating account (not just active)
                    if let Some(handle) = self.accounts.get_mut(account_event.account_index) {
                        handle.folder_list = folders.clone();
                    }
                    // Only process UI updates if active
                    if is_active {
                        self.handle_imap_folder_list(folders).await;
                    }
                }
                ImapEvent::FolderSelected { folder } => {
                    if is_active {
                        self.handle_imap_folder_selected(folder);
                    }
                }
                ImapEvent::PrefetchComplete { folder } => {
                    if is_active {
                        self.handle_imap_prefetch_complete(folder).await;
                    }
                }
                ImapEvent::AttachmentFetched {
                    uid,
                    attachment_index,
                    attachment,
                    data,
                } => {
                    if is_active {
                        self.handle_attachment_fetched(uid, attachment_index, attachment, data)
                            .await;
                    }
                }
                ImapEvent::AttachmentFetchFailed {
                    uid,
                    attachment_index,
                    error,
                } => {
                    if is_active {
                        self.handle_attachment_fetch_failed(uid, attachment_index, error);
                    }
                }
                ImapEvent::Error(e) => {
                    if is_active {
                        self.handle_imap_error(e);
                    }
                }
            }
        }

        // Update other accounts info for status bar after processing events
        self.refresh_other_accounts_info();

        had_events
    }

    /// Handle events from folder monitors (e.g., Sent folder).
    /// These are handled separately to avoid affecting main UI state.
    async fn handle_folder_monitor_event(
        &mut self,
        is_active: bool,
        folder: &str,
        event: &ImapEvent,
    ) {
        let is_sent = folder.to_lowercase().contains("sent");

        match event {
            ImapEvent::SyncComplete { .. } => {
                // Reload for Sent folder in conversation mode to update threads
                if is_sent
                    && is_active
                    && self.state.conversation_mode
                    && self.state.folder.current == "INBOX"
                {
                    tracing::debug!("Sent folder synced, refreshing conversation threads");
                    self.reload_from_cache().await;
                }
            }
            ImapEvent::Error(e) => {
                tracing::warn!("Folder monitor '{}' error: {}", folder, e);
            }
            _ => {
                tracing::debug!("Folder monitor '{}' event: {:?}", folder, event);
            }
        }
    }

    /// Process events from the AI actor (non-blocking). Returns true if any events were processed.
    pub(crate) fn process_ai_events(&mut self) -> bool {
        use crate::ai::AiEvent;
        use crate::app::state::PolishPreview;

        let Some(ref mut ai) = self.ai_actor else {
            return false;
        };

        let mut had_events = false;
        while let Ok(event) = ai.event_rx.try_recv() {
            had_events = true;
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
        had_events
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

    /// Get cache key for an email by UID, using the email's actual folder.
    /// Falls back to current folder if email not found or has no folder.
    /// Use this when you only have a UID and need the correct cache key for conversation mode.
    pub(crate) fn cache_key_for_uid(&self, uid: u32) -> String {
        let folder = self
            .state
            .emails
            .iter()
            .find(|e| e.uid == uid)
            .and_then(|e| e.folder.clone())
            .unwrap_or_else(|| self.state.folder.current.clone());
        folder_cache_key(self.account_id(), &folder)
    }

    /// Get the folder for an email by UID.
    /// Falls back to current folder if email not found or has no folder.
    pub(crate) fn folder_for_uid(&self, uid: u32) -> String {
        self.state
            .emails
            .iter()
            .find(|e| e.uid == uid)
            .and_then(|e| e.folder.clone())
            .unwrap_or_else(|| self.state.folder.current.clone())
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

        // Clear stale body if current email changed (e.g., after sent emails merged)
        // and schedule prefetch for the new current email
        if let Some(current) = self.state.current_email_from_thread()
            && self.prefetch.last_uid != Some(current.uid)
        {
            self.state.reader.set_body(None);
            self.prefetch.last_uid = None;
            // Schedule prefetch for the new current email
            self.schedule_prefetch().await;
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
    }

    /// Find the Sent folder name from the available folder list
    pub(crate) fn find_sent_folder(&self) -> Option<String> {
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
    pub(crate) fn schedule_folder_prefetch(&self) {
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

    /// Process pending deletions that have exceeded the grace period.
    /// Returns true if any deletions were processed.
    async fn process_pending_deletions(&mut self) -> bool {
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

        let had_deletions = !to_execute.is_empty();

        // Execute the deletions (route to correct account even if we switched)
        for (uid, account_id, folder) in to_execute {
            if let Some(account_idx) = self.accounts.index_of(&account_id) {
                self.accounts
                    .send_command_to(account_idx, ImapCommand::Delete { uid, folder })
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

        had_deletions
    }
}
