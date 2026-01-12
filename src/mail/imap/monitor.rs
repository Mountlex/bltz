//! Folder monitor: lightweight IDLE-only actor for monitoring additional folders.
//!
//! Unlike the main IMAP actor, monitors:
//! - Only handle IDLE + sync for one fixed folder
//! - Don't process commands (except Shutdown)
//! - Designed for monitoring Sent folder in conversation mode

use std::sync::Arc;
use tokio::sync::mpsc;

use crate::cache::Cache;
use crate::constants::{IDLE_TIMEOUT_SECS, MAX_RETRIES, MAX_RETRY_DELAY_SECS};

use super::{ImapClient, ImapError, ImapEvent, folder_cache_key};

/// Event from a folder monitor, tagged with its source folder.
#[derive(Debug, Clone)]
pub struct FolderMonitorEvent {
    pub folder: String,
    pub event: ImapEvent,
}

/// Handle for a folder monitor.
pub struct FolderMonitorHandle {
    pub folder: String,
    /// Shutdown signal sender
    shutdown_tx: mpsc::Sender<()>,
    /// Events from this monitor
    pub event_rx: mpsc::Receiver<FolderMonitorEvent>,
}

impl FolderMonitorHandle {
    /// Request shutdown of the monitor.
    pub async fn shutdown(&self) {
        self.shutdown_tx.send(()).await.ok();
    }
}

/// Spawn a folder monitor for a specific folder.
///
/// The monitor will:
/// 1. Connect and sync the folder
/// 2. Enter IDLE to wait for changes
/// 3. Sync when changes are detected
/// 4. Repeat until shutdown
pub fn spawn_folder_monitor(
    client: ImapClient,
    cache: Arc<Cache>,
    account_id: String,
    folder: String,
) -> FolderMonitorHandle {
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
    let (event_tx, event_rx) = mpsc::channel(64);

    let folder_clone = folder.clone();
    tokio::spawn(folder_monitor_loop(
        client,
        cache,
        account_id,
        folder_clone,
        shutdown_rx,
        event_tx,
    ));

    FolderMonitorHandle {
        folder,
        shutdown_tx,
        event_rx,
    }
}

/// The folder monitor loop.
async fn folder_monitor_loop(
    mut client: ImapClient,
    cache: Arc<Cache>,
    account_id: String,
    folder: String,
    mut shutdown_rx: mpsc::Receiver<()>,
    event_tx: mpsc::Sender<FolderMonitorEvent>,
) {
    let send_event = |event: ImapEvent| {
        let folder = folder.clone();
        let event_tx = event_tx.clone();
        async move {
            event_tx
                .send(FolderMonitorEvent { folder, event })
                .await
                .ok();
        }
    };

    // Connect with retry logic
    let mut retry_delay = 1u64;

    for attempt in 1..=MAX_RETRIES {
        // Check for shutdown
        if shutdown_rx.try_recv().is_ok() {
            tracing::debug!("Folder monitor '{}' shutdown during connect", folder);
            return;
        }

        match client.connect().await {
            Ok(_) => {
                send_event(ImapEvent::Connected).await;
                break;
            }
            Err(e) => {
                let msg = format!(
                    "Folder monitor '{}' connection attempt {}/{} failed: {}",
                    folder, attempt, MAX_RETRIES, e
                );
                tracing::warn!("{}", msg);

                if attempt == MAX_RETRIES {
                    send_event(ImapEvent::Error(ImapError::MaxRetriesExceeded)).await;
                    return;
                }

                tokio::time::sleep(std::time::Duration::from_secs(retry_delay)).await;
                retry_delay = (retry_delay * 2).min(MAX_RETRY_DELAY_SECS);
            }
        }
    }

    // Select the folder
    if let Err(e) = client.select_folder(&folder).await {
        tracing::warn!("Failed to select folder '{}': {}", folder, e);
        send_event(ImapEvent::Error(ImapError::MailboxNotFound(folder.clone()))).await;
        return;
    }

    // Initial sync
    do_sync(&mut client, &cache, &account_id, &folder, &event_tx).await;

    // IDLE loop
    loop {
        // Check for shutdown
        if shutdown_rx.try_recv().is_ok() {
            tracing::debug!("Folder monitor '{}' shutting down", folder);
            client.disconnect().await.ok();
            return;
        }

        // Re-select folder (might have been deselected)
        if let Err(e) = client.select_folder(&folder).await {
            tracing::warn!("Folder monitor '{}' failed to select folder: {}", folder, e);
            // Try to reconnect
            if reconnect(&mut client).await.is_err() {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            continue;
        }

        // Take session for IDLE
        let session = match client.take_session() {
            Some(s) => s,
            None => {
                tracing::warn!("Folder monitor '{}': no session for IDLE", folder);
                if reconnect(&mut client).await.is_err() {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                continue;
            }
        };

        // Start IDLE
        let mut idle = session.idle();
        if let Err(e) = idle.init().await {
            tracing::warn!("Folder monitor '{}' failed to init IDLE: {:?}", folder, e);
            if let Ok(session) = idle.done().await {
                client.restore_session(session);
            }
            if reconnect(&mut client).await.is_err() {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            continue;
        }

        tracing::debug!("Folder monitor '{}' IDLE started", folder);

        let (idle_future, _stop_source) = idle.wait();
        let idle_with_timeout = tokio::time::timeout(
            std::time::Duration::from_secs(IDLE_TIMEOUT_SECS),
            idle_future,
        );

        tokio::select! {
            result = idle_with_timeout => {
                // Get session back
                match idle.done().await {
                    Ok(session) => client.restore_session(session),
                    Err(e) => {
                        tracing::error!("Folder monitor '{}' failed to end IDLE: {:?}", folder, e);
                        if reconnect(&mut client).await.is_err() {
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                        continue;
                    }
                }

                match result {
                    Ok(Ok(_)) => {
                        tracing::info!("Folder monitor '{}' received notification", folder);
                        send_event(ImapEvent::NewMail { count: 1 }).await;
                        do_sync(&mut client, &cache, &account_id, &folder, &event_tx).await;
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("Folder monitor '{}' IDLE error: {:?}", folder, e);
                        if reconnect(&mut client).await.is_err() {
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                    Err(_) => {
                        // Timeout - refresh IDLE
                        tracing::debug!("Folder monitor '{}' IDLE timeout, refreshing", folder);
                    }
                }
            }

            _ = shutdown_rx.recv() => {
                tracing::debug!("Folder monitor '{}' received shutdown signal", folder);
                // Try to cleanly end IDLE
                if let Ok(session) = idle.done().await {
                    client.restore_session(session);
                }
                client.disconnect().await.ok();
                return;
            }
        }
    }
}

/// Sync the folder and send events.
async fn do_sync(
    client: &mut ImapClient,
    cache: &Cache,
    account_id: &str,
    folder: &str,
    event_tx: &mpsc::Sender<FolderMonitorEvent>,
) {
    let cache_key = folder_cache_key(account_id, folder);

    event_tx
        .send(FolderMonitorEvent {
            folder: folder.to_string(),
            event: ImapEvent::SyncStarted,
        })
        .await
        .ok();

    // Sync folder using the client's sync method
    tracing::info!(
        "Folder monitor '{}': starting sync (cache_key='{}')",
        folder,
        cache_key
    );
    match client.sync_current_folder(cache, account_id, folder).await {
        Ok(result) => {
            let total = cache.get_email_count(&cache_key).await.unwrap_or(0);
            tracing::info!(
                "Folder monitor '{}': sync complete, {} new emails, {} total in cache (full_sync={})",
                folder,
                result.new_emails.len(),
                total,
                result.full_sync
            );
            event_tx
                .send(FolderMonitorEvent {
                    folder: folder.to_string(),
                    event: ImapEvent::SyncComplete {
                        new_count: result.new_emails.len(),
                        total,
                        full_sync: result.full_sync,
                    },
                })
                .await
                .ok();
        }
        Err(e) => {
            tracing::warn!("Folder monitor '{}' sync failed: {}", folder, e);
            event_tx
                .send(FolderMonitorEvent {
                    folder: folder.to_string(),
                    event: ImapEvent::Error(ImapError::SyncFailed(e.to_string())),
                })
                .await
                .ok();
        }
    }
}

/// Attempt to reconnect the client.
async fn reconnect(client: &mut ImapClient) -> anyhow::Result<()> {
    client.disconnect().await.ok();
    client.connect().await
}
