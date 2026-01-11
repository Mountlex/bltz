//! IMAP client operations: connection, fetch, folder, and flag management.

use anyhow::{Context, Result};
use async_imap::types::{Fetch, Flag, Mailbox};
use futures::StreamExt;

use crate::config::AuthMethod;

use super::{ImapClient, ImapSession, XOAuth2Authenticator};
use crate::mail::parser::{parse_envelope, parse_flags_from_imap};
use crate::mail::types::{EmailBody, EmailFlags, EmailHeader};

impl ImapClient {
    //
    // Connection Management
    //

    pub async fn connect(&mut self) -> Result<()> {
        use std::sync::Arc;
        use tokio::net::TcpStream;
        use tokio_rustls::TlsConnector;
        use tokio_util::compat::TokioAsyncReadCompatExt;

        let addr = format!("{}:{}", self.config.server, self.config.port);

        let tcp = TcpStream::connect(&addr)
            .await
            .with_context(|| format!("Failed to connect to {}", addr))?;

        // Build rustls config with webpki root certificates
        let root_store =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let connector = TlsConnector::from(Arc::new(config));
        let server_name = self
            .config
            .server
            .clone()
            .try_into()
            .context("Invalid server name")?;
        let tls_stream = connector
            .connect(server_name, tcp)
            .await
            .context("TLS handshake failed")?;

        // Wrap with compat layer for futures-io compatibility (required by async-imap)
        let client = async_imap::Client::new(tls_stream.compat());

        // Authenticate based on configured auth method
        let mut session = match &self.auth_method {
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

        // Check for UIDPLUS capability (RFC 4315) for safer deletion
        if let Ok(caps) = session.capabilities().await {
            self.has_uidplus = caps.has(&async_imap::types::Capability::Atom("UIDPLUS".into()));
            if self.has_uidplus {
                tracing::debug!("Server supports UIDPLUS extension");
            }
        }

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

    pub(crate) async fn ensure_connected(&mut self) -> Result<()> {
        if !self.is_connected() {
            self.connect().await?;
        }
        Ok(())
    }

    pub(crate) fn session(&mut self) -> Result<&mut ImapSession> {
        self.session
            .as_mut()
            .context("Not connected to IMAP server")
    }

    //
    // Folder Operations
    //

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

    //
    // Fetch Operations
    //

    pub(crate) async fn fetch_all_headers(&mut self) -> Result<Vec<EmailHeader>> {
        self.fetch_headers("1:*").await
    }

    pub(crate) async fn fetch_headers_from(&mut self, start_uid: u32) -> Result<Vec<EmailHeader>> {
        self.fetch_headers(&format!("{}:*", start_uid)).await
    }

    /// Fetch all UIDs from the current folder (lightweight, for deletion detection).
    pub(crate) async fn fetch_all_uids(&mut self) -> Result<Vec<u32>> {
        self.ensure_connected().await?;

        let session = self.session()?;
        let mut messages = session
            .uid_fetch("1:*", "UID")
            .await
            .context("Failed to fetch UIDs")?;

        let mut uids = Vec::new();
        while let Some(result) = messages.next().await {
            if let Ok(fetch) = result
                && let Some(uid) = fetch.uid
            {
                uids.push(uid);
            }
        }

        Ok(uids)
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
                return Ok(crate::mail::parser::parse_body(body));
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
                let body = crate::mail::parser::parse_body(body_data);
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

    /// Fetch raw message data for a single email (used for attachment extraction).
    pub async fn fetch_raw(&mut self, uid: u32) -> Result<Vec<u8>> {
        self.ensure_connected().await?;

        let session = self.session()?;
        let mut messages = session
            .uid_fetch(uid.to_string(), "BODY[]")
            .await
            .context("Failed to fetch raw message")?;

        while let Some(result) = messages.next().await {
            let fetch = result.context("Failed to fetch message")?;
            if let Some(body) = fetch.body() {
                return Ok(body.to_vec());
            }
        }

        anyhow::bail!("No body data found for UID {}", uid)
    }

    //
    // Flag Operations
    //

    /// Fetch current flags for a single email from the server.
    pub async fn fetch_flags(&mut self, uid: u32) -> Result<EmailFlags> {
        self.ensure_connected().await?;

        let session = self.session()?;
        let mut messages = session
            .uid_fetch(uid.to_string(), "FLAGS")
            .await
            .context("Failed to fetch flags")?;

        while let Some(result) = messages.next().await {
            if let Ok(fetch) = result {
                let flag_vec: Vec<async_imap::types::Flag> = fetch.flags().collect();
                return Ok(parse_flags_from_imap(&flag_vec));
            }
        }

        // If we couldn't fetch flags, return empty rather than error
        // This allows the caller to decide how to handle the case
        Ok(EmailFlags::empty())
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

        // Expunge - use UID EXPUNGE if available (RFC 4315) for safer operation
        // UID EXPUNGE only removes the specified message, while regular EXPUNGE
        // removes ALL messages with \Deleted flag
        let has_uidplus = self.has_uidplus;
        let session = self.session()?;

        if has_uidplus {
            // Use UID EXPUNGE for targeted deletion (only affects this specific UID)
            // Format: UID EXPUNGE <sequence-set>
            let cmd = format!("UID EXPUNGE {}", uid);
            session
                .run_command_and_check_ok(&cmd)
                .await
                .context("UID EXPUNGE failed")?;
            tracing::debug!("Used UID EXPUNGE for uid {}", uid);
        } else {
            // Server doesn't support UIDPLUS - use regular EXPUNGE
            // WARNING: This affects ALL messages with \Deleted flag, not just this UID
            // Only safe if we're certain no other emails have \Deleted flag
            tracing::warn!(
                "Server lacks UIDPLUS support, using regular EXPUNGE for uid {} - \
                 this may affect other emails marked for deletion",
                uid
            );
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
        }

        Ok(())
    }
}

/// Parse a single email from an IMAP FETCH response
pub(crate) fn parse_fetch(fetch: &Fetch) -> Option<EmailHeader> {
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
