use anyhow::{Context, Result};
use async_imap::types::{Fetch, Flag, Mailbox};
use async_native_tls::TlsStream;
use futures::StreamExt;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

use crate::cache::{Cache, SyncState};
use crate::config::{AuthMethod, ImapConfig};
use crate::constants::{
    FLAG_SYNC_BATCH_SIZE, IDLE_TIMEOUT_SECS, MAX_RETRIES, MAX_RETRY_DELAY_SECS,
};

use super::parser::{parse_envelope, parse_flags_from_imap};
use super::types::{EmailBody, EmailFlags, EmailHeader};

/// XOAUTH2 authenticator for IMAP
struct XOAuth2Authenticator {
    user: String,
    access_token: String,
}

impl async_imap::Authenticator for XOAuth2Authenticator {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> Self::Response {
        // XOAUTH2 format: "user=" + user + "\x01auth=Bearer " + token + "\x01\x01"
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.user, self.access_token
        )
    }
}

/// Commands sent TO the IMAP actor
#[derive(Debug)]
pub enum ImapCommand {
    Sync,
    FetchBody {
        uid: u32,
    },
    /// Batch fetch multiple bodies in a single IMAP request (more efficient)
    FetchBodies {
        uids: Vec<u32>,
    },
    SetFlag {
        uid: u32,
        flag: EmailFlags,
    },
    RemoveFlag {
        uid: u32,
        flag: EmailFlags,
    },
    Delete {
        uid: u32,
    },
    SelectFolder {
        folder: String,
    },
    ListFolders,
    /// Prefetch a folder in background (sync without selecting it as active)
    PrefetchFolder {
        folder: String,
    },
    Shutdown,
}

/// Build a cache key that includes the folder
/// Format: "account_id/folder" to isolate emails per-folder
pub fn folder_cache_key(account_id: &str, folder: &str) -> String {
    format!("{}/{}", account_id, folder)
}

/// Events sent FROM the IMAP actor
#[derive(Debug, Clone)]
pub enum ImapEvent {
    Connected,
    SyncStarted,
    SyncComplete {
        new_count: usize,
        total: usize,
        full_sync: bool,
    },
    NewMail {
        count: usize,
    },
    BodyFetched {
        uid: u32,
        body: EmailBody,
    },
    BodyFetchFailed {
        uid: u32,
        error: String,
    },
    FlagUpdated {
        uid: u32,
        flags: EmailFlags,
    },
    #[allow(dead_code)]
    Deleted {
        uid: u32,
    },
    FolderSelected {
        folder: String,
    },
    FolderList {
        folders: Vec<String>,
    },
    /// Background prefetch of a folder completed
    PrefetchComplete {
        folder: String,
    },
    Error(String),
}

type ImapSession = async_imap::Session<TlsStream<Compat<TcpStream>>>;

pub struct ImapClient {
    session: Option<ImapSession>,
    pub config: ImapConfig,
    pub username: String,
    password: String,
    auth_method: AuthMethod,
}

/// Handle for controlling the IMAP actor
pub struct ImapActorHandle {
    pub cmd_tx: mpsc::Sender<ImapCommand>,
    pub event_rx: mpsc::Receiver<ImapEvent>,
}

pub struct SyncResult {
    pub new_emails: Vec<EmailHeader>,
    pub full_sync: bool,
}

impl ImapClient {
    pub fn new(
        config: ImapConfig,
        username: String,
        password: String,
        auth_method: AuthMethod,
    ) -> Self {
        Self {
            session: None,
            config,
            username,
            password,
            auth_method,
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        let addr = format!("{}:{}", self.config.server, self.config.port);

        let tcp = TcpStream::connect(&addr)
            .await
            .with_context(|| format!("Failed to connect to {}", addr))?;

        // Wrap tokio stream with compat layer for futures-io compatibility
        let tcp_compat = tcp.compat();

        let tls = async_native_tls::TlsConnector::new();
        let tls_stream = tls
            .connect(&self.config.server, tcp_compat)
            .await
            .context("TLS handshake failed")?;

        let client = async_imap::Client::new(tls_stream);

        // Authenticate based on configured auth method
        let session = match &self.auth_method {
            AuthMethod::Password => client
                .login(&self.username, &self.password)
                .await
                .map_err(|e| anyhow::anyhow!("Login failed: {:?}", e.0))?,
            AuthMethod::OAuth2 { .. } => {
                // For OAuth2, the password field contains the access token
                let authenticator = XOAuth2Authenticator {
                    user: self.username.clone(),
                    access_token: self.password.clone(),
                };
                client
                    .authenticate("XOAUTH2", authenticator)
                    .await
                    .map_err(|e| anyhow::anyhow!("XOAUTH2 authentication failed: {:?}", e.0))?
            }
        };

        self.session = Some(session);
        tracing::info!("Connected to IMAP server {}", self.config.server);

        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(mut session) = self.session.take() {
            session.logout().await.ok();
        }
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    /// Take the session out of the client (for IDLE)
    pub fn take_session(&mut self) -> Option<ImapSession> {
        self.session.take()
    }

    /// Put the session back into the client
    pub fn restore_session(&mut self, session: ImapSession) {
        self.session = Some(session);
    }

    async fn ensure_connected(&mut self) -> Result<()> {
        if !self.is_connected() {
            self.connect().await?;
        }
        Ok(())
    }

    fn session(&mut self) -> Result<&mut ImapSession> {
        self.session
            .as_mut()
            .context("Not connected to IMAP server")
    }

    #[allow(dead_code)]
    pub async fn select_inbox(&mut self) -> Result<Mailbox> {
        self.select_folder("INBOX").await
    }

    pub async fn select_folder(&mut self, folder: &str) -> Result<Mailbox> {
        self.ensure_connected().await?;
        let mailbox = self
            .session()?
            .select(folder)
            .await
            .with_context(|| format!("Failed to select folder '{}'", folder))?;
        Ok(mailbox)
    }

    pub async fn list_folders(&mut self) -> Result<Vec<String>> {
        self.ensure_connected().await?;
        let session = self.session()?;

        // List all folders under the root
        let mut folders = Vec::new();
        let mut list_stream = session.list(Some(""), Some("*")).await?;

        while let Some(result) = list_stream.next().await {
            if let Ok(name) = result {
                folders.push(name.name().to_string());
            }
        }

        // Sort folders with common ones first
        folders.sort_by(|a, b| {
            let priority = |s: &str| -> u8 {
                match s.to_uppercase().as_str() {
                    "INBOX" => 0,
                    s if s.contains("SENT") => 1,
                    s if s.contains("DRAFT") => 2,
                    s if s.contains("TRASH") || s.contains("DELETED") => 3,
                    s if s.contains("SPAM") || s.contains("JUNK") => 4,
                    s if s.contains("ARCHIVE") => 5,
                    _ => 10,
                }
            };
            priority(a).cmp(&priority(b)).then_with(|| a.cmp(b))
        });

        Ok(folders)
    }

    #[allow(dead_code)]
    pub async fn sync_inbox(&mut self, cache: &Cache, account_id: &str) -> Result<SyncResult> {
        self.sync_folder_internal(cache, account_id, "INBOX").await
    }

    /// Sync the specified folder (or currently selected folder)
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

        let server_uid_validity = mailbox.uid_validity.unwrap_or(0);
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
            // Fetch first, then clear - so we don't lose data on fetch failure
            let emails = self.fetch_all_headers().await?;
            tracing::info!("Fetched {} emails from folder '{}'", emails.len(), folder);
            cache.clear_emails(&cache_key).await?;
            emails
        } else {
            // Sync flags for existing emails first
            self.sync_flags(cache, &cache_key).await?;

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
        if !new_emails.is_empty() {
            tracing::debug!(
                "Inserting {} emails into cache for folder '{}'",
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
                    let server_flags = super::parser::parse_flags_from_imap(&flag_vec);

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

    async fn fetch_all_headers(&mut self) -> Result<Vec<EmailHeader>> {
        self.fetch_headers("1:*").await
    }

    async fn fetch_headers_from(&mut self, start_uid: u32) -> Result<Vec<EmailHeader>> {
        self.fetch_headers(&format!("{}:*", start_uid)).await
    }

    async fn fetch_headers(&mut self, sequence: &str) -> Result<Vec<EmailHeader>> {
        let session = self.session()?;

        let mut messages = session
            .uid_fetch(
                sequence,
                "(UID FLAGS BODY.PEEK[HEADER] BODY.PEEK[TEXT]<0.200>)",
            )
            .await
            .context("Failed to fetch messages")?;

        let mut headers = Vec::new();

        while let Some(result) = messages.next().await {
            let fetch = result.context("Failed to fetch message")?;
            if let Some(header) = parse_fetch(&fetch) {
                headers.push(header);
            }
        }

        // Sort by date descending
        headers.sort_by(|a, b| b.date.cmp(&a.date));

        tracing::info!("Fetched {} email headers", headers.len());
        Ok(headers)
    }

    pub async fn fetch_body(&mut self, uid: u32) -> Result<EmailBody> {
        self.ensure_connected().await?;

        let session = self.session()?;
        let mut messages = session
            .uid_fetch(uid.to_string(), "BODY[]")
            .await
            .context("Failed to fetch message body")?;

        while let Some(result) = messages.next().await {
            let fetch = result.context("Failed to fetch message")?;
            if let Some(body) = fetch.body() {
                return Ok(super::parser::parse_body(body));
            }
        }

        Ok(EmailBody::default())
    }

    /// Batch fetch multiple bodies in a single IMAP request.
    /// Returns a Vec of (uid, body) pairs for successfully fetched bodies.
    pub async fn fetch_bodies(&mut self, uids: &[u32]) -> Result<Vec<(u32, EmailBody)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }

        self.ensure_connected().await?;

        // Build UID sequence set: "1,2,3,4"
        let uid_set = uids
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let session = self.session()?;
        let mut messages = session
            .uid_fetch(&uid_set, "BODY[]")
            .await
            .context("Failed to fetch message bodies")?;

        let mut results = Vec::with_capacity(uids.len());

        while let Some(result) = messages.next().await {
            if let Ok(fetch) = result
                && let (Some(uid), Some(body_data)) = (fetch.uid, fetch.body())
            {
                let body = super::parser::parse_body(body_data);
                results.push((uid, body));
            }
        }

        tracing::debug!(
            "Batch fetched {} bodies (requested {})",
            results.len(),
            uids.len()
        );
        Ok(results)
    }

    pub async fn add_flag(&mut self, uid: u32, flag: EmailFlags) -> Result<()> {
        self.ensure_connected().await?;

        let flag_str = match flag {
            EmailFlags::SEEN => "\\Seen",
            EmailFlags::ANSWERED => "\\Answered",
            EmailFlags::FLAGGED => "\\Flagged",
            EmailFlags::DELETED => "\\Deleted",
            EmailFlags::DRAFT => "\\Draft",
            _ => return Ok(()),
        };

        let session = self.session()?;
        let responses: Vec<_> = session
            .uid_store(uid.to_string(), format!("+FLAGS ({})", flag_str))
            .await
            .context("Failed to add flag")?
            .collect()
            .await;

        // Check for errors in the stream responses
        for response in responses {
            if let Err(e) = response {
                tracing::warn!("Error in add_flag response: {:?}", e);
            }
        }

        Ok(())
    }

    pub async fn remove_flag(&mut self, uid: u32, flag: EmailFlags) -> Result<()> {
        self.ensure_connected().await?;

        let flag_str = match flag {
            EmailFlags::SEEN => "\\Seen",
            EmailFlags::ANSWERED => "\\Answered",
            EmailFlags::FLAGGED => "\\Flagged",
            EmailFlags::DELETED => "\\Deleted",
            EmailFlags::DRAFT => "\\Draft",
            _ => return Ok(()),
        };

        let session = self.session()?;
        let responses: Vec<_> = session
            .uid_store(uid.to_string(), format!("-FLAGS ({})", flag_str))
            .await
            .context("Failed to remove flag")?
            .collect()
            .await;

        // Check for errors in the stream responses
        for response in responses {
            if let Err(e) = response {
                tracing::warn!("Error in remove_flag response: {:?}", e);
            }
        }

        Ok(())
    }

    pub async fn delete(&mut self, uid: u32) -> Result<()> {
        self.ensure_connected().await?;

        // Mark as deleted
        self.add_flag(uid, EmailFlags::DELETED).await?;

        // Expunge
        let session = self.session()?;
        let responses: Vec<_> = session
            .expunge()
            .await
            .context("Failed to expunge")?
            .collect()
            .await;

        // Check for errors in the stream responses
        for response in responses {
            if let Err(e) = response {
                tracing::warn!("Error in expunge response: {:?}", e);
            }
        }

        Ok(())
    }
}

fn parse_fetch(fetch: &Fetch) -> Option<EmailHeader> {
    let uid = fetch.uid?;

    // Collect flags from iterator
    let flag_vec: Vec<Flag> = fetch.flags().collect();
    let flags = parse_flags_from_imap(&flag_vec);

    // Combine header and partial body for parsing
    let header_bytes = fetch.header()?;
    let body_preview = fetch.text().unwrap_or(&[]);

    let mut raw = Vec::with_capacity(header_bytes.len() + 4 + body_preview.len());
    raw.extend_from_slice(header_bytes);
    raw.extend_from_slice(b"\r\n\r\n");
    raw.extend_from_slice(body_preview);

    parse_envelope(uid, &raw, flags)
}

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
                event_tx.send(ImapEvent::Connected).await.ok();
                break;
            }
            Err(e) => {
                let msg = format!(
                    "Connection attempt {}/{} failed: {}",
                    attempt, MAX_RETRIES, e
                );
                tracing::warn!("{}", msg);
                event_tx.send(ImapEvent::Error(msg)).await.ok();

                if attempt == MAX_RETRIES {
                    event_tx
                        .send(ImapEvent::Error(
                            "Max retries exceeded, giving up".to_string(),
                        ))
                        .await
                        .ok();
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
                event_tx
                    .send(ImapEvent::Error("Too many consecutive errors".to_string()))
                    .await
                    .ok();
                let delay = (2u64.pow(consecutive_errors.min(5))).min(60);
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            }

            // Try to reconnect
            if let Err(e) = reconnect(&mut client).await {
                event_tx
                    .send(ImapEvent::Error(format!("Reconnect failed: {}", e)))
                    .await
                    .ok();
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
                if let Err(e) = reconnect(&mut client).await {
                    event_tx
                        .send(ImapEvent::Error(format!("Reconnect failed: {}", e)))
                        .await
                        .ok();
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
            if let Err(e) = reconnect(&mut client).await {
                event_tx
                    .send(ImapEvent::Error(format!("Reconnect failed: {}", e)))
                    .await
                    .ok();
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
                        event_tx.send(ImapEvent::NewMail { count: 1 }).await.ok();
                        do_sync_folder(&mut client, &cache, &account_id, &current_folder, &event_tx).await;
                    }
                    // IDLE error
                    Ok(Err(e)) => {
                        tracing::warn!("IDLE error: {:?}", e);
                        // Try to reconnect
                        if let Err(e) = reconnect(&mut client).await {
                            event_tx.send(ImapEvent::Error(format!("Reconnect failed: {}", e))).await.ok();
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
                        if let Err(e) = reconnect(&mut client).await {
                            event_tx.send(ImapEvent::Error(format!("Reconnect failed: {}", e))).await.ok();
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
    // Use folder-specific cache key for operations
    let cache_key = folder_cache_key(account_id, current_folder);

    match cmd {
        ImapCommand::Sync => {
            do_sync_folder(client, cache, account_id, current_folder, event_tx).await;
        }
        ImapCommand::FetchBody { uid } => {
            // Check cache first (using folder-specific key)
            if let Ok(Some(body)) = cache.get_email_body(&cache_key, uid).await {
                event_tx
                    .send(ImapEvent::BodyFetched { uid, body })
                    .await
                    .ok();
                return;
            }

            // Fetch from server
            match client.fetch_body(uid).await {
                Ok(body) => {
                    // Cache the body with folder-specific key
                    if let Err(e) = cache.insert_email_body(&cache_key, uid, &body).await {
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
        }
        ImapCommand::FetchBodies { uids } => {
            // Filter out UIDs already in cache
            let cached_uids = cache
                .get_cached_body_uids(&cache_key, &uids)
                .await
                .unwrap_or_default();
            let uids_to_fetch: Vec<u32> = uids
                .into_iter()
                .filter(|uid| !cached_uids.contains(uid))
                .collect();

            // Send events for cached bodies immediately
            for uid in &cached_uids {
                if let Ok(Some(body)) = cache.get_email_body(&cache_key, *uid).await {
                    event_tx
                        .send(ImapEvent::BodyFetched { uid: *uid, body })
                        .await
                        .ok();
                }
            }

            // Fetch remaining bodies from server in a single batch request
            if !uids_to_fetch.is_empty() {
                match client.fetch_bodies(&uids_to_fetch).await {
                    Ok(fetched) => {
                        for (uid, body) in fetched {
                            // Cache each body
                            if let Err(e) = cache.insert_email_body(&cache_key, uid, &body).await {
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
        }
        ImapCommand::SetFlag { uid, flag } => {
            match client.add_flag(uid, flag).await {
                Ok(_) => {
                    // Get current flags from cache and add the new one
                    // Always send FlagUpdated event to keep UI in sync even if cache fails
                    let new_flags = match cache.get_email(&cache_key, uid).await {
                        Ok(Some(email)) => email.flags | flag,
                        _ => {
                            tracing::warn!(
                                "Cache lookup failed for UID {}, using flag as fallback",
                                uid
                            );
                            flag // Fallback: just the flag that was set
                        }
                    };
                    if let Err(e) = cache.update_flags(&cache_key, uid, new_flags).await {
                        tracing::warn!("Failed to update cache flags for UID {}: {}", uid, e);
                    }
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
        }
        ImapCommand::RemoveFlag { uid, flag } => {
            match client.remove_flag(uid, flag).await {
                Ok(_) => {
                    // Get current flags from cache and remove the flag
                    // Always send FlagUpdated event to keep UI in sync even if cache fails
                    let new_flags = match cache.get_email(&cache_key, uid).await {
                        Ok(Some(email)) => email.flags & !flag,
                        _ => {
                            tracing::warn!(
                                "Cache lookup failed for UID {}, using empty flags as fallback",
                                uid
                            );
                            EmailFlags::empty() // Fallback: empty flags
                        }
                    };
                    if let Err(e) = cache.update_flags(&cache_key, uid, new_flags).await {
                        tracing::warn!("Failed to update cache flags for UID {}: {}", uid, e);
                    }
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
        }
        ImapCommand::Delete { uid } => match client.delete(uid).await {
            Ok(_) => {
                if let Err(e) = cache.delete_email(&cache_key, uid).await {
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
        },
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

            // Select and sync the prefetch folder (silently)
            if client.select_folder(&folder).await.is_ok() {
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

            // Re-select original folder for IDLE to work on correct mailbox
            if *current_folder != original_folder
                && client.select_folder(&original_folder).await.is_ok()
            {
                *current_folder = original_folder;
            }
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
