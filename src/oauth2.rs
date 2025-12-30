//! OAuth2 support for Gmail using the installed app flow
//!
//! Uses Google's OAuth2 installed app flow which opens a browser for authentication.
//! This is required because Gmail scopes are not supported by the device code flow.

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

// Google OAuth2 endpoints
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

// Gmail scope for IMAP/SMTP access
const GMAIL_SCOPE: &str = "https://mail.google.com/";

/// Default Google OAuth2 client ID placeholder
///
/// IMPORTANT: You must register your own OAuth2 client at:
/// https://console.cloud.google.com/apis/credentials
///
/// Steps:
/// 1. Create a Google Cloud project
/// 2. Enable the Gmail API
/// 3. Configure OAuth consent screen (add https://mail.google.com/ scope)
/// 4. Create OAuth client ID with type "Desktop app"
/// 5. Set BLTZ_OAUTH_CLIENT_ID and BLTZ_OAUTH_CLIENT_SECRET environment variables
///
/// See: https://developers.google.com/identity/protocols/oauth2/native-app
/// Get the OAuth2 client ID from environment variable
pub fn get_client_id() -> Option<String> {
    std::env::var("BLTZ_OAUTH_CLIENT_ID").ok()
}

/// Get the OAuth2 client secret from environment variable (BLTZ_OAUTH_CLIENT_SECRET)
pub fn get_client_secret() -> Option<String> {
    std::env::var("BLTZ_OAUTH_CLIENT_SECRET").ok()
}

/// OAuth2 tokens returned after successful authorization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub token_type: Option<String>,
}

/// PKCE code verifier and challenge
struct PkceChallenge {
    verifier: String,
    challenge: String,
}

impl PkceChallenge {
    fn new() -> Result<Self> {
        // Generate a random 32-byte verifier using cryptographically secure RNG
        let mut verifier_bytes = [0u8; 32];
        getrandom::fill(&mut verifier_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to generate random bytes: {}", e))?;
        let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(verifier_bytes);

        // Create SHA256 challenge using sha2 crate
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let challenge_hash = hasher.finalize();
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(challenge_hash);

        Ok(Self {
            verifier,
            challenge,
        })
    }
}

/// Error response from Google
#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
    #[allow(dead_code)]
    error_description: Option<String>,
}

/// Gmail OAuth2 client for installed app flow
pub struct GmailOAuth2 {
    client_id: String,
    client_secret: Option<String>,
    http_client: reqwest::Client,
}

impl GmailOAuth2 {
    /// Create a new Gmail OAuth2 client
    pub fn new(client_id: &str, client_secret: Option<&str>) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client_id: client_id.to_string(),
            client_secret: client_secret.map(|s| s.to_string()),
            http_client,
        })
    }

    /// Start the OAuth flow by opening a browser and waiting for the callback
    ///
    /// Returns the authorization code and redirect URI used
    pub fn start_auth_flow(&self) -> Result<AuthFlowState> {
        // Find an available port
        let listener = TcpListener::bind("127.0.0.1:0").context("Failed to bind to local port")?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://127.0.0.1:{}", port);

        // Generate PKCE challenge
        let pkce = PkceChallenge::new()?;

        // Generate random state parameter for CSRF protection
        let mut state_bytes = [0u8; 16];
        getrandom::fill(&mut state_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to generate random state: {}", e))?;
        let state = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(state_bytes);

        // Build authorization URL with state parameter
        let auth_url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent&state={}",
            GOOGLE_AUTH_URL,
            urlencod(&self.client_id),
            urlencod(&redirect_uri),
            urlencod(GMAIL_SCOPE),
            urlencod(&pkce.challenge),
            urlencod(&state),
        );

        tracing::debug!("OAuth2 redirect URI: {}", redirect_uri);

        Ok(AuthFlowState {
            auth_url,
            redirect_uri,
            pkce_verifier: pkce.verifier,
            state,
            listener,
        })
    }

    /// Wait for the OAuth callback and extract the authorization code
    pub fn wait_for_callback(auth_state: &AuthFlowState) -> Result<String> {
        use std::io::ErrorKind;

        // Set to non-blocking so we can implement a timeout
        auth_state.listener.set_nonblocking(true)?;

        // Poll for connection with timeout (2 minutes)
        let timeout = Duration::from_secs(120);
        let start = std::time::Instant::now();

        let mut stream = loop {
            match auth_state.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    if start.elapsed() > timeout {
                        bail!("OAuth callback timed out. Please try again.");
                    }
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(e) => {
                    return Err(e).context("Failed to accept OAuth callback connection");
                }
            }
        };

        // Read the request
        let mut reader = BufReader::new(&stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line)?;

        // Helper to parse query parameters
        let parse_query_param = |query: &str, param: &str| -> Option<String> {
            query
                .split('&')
                .find(|p| p.starts_with(&format!("{}=", param)))
                .map(|p| p.trim_start_matches(&format!("{}=", param)).to_string())
        };

        // Extract query string from request
        let query = request_line
            .split_whitespace()
            .nth(1)
            .and_then(|path| path.split('?').nth(1))
            .unwrap_or("");

        // Check for error first
        if let Some(error) = parse_query_param(query, "error") {
            let error = error.split(' ').next().unwrap_or(&error);
            let error_desc = parse_query_param(query, "error_description")
                .map(|s| s.split(' ').next().unwrap_or(&s).to_string())
                .map(|s| urldecodd(&s))
                .unwrap_or_default();

            // Send error response to browser with escaped HTML
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                <html><body><h1>Authorization Failed</h1>\
                <p>Error: {}</p><p>{}</p>\
                <p>Please close this window and try again.</p></body></html>",
                escape_html(error),
                escape_html(&error_desc)
            );
            stream.write_all(response.as_bytes()).ok();

            bail!("Authorization failed: {} - {}", error, error_desc);
        }

        // Validate state parameter for CSRF protection
        let returned_state = parse_query_param(query, "state")
            .context("No state parameter in callback - possible CSRF attack")?;
        if returned_state != auth_state.state {
            bail!("State parameter mismatch - possible CSRF attack");
        }

        // Parse the authorization code from the request
        let code = parse_query_param(query, "code").context(
            "No authorization code in callback. The browser may have sent an unexpected response.",
        )?;

        // Send success response to browser
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
            <html><body><h1>Authorization successful!</h1>\
            <p>You can close this window and return to bltz.</p>\
            <script>window.close();</script></body></html>";
        stream.write_all(response.as_bytes())?;

        Ok(code)
    }

    /// Exchange authorization code for tokens
    pub async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        pkce_verifier: &str,
    ) -> Result<OAuth2Tokens> {
        let mut params = vec![
            ("client_id", self.client_id.as_str()),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
            ("code_verifier", pkce_verifier),
        ];

        // Add client_secret if available (required for some client types)
        let secret_str;
        if let Some(ref secret) = self.client_secret {
            secret_str = secret.clone();
            params.push(("client_secret", &secret_str));
        }

        let response = self
            .http_client
            .post(GOOGLE_TOKEN_URL)
            .form(&params)
            .send()
            .await
            .context("Failed to exchange authorization code")?;

        if !response.status().is_success() {
            let error: ErrorResponse = response.json().await.unwrap_or(ErrorResponse {
                error: "unknown_error".to_string(),
                error_description: None,
            });
            bail!("Token exchange failed: {}", error.error);
        }

        response
            .json()
            .await
            .context("Failed to parse token response")
    }

    /// Refresh an access token using a refresh token
    pub async fn refresh_access_token(&self, refresh_token: &str) -> Result<OAuth2Tokens> {
        let mut params = vec![
            ("client_id", self.client_id.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ];

        let secret_str;
        if let Some(ref secret) = self.client_secret {
            secret_str = secret.clone();
            params.push(("client_secret", &secret_str));
        }

        let response = self
            .http_client
            .post(GOOGLE_TOKEN_URL)
            .form(&params)
            .send()
            .await
            .context("Failed to refresh token")?;

        if !response.status().is_success() {
            let error: ErrorResponse = response.json().await.unwrap_or(ErrorResponse {
                error: "unknown_error".to_string(),
                error_description: None,
            });
            bail!("Token refresh failed: {}", error.error);
        }

        response
            .json()
            .await
            .context("Failed to parse refresh token response")
    }
}

/// State for an in-progress OAuth flow
pub struct AuthFlowState {
    pub auth_url: String,
    pub redirect_uri: String,
    pub pkce_verifier: String,
    pub state: String,
    listener: TcpListener,
}

/// Escape HTML special characters to prevent XSS
fn escape_html(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect::<Vec<_>>(),
            '>' => "&gt;".chars().collect::<Vec<_>>(),
            '"' => "&quot;".chars().collect::<Vec<_>>(),
            '\'' => "&#x27;".chars().collect::<Vec<_>>(),
            _ => vec![c],
        })
        .collect()
}

/// URL-encode a string
fn urlencod(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
            _ => {
                for b in c.to_string().as_bytes() {
                    result.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    result
}

/// URL-decode a string
fn urldecodd(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

/// Get a fresh access token using a stored refresh token
pub async fn get_access_token(client_id: &str, refresh_token: &str) -> Result<String> {
    let client_secret = get_client_secret();
    let oauth = GmailOAuth2::new(client_id, client_secret.as_deref())?;
    let tokens = oauth.refresh_access_token(refresh_token).await?;
    Ok(tokens.access_token)
}

/// Build the XOAUTH2 SASL authentication string
///
/// Format: base64("user=" + email + "\x01auth=Bearer " + access_token + "\x01\x01")
#[allow(dead_code)]
pub fn build_xoauth2_string(email: &str, access_token: &str) -> String {
    let auth_string = format!("user={}\x01auth=Bearer {}\x01\x01", email, access_token);
    base64::engine::general_purpose::STANDARD.encode(auth_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_xoauth2_string() {
        let result = build_xoauth2_string("user@example.com", "ya29.test_token");
        // Verify it's valid base64
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&result)
            .unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();

        assert_eq!(
            decoded_str,
            "user=user@example.com\x01auth=Bearer ya29.test_token\x01\x01"
        );
    }

    #[test]
    fn test_urlencod() {
        assert_eq!(urlencod("hello"), "hello");
        assert_eq!(urlencod("hello world"), "hello%20world");
        assert_eq!(urlencod("a=b&c=d"), "a%3Db%26c%3Dd");
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("hello"), "hello");
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("a&b"), "a&amp;b");
        assert_eq!(escape_html("\"test\""), "&quot;test&quot;");
    }
}
