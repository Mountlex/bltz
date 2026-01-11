//! IMAP actor: manages connection lifecycle, IDLE, command dispatch, and sync.

use anyhow::{Context, Result};
use async_imap::types::Flag;
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::cache::{Cache, SyncState};
use crate::constants::{
    FLAG_SYNC_BATCH_SIZE, IDLE_TIMEOUT_SECS, MAX_RETRIES, MAX_RETRY_DELAY_SECS,
};
use crate::mail::parser::{extract_attachment_data, parse_attachments};
use crate::mail::types::EmailFlags;

use super::{ImapActorHandle, ImapClient, ImapCommand, ImapEvent, SyncResult, folder_cache_key};

/// Spawn the IMAP actor and return a handle to control it.
/// The actor maintains a single connection and uses IDLE for push notifications.
pub fn spawn_imap_actor(
    client: ImapClient,
    cache: Arc<Cache>,
    account_id: String,
) -> ImapActorHandle {
    // Increased channel capacity to handle high-activity periods (large syncs, batch operations)
    let (cmd_tx, cmd_rx) = mpsc::channel(128);
    let (event_tx, event_rx) = mpsc::channel(256);

    tokio::spawn(imap_actor(client, cache, account_id, cmd_rx, event_tx));

    ImapActorHandle { cmd_tx, event_rx }
}

/// The main IMAP actor loop.
/// Uses `tokio::select!` to handle both IDLE notifications and commands.
async fn imap_actor(
    mut client: ImapClient,
    cache: Arc<Cache>,
    account_id: String,
    mut cmd_rx: mpsc::Receiver<ImapCommand>,
    event_tx: mpsc::Sender<ImapEvent>,
) {
    // Track the current folder (default to INBOX)
    let mut current_folder = "INBOX".to_string();

    // Connect with retry logic
    let mut retry_delay = 1u64;

    for attempt in 1..=MAX_RETRIES {
        // Check for shutdown command while connecting
        match cmd_rx.try_recv() {
            Ok(ImapCommand::Shutdown) | Err(mpsc::error::TryRecvError::Disconnected) => {
                tracing::info!("Shutdown requested during connection");
                return;
            }
            _ => {}
        }

        match client.connect().await {
            Ok(_) => {
                if let Err(e) = event_tx.send(ImapEvent::Connected).await {
                    tracing::debug!("Failed to send Connected event: {}", e);
                }
                break;
            }
            Err(e) => {
                let msg = format!(
                    "Connection attempt {}/{} failed: {}",
                    attempt, MAX_RETRIES, e
                );
                tracing::warn!("{}", msg);
                if let Err(e) = event_tx.send(ImapEvent::Error(msg)).await {
                    tracing::debug!("Failed to send Error event: {}", e);
                }

                if attempt == MAX_RETRIES {
                    if let Err(e) = event_tx
                        .send(ImapEvent::Error(
                            "Max retries exceeded, giving up".to_string(),
                        ))
                        .await
                    {
                        tracing::debug!("Failed to send max retries error: {}", e);
                    }
                    return;
                }

                tokio::time::sleep(std::time::Duration::from_secs(retry_delay)).await;
                retry_delay = (retry_delay * 2).min(MAX_RETRY_DELAY_SECS);
            }
        }
    }

    // Initial sync
    do_sync_folder(&mut client, &cache, &account_id, &current_folder, &event_tx).await;

    // Track consecutive errors for backoff
    let mut consecutive_errors = 0u32;

    loop {
        // Select current folder for IDLE
        if let Err(e) = client.select_folder(&current_folder).await {
            tracing::warn!("Failed to select folder '{}': {}", current_folder, e);
            consecutive_errors += 1;

            if consecutive_errors > 5 {
                if let Err(e) = event_tx
                    .send(ImapEvent::Error("Too many consecutive errors".to_string()))
                    .await
                {
                    tracing::debug!("Failed to send consecutive errors event: {}", e);
                }
                let delay = (2u64.pow(consecutive_errors.min(5))).min(60);
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            }

            // Try to reconnect
            if let Err(e) = reconnect(&mut client).await {
                if let Err(send_err) = event_tx
                    .send(ImapEvent::Error(format!("Reconnect failed: {}", e)))
                    .await
                {
                    tracing::debug!("Failed to send reconnect error event: {}", send_err);
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            continue;
        }

        // Reset error count on success
        consecutive_errors = 0;

        // Take session for IDLE
        let session = match client.take_session() {
            Some(s) => s,
            None => {
                tracing::warn!("No session available for IDLE");
                if let Err(e) = reconnect(&mut client).await
                    && let Err(send_err) = event_tx
                        .send(ImapEvent::Error(format!("Reconnect failed: {}", e)))
                        .await
                {
                    tracing::debug!("Failed to send reconnect error event: {}", send_err);
                }
                continue;
            }
        };

        // Start IDLE
        let mut idle = session.idle();
        if let Err(e) = idle.init().await {
            tracing::warn!("Failed to init IDLE: {:?}", e);
            if let Ok(session) = idle.done().await {
                client.restore_session(session);
            }
            // Reconnect since IDLE failed
            if let Err(e) = reconnect(&mut client).await
                && let Err(send_err) = event_tx
                    .send(ImapEvent::Error(format!("Reconnect failed: {}", e)))
                    .await
            {
                tracing::debug!("Failed to send reconnect error event: {}", send_err);
            }
            continue;
        }

        tracing::debug!("IDLE started, waiting for notifications...");

        // Get the wait future and stop source
        let (idle_future, stop_source) = idle.wait();

        // Wrap idle_future in a timeout
        let idle_with_timeout = tokio::time::timeout(
            std::time::Duration::from_secs(IDLE_TIMEOUT_SECS),
            idle_future,
        );

        tokio::select! {
            // IDLE completed (notification or timeout)
            result = idle_with_timeout => {
                // Get session back first
                match idle.done().await {
                    Ok(session) => client.restore_session(session),
                    Err(e) => {
                        tracing::error!("Failed to end IDLE: {:?}", e);
                        if let Err(e) = reconnect(&mut client).await {
                            event_tx.send(ImapEvent::Error(format!("Reconnect failed: {}", e))).await.ok();
                        }
                        continue;
                    }
                }

                match result {
                    // Server sent notification - sync new mail
                    Ok(Ok(_)) => {
                        tracing::info!("IDLE: Server notification received");
                        if let Err(e) = event_tx.send(ImapEvent::NewMail { count: 1 }).await {
                            tracing::debug!("Failed to send NewMail event: {}", e);
                        }
                        do_sync_folder(&mut client, &cache, &account_id, &current_folder, &event_tx).await;
                    }
                    // IDLE error
                    Ok(Err(e)) => {
                        tracing::warn!("IDLE error: {:?}", e);
                        // Try to reconnect
                        if let Err(e) = reconnect(&mut client).await
                            && let Err(send_err) = event_tx.send(ImapEvent::Error(format!("Reconnect failed: {}", e))).await
                        {
                            tracing::debug!("Failed to send reconnect error event: {}", send_err);
                        }
                    }
                    // Timeout - just refresh IDLE (servers may drop long IDLEs)
                    Err(_) => {
                        tracing::debug!("IDLE timeout, refreshing...");
                    }
                }
            }

            // Command received - interrupt IDLE and handle it
            cmd = cmd_rx.recv() => {
                // Drop stop_source to interrupt IDLE immediately
                drop(stop_source);

                // Get session back
                match idle.done().await {
                    Ok(session) => client.restore_session(session),
                    Err(e) => {
                        tracing::error!("Failed to end IDLE after command: {:?}", e);
                        if let Err(e) = reconnect(&mut client).await
                            && let Err(send_err) = event_tx.send(ImapEvent::Error(format!("Reconnect failed: {}", e))).await
                        {
                            tracing::debug!("Failed to send reconnect error event: {}", send_err);
                        }
                        continue;
                    }
                }

                match cmd {
                    Some(ImapCommand::Shutdown) => {
                        tracing::info!("IMAP actor shutting down");
                        client.disconnect().await.ok();
                        break;
                    }
                    Some(cmd) => {
                        handle_command(&mut client, &cache, &account_id, &mut current_folder, cmd, &event_tx).await;
                    }
                    None => {
                        // Channel closed, shutdown
                        tracing::info!("Command channel closed, shutting down");
                        client.disconnect().await.ok();
                        break;
                    }
                }
            }
        }
    }
}

/// Handle a command from the UI
async fn handle_command(
    client: &mut ImapClient,
    cache: &Cache,
    account_id: &str,
    current_folder: &mut String,
    cmd: ImapCommand,
    event_tx: &mpsc::Sender<ImapEvent>,
) {
    match cmd {
        ImapCommand::Sync => {
            do_sync_folder(client, cache, account_id, current_folder, event_tx).await;
        }
        ImapCommand::FetchBody { uid, folder } => {
            // Use the provided folder for cache key (may differ from current_folder in conversation mode)
            let body_cache_key = folder_cache_key(account_id, &folder);

            // Check cache first
            if let Ok(Some(body)) = cache.get_email_body(&body_cache_key, uid).await {
                event_tx
                    .send(ImapEvent::BodyFetched { uid, body })
                    .await
                    .ok();
                return;
            }

            // Save original folder for restoration after operation
            let original_folder = current_folder.clone();
            let needs_folder_switch = folder != *current_folder;

            // Select the folder if it differs from current (needed for conversation mode)
            if needs_folder_switch && let Err(e) = client.select_folder(&folder).await {
                tracing::warn!("Failed to select folder '{}' for body fetch: {}", folder, e);
                event_tx
                    .send(ImapEvent::BodyFetchFailed {
                        uid,
                        error: format!("Failed to select folder: {}", e),
                    })
                    .await
                    .ok();
                return;
            }

            // Fetch from server
            match client.fetch_body(uid).await {
                Ok(body) => {
                    // Cache the body with folder-specific key
                    if let Err(e) = cache.insert_email_body(&body_cache_key, uid, &body).await {
                        tracing::warn!("Failed to cache email body for UID {}: {}", uid, e);
                    }
                    if let Err(e) = event_tx.send(ImapEvent::BodyFetched { uid, body }).await {
                        tracing::debug!("Failed to send BodyFetched event: {}", e);
                    }
                }
                Err(e) => {
                    event_tx
                        .send(ImapEvent::BodyFetchFailed {
                            uid,
                            error: e.to_string(),
                        })
                        .await
                        .ok();
                }
            }

            // Switch back to original folder for IDLE (with recovery on failure)
            restore_folder_after_operation(
                client,
                current_folder,
                &original_folder,
                needs_folder_switch,
                event_tx,
            )
            .await;
        }
        ImapCommand::FetchBodies { uids, folder } => {
            // Use the provided folder for cache key
            let body_cache_key = folder_cache_key(account_id, &folder);

            // Filter out UIDs already in cache
            let cached_uids = cache
                .get_cached_body_uids(&body_cache_key, &uids)
                .await
                .unwrap_or_default();
            let uids_to_fetch: Vec<u32> = uids
                .into_iter()
                .filter(|uid| !cached_uids.contains(uid))
                .collect();

            // Send events for cached bodies immediately
            for uid in &cached_uids {
                if let Ok(Some(body)) = cache.get_email_body(&body_cache_key, *uid).await {
                    event_tx
                        .send(ImapEvent::BodyFetched { uid: *uid, body })
                        .await
                        .ok();
                }
            }

            // Save original folder for restoration after operation
            let original_folder = current_folder.clone();
            let needs_folder_switch = folder != *current_folder;

            // Select the folder if it differs from current
            if needs_folder_switch
                && !uids_to_fetch.is_empty()
                && let Err(e) = client.select_folder(&folder).await
            {
                tracing::warn!(
                    "Failed to select folder '{}' for batch body fetch: {}",
                    folder,
                    e
                );
                for uid in uids_to_fetch {
                    event_tx
                        .send(ImapEvent::BodyFetchFailed {
                            uid,
                            error: format!("Failed to select folder: {}", e),
                        })
                        .await
                        .ok();
                }
                return;
            }

            // Fetch remaining bodies from server in a single batch request
            if !uids_to_fetch.is_empty() {
                match client.fetch_bodies(&uids_to_fetch).await {
                    Ok(fetched) => {
                        for (uid, body) in fetched {
                            // Cache each body
                            if let Err(e) =
                                cache.insert_email_body(&body_cache_key, uid, &body).await
                            {
                                tracing::warn!("Failed to cache email body for UID {}: {}", uid, e);
                            }
                            if let Err(e) =
                                event_tx.send(ImapEvent::BodyFetched { uid, body }).await
                            {
                                tracing::debug!("Failed to send BodyFetched event: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        // Send failure for all requested UIDs
                        for uid in uids_to_fetch {
                            event_tx
                                .send(ImapEvent::BodyFetchFailed {
                                    uid,
                                    error: e.to_string(),
                                })
                                .await
                                .ok();
                        }
                    }
                }
            }

            // Switch back to original folder for IDLE (with recovery on failure)
            restore_folder_after_operation(
                client,
                current_folder,
                &original_folder,
                needs_folder_switch,
                event_tx,
            )
            .await;
        }
        ImapCommand::SetFlag { uid, flag, folder } => {
            // Use folder-specific cache key (important for conversation mode)
            let flag_cache_key = folder_cache_key(account_id, &folder);

            // Save original folder for restoration after operation
            let original_folder = current_folder.clone();
            let needs_folder_switch = folder != *current_folder;

            // Switch to correct folder if needed
            if needs_folder_switch && let Err(e) = client.select_folder(&folder).await {
                tracing::error!("Failed to select folder '{}' for flag: {}", folder, e);
                event_tx
                    .send(ImapEvent::Error(format!(
                        "Failed to select folder '{}': {}",
                        folder, e
                    )))
                    .await
                    .ok();
                // Don't attempt flag operation on wrong folder
                return;
            }

            match client.add_flag(uid, flag).await {
                Ok(_) => {
                    // Use atomic cache operation to add flag (avoids read-modify-write race)
                    let new_flags = match cache.add_flag(&flag_cache_key, uid, flag).await {
                        Ok(flags) => flags,
                        Err(e) => {
                            tracing::warn!(
                                "Cache add_flag failed for UID {}: {}, fetching from server",
                                uid,
                                e
                            );
                            // Fallback: fetch current flags from server
                            client.fetch_flags(uid).await.unwrap_or(flag)
                        }
                    };
                    if let Err(e) = event_tx
                        .send(ImapEvent::FlagUpdated {
                            uid,
                            flags: new_flags,
                        })
                        .await
                    {
                        tracing::error!("Failed to send FlagUpdated event: {}", e);
                    }
                }
                Err(e) => {
                    if let Err(send_err) = event_tx
                        .send(ImapEvent::Error(format!("Failed to set flag: {}", e)))
                        .await
                    {
                        tracing::error!("Failed to send error event: {}", send_err);
                    }
                }
            }

            // Switch back to original folder for IDLE (with recovery on failure)
            restore_folder_after_operation(
                client,
                current_folder,
                &original_folder,
                needs_folder_switch,
                event_tx,
            )
            .await;
        }
        ImapCommand::RemoveFlag { uid, flag, folder } => {
            // Use folder-specific cache key (important for conversation mode)
            let flag_cache_key = folder_cache_key(account_id, &folder);

            // Save original folder for restoration after operation
            let original_folder = current_folder.clone();
            let needs_folder_switch = folder != *current_folder;

            // Switch to correct folder if needed
            if needs_folder_switch && let Err(e) = client.select_folder(&folder).await {
                tracing::error!("Failed to select folder '{}' for flag: {}", folder, e);
                event_tx
                    .send(ImapEvent::Error(format!(
                        "Failed to select folder '{}': {}",
                        folder, e
                    )))
                    .await
                    .ok();
                // Don't attempt flag operation on wrong folder
                return;
            }

            match client.remove_flag(uid, flag).await {
                Ok(_) => {
                    // Use atomic cache operation to remove flag (avoids read-modify-write race)
                    let new_flags = match cache.remove_flag(&flag_cache_key, uid, flag).await {
                        Ok(flags) => flags,
                        Err(e) => {
                            tracing::warn!(
                                "Cache remove_flag failed for UID {}: {}, fetching from server",
                                uid,
                                e
                            );
                            // Fallback: fetch current flags from server
                            client.fetch_flags(uid).await.unwrap_or(EmailFlags::empty())
                        }
                    };
                    if let Err(e) = event_tx
                        .send(ImapEvent::FlagUpdated {
                            uid,
                            flags: new_flags,
                        })
                        .await
                    {
                        tracing::error!("Failed to send FlagUpdated event: {}", e);
                    }
                }
                Err(e) => {
                    if let Err(send_err) = event_tx
                        .send(ImapEvent::Error(format!("Failed to remove flag: {}", e)))
                        .await
                    {
                        tracing::error!("Failed to send error event: {}", send_err);
                    }
                }
            }

            // Switch back to original folder for IDLE (with recovery on failure)
            restore_folder_after_operation(
                client,
                current_folder,
                &original_folder,
                needs_folder_switch,
                event_tx,
            )
            .await;
        }
        ImapCommand::Delete { uid, folder } => {
            // Use folder-specific cache key (important for conversation mode)
            let delete_cache_key = folder_cache_key(account_id, &folder);

            // Save original folder for restoration after operation
            let original_folder = current_folder.clone();
            let needs_folder_switch = folder != *current_folder;

            // Switch to correct folder if needed
            if needs_folder_switch && let Err(e) = client.select_folder(&folder).await {
                tracing::error!("Failed to select folder '{}' for delete: {}", folder, e);
                event_tx
                    .send(ImapEvent::Error(format!(
                        "Failed to select folder '{}': {}",
                        folder, e
                    )))
                    .await
                    .ok();
                // Don't attempt delete on wrong folder
                return;
            }

            match client.delete(uid).await {
                Ok(_) => {
                    if let Err(e) = cache.delete_email(&delete_cache_key, uid).await {
                        tracing::warn!("Failed to delete email from cache: {}", e);
                    }
                    if let Err(e) = event_tx.send(ImapEvent::Deleted { uid }).await {
                        tracing::error!("Failed to send Deleted event: {}", e);
                    }
                }
                Err(e) => {
                    if let Err(send_err) = event_tx
                        .send(ImapEvent::Error(format!("Failed to delete: {}", e)))
                        .await
                    {
                        tracing::error!("Failed to send error event: {}", send_err);
                    }
                }
            }

            // Switch back to original folder for IDLE (with recovery on failure)
            restore_folder_after_operation(
                client,
                current_folder,
                &original_folder,
                needs_folder_switch,
                event_tx,
            )
            .await;
        }
        ImapCommand::SelectFolder { folder } => {
            match client.select_folder(&folder).await {
                Ok(_) => {
                    // Update the current folder tracker
                    *current_folder = folder.clone();
                    event_tx
                        .send(ImapEvent::FolderSelected { folder })
                        .await
                        .ok();
                }
                Err(e) => {
                    event_tx
                        .send(ImapEvent::Error(format!("Failed to select folder: {}", e)))
                        .await
                        .ok();
                }
            }
        }
        ImapCommand::ListFolders => match client.list_folders().await {
            Ok(folders) => {
                event_tx.send(ImapEvent::FolderList { folders }).await.ok();
            }
            Err(e) => {
                event_tx
                    .send(ImapEvent::Error(format!("Failed to list folders: {}", e)))
                    .await
                    .ok();
            }
        },
        ImapCommand::PrefetchFolder { folder } => {
            // Background prefetch: sync a folder without changing the active folder
            let original_folder = current_folder.clone();
            let mut folder_changed = false;

            // Select and sync the prefetch folder (silently)
            if client.select_folder(&folder).await.is_ok() {
                folder_changed = true;
                if let Err(e) = client.sync_current_folder(cache, account_id, &folder).await {
                    tracing::warn!("Prefetch sync failed for '{}': {}", folder, e);
                } else {
                    tracing::debug!("Prefetch complete for '{}'", folder);
                }
                event_tx
                    .send(ImapEvent::PrefetchComplete { folder })
                    .await
                    .ok();
            }

            // Re-select original folder for IDLE (with recovery on failure)
            restore_folder_after_operation(
                client,
                current_folder,
                &original_folder,
                folder_changed,
                event_tx,
            )
            .await;
        }
        ImapCommand::FetchAttachment {
            uid,
            folder,
            attachment_index,
        } => {
            let body_cache_key = folder_cache_key(account_id, &folder);

            // First try to get raw message from cache
            if let Ok(Some(raw)) = cache.get_raw_message(&body_cache_key, uid).await {
                let attachments = parse_attachments(&raw);
                if let Some(attachment) = attachments.get(attachment_index).cloned()
                    && let Some(data) = extract_attachment_data(&raw, attachment_index)
                {
                    event_tx
                        .send(ImapEvent::AttachmentFetched {
                            uid,
                            attachment_index,
                            attachment,
                            data,
                        })
                        .await
                        .ok();
                    return;
                }
            }

            // Save original folder for restoration after operation
            let original_folder = current_folder.clone();
            let needs_folder_switch = folder != *current_folder;

            // Need to fetch from server
            if needs_folder_switch && let Err(e) = client.select_folder(&folder).await {
                event_tx
                    .send(ImapEvent::AttachmentFetchFailed {
                        uid,
                        attachment_index,
                        error: format!("Failed to select folder: {}", e),
                    })
                    .await
                    .ok();
                return;
            }

            // Fetch raw message from server
            match client.fetch_raw(uid).await {
                Ok(raw) => {
                    let attachments = parse_attachments(&raw);
                    if let Some(attachment) = attachments.get(attachment_index).cloned() {
                        if let Some(data) = extract_attachment_data(&raw, attachment_index) {
                            // Cache the raw message and attachment metadata for future use
                            let body = crate::mail::parser::parse_body(&raw);
                            if let Err(e) = cache
                                .insert_email_body_with_raw(&body_cache_key, uid, &body, &raw)
                                .await
                            {
                                tracing::warn!(
                                    "Failed to cache raw message for UID {}: {}",
                                    uid,
                                    e
                                );
                            }
                            if let Err(e) = cache
                                .insert_attachments(&body_cache_key, uid, &attachments)
                                .await
                            {
                                tracing::warn!(
                                    "Failed to cache attachments for UID {}: {}",
                                    uid,
                                    e
                                );
                            }

                            event_tx
                                .send(ImapEvent::AttachmentFetched {
                                    uid,
                                    attachment_index,
                                    attachment,
                                    data,
                                })
                                .await
                                .ok();
                        } else {
                            event_tx
                                .send(ImapEvent::AttachmentFetchFailed {
                                    uid,
                                    attachment_index,
                                    error: "Failed to extract attachment data".to_string(),
                                })
                                .await
                                .ok();
                        }
                    } else {
                        event_tx
                            .send(ImapEvent::AttachmentFetchFailed {
                                uid,
                                attachment_index,
                                error: format!(
                                    "Attachment index {} not found (only {} attachments)",
                                    attachment_index,
                                    attachments.len()
                                ),
                            })
                            .await
                            .ok();
                    }
                }
                Err(e) => {
                    event_tx
                        .send(ImapEvent::AttachmentFetchFailed {
                            uid,
                            attachment_index,
                            error: e.to_string(),
                        })
                        .await
                        .ok();
                }
            }

            // Switch back to original folder (with recovery on failure)
            restore_folder_after_operation(
                client,
                current_folder,
                &original_folder,
                needs_folder_switch,
                event_tx,
            )
            .await;
        }
        ImapCommand::Shutdown => {
            // Handled in the main loop
        }
    }
}

/// Perform sync for a specific folder and send events
async fn do_sync_folder(
    client: &mut ImapClient,
    cache: &Cache,
    account_id: &str,
    folder: &str,
    event_tx: &mpsc::Sender<ImapEvent>,
) {
    tracing::info!("Starting sync for folder '{}'...", folder);
    event_tx.send(ImapEvent::SyncStarted).await.ok();

    let cache_key = folder_cache_key(account_id, folder);

    match client.sync_current_folder(cache, account_id, folder).await {
        Ok(result) => {
            let total = cache.get_email_count(&cache_key).await.unwrap_or(0);
            tracing::info!(
                "Sync complete for '{}': {} new emails, {} total, full_sync={}",
                folder,
                result.new_emails.len(),
                total,
                result.full_sync
            );
            event_tx
                .send(ImapEvent::SyncComplete {
                    new_count: result.new_emails.len(),
                    total,
                    full_sync: result.full_sync,
                })
                .await
                .ok();
        }
        Err(e) => {
            tracing::error!("Sync failed for '{}': {}", folder, e);
            event_tx
                .send(ImapEvent::Error(format!("Sync failed: {}", e)))
                .await
                .ok();
        }
    }
}

/// Reconnect to the IMAP server
async fn reconnect(client: &mut ImapClient) -> Result<()> {
    tracing::info!("Attempting to reconnect...");
    client.disconnect().await.ok();
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    client.connect().await
}

/// Attempt to restore the original folder after a temporary folder switch.
/// If restoration fails, forces a reconnect to recover from desync state.
/// Returns true if folder was successfully restored (or didn't need switching).
async fn restore_folder_after_operation(
    client: &mut ImapClient,
    current_folder: &mut String,
    original_folder: &str,
    needs_folder_switch: bool,
    event_tx: &mpsc::Sender<ImapEvent>,
) -> bool {
    if !needs_folder_switch {
        return true;
    }

    match client.select_folder(original_folder).await {
        Ok(_) => {
            // Successfully restored - ensure tracker is correct
            *current_folder = original_folder.to_string();
            true
        }
        Err(e) => {
            tracing::error!(
                "Failed to restore folder '{}' after operation, forcing reconnect: {}",
                original_folder,
                e
            );
            // Critical: IDLE would monitor wrong folder. Force reconnect to recover.
            if let Err(reconnect_err) = reconnect(client).await {
                event_tx
                    .send(ImapEvent::Error(format!(
                        "Folder desync recovery failed: {}",
                        reconnect_err
                    )))
                    .await
                    .ok();
                return false;
            }

            // After reconnect, try to select the original folder again
            match client.select_folder(original_folder).await {
                Ok(_) => {
                    *current_folder = original_folder.to_string();
                    tracing::info!("Folder '{}' restored after reconnect", original_folder);
                    true
                }
                Err(e2) => {
                    // Even after reconnect we can't select - fall back to INBOX
                    tracing::error!(
                        "Cannot restore folder '{}' even after reconnect: {}, falling back to INBOX",
                        original_folder,
                        e2
                    );
                    if client.select_folder("INBOX").await.is_ok() {
                        *current_folder = "INBOX".to_string();
                        event_tx
                            .send(ImapEvent::Error(format!(
                                "Folder '{}' unavailable, switched to INBOX",
                                original_folder
                            )))
                            .await
                            .ok();
                    }
                    false
                }
            }
        }
    }
}

//
// Sync operations on ImapClient
//

impl ImapClient {
    #[allow(dead_code)]
    pub async fn sync_inbox(&mut self, cache: &Cache, account_id: &str) -> Result<SyncResult> {
        self.sync_folder_internal(cache, account_id, "INBOX").await
    }

    /// Sync the specified folder (or currently selected folder).
    /// This is exposed for use by folder monitors.
    pub async fn sync_current_folder(
        &mut self,
        cache: &Cache,
        account_id: &str,
        folder: &str,
    ) -> Result<SyncResult> {
        self.sync_folder_internal(cache, account_id, folder).await
    }

    async fn sync_folder_internal(
        &mut self,
        cache: &Cache,
        account_id: &str,
        folder: &str,
    ) -> Result<SyncResult> {
        let mailbox = self.select_folder(folder).await?;

        // UID validity is required per RFC 3501 - if missing, something is wrong
        let server_uid_validity = mailbox.uid_validity.ok_or_else(|| {
            anyhow::anyhow!(
                "Server did not provide UID validity for folder '{}'",
                folder
            )
        })?;
        let server_uid_next = mailbox.uid_next.unwrap_or(1);
        let message_count = mailbox.exists;

        // Use folder-specific cache key
        let cache_key = folder_cache_key(account_id, folder);

        tracing::debug!(
            "Mailbox '{}' state: uid_validity={}, uid_next={}, exists={}",
            folder,
            server_uid_validity,
            server_uid_next,
            message_count
        );

        let local_state = cache.get_sync_state(&cache_key).await?;
        tracing::debug!(
            "Local state for '{}': uid_validity={:?}, uid_next={:?}",
            folder,
            local_state.uid_validity,
            local_state.uid_next
        );

        let needs_full_sync = local_state.needs_full_sync(server_uid_validity);

        let new_emails = if needs_full_sync {
            tracing::info!(
                "Performing full sync for folder '{}' (UID validity changed or first sync)",
                folder
            );
            // Fetch all headers from server
            let emails = self.fetch_all_headers().await?;
            tracing::info!("Fetched {} emails from folder '{}'", emails.len(), folder);

            // Insert/update emails first (uses INSERT OR REPLACE, safe if crash occurs here)
            if !emails.is_empty() {
                cache.insert_emails(&cache_key, &emails).await?;
            }

            // Then delete emails that are no longer on the server
            // This is safer than clear_emails() as it preserves data if crash occurs
            let server_uids: Vec<u32> = emails.iter().map(|e| e.uid).collect();
            let deleted = cache.delete_emails_not_in(&cache_key, &server_uids).await?;
            if deleted > 0 {
                tracing::info!(
                    "Removed {} stale emails from cache for folder '{}'",
                    deleted,
                    folder
                );
            }

            emails
        } else {
            // Sync flags for existing emails first
            self.sync_flags(cache, &cache_key).await?;

            // Detect server-side deletions by comparing server UIDs with cached UIDs
            let server_uids = self.fetch_all_uids().await?;
            let deleted = cache.delete_emails_not_in(&cache_key, &server_uids).await?;
            if deleted > 0 {
                tracing::info!(
                    "Removed {} server-deleted emails from cache for folder '{}'",
                    deleted,
                    folder
                );
            }

            // Then fetch any new emails
            if let Some(start_uid) = local_state.new_messages_start(server_uid_next) {
                tracing::info!(
                    "Fetching new messages from UID {} in folder '{}'",
                    start_uid,
                    folder
                );
                self.fetch_headers_from(start_uid).await?
            } else {
                tracing::debug!(
                    "No new messages in '{}' (server_uid_next={}, local_uid_next={:?})",
                    folder,
                    server_uid_next,
                    local_state.uid_next
                );
                Vec::new()
            }
        };

        // Store new emails in cache with folder-specific key
        // Skip for full sync since emails were already inserted above
        if !needs_full_sync && !new_emails.is_empty() {
            tracing::debug!(
                "Inserting {} new emails into cache for folder '{}'",
                new_emails.len(),
                folder
            );
            cache.insert_emails(&cache_key, &new_emails).await?;
        }

        let sync_state = SyncState {
            uid_validity: Some(server_uid_validity),
            uid_next: Some(server_uid_next),
            last_sync: Some(chrono::Utc::now().timestamp()),
        };

        cache.set_sync_state(&cache_key, &sync_state).await?;

        Ok(SyncResult {
            new_emails,
            full_sync: needs_full_sync,
        })
    }

    /// Sync flags for all cached emails with the server
    async fn sync_flags(&mut self, cache: &Cache, account_id: &str) -> Result<()> {
        // Get all cached email UIDs and their current flags
        let cached_emails = cache.get_all_uid_flags(account_id).await?;
        if cached_emails.is_empty() {
            return Ok(());
        }

        tracing::debug!("Syncing flags for {} cached emails", cached_emails.len());

        // Fetch flags from server in batches (to avoid command line length limits)
        let mut updated_count = 0;

        for chunk in cached_emails.chunks(FLAG_SYNC_BATCH_SIZE) {
            // Build HashMap for O(1) lookup instead of O(n) linear search
            let cached_map: std::collections::HashMap<u32, EmailFlags> =
                chunk.iter().cloned().collect();

            let uids: Vec<String> = chunk.iter().map(|(uid, _)| uid.to_string()).collect();
            let uid_set = uids.join(",");

            let session = self.session()?;
            let mut messages = session
                .uid_fetch(&uid_set, "(UID FLAGS)")
                .await
                .context("Failed to fetch flags")?;

            while let Some(result) = messages.next().await {
                let fetch = result.context("Failed to fetch message flags")?;
                if let Some(uid) = fetch.uid {
                    let flag_vec: Vec<Flag> = fetch.flags().collect();
                    let server_flags = crate::mail::parser::parse_flags_from_imap(&flag_vec);

                    // O(1) lookup using HashMap (was O(n) linear search)
                    if let Some(&cached_flags) = cached_map.get(&uid)
                        && server_flags != cached_flags
                    {
                        tracing::debug!(
                            "Flags changed for UID {}: {:?} -> {:?}",
                            uid,
                            cached_flags,
                            server_flags
                        );
                        cache.update_flags(account_id, uid, server_flags).await?;
                        updated_count += 1;
                    }
                }
            }
        }

        if updated_count > 0 {
            tracing::info!("Updated flags for {} emails", updated_count);
        }

        Ok(())
    }
}
