use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    pub struct EmailFlags: u32 {
        const SEEN = 0b00000001;
        const ANSWERED = 0b00000010;
        const FLAGGED = 0b00000100;
        const DELETED = 0b00001000;
        const DRAFT = 0b00010000;
    }
}

#[derive(Debug, Clone)]
pub struct EmailHeader {
    pub uid: u32,
    pub message_id: Option<String>,
    pub subject: String,
    pub from_addr: String,
    pub from_name: Option<String>,
    pub to_addr: Option<String>,
    pub date: i64,
    pub flags: EmailFlags,
    pub has_attachments: bool,
    pub preview: Option<String>,
    pub body_cached: bool,
    // Threading fields
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

impl EmailHeader {
    pub fn is_seen(&self) -> bool {
        self.flags.contains(EmailFlags::SEEN)
    }

    pub fn is_flagged(&self) -> bool {
        self.flags.contains(EmailFlags::FLAGGED)
    }

    pub fn display_from(&self) -> &str {
        self.from_name.as_deref().unwrap_or(&self.from_addr)
    }
}

#[derive(Debug, Clone, Default)]
pub struct EmailBody {
    pub text: Option<String>,
    pub html: Option<String>,
}

impl EmailBody {
    /// Get displayable text content
    /// Returns plain text if available, otherwise strips HTML tags from HTML content
    pub fn display_text(&self) -> String {
        if let Some(ref text) = self.text {
            text.clone()
        } else if let Some(ref html) = self.html {
            // Strip HTML tags for display
            strip_html_tags(html)
        } else {
            "[No content]".to_string()
        }
    }
}

/// Convert HTML to readable plain text
fn strip_html_tags(html: &str) -> String {
    html2text::from_read(html.as_bytes(), 80)
}

#[derive(Debug, Clone)]
pub struct ComposeEmail {
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    /// Index of the account to send from (None = use currently active account)
    pub from_account_index: Option<usize>,
}

impl ComposeEmail {
    pub fn new() -> Self {
        Self {
            to: String::new(),
            cc: String::new(),
            subject: String::new(),
            body: String::new(),
            in_reply_to: None,
            references: None,
            from_account_index: None,
        }
    }

    /// Create a new email with a specific sending account
    pub fn new_from_account(account_index: usize) -> Self {
        Self {
            from_account_index: Some(account_index),
            ..Self::new()
        }
    }

    pub fn reply_to(original: &EmailHeader, original_body: &str) -> Self {
        let subject = if original.subject.starts_with("Re:") {
            original.subject.clone()
        } else {
            format!("Re: {}", original.subject)
        };

        let quoted_body = original_body
            .lines()
            .map(|line| format!("> {}", line))
            .collect::<Vec<_>>()
            .join("\n");

        let body = format!(
            "\n\nOn {}, {} wrote:\n{}",
            chrono::DateTime::from_timestamp(original.date, 0)
                .map(|dt| dt.format("%b %d, %Y at %H:%M").to_string())
                .unwrap_or_default(),
            original.display_from(),
            quoted_body
        );

        // Build references chain: original's references + original's message-id
        let references = if let Some(ref mid) = original.message_id {
            let mut refs = original.references.clone();
            refs.push(mid.clone());
            Some(refs.join(" "))
        } else {
            None
        };

        Self {
            to: original.from_addr.clone(),
            cc: String::new(),
            subject,
            body,
            in_reply_to: original.message_id.clone(),
            references,
            from_account_index: None,
        }
    }

    /// Create a reply-all email that includes original sender and all CC recipients
    pub fn reply_all(original: &EmailHeader, original_body: &str, my_email: &str) -> Self {
        let subject = if original.subject.starts_with("Re:") {
            original.subject.clone()
        } else {
            format!("Re: {}", original.subject)
        };

        let quoted_body = original_body
            .lines()
            .map(|line| format!("> {}", line))
            .collect::<Vec<_>>()
            .join("\n");

        let body = format!(
            "\n\nOn {}, {} wrote:\n{}",
            chrono::DateTime::from_timestamp(original.date, 0)
                .map(|dt| dt.format("%b %d, %Y at %H:%M").to_string())
                .unwrap_or_default(),
            original.display_from(),
            quoted_body
        );

        // Build references chain: original's references + original's message-id
        let references = if let Some(ref mid) = original.message_id {
            let mut refs = original.references.clone();
            refs.push(mid.clone());
            Some(refs.join(" "))
        } else {
            None
        };

        // To: original sender
        let to = original.from_addr.clone();

        // CC: original To recipients (excluding ourselves) + original CC recipients
        let mut cc_addrs: Vec<String> = Vec::new();

        // Add original To recipients (could be multiple comma-separated)
        if let Some(ref to_addr) = original.to_addr {
            for addr in to_addr.split(',') {
                let addr = addr.trim();
                // Skip our own email
                if !addr.eq_ignore_ascii_case(my_email) && !addr.is_empty() {
                    cc_addrs.push(addr.to_string());
                }
            }
        }

        // Remove the original sender from CC (they're already in To)
        cc_addrs.retain(|addr| !addr.eq_ignore_ascii_case(&original.from_addr));

        let cc = cc_addrs.join(", ");

        Self {
            to,
            cc,
            subject,
            body,
            in_reply_to: original.message_id.clone(),
            references,
            from_account_index: None,
        }
    }

    pub fn forward(original: &EmailHeader, original_body: &str) -> Self {
        let subject = if original.subject.to_lowercase().starts_with("fwd:") {
            original.subject.clone()
        } else {
            format!("Fwd: {}", original.subject)
        };

        let body = format!(
            "\n\n---------- Forwarded message ----------\n\
             From: {}\n\
             Date: {}\n\
             Subject: {}\n\n\
             {}",
            original.display_from(),
            chrono::DateTime::from_timestamp(original.date, 0)
                .map(|dt| dt.format("%b %d, %Y at %H:%M").to_string())
                .unwrap_or_default(),
            original.subject,
            original_body
        );

        Self {
            to: String::new(), // User fills in recipient
            cc: String::new(),
            subject,
            body,
            in_reply_to: None, // Forward is not a reply
            references: None,
            from_account_index: None,
        }
    }
}

impl Default for ComposeEmail {
    fn default() -> Self {
        Self::new()
    }
}
