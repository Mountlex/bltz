//! Email body prefetching

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::app::state::View;
use crate::mail::ImapCommand;

use super::super::{App, PREFETCH_DEBOUNCE_MS};

impl App {
    /// Schedule a prefetch for the currently selected email and nearby emails (debounced)
    pub(crate) async fn schedule_prefetch(&mut self) {
        if !matches!(self.state.view, View::Inbox) {
            return;
        }

        let current_uid = match self.state.current_email_from_thread() {
            Some(email) => email.uid,
            None => return,
        };

        // Skip if we already have this body loaded
        if self.last_prefetch_uid == Some(current_uid) && self.state.reader.body.is_some() {
            return;
        }

        // Reset scroll for new email
        self.state.reader.scroll = 0;

        // Check local cache for current email first (instant, no debounce needed)
        // Use email's folder to get the correct cache key (important for sent emails in conversation mode)
        let current_email = self.state.current_email_from_thread().cloned();
        let email_cache_key = current_email
            .as_ref()
            .map(|e| self.email_cache_key(e))
            .unwrap_or_else(|| self.cache_key());
        if let Ok(Some(body)) = self
            .cache
            .get_email_body(&email_cache_key, current_uid)
            .await
        {
            self.state.reader.set_body(Some(body));
            self.last_prefetch_uid = Some(current_uid);
        } else {
            // Only clear if not in cache - avoids flash of empty content
            self.state.reader.set_body(None);

            // If current email is from a different folder (e.g., Sent in conversation mode),
            // fetch it directly since batch prefetch only handles current folder
            if let Some(ref email) = current_email {
                let email_folder = email
                    .folder
                    .clone()
                    .unwrap_or_else(|| self.state.folder.current.clone());
                if email_folder != self.state.folder.current
                    && !self.in_flight_fetches.contains(&current_uid)
                {
                    // Fetch this email's body directly
                    // Only mark as in-flight if send succeeds to avoid stuck UIDs
                    match self.accounts.active().imap_handle.cmd_tx.try_send(
                        ImapCommand::FetchBody {
                            uid: current_uid,
                            folder: email_folder,
                        },
                    ) {
                        Ok(_) => {
                            self.state.status.loading = true;
                            self.in_flight_fetches.insert(current_uid);
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                            tracing::warn!(
                                "IMAP command queue full, skipping prefetch for uid {}",
                                current_uid
                            );
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                            tracing::error!("IMAP actor disconnected");
                        }
                    }
                }
            }
        }
        let cache_key = self.cache_key();

        // Get nearby UIDs for prefetching
        let radius = self.config.cache.prefetch_radius;
        let all_uids = self.state.nearby_email_uids(radius);

        // Filter out UIDs that are in-flight first
        let candidate_uids: Vec<u32> = all_uids
            .into_iter()
            .filter(|uid| !self.in_flight_fetches.contains(uid))
            .collect();

        // Batch check cache for all candidates (single query instead of N queries)
        let cached_uids = self
            .cache
            .get_cached_body_uids(&cache_key, &candidate_uids)
            .await
            .unwrap_or_default();

        // Filter out already cached UIDs
        let uids_to_fetch: Vec<u32> = candidate_uids
            .into_iter()
            .filter(|uid| !cached_uids.contains(uid))
            .collect();

        if uids_to_fetch.is_empty() {
            // Don't clear pending_prefetch if there are already pending UIDs
            // (preserves previous navigation's prefetch requests)
            return;
        }

        // Merge with existing pending prefetch to avoid losing UIDs during rapid navigation
        let (merged_uids, timestamp) = match self.pending_prefetch.take() {
            Some((existing_uids, ts)) => {
                // Use HashSet for O(1) deduplication instead of O(n) Vec::contains
                let mut uid_set: HashSet<u32> = existing_uids.into_iter().collect();
                uid_set.extend(uids_to_fetch);
                (uid_set.into_iter().collect(), ts) // Keep original timestamp for debounce
            }
            None => (uids_to_fetch, Instant::now()),
        };

        self.pending_prefetch = Some((merged_uids, timestamp));
    }

    /// Process any pending prefetch if debounce delay has passed
    pub(crate) async fn process_pending_prefetch(&mut self) {
        let (uids, requested_at) = match self.pending_prefetch.take() {
            Some(p) => p,
            None => return,
        };

        // Check if debounce delay has passed
        if requested_at.elapsed() < Duration::from_millis(PREFETCH_DEBOUNCE_MS) {
            // Put it back - not ready yet
            self.pending_prefetch = Some((uids, requested_at));
            return;
        }

        // Get current selection for tracking
        let current_email = self.state.current_email_from_thread().cloned();
        let current_uid = current_email.as_ref().map(|e| e.uid);

        let cache_key = self.cache_key();

        // Batch check which UIDs are already cached (might have been populated by sync)
        let cached_uids = self
            .cache
            .get_cached_body_uids(&cache_key, &uids)
            .await
            .unwrap_or_default();

        // If current email is now cached, load it (use email's folder for correct cache key)
        if let Some(ref email) = current_email
            && self.state.reader.body.is_none()
        {
            let email_cache_key = self.email_cache_key(email);
            if let Ok(Some(body)) = self.cache.get_email_body(&email_cache_key, email.uid).await {
                self.state.reader.set_body(Some(body));
                self.last_prefetch_uid = Some(email.uid);
            }
        }

        // Filter to only uncached UIDs from the current folder
        // (In conversation mode, emails from other folders like Sent are merged in,
        // but we can only batch-fetch from one folder at a time via IMAP)
        let current_folder = &self.state.folder.current;
        let uids_to_fetch: Vec<u32> = uids
            .into_iter()
            .filter(|uid| !cached_uids.contains(uid))
            .filter(|uid| {
                // Only prefetch emails from current folder
                self.state
                    .emails
                    .iter()
                    .find(|e| e.uid == *uid)
                    .map(|e| e.folder.as_deref() == Some(current_folder) || e.folder.is_none())
                    .unwrap_or(true)
            })
            .collect();

        // Send single batch fetch command (more efficient than N individual requests)
        if !uids_to_fetch.is_empty() {
            match self
                .accounts
                .active()
                .imap_handle
                .cmd_tx
                .try_send(ImapCommand::FetchBodies {
                    uids: uids_to_fetch.clone(),
                    folder: current_folder.clone(),
                }) {
                Ok(_) => {
                    // Track all UIDs as in-flight
                    for uid in uids_to_fetch {
                        self.in_flight_fetches.insert(uid);
                    }
                }
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    tracing::warn!(
                        "IMAP command queue full, skipping batch prefetch for {} uids",
                        uids_to_fetch.len()
                    );
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    tracing::error!("IMAP actor disconnected");
                }
            }
        }

        // Update last_prefetch_uid to current selection
        if let Some(uid) = current_uid {
            self.last_prefetch_uid = Some(uid);
        }
    }
}
