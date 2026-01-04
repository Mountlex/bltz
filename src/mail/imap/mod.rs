//! IMAP client and actor for email synchronization.
//!
//! This module is split into:
//! - `mod.rs` - Types, structs, enums, and public API
//! - `client.rs` - Connection, fetch, folder, and flag operations
//! - `actor.rs` - Actor loop, command dispatch, and sync operations

mod actor;
mod client;

use async_native_tls::TlsStream;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::compat::Compat;

use crate::config::{AuthMethod, ImapConfig};

use super::types::{Attachment, EmailBody, EmailFlags, EmailHeader};

// Re-export public API
pub use actor::spawn_imap_actor;

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
    Error(String),
}

pub(crate) type ImapSession = async_imap::Session<TlsStream<Compat<TcpStream>>>;

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
}
