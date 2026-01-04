//! Attachment metadata caching operations.

use anyhow::Result;
use sqlx::{Row, SqlitePool};

use crate::mail::types::Attachment;

/// Insert attachment metadata for an email.
/// Replaces any existing attachments for the email.
pub async fn insert_attachments(
    pool: &SqlitePool,
    account_id: &str,
    email_uid: u32,
    attachments: &[Attachment],
) -> Result<()> {
    // Delete existing attachments for this email first
    sqlx::query("DELETE FROM attachments WHERE account_id = ? AND email_uid = ?")
        .bind(account_id)
        .bind(email_uid as i64)
        .execute(pool)
        .await?;

    for attachment in attachments {
        sqlx::query(
            "INSERT INTO attachments (account_id, email_uid, filename, mime_type, size, content_id) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(account_id)
        .bind(email_uid as i64)
        .bind(&attachment.filename)
        .bind(&attachment.mime_type)
        .bind(attachment.size as i64)
        .bind(&attachment.content_id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// Get attachment metadata for an email.
pub async fn get_attachments(
    pool: &SqlitePool,
    account_id: &str,
    email_uid: u32,
) -> Result<Vec<Attachment>> {
    let rows = sqlx::query(
        "SELECT id, filename, mime_type, size, content_id FROM attachments WHERE account_id = ? AND email_uid = ? ORDER BY id",
    )
    .bind(account_id)
    .bind(email_uid as i64)
    .fetch_all(pool)
    .await?;

    let attachments = rows
        .into_iter()
        .map(|row| Attachment {
            id: row.get("id"),
            filename: row.get::<Option<String>, _>("filename").unwrap_or_default(),
            mime_type: row
                .get::<Option<String>, _>("mime_type")
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            size: row.get::<i64, _>("size") as usize,
            content_id: row.get("content_id"),
        })
        .collect();

    Ok(attachments)
}

/// Get raw message for an email (for extracting attachment data).
pub async fn get_raw_message(
    pool: &SqlitePool,
    account_id: &str,
    uid: u32,
) -> Result<Option<Vec<u8>>> {
    let row = sqlx::query("SELECT raw_message FROM email_bodies WHERE account_id = ? AND uid = ?")
        .bind(account_id)
        .bind(uid as i64)
        .fetch_optional(pool)
        .await?;

    Ok(row.and_then(|r| r.get::<Option<Vec<u8>>, _>("raw_message")))
}
