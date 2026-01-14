//! IMAP connection pool for parallel operations.
//!
//! Maintains a pool of connected IMAP clients that can be borrowed for parallel
//! operations like batch body fetching. Connections are kept alive for reuse,
//! eliminating the overhead of repeated TCP+TLS+login sequences.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::ImapClient;
use crate::config::{AuthMethod, ImapConfig};

/// Default maximum number of connections in the pool.
const DEFAULT_POOL_SIZE: usize = 4;

/// A pool of IMAP connections for parallel operations.
///
/// The pool manages a set of connected IMAP clients that can be borrowed
/// for parallel operations and returned for reuse. This avoids the overhead
/// of creating new connections for each parallel operation.
pub struct ImapConnectionPool {
    /// Connected clients ready to be borrowed
    clients: Arc<Mutex<Vec<ImapClient>>>,
    /// Configuration for creating new clients
    config: ImapConfig,
    username: String,
    password: String,
    auth_method: AuthMethod,
    max_size: usize,
}

impl ImapConnectionPool {
    /// Create a new connection pool with the given configuration.
    ///
    /// The pool starts empty and connections are created on-demand when borrowed.
    pub fn new(
        config: ImapConfig,
        username: String,
        password: String,
        auth_method: AuthMethod,
    ) -> Self {
        Self {
            clients: Arc::new(Mutex::new(Vec::with_capacity(DEFAULT_POOL_SIZE))),
            config,
            username,
            password,
            auth_method,
            max_size: DEFAULT_POOL_SIZE,
        }
    }

    /// Borrow a connected client from the pool.
    ///
    /// Returns an already-connected client if available, otherwise creates
    /// and connects a new one. The caller should return the client using
    /// `return_client()` when done.
    pub async fn borrow(&self) -> Result<ImapClient> {
        // Try to get an existing connected client
        let existing = {
            let mut pool = self.clients.lock().await;
            pool.pop()
        };

        match existing {
            Some(client) if client.is_connected() => {
                tracing::debug!("Pool: reusing connected client");
                Ok(client)
            }
            Some(mut client) => {
                // Client was in pool but disconnected (server timeout, etc.)
                tracing::debug!("Pool: reconnecting stale client");
                client.connect().await?;
                Ok(client)
            }
            None => {
                // Pool empty, create new client
                tracing::debug!("Pool: creating new client");
                let mut client = self.create_client();
                client.connect().await?;
                Ok(client)
            }
        }
    }

    /// Return a client to the pool for reuse.
    ///
    /// The client is kept connected for fast reuse. If the client is
    /// disconnected or the pool is full, the client is dropped.
    pub async fn return_client(&self, client: ImapClient) {
        // Only return connected clients to pool
        if !client.is_connected() {
            tracing::debug!("Pool: dropping disconnected client");
            return;
        }

        let mut pool = self.clients.lock().await;
        if pool.len() < self.max_size {
            tracing::debug!("Pool: returning client (pool size: {})", pool.len() + 1);
            pool.push(client);
        } else {
            // Pool full, drop this client (disconnect happens on drop)
            tracing::debug!("Pool: pool full, dropping client");
        }
    }

    fn create_client(&self) -> ImapClient {
        ImapClient::new(
            self.config.clone(),
            self.username.clone(),
            self.password.clone(),
            self.auth_method.clone(),
        )
    }
}

impl Clone for ImapConnectionPool {
    fn clone(&self) -> Self {
        Self {
            clients: Arc::clone(&self.clients),
            config: self.config.clone(),
            username: self.username.clone(),
            password: self.password.clone(),
            auth_method: self.auth_method.clone(),
            max_size: self.max_size,
        }
    }
}
