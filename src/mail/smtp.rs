use anyhow::{Context, Result};
use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::{AuthMethod, SmtpConfig};

use super::types::ComposeEmail;

pub struct SmtpClient {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from_email: String,
    from_name: Option<String>,
}

impl SmtpClient {
    #[allow(dead_code)]
    pub async fn new(
        config: &SmtpConfig,
        username: &str,
        password: &str,
        from_email: &str,
        from_name: Option<&str>,
    ) -> Result<Self> {
        Self::new_with_auth(
            config,
            username,
            password,
            from_email,
            from_name,
            &AuthMethod::Password,
        )
        .await
    }

    pub async fn new_with_auth(
        config: &SmtpConfig,
        username: &str,
        password: &str,
        from_email: &str,
        from_name: Option<&str>,
        auth_method: &AuthMethod,
    ) -> Result<Self> {
        let creds = Credentials::new(username.to_string(), password.to_string());

        // Select authentication mechanism based on auth method
        let mechanisms = match auth_method {
            AuthMethod::Password => vec![Mechanism::Plain, Mechanism::Login],
            AuthMethod::OAuth2 { .. } => vec![Mechanism::Xoauth2],
        };

        // Always require TLS for security - plaintext SMTP exposes credentials
        if !config.tls {
            tracing::warn!("SMTP TLS disabled in config - enabling anyway for security");
        }

        let transport = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.server)
            .context("Failed to create SMTP transport")?
            .port(config.port)
            .credentials(creds)
            .authentication(mechanisms)
            .build();

        Ok(Self {
            transport,
            from_email: from_email.to_string(),
            from_name: from_name.map(|s| s.to_string()),
        })
    }

    pub async fn send(&self, compose: &ComposeEmail) -> Result<()> {
        let from_mailbox = if let Some(ref name) = self.from_name {
            format!("{} <{}>", name, self.from_email)
                .parse::<Mailbox>()
                .context("Invalid from address")?
        } else {
            self.from_email
                .parse::<Mailbox>()
                .context("Invalid from address")?
        };

        let mut builder = Message::builder()
            .from(from_mailbox)
            .subject(&compose.subject);

        // Add To recipients (handle comma-separated list and trailing commas)
        for to_addr in compose.to.split(',') {
            let to_addr = to_addr.trim();
            if !to_addr.is_empty() {
                let to_mailbox = to_addr
                    .parse::<Mailbox>()
                    .context(format!("Invalid recipient address: {}", to_addr))?;
                builder = builder.to(to_mailbox);
            }
        }

        // Add CC recipients if present
        if !compose.cc.is_empty() {
            for cc_addr in compose.cc.split(',') {
                let cc_addr = cc_addr.trim();
                if !cc_addr.is_empty() {
                    let cc_mailbox = cc_addr
                        .parse::<Mailbox>()
                        .context(format!("Invalid CC address: {}", cc_addr))?;
                    builder = builder.cc(cc_mailbox);
                }
            }
        }

        if let Some(ref reply_to) = compose.in_reply_to {
            builder = builder.in_reply_to(reply_to.clone());
        }

        if let Some(ref references) = compose.references {
            builder = builder.references(references.clone());
        }

        let message = builder
            .header(ContentType::TEXT_PLAIN)
            .body(compose.body.clone())
            .context("Failed to build email message")?;

        self.transport
            .send(message)
            .await
            .context("Failed to send email")?;

        tracing::info!("Email sent to {} (cc: {})", compose.to, compose.cc);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compose_email() {
        let compose = ComposeEmail::new();
        assert!(compose.to.is_empty());
        assert!(compose.subject.is_empty());
        assert!(compose.body.is_empty());
    }
}
