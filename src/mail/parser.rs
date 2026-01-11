use mail_parser::{MessageParser, MimeHeaders, PartType};

use super::types::{Attachment, EmailBody, EmailFlags, EmailHeader};

pub fn parse_envelope(uid: u32, raw: &[u8], flags: EmailFlags) -> Option<EmailHeader> {
    let message = match MessageParser::default().parse(raw) {
        Some(msg) => msg,
        None => {
            tracing::warn!(
                "Failed to parse email UID {}: malformed message ({} bytes)",
                uid,
                raw.len()
            );
            return None;
        }
    };

    let from = match message.from().and_then(|f| f.first()) {
        Some(f) => f,
        None => {
            tracing::debug!("Email UID {} has no From header", uid);
            return None;
        }
    };
    let from_addr = match from.address() {
        Some(addr) => addr.to_string(),
        None => {
            tracing::debug!("Email UID {} has From header without address", uid);
            return None;
        }
    };
    let from_name = from.name().map(|s| s.to_string());

    // Extract all To recipients (comma-separated)
    let to_addr = message.to().map(|addrs| {
        addrs
            .iter()
            .filter_map(|addr| addr.address())
            .collect::<Vec<_>>()
            .join(", ")
    });

    // Extract all CC recipients (comma-separated)
    let cc_addr = message.cc().map(|addrs| {
        addrs
            .iter()
            .filter_map(|addr| addr.address())
            .collect::<Vec<_>>()
            .join(", ")
    });

    let subject = message.subject().map(|s| s.to_string()).unwrap_or_default();

    let date = match message.date() {
        Some(d) => d.to_timestamp(),
        None => {
            tracing::debug!("Email UID {} has no Date header, using epoch", uid);
            0 // Use epoch as fallback instead of dropping the email
        }
    };

    let message_id = message.message_id().map(|s| s.to_string());

    let has_attachments = message.attachments().count() > 0;

    let preview = extract_preview(&message, 100);

    // Extract threading headers
    let in_reply_to = message
        .in_reply_to()
        .as_text_list()
        .and_then(|ids| ids.first().map(|s| s.to_string()));

    let references: Vec<String> = message
        .references()
        .as_text_list()
        .map(|ids| ids.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();

    Some(EmailHeader {
        uid,
        message_id,
        subject,
        from_addr,
        from_name,
        to_addr,
        cc_addr,
        date,
        flags,
        has_attachments,
        preview,
        body_cached: false,
        in_reply_to,
        references,
        folder: None, // Set by caller when storing
    })
}

pub fn parse_body(raw: &[u8]) -> EmailBody {
    let Some(message) = MessageParser::default().parse(raw) else {
        return EmailBody::default();
    };

    let text = extract_text_body(&message);
    let html = extract_html_body(&message);

    EmailBody { text, html }
}

/// Parse attachment metadata from raw email
pub fn parse_attachments(raw: &[u8]) -> Vec<Attachment> {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Vec::new();
    };

    message
        .attachments()
        .enumerate()
        .map(|(i, part)| {
            let filename = part
                .attachment_name()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("attachment_{}", i + 1));

            let mime_type = part
                .content_type()
                .map(|ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("octet-stream")))
                .unwrap_or_else(|| "application/octet-stream".to_string());

            let size = match &part.body {
                PartType::Binary(data) => data.len(),
                PartType::Text(data) => data.len(),
                PartType::Html(data) => data.len(),
                _ => 0,
            };

            let content_id = part.content_id().map(|s| s.to_string());

            Attachment {
                id: 0,
                filename,
                mime_type,
                size,
                content_id,
            }
        })
        .collect()
}

/// Extract binary data for a specific attachment by index
pub fn extract_attachment_data(raw: &[u8], index: usize) -> Option<Vec<u8>> {
    let message = MessageParser::default().parse(raw)?;
    let part = message.attachments().nth(index)?;

    match &part.body {
        PartType::Binary(data) => Some(data.to_vec()),
        PartType::Text(data) => Some(data.as_bytes().to_vec()),
        PartType::Html(data) => Some(data.as_bytes().to_vec()),
        _ => None,
    }
}

fn extract_text_body(message: &mail_parser::Message) -> Option<String> {
    // First try to get text body parts
    for part in message.text_bodies() {
        if let PartType::Text(text) = &part.body {
            return Some(text.to_string());
        }
    }

    // Fallback: try to extract from any part
    for part in message.parts.iter() {
        if let PartType::Text(text) = &part.body {
            let content_type = part.content_type();
            if content_type
                .map(|ct| ct.subtype() == Some("plain"))
                .unwrap_or(true)
            {
                return Some(text.to_string());
            }
        }
    }

    None
}

fn extract_html_body(message: &mail_parser::Message) -> Option<String> {
    for part in message.html_bodies() {
        if let PartType::Html(html) = &part.body {
            return Some(html.to_string());
        }
    }

    None
}

fn extract_preview(message: &mail_parser::Message, max_len: usize) -> Option<String> {
    let text = extract_text_body(message)?;

    let preview: String = text
        .chars()
        .filter(|c| !c.is_control() || *c == ' ')
        .take(max_len)
        .collect();

    let preview = preview.trim().to_string();

    if preview.is_empty() {
        None
    } else {
        Some(preview)
    }
}

pub fn parse_flags_from_imap(flags: &[async_imap::types::Flag<'_>]) -> EmailFlags {
    let mut result = EmailFlags::empty();

    for flag in flags {
        match flag {
            async_imap::types::Flag::Seen => result |= EmailFlags::SEEN,
            async_imap::types::Flag::Answered => result |= EmailFlags::ANSWERED,
            async_imap::types::Flag::Flagged => result |= EmailFlags::FLAGGED,
            async_imap::types::Flag::Deleted => result |= EmailFlags::DELETED,
            async_imap::types::Flag::Draft => result |= EmailFlags::DRAFT,
            _ => {}
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_email() {
        let raw = b"From: sender@example.com\r\n\
                    To: recipient@example.com\r\n\
                    Subject: Test Email\r\n\
                    Date: Mon, 1 Jan 2024 12:00:00 +0000\r\n\
                    Message-ID: <test@example.com>\r\n\
                    \r\n\
                    Hello, this is a test email.";

        let header = parse_envelope(1, raw, EmailFlags::empty()).unwrap();
        assert_eq!(header.subject, "Test Email");
        assert_eq!(header.from_addr, "sender@example.com");
        assert!(header.preview.is_some());

        let body = parse_body(raw);
        assert!(body.text.is_some());
        assert!(body.text.unwrap().contains("Hello"));
    }
}
