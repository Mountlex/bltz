//! Parallel IMAP sync for initial full sync operations.
//!
//! This module provides parallel fetching of email headers using multiple
//! IMAP connections to achieve ~3-4x speedup during initial mailbox sync.

use anyhow::Result;
use tokio::task::JoinSet;

use crate::mail::types::EmailHeader;

use super::ImapClient;

/// Default number of parallel connections for sync.
/// Most IMAP servers allow 5-15 concurrent connections per user.
pub const DEFAULT_SYNC_CONCURRENCY: usize = 4;

/// Minimum chunk size to avoid overhead of too many small requests.
const MIN_CHUNK_SIZE: usize = 100;

/// Minimum mailbox size to use parallel sync.
/// For smaller mailboxes, single connection is faster due to connection overhead.
const MIN_PARALLEL_THRESHOLD: usize = 200;

/// Result of a chunk fetch operation
struct ChunkResult {
    chunk_index: usize,
    headers: Result<Vec<EmailHeader>>,
}

/// Fetch all headers in parallel using multiple connections.
///
/// # Arguments
/// * `client` - The primary client (used to get UIDs and as template for workers)
/// * `folder` - The folder to sync
/// * `concurrency` - Number of parallel connections (default: 4)
///
/// # Returns
/// Vector of all email headers, sorted by date descending
pub async fn parallel_fetch_all_headers(
    client: &mut ImapClient,
    folder: &str,
    concurrency: usize,
) -> Result<Vec<EmailHeader>> {
    // Step 1: Get all UIDs (lightweight operation ~5ms for 10k emails)
    let all_uids = client.fetch_all_uids().await?;

    if all_uids.is_empty() {
        return Ok(Vec::new());
    }

    let total = all_uids.len();
    tracing::info!(
        "Parallel sync: {} UIDs to fetch with {} connections",
        total,
        concurrency
    );

    // For small mailboxes, use single connection (connection overhead > parallelism benefit)
    if total < MIN_PARALLEL_THRESHOLD {
        tracing::debug!("Small mailbox ({} emails), using single connection", total);
        return client.fetch_all_headers().await;
    }

    // Step 2: Chunk UIDs for parallel fetching
    let chunk_size = (total / concurrency).max(MIN_CHUNK_SIZE);
    let chunks: Vec<Vec<u32>> = all_uids.chunks(chunk_size).map(|c| c.to_vec()).collect();

    let actual_chunks = chunks.len();
    tracing::info!(
        "Split into {} chunks of ~{} UIDs each",
        actual_chunks,
        chunk_size
    );

    // Step 3: Spawn worker tasks
    let mut join_set = JoinSet::new();

    for (index, chunk) in chunks.into_iter().enumerate() {
        // Clone client config to create new connection for this worker
        let mut worker_client = client.clone_config();
        let folder = folder.to_string();

        join_set.spawn(async move {
            let result = fetch_chunk(&mut worker_client, &folder, &chunk).await;
            ChunkResult {
                chunk_index: index,
                headers: result,
            }
        });
    }

    // Step 4: Collect results from all workers
    let mut all_headers = Vec::with_capacity(total);
    let mut failed_chunks = Vec::new();

    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(chunk_result) => match chunk_result.headers {
                Ok(headers) => {
                    tracing::debug!(
                        "Chunk {} completed: {} headers",
                        chunk_result.chunk_index,
                        headers.len()
                    );
                    all_headers.extend(headers);
                }
                Err(e) => {
                    tracing::warn!("Chunk {} failed: {}", chunk_result.chunk_index, e);
                    failed_chunks.push(chunk_result.chunk_index);
                }
            },
            Err(e) => {
                tracing::error!("Worker task panicked: {}", e);
            }
        }
    }

    if !failed_chunks.is_empty() {
        tracing::warn!(
            "{} chunks failed, some emails may be missing",
            failed_chunks.len()
        );
    }

    // Step 5: Sort all headers by date descending
    all_headers.sort_by(|a, b| b.date.cmp(&a.date));

    tracing::info!(
        "Parallel sync complete: {} headers fetched",
        all_headers.len()
    );
    Ok(all_headers)
}

/// Fetch a single chunk of headers using a dedicated connection.
async fn fetch_chunk(
    client: &mut ImapClient,
    folder: &str,
    uids: &[u32],
) -> Result<Vec<EmailHeader>> {
    // Connect to IMAP server
    client.connect().await?;

    // Select the folder
    client.select_folder(folder).await?;

    // Fetch headers for this chunk
    let headers = client.fetch_headers_by_uids(uids).await?;

    // Clean disconnect
    client.disconnect().await.ok();

    Ok(headers)
}
