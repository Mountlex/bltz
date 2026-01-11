use std::time::Instant;

use crate::config::AccountConfig;
use crate::mail::{FolderMonitorHandle, ImapActorHandle};

/// Per-account state and handles
#[allow(dead_code)]
pub struct AccountHandle {
    /// Account configuration
    pub config: AccountConfig,
    /// IMAP actor handle for this account (main actor - handles commands + IDLE on current folder)
    pub imap_handle: ImapActorHandle,
    /// Optional folder monitors for multi-folder IDLE (e.g., Sent folder)
    pub folder_monitors: Vec<FolderMonitorHandle>,
    /// Account identifier (email address)
    pub account_id: String,
    /// Whether the IMAP connection is established
    pub connected: bool,
    /// Number of unread emails
    pub unread_count: usize,
    /// Number of new emails since last viewed
    pub unread_since_viewed: usize,
    /// Whether there are new emails (for notification badge)
    pub has_new_mail: bool,
    /// Last sync timestamp
    pub last_sync: Option<Instant>,
    /// Last error message (if any)
    pub last_error: Option<String>,
    /// Available folders for this account (populated from IMAP LIST)
    pub folder_list: Vec<String>,
}

impl AccountHandle {
    pub fn new(config: AccountConfig, imap_handle: ImapActorHandle) -> Self {
        let account_id = config.email.clone();
        Self {
            config,
            imap_handle,
            folder_monitors: Vec::new(),
            account_id,
            connected: false,
            unread_count: 0,
            unread_since_viewed: 0,
            has_new_mail: false,
            last_sync: None,
            last_error: None,
            folder_list: Vec::new(),
        }
    }

    /// Create with folder monitors for multi-folder IDLE
    #[allow(dead_code)]
    pub fn with_monitors(
        config: AccountConfig,
        imap_handle: ImapActorHandle,
        folder_monitors: Vec<FolderMonitorHandle>,
    ) -> Self {
        let account_id = config.email.clone();
        Self {
            config,
            imap_handle,
            folder_monitors,
            account_id,
            connected: false,
            unread_count: 0,
            unread_since_viewed: 0,
            has_new_mail: false,
            last_sync: None,
            last_error: None,
            folder_list: Vec::new(),
        }
    }

    /// Shutdown folder monitors
    pub async fn shutdown_monitors(&self) {
        for monitor in &self.folder_monitors {
            monitor.shutdown().await;
        }
    }

    /// Get account name for UI display
    /// Priority: name > display_name > email
    pub fn display_name(&self) -> &str {
        self.config.account_name()
    }

    /// Get a short identifier for status bar display
    pub fn short_name(&self) -> String {
        // Priority: name > display_name > email local part
        if let Some(ref name) = self.config.name {
            // Use first word of account name
            name.split_whitespace().next().unwrap_or(name).to_string()
        } else if let Some(ref name) = self.config.display_name {
            // Use first word of display name
            name.split_whitespace().next().unwrap_or(name).to_string()
        } else {
            // Use local part of email
            self.config
                .email
                .split('@')
                .next()
                .unwrap_or(&self.config.email)
                .to_string()
        }
    }

    /// Mark account as viewed (clears new mail indicator)
    pub fn mark_viewed(&mut self) {
        self.unread_since_viewed = 0;
        self.has_new_mail = false;
    }

    /// Update state when new mail arrives
    pub fn on_new_mail(&mut self, count: usize) {
        self.unread_since_viewed += count;
        self.has_new_mail = true;
    }

    /// Update state on successful sync
    #[allow(dead_code)]
    pub fn on_sync_complete(&mut self, unread_count: usize) {
        self.unread_count = unread_count;
        self.last_sync = Some(Instant::now());
        self.last_error = None;
    }

    /// Update state on connection
    pub fn on_connected(&mut self) {
        self.connected = true;
        self.last_error = None;
    }

    /// Update state on error
    pub fn on_error(&mut self, error: String) {
        self.connected = false;
        self.last_error = Some(error);
    }
}
