//! Email body prefetching

use std::time::{Duration, Instant};

use crate::mail::ImapCommand;
use crate::ui::app::View;

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
        let cache_key = self.cache_key();
        if let Ok(Some(body)) = self.cache.get_email_body(&cache_key, current_uid).await {
            self.state.reader.body = Some(body);
            self.last_prefetch_uid = Some(current_uid);
        } else {
            // Only clear if not in cache - avoids flash of empty content
            self.state.reader.body = None;
        }

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
            self.pending_prefetch = None;
            return;
        }

        // Schedule prefetch with debounce - will be sent after delay
        self.pending_prefetch = Some((uids_to_fetch, Instant::now()));
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

        // Get current selection's UID for tracking
        let current_uid = self.state.current_email_from_thread().map(|e| e.uid);

        let cache_key = self.cache_key();

        // Batch check which UIDs are already cached (might have been populated by sync)
        let cached_uids = self
            .cache
            .get_cached_body_uids(&cache_key, &uids)
            .await
            .unwrap_or_default();

        // If current email is now cached, load it
        if let Some(cur_uid) = current_uid {
            if cached_uids.contains(&cur_uid) && self.state.reader.body.is_none() {
                if let Ok(Some(body)) = self.cache.get_email_body(&cache_key, cur_uid).await {
                    self.state.reader.body = Some(body);
                    self.last_prefetch_uid = Some(cur_uid);
                }
            }
        }

        // Filter to only uncached UIDs
        let uids_to_fetch: Vec<u32> = uids
            .into_iter()
            .filter(|uid| !cached_uids.contains(uid))
            .collect();

        // Send single batch fetch command (more efficient than N individual requests)
        if !uids_to_fetch.is_empty() {
            if self
                .accounts
                .active()
                .imap_handle
                .cmd_tx
                .try_send(ImapCommand::FetchBodies {
                    uids: uids_to_fetch.clone(),
                })
                .is_ok()
            {
                // Track all UIDs as in-flight
                for uid in uids_to_fetch {
                    self.in_flight_fetches.insert(uid);
                }
            }
        }

        // Update last_prefetch_uid to current selection
        if let Some(uid) = current_uid {
            self.last_prefetch_uid = Some(uid);
        }
    }
}
