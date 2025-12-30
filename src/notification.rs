//! Desktop notification support for new mail alerts

use crate::config::{AccountConfig, Config};

/// Send a desktop notification for new mail
pub fn notify_new_mail(
    config: &Config,
    account: &AccountConfig,
    count: usize,
    subject_preview: Option<&str>,
) {
    // Check if notifications are enabled for this account
    if !config.notifications_enabled_for(account) {
        return;
    }

    let account_name = account.display_name_or_email();

    let summary = if count == 1 {
        format!("New mail - {}", account_name)
    } else {
        format!("{} new emails - {}", count, account_name)
    };

    let body = if config.notifications.show_preview {
        subject_preview.map(|s| {
            // Truncate long subjects
            if s.len() > 100 {
                format!("{}...", &s[..97])
            } else {
                s.to_string()
            }
        })
    } else {
        None
    };

    // Send the notification (fire and forget, don't block on errors)
    if let Err(e) = send_notification(&summary, body.as_deref()) {
        tracing::warn!("Failed to send desktop notification: {}", e);
    }
}

/// Low-level notification sending
fn send_notification(summary: &str, body: Option<&str>) -> Result<(), notify_rust::error::Error> {
    use notify_rust::Notification;

    let mut notification = Notification::new();
    notification
        .summary(summary)
        .appname("bltz")
        .timeout(notify_rust::Timeout::Milliseconds(5000));

    if let Some(body) = body {
        notification.body(body);
    }

    // Try to use a mail icon if available
    notification.icon("mail-unread");

    notification.show()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthMethod, ImapConfig, NotificationConfig, SmtpConfig};

    fn test_account(notifications: Option<bool>) -> AccountConfig {
        AccountConfig {
            email: "test@example.com".to_string(),
            display_name: Some("Test Account".to_string()),
            imap: ImapConfig {
                server: "imap.example.com".to_string(),
                port: 993,
                tls: true,
            },
            smtp: SmtpConfig {
                server: "smtp.example.com".to_string(),
                port: 587,
                tls: true,
            },
            notifications,
            auth: AuthMethod::Password,
        }
    }

    fn test_config(global_enabled: bool) -> Config {
        Config {
            accounts: vec![test_account(None)],
            default_account: Some(0),
            notifications: NotificationConfig {
                enabled: global_enabled,
                show_preview: true,
            },
            ui: Default::default(),
            cache: Default::default(),
            ai: Default::default(),
        }
    }

    #[test]
    fn test_notifications_respect_global_setting() {
        let config = test_config(false);
        let account = test_account(None);

        // Should not notify when globally disabled
        assert!(!config.notifications_enabled_for(&account));
    }

    #[test]
    fn test_notifications_respect_per_account_override() {
        let config = test_config(true);
        let account = test_account(Some(false));

        // Per-account false should override global true
        assert!(!config.notifications_enabled_for(&account));
    }

    #[test]
    fn test_notifications_enabled_when_both_true() {
        let config = test_config(true);
        let account = test_account(None);

        // Should notify when globally enabled and account doesn't override
        assert!(config.notifications_enabled_for(&account));
    }
}
