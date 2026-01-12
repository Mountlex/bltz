//! IMAP client and actor for email synchronization.
//!
//! This module is split into:
//! - `mod.rs` - Types, structs, enums, and public API
//! - `client.rs` - Connection, fetch, folder, and flag operations
//! - `actor.rs` - Actor loop, command dispatch, and sync operations
//! - `monitor.rs` - Lightweight folder monitors for multi-folder IDLE

mod actor;
mod client;
mod monitor;
pub(crate) mod parallel_sync;

use thiserror::Error;

/// Structured error types for IMAP operations.
/// These provide actionable error categories for programmatic handling.
#[derive(Debug, Clone, Error)]
pub enum ImapError {
    // Authentication errors
    #[error("Login failed: {0}")]
    LoginFailed(String),
    #[error("OAuth2 authentication failed: {0}")]
    OAuth2Failed(String),
    #[error("OAuth2 token expired, re-authentication required")]
    OAuth2Expired,

    // Network errors
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("TLS handshake failed: {0}")]
    TlsFailed(String),
    #[error("Connection timeout")]
    Timeout,
    #[error("Connection lost")]
    Disconnected,

    // Server errors
    #[error("Mailbox not found: {0}")]
    MailboxNotFound(String),
    #[allow(dead_code)]
    #[error("Server rejected operation: {0}")]
    ServerRejected(String),

    // Operation errors
    #[error("Sync failed: {0}")]
    SyncFailed(String),
    #[allow(dead_code)]
    #[error("Fetch failed: {0}")]
    FetchFailed(String),
    #[error("Max retries exceeded")]
    MaxRetriesExceeded,

    // Generic fallback for uncategorized errors
    #[error("{0}")]
    Other(String),
}

impl ImapError {
    /// Convert an anyhow::Error to an ImapError, inferring category from message content.
    pub fn from_anyhow(err: &anyhow::Error) -> Self {
        let msg = err.to_string();
        Self::categorize(&msg)
    }

    /// Categorize an error message into the appropriate ImapError variant.
    pub fn categorize(msg: &str) -> Self {
        let lower = msg.to_lowercase();

        // Authentication errors
        if lower.contains("login failed")
            || lower.contains("authentication failed")
            || lower.contains("invalid credentials")
        {
            if lower.contains("oauth") || lower.contains("xoauth2") {
                return ImapError::OAuth2Failed(msg.to_string());
            }
            return ImapError::LoginFailed(msg.to_string());
        }
        if lower.contains("token expired") || lower.contains("token invalid") {
            return ImapError::OAuth2Expired;
        }

        // Network errors
        if lower.contains("connection refused")
            || lower.contains("failed to connect")
            || lower.contains("network unreachable")
        {
            return ImapError::ConnectionFailed(msg.to_string());
        }
        if lower.contains("tls") || lower.contains("handshake") || lower.contains("certificate") {
            return ImapError::TlsFailed(msg.to_string());
        }
        if lower.contains("timeout") || lower.contains("timed out") {
            return ImapError::Timeout;
        }
        if lower.contains("connection reset")
            || lower.contains("broken pipe")
            || lower.contains("eof")
        {
            return ImapError::Disconnected;
        }

        // Server errors
        if lower.contains("no such mailbox")
            || lower.contains("mailbox not found")
            || lower.contains("doesn't exist")
        {
            return ImapError::MailboxNotFound(msg.to_string());
        }

        // Default to Other
        ImapError::Other(msg.to_string())
    }
}

use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::client::TlsStream;
use tokio_util::compat::Compat;

use crate::config::{AuthMethod, ImapConfig};

use super::types::{Attachment, EmailBody, EmailFlags, EmailHeader};

// Re-export public API
pub use actor::spawn_imap_actor;
pub use monitor::{FolderMonitorEvent, FolderMonitorHandle, spawn_folder_monitor};

/// XOAUTH2 authenticator for IMAP
pub(crate) struct XOAuth2Authenticator {
    pub user: String,
    pub access_token: String,
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
        folder: String,
    },
    /// Batch fetch multiple bodies in a single IMAP request (more efficient)
    FetchBodies {
        uids: Vec<u32>,
        folder: String,
    },
    SetFlag {
        uid: u32,
        flag: EmailFlags,
        folder: String,
    },
    RemoveFlag {
        uid: u32,
        flag: EmailFlags,
        folder: String,
    },
    Delete {
        uid: u32,
        folder: String,
    },
    SelectFolder {
        folder: String,
    },
    ListFolders,
    /// Prefetch a folder in background (sync without selecting it as active)
    PrefetchFolder {
        folder: String,
    },
    /// Fetch attachment data by index from an email
    FetchAttachment {
        uid: u32,
        folder: String,
        attachment_index: usize,
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
        folder: String,
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
    /// Attachment data fetched successfully
    AttachmentFetched {
        uid: u32,
        attachment_index: usize,
        attachment: Attachment,
        data: Vec<u8>,
    },
    /// Attachment fetch failed
    AttachmentFetchFailed {
        uid: u32,
        attachment_index: usize,
        error: String,
    },
    Error(ImapError),
}

pub(crate) type ImapSession = async_imap::Session<Compat<TlsStream<TcpStream>>>;

pub struct ImapClient {
    pub(crate) session: Option<ImapSession>,
    pub config: ImapConfig,
    pub username: String,
    pub(crate) password: String,
    pub(crate) auth_method: AuthMethod,
    /// Whether the server supports UIDPLUS extension (RFC 4315)
    pub(crate) has_uidplus: bool,
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
            has_uidplus: false,
        }
    }

    /// Clone connection config to create a new client for parallel operations.
    /// The new client will have no active session and must connect separately.
    pub fn clone_config(&self) -> Self {
        Self {
            session: None,
            config: self.config.clone(),
            username: self.username.clone(),
            password: self.password.clone(),
            auth_method: self.auth_method.clone(),
            has_uidplus: false,
        }
    }
}
