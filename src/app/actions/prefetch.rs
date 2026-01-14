//! Email body prefetching

use std::collections::HashSet;
use std::collections::hash_map::Entry;
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
        if self.prefetch.last_uid == Some(current_uid) && self.state.reader.body.is_some() {
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
            self.prefetch.last_uid = Some(current_uid);
        } else {
            // Only clear if not in cache - avoids flash of empty content
            self.state.reader.set_body(None);

            // Spawn background task to fetch body (non-blocking)
            // This allows the UI to remain responsive while fetching
            if let Some(ref email) = current_email {
                let email_folder = email
                    .folder
                    .clone()
                    .unwrap_or_else(|| self.state.folder.current.clone());
                if let Entry::Vacant(entry) = self.prefetch.in_flight.entry(current_uid) {
                    entry.insert(Instant::now());

                    // Clone what we need for the spawned task
                    let pool = self.accounts.active().pool.clone();
                    let result_tx = self.body_fetch_tx.clone();
                    let folder = email_folder.clone();
                    let cache_key = email_cache_key.clone();
                    let uid = current_uid;

                    // Spawn background task - does NOT block UI
                    tokio::spawn(async move {
                        let result = async {
                            let mut client = pool.borrow().await?;
                            // Inner block ensures client is always returned after successful borrow
                            let fetch_result = async {
                                client.select_folder(&folder).await?;
                                client.fetch_body(uid).await
                            }
                            .await;
                            pool.return_client(client).await; // Always return after borrow
                            fetch_result
                        }
                        .await;

                        // Send result back to main thread
                        if let Err(e) = result_tx
                            .send(crate::app::BodyFetchResult {
                                uid,
                                folder,
                                cache_key,
                                result: result.map_err(|e: anyhow::Error| e.to_string()),
                            })
                            .await
                        {
                            tracing::warn!(
                                "Failed to send body fetch result for uid {}: {}",
                                uid,
                                e
                            );
                        }
                    });
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
            .filter(|uid| !self.prefetch.in_flight.contains_key(uid))
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
        let (merged_uids, timestamp) = match self.prefetch.pending.take() {
            Some((existing_uids, ts)) => {
                // Use HashSet for O(1) deduplication instead of O(n) Vec::contains
                let mut uid_set: HashSet<u32> = existing_uids.into_iter().collect();
                uid_set.extend(uids_to_fetch);
                (uid_set.into_iter().collect(), ts) // Keep original timestamp for debounce
            }
            None => (uids_to_fetch, Instant::now()),
        };

        self.prefetch.pending = Some((merged_uids, timestamp));
    }

    /// Process any pending prefetch if debounce delay has passed
    pub(crate) async fn process_pending_prefetch(&mut self) {
        let (uids, requested_at) = match self.prefetch.pending.take() {
            Some(p) => p,
            None => return,
        };

        // Check if debounce delay has passed
        if requested_at.elapsed() < Duration::from_millis(PREFETCH_DEBOUNCE_MS) {
            // Put it back - not ready yet
            self.prefetch.pending = Some((uids, requested_at));
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
                self.prefetch.last_uid = Some(email.uid);
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
                    // Track all UIDs as in-flight with current timestamp
                    let now = Instant::now();
                    for uid in uids_to_fetch {
                        self.prefetch.in_flight.insert(uid, now);
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
            self.prefetch.last_uid = Some(uid);
        }
    }

    /// Process body fetch results from background tasks (non-blocking)
    /// Returns true if any results were processed
    pub(crate) async fn process_body_fetch_results(&mut self) -> bool {
        const IN_FLIGHT_TIMEOUT_SECS: u64 = 30;

        let mut processed = false;

        // Evict stale in-flight entries (> 30 seconds old) to prevent unbounded growth
        let now = Instant::now();
        self.prefetch.in_flight.retain(|uid, added_at| {
            let is_stale =
                now.duration_since(*added_at) > Duration::from_secs(IN_FLIGHT_TIMEOUT_SECS);
            if is_stale {
                tracing::debug!("Evicting stale in-flight uid {} (timed out)", uid);
            }
            !is_stale
        });

        // Process all available results (non-blocking)
        while let Ok(result) = self.body_fetch_rx.try_recv() {
            processed = true;

            // Remove from in-flight tracking
            self.prefetch.in_flight.remove(&result.uid);

            match result.result {
                Ok(body) => {
                    // Cache the body for future use
                    if let Err(e) = self
                        .cache
                        .insert_email_body(&result.cache_key, result.uid, &body)
                        .await
                    {
                        tracing::warn!("Failed to cache body for uid {}: {}", result.uid, e);
                    }

                    // Check if this is the currently selected email with folder verification
                    // In conversation mode, same UID can exist in both INBOX and Sent folder
                    let current_email = self.state.current_email_from_thread();
                    let should_display = current_email
                        .map(|e| {
                            e.uid == result.uid
                                && (e.folder.as_deref() == Some(&result.folder)
                                    || (e.folder.is_none()
                                        && result.folder == self.state.folder.current))
                        })
                        .unwrap_or(false);

                    if should_display && self.state.reader.body.is_none() {
                        self.state.reader.set_body(Some(body));
                        self.prefetch.last_uid = Some(result.uid);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to fetch body for uid {} in {}: {}",
                        result.uid,
                        result.folder,
                        e
                    );
                }
            }
        }

        processed
    }
}
