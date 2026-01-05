use anyhow::Result;
use std::sync::Arc;

use crate::cache::Cache;
use crate::config::{AccountConfig, AuthMethod, Config};
use crate::credentials::CredentialStore;
use crate::mail::{ImapClient, ImapCommand, ImapEvent, spawn_folder_monitor, spawn_imap_actor};

use super::AccountHandle;

/// Event from any account, tagged with account index and optionally folder
#[derive(Debug)]
pub struct AccountEvent {
    /// Index of the account that generated this event
    pub account_index: usize,
    /// The IMAP event
    pub event: ImapEvent,
    /// Source folder (Some for monitor events, None for main actor)
    pub folder: Option<String>,
}

/// Manages multiple email accounts with parallel IMAP connections
pub struct AccountManager {
    /// All account handles
    handles: Vec<AccountHandle>,
    /// Currently active account index
    active_index: usize,
    /// Cache reference for spawning folder monitors
    cache: Arc<Cache>,
}

impl AccountManager {
    /// Get credentials for an account based on its auth method.
    /// For OAuth2, exchanges refresh token for a fresh access token.
    async fn get_credentials(config: &AccountConfig) -> Result<String> {
        let credentials = CredentialStore::new(&config.email);

        match &config.auth {
            AuthMethod::Password => credentials.get_imap_password(),
            AuthMethod::OAuth2 { client_id, .. } => {
                let refresh_token = credentials.get_oauth2_refresh_token().map_err(|e| {
                    anyhow::anyhow!(
                        "OAuth2 refresh token not found: {}. Please re-authenticate.",
                        e
                    )
                })?;

                crate::oauth2::get_access_token(client_id, &refresh_token)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to refresh OAuth2 access token: {}. Please re-authenticate.",
                            e
                        )
                    })
            }
        }
    }

    /// Create a new account manager and spawn IMAP actors for all accounts
    pub async fn new(config: &Config, cache: Arc<Cache>) -> Result<Self> {
        let mut handles = Vec::new();

        for account_config in &config.accounts {
            let handle = Self::spawn_account(account_config.clone(), Arc::clone(&cache)).await?;
            handles.push(handle);
        }

        if handles.is_empty() {
            anyhow::bail!("No accounts configured");
        }

        let active_index = config.default_account.unwrap_or(0).min(handles.len() - 1);

        Ok(Self {
            handles,
            active_index,
            cache,
        })
    }

    /// Spawn a single account's IMAP actor
    async fn spawn_account(config: AccountConfig, cache: Arc<Cache>) -> Result<AccountHandle> {
        let password = Self::get_credentials(&config).await?;

        let imap_client = ImapClient::new(
            config.imap.clone(),
            config.email.clone(),
            password,
            config.auth.clone(),
        );

        let account_id = config.email.clone();
        let imap_handle = spawn_imap_actor(imap_client, cache, account_id);

        Ok(AccountHandle::new(config, imap_handle))
    }

    /// Spawn a folder monitor for a specific account
    /// Returns true if the monitor was spawned, false if already monitoring that folder
    pub async fn spawn_folder_monitor(
        &mut self,
        account_index: usize,
        folder: &str,
    ) -> Result<bool> {
        let handle = self
            .handles
            .get_mut(account_index)
            .ok_or_else(|| anyhow::anyhow!("Invalid account index: {}", account_index))?;

        // Check if we're already monitoring this folder
        if handle.folder_monitors.iter().any(|m| m.folder == folder) {
            return Ok(false);
        }

        let config = &handle.config;
        let password = Self::get_credentials(config).await?;

        let imap_client = ImapClient::new(
            config.imap.clone(),
            config.email.clone(),
            password,
            config.auth.clone(),
        );

        let account_id = config.email.clone();
        let monitor_handle = spawn_folder_monitor(
            imap_client,
            Arc::clone(&self.cache),
            account_id,
            folder.to_string(),
        );

        tracing::info!(
            "Spawned folder monitor for '{}' on account '{}'",
            folder,
            handle.account_id
        );

        handle.folder_monitors.push(monitor_handle);
        Ok(true)
    }

    /// Get the number of accounts
    pub fn count(&self) -> usize {
        self.handles.len()
    }

    /// Get the active account index
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// Get the active account handle
    pub fn active(&self) -> &AccountHandle {
        &self.handles[self.active_index]
    }

    /// Get mutable reference to active account handle
    #[allow(dead_code)]
    pub fn active_mut(&mut self) -> &mut AccountHandle {
        &mut self.handles[self.active_index]
    }

    /// Get account handle by index
    pub fn get(&self, index: usize) -> Option<&AccountHandle> {
        self.handles.get(index)
    }

    /// Get mutable account handle by index
    #[allow(dead_code)]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut AccountHandle> {
        self.handles.get_mut(index)
    }

    /// Get account handle by account_id (email)
    #[allow(dead_code)]
    pub fn get_by_id(&self, account_id: &str) -> Option<&AccountHandle> {
        self.handles.iter().find(|h| h.account_id == account_id)
    }

    /// Get mutable account handle by account_id
    #[allow(dead_code)]
    pub fn get_by_id_mut(&mut self, account_id: &str) -> Option<&mut AccountHandle> {
        self.handles.iter_mut().find(|h| h.account_id == account_id)
    }

    /// Get index of account by account_id
    #[allow(dead_code)]
    pub fn index_of(&self, account_id: &str) -> Option<usize> {
        self.handles.iter().position(|h| h.account_id == account_id)
    }

    /// Switch to the next account
    pub fn next_account(&mut self) {
        if self.handles.len() > 1 {
            // Mark current as viewed before switching
            self.handles[self.active_index].mark_viewed();
            self.active_index = (self.active_index + 1) % self.handles.len();
        }
    }

    /// Switch to the previous account
    pub fn prev_account(&mut self) {
        if self.handles.len() > 1 {
            // Mark current as viewed before switching
            self.handles[self.active_index].mark_viewed();
            self.active_index = if self.active_index == 0 {
                self.handles.len() - 1
            } else {
                self.active_index - 1
            };
        }
    }

    /// Switch to a specific account by index
    #[allow(dead_code)]
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index < self.handles.len() && index != self.active_index {
            self.handles[self.active_index].mark_viewed();
            self.active_index = index;
            true
        } else {
            false
        }
    }

    /// Iterate over all account handles
    pub fn iter(&self) -> impl Iterator<Item = &AccountHandle> {
        self.handles.iter()
    }

    /// Iterate over all account handles with indices
    pub fn iter_enumerated(&self) -> impl Iterator<Item = (usize, &AccountHandle)> {
        self.handles.iter().enumerate()
    }

    /// Poll for events from all accounts (non-blocking)
    /// Returns events tagged with their account index and optionally folder
    pub fn poll_events(&mut self) -> Vec<AccountEvent> {
        let mut events = Vec::new();

        for (index, handle) in self.handles.iter_mut().enumerate() {
            // Poll main IMAP actor events
            while let Ok(event) = handle.imap_handle.event_rx.try_recv() {
                // Update handle state based on event
                match &event {
                    ImapEvent::Connected => {
                        handle.on_connected();
                    }
                    ImapEvent::NewMail { count } => {
                        handle.on_new_mail(*count);
                    }
                    ImapEvent::SyncComplete { .. } => {
                        // Unread count will be updated by the app after cache query
                        handle.last_sync = Some(std::time::Instant::now());
                    }
                    ImapEvent::Error(msg) => {
                        handle.on_error(msg.clone());
                    }
                    _ => {}
                }

                events.push(AccountEvent {
                    account_index: index,
                    event,
                    folder: None, // Main actor - no specific folder
                });
            }

            // Poll folder monitor events
            for monitor in &mut handle.folder_monitors {
                while let Ok(folder_event) = monitor.event_rx.try_recv() {
                    // Update handle state for monitor events too
                    match &folder_event.event {
                        ImapEvent::NewMail { count } => {
                            // New mail in monitored folder (e.g., Sent)
                            // Don't update new_mail badge for Sent folder
                            tracing::debug!(
                                "New mail in monitored folder '{}': {} emails",
                                folder_event.folder,
                                count
                            );
                        }
                        ImapEvent::SyncComplete { new_count, .. } => {
                            tracing::debug!(
                                "Sync complete for monitored folder '{}': {} new",
                                folder_event.folder,
                                new_count
                            );
                        }
                        ImapEvent::Error(msg) => {
                            tracing::warn!(
                                "Error in monitored folder '{}': {}",
                                folder_event.folder,
                                msg
                            );
                        }
                        _ => {}
                    }

                    events.push(AccountEvent {
                        account_index: index,
                        event: folder_event.event,
                        folder: Some(folder_event.folder),
                    });
                }
            }
        }

        events
    }

    /// Send a command to the active account
    pub async fn send_command(&self, cmd: ImapCommand) -> Result<()> {
        let cmd_name = format!("{:?}", cmd);
        self.handles[self.active_index]
            .imap_handle
            .cmd_tx
            .send(cmd)
            .await
            .map_err(|e| {
                tracing::warn!("Failed to send IMAP command {}: {}", cmd_name, e);
                anyhow::anyhow!("Failed to send command: {}", e)
            })
    }

    /// Send a command to a specific account by index
    #[allow(dead_code)]
    pub async fn send_command_to(&self, index: usize, cmd: ImapCommand) -> Result<()> {
        if let Some(handle) = self.handles.get(index) {
            let cmd_name = format!("{:?}", cmd);
            handle.imap_handle.cmd_tx.send(cmd).await.map_err(|e| {
                tracing::warn!(
                    "Failed to send IMAP command {} to account {}: {}",
                    cmd_name,
                    index,
                    e
                );
                anyhow::anyhow!("Failed to send command: {}", e)
            })
        } else {
            anyhow::bail!("Invalid account index: {}", index)
        }
    }

    /// Shutdown all IMAP actors and folder monitors
    pub async fn shutdown(&self) {
        for handle in &self.handles {
            // Shutdown folder monitors first
            handle.shutdown_monitors().await;

            // Then shutdown main IMAP actor
            handle
                .imap_handle
                .cmd_tx
                .send(ImapCommand::Shutdown)
                .await
                .ok();
        }
    }

    /// Check if any account has new mail (for notification badge)
    #[allow(dead_code)]
    pub fn any_has_new_mail(&self) -> bool {
        self.handles.iter().any(|h| h.has_new_mail)
    }

    /// Get total unread count across all accounts
    #[allow(dead_code)]
    pub fn total_unread(&self) -> usize {
        self.handles.iter().map(|h| h.unread_count).sum()
    }

    /// Get accounts with new mail (for status bar indicators)
    #[allow(dead_code)]
    pub fn accounts_with_new_mail(&self) -> Vec<(usize, &AccountHandle)> {
        self.handles
            .iter()
            .enumerate()
            .filter(|(i, h)| *i != self.active_index && h.has_new_mail)
            .collect()
    }
}
