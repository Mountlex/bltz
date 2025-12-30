//! Add account wizard actions

use anyhow::Context;

use crate::config::{AccountConfig, AuthMethod, ImapConfig, SmtpConfig};
use crate::credentials::CredentialStore;
use crate::oauth2::{GmailOAuth2, get_client_id, get_client_secret};
use crate::ui::app::{AddAccountAuth, AddAccountStep, View};

use super::super::App;

impl App {
    pub(super) async fn wizard_next(&mut self) {
        // Extract info we need from wizard state first
        let (current_step, auth_method, email, password, imap_server, smtp_server) = {
            if let View::AddAccount { step, data } = &self.state.view {
                (
                    step.clone(),
                    data.auth_method.clone(),
                    data.email.clone(),
                    data.password.clone(),
                    data.imap_server.clone(),
                    data.smtp_server.clone(),
                )
            } else {
                return;
            }
        };

        let next_step = match current_step {
            AddAccountStep::ChooseAuthMethod => AddAccountStep::EnterEmail,
            AddAccountStep::EnterEmail => {
                if email.is_empty() || !email.contains('@') {
                    self.state.set_error("Please enter a valid email address");
                    return;
                }
                match auth_method {
                    AddAccountAuth::Password => AddAccountStep::EnterPassword,
                    AddAccountAuth::OAuth2Gmail => {
                        // Start OAuth2 flow (opens browser, waits for callback, exchanges code)
                        tracing::info!("Starting OAuth2 flow for {}", email);
                        match self.start_oauth2_flow().await {
                            Ok(()) => {
                                tracing::info!("OAuth2 flow completed successfully");
                                // OAuth2 flow is complete, skip to IMAP server step
                                AddAccountStep::EnterImapServer
                            }
                            Err(e) => {
                                tracing::error!("Failed to start OAuth2 flow: {}", e);
                                self.state.set_error(format!("OAuth2 error: {}", e));
                                return;
                            }
                        }
                    }
                }
            }
            AddAccountStep::EnterPassword => {
                if password.is_empty() {
                    self.state.set_error("Password cannot be empty");
                    return;
                }
                AddAccountStep::EnterImapServer
            }
            AddAccountStep::OAuth2Flow => {
                // OAuth2 flow advances automatically when complete
                return;
            }
            AddAccountStep::EnterImapServer => {
                // Auto-fill server if empty
                if imap_server.is_empty()
                    && let View::AddAccount { data, .. } = &mut self.state.view
                    && let Some(domain) = email.split('@').nth(1)
                {
                    if domain.contains("gmail") {
                        data.imap_server = "imap.gmail.com".to_string();
                    } else {
                        data.imap_server = format!("imap.{}", domain);
                    }
                }
                AddAccountStep::EnterSmtpServer
            }
            AddAccountStep::EnterSmtpServer => {
                // Auto-fill server if empty
                if smtp_server.is_empty()
                    && let View::AddAccount { data, .. } = &mut self.state.view
                    && let Some(domain) = email.split('@').nth(1)
                {
                    if domain.contains("gmail") {
                        data.smtp_server = "smtp.gmail.com".to_string();
                    } else {
                        data.smtp_server = format!("smtp.{}", domain);
                    }
                }
                AddAccountStep::Confirm
            }
            AddAccountStep::Confirm => {
                // This is handled by wizard_confirm
                return;
            }
        };

        // Update the step
        if let View::AddAccount { step, .. } = &mut self.state.view {
            *step = next_step;
        }
    }

    /// Start the OAuth2 installed app flow
    ///
    /// This opens a browser for authentication and waits for the callback
    async fn start_oauth2_flow(&mut self) -> anyhow::Result<()> {
        // Get OAuth2 credentials from environment variables
        let client_id = get_client_id().ok_or_else(|| {
            anyhow::anyhow!(
                "OAuth2 client ID not configured.\n\
                 Set BLTZ_OAUTH_CLIENT_ID environment variable.\n\
                 See: https://developers.google.com/identity/protocols/oauth2/native-app"
            )
        })?;
        let client_secret = get_client_secret();

        tracing::debug!("Creating GmailOAuth2 with client_id: {}", client_id);
        let oauth = GmailOAuth2::new(&client_id, client_secret.as_deref())?;

        // Start the auth flow (creates listener, generates PKCE, builds URL)
        tracing::debug!("Starting OAuth2 flow...");
        let flow_state = oauth.start_auth_flow()?;

        tracing::info!(
            "Opening browser for OAuth2 authorization: {}",
            flow_state.auth_url
        );

        // Update UI to show we're waiting
        if let View::AddAccount { data, .. } = &mut self.state.view {
            data.oauth2_url = Some(flow_state.auth_url.clone());
            data.oauth2_status = Some("Opening browser for authorization...".to_string());
            data.oauth2_client_id = Some(client_id.clone());
        }

        // Open the browser
        if let Err(e) = open::that(&flow_state.auth_url) {
            tracing::warn!(
                "Failed to open browser: {}. Please open the URL manually.",
                e
            );
        }

        // Update status
        if let View::AddAccount { data, .. } = &mut self.state.view {
            data.oauth2_status = Some("Waiting for browser authorization...".to_string());
        }

        // Wait for callback in a blocking task (since the TcpListener is blocking)
        let redirect_uri = flow_state.redirect_uri.clone();
        let pkce_verifier = flow_state.pkce_verifier.clone();

        let code = tokio::task::spawn_blocking(move || GmailOAuth2::wait_for_callback(&flow_state))
            .await
            .context("OAuth callback task failed")??;

        tracing::info!("Received authorization code");

        // Update status
        if let View::AddAccount { data, .. } = &mut self.state.view {
            data.oauth2_status = Some("Exchanging code for tokens...".to_string());
        }

        // Exchange code for tokens
        let tokens = oauth
            .exchange_code(&code, &redirect_uri, &pkce_verifier)
            .await?;

        tracing::info!("OAuth2 authorization successful");

        // Store the refresh token
        if let Some(ref refresh_token) = tokens.refresh_token {
            if let View::AddAccount { data, .. } = &mut self.state.view {
                // Store in wizard data for now, will be saved to keyring on confirm
                data.oauth2_refresh_token = Some(refresh_token.clone());
                data.oauth2_status = Some("Authorization successful!".to_string());
            }
        } else {
            anyhow::bail!("No refresh token received from Google. Please try again.");
        }

        Ok(())
    }

    pub(super) fn wizard_back(&mut self) {
        if let View::AddAccount { step, data } = &mut self.state.view {
            let prev_step = match step {
                AddAccountStep::ChooseAuthMethod => {
                    // Cancel wizard
                    self.state.view = View::Inbox;
                    return;
                }
                AddAccountStep::EnterEmail => AddAccountStep::ChooseAuthMethod,
                AddAccountStep::EnterPassword => AddAccountStep::EnterEmail,
                AddAccountStep::OAuth2Flow => AddAccountStep::EnterEmail,
                AddAccountStep::EnterImapServer => match data.auth_method {
                    AddAccountAuth::Password => AddAccountStep::EnterPassword,
                    AddAccountAuth::OAuth2Gmail => AddAccountStep::EnterEmail,
                },
                AddAccountStep::EnterSmtpServer => AddAccountStep::EnterImapServer,
                AddAccountStep::Confirm => AddAccountStep::EnterSmtpServer,
            };
            *step = prev_step;
        }
    }

    pub(super) async fn wizard_confirm(&mut self) {
        if let View::AddAccount { data, .. } = &self.state.view {
            // Create the new account config
            let auth = match data.auth_method {
                AddAccountAuth::Password => AuthMethod::Password,
                AddAccountAuth::OAuth2Gmail => AuthMethod::OAuth2 {
                    provider: "gmail".to_string(),
                    client_id: data.oauth2_client_id.clone().unwrap_or_default(),
                },
            };

            let new_account = AccountConfig {
                email: data.email.clone(),
                display_name: None,
                imap: ImapConfig {
                    server: data.imap_server.clone(),
                    port: 993,
                    tls: true,
                },
                smtp: SmtpConfig {
                    server: data.smtp_server.clone(),
                    port: 587,
                    tls: true,
                },
                notifications: None,
                auth,
            };

            // Store credentials
            let creds = CredentialStore::new(&data.email);
            match data.auth_method {
                AddAccountAuth::Password => {
                    if let Err(e) = creds.set_password(&data.password) {
                        self.state
                            .set_error(format!("Failed to store password: {}", e));
                        return;
                    }
                }
                AddAccountAuth::OAuth2Gmail => {
                    // Save the OAuth2 refresh token to keyring
                    if let Some(ref refresh_token) = data.oauth2_refresh_token {
                        if let Err(e) = creds.set_oauth2_refresh_token(refresh_token) {
                            self.state
                                .set_error(format!("Failed to store OAuth2 token: {}", e));
                            return;
                        }
                    } else {
                        self.state.set_error(
                            "No OAuth2 refresh token available. Please re-authenticate."
                                .to_string(),
                        );
                        return;
                    }
                }
            }

            // Load current config, add account, and save
            match crate::config::Config::load() {
                Ok(mut config) => {
                    config.accounts.push(new_account);
                    if let Err(e) = config.save() {
                        self.state
                            .set_error(format!("Failed to save config: {}", e));
                        return;
                    }
                    self.state.set_status(format!(
                        "Account {} added! Restart to load the new account.",
                        data.email
                    ));
                }
                Err(e) => {
                    self.state
                        .set_error(format!("Failed to load config: {}", e));
                    return;
                }
            }
        }

        // Return to inbox
        self.state.view = View::Inbox;
    }
}
