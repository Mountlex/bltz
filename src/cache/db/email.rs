//! Email header CRUD operations.

use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use crate::mail::types::{EmailFlags, EmailHeader};

/// Convert a SQLite row to an EmailHeader.
fn row_to_email_header(row: SqliteRow) -> EmailHeader {
    let references_str: Option<String> = row.get("references_list");
    let references = references_str
        .map(|s| s.split_whitespace().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    EmailHeader {
        uid: row.get::<i64, _>("uid") as u32,
        message_id: row.get("message_id"),
        subject: row.get("subject"),
        from_addr: row.get("from_addr"),
        from_name: row.get("from_name"),
        to_addr: row.get("to_addr"),
        cc_addr: row.get("cc_addr"),
        date: row.get("date"),
        flags: EmailFlags::from_bits_truncate(row.get::<i64, _>("flags") as u32),
        has_attachments: row.get("has_attachments"),
        preview: row.get("preview"),
        body_cached: row.get("body_cached"),
        in_reply_to: row.get("in_reply_to"),
        references,
        folder: row.get("folder"),
    }
}

/// Insert a single email header (for tests).
#[cfg(test)]
pub async fn insert_email(pool: &SqlitePool, account_id: &str, header: &EmailHeader) -> Result<()> {
    let references_str = if header.references.is_empty() {
        None
    } else {
        Some(header.references.join(" "))
    };
    // Extract folder from account_id (format: "account/folder") or use header.folder
    let folder = header
        .folder
        .clone()
        .or_else(|| account_id.split('/').nth(1).map(String::from));
    sqlx::query(
        r#"
        INSERT OR REPLACE INTO emails
        (uid, account_id, message_id, subject, from_addr, from_name, to_addr, cc_addr, date, flags, has_attachments, preview, body_cached, in_reply_to, references_list, folder)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(header.uid as i64)
    .bind(account_id)
    .bind(&header.message_id)
    .bind(&header.subject)
    .bind(&header.from_addr)
    .bind(&header.from_name)
    .bind(&header.to_addr)
    .bind(&header.cc_addr)
    .bind(header.date)
    .bind(header.flags.bits() as i64)
    .bind(header.has_attachments)
    .bind(&header.preview)
    .bind(header.body_cached)
    .bind(&header.in_reply_to)
    .bind(references_str)
    .bind(&folder)
    .execute(pool)
    .await?;
    Ok(())
}

/// Insert multiple email headers in a transaction.
pub async fn insert_emails(
    pool: &SqlitePool,
    account_id: &str,
    headers: &[EmailHeader],
) -> Result<()> {
    // Use a transaction for batch insert
    let mut tx = pool.begin().await?;

    // Extract folder from account_id (format: "account/folder")
    let default_folder: Option<String> = account_id.split('/').nth(1).map(String::from);

    for header in headers {
        let references_str = if header.references.is_empty() {
            None
        } else {
            Some(header.references.join(" "))
        };
        let folder = header.folder.as_ref().or(default_folder.as_ref());
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO emails
            (uid, account_id, message_id, subject, from_addr, from_name, to_addr, cc_addr, date, flags, has_attachments, preview, body_cached, in_reply_to, references_list, folder)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(header.uid as i64)
        .bind(account_id)
        .bind(&header.message_id)
        .bind(&header.subject)
        .bind(&header.from_addr)
        .bind(&header.from_name)
        .bind(&header.to_addr)
        .bind(&header.cc_addr)
        .bind(header.date)
        .bind(header.flags.bits() as i64)
        .bind(header.has_attachments)
        .bind(&header.preview)
        .bind(header.body_cached)
        .bind(&header.in_reply_to)
        .bind(references_str)
        .bind(folder)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Get emails with OFFSET pagination (legacy, kept for compatibility).
pub async fn get_emails(
    pool: &SqlitePool,
    account_id: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<EmailHeader>> {
    let rows = sqlx::query(
        r#"
        SELECT uid, message_id, subject, from_addr, from_name, to_addr, cc_addr, date, flags, has_attachments, preview, body_cached, in_reply_to, references_list, folder
        FROM emails
        WHERE account_id = ?
        ORDER BY date DESC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(account_id)
    .bind(limit as i64)
    .bind(offset as i64)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_email_header).collect())
}

/// Keyset pagination: get emails before a given (date, uid) cursor (O(1) vs O(offset)).
/// Uses composite cursor to handle identical timestamps deterministically.
pub async fn get_emails_before_cursor(
    pool: &SqlitePool,
    account_id: &str,
    cursor: Option<(i64, u32)>,
    limit: usize,
) -> Result<Vec<EmailHeader>> {
    let rows = match cursor {
        Some((date, uid)) => {
            // Composite cursor: get emails that are either:
            // 1. Older than the cursor date, OR
            // 2. Same date but with a smaller UID
            sqlx::query(
                r#"
                SELECT uid, message_id, subject, from_addr, from_name, to_addr, cc_addr, date, flags, has_attachments, preview, body_cached, in_reply_to, references_list, folder
                FROM emails
                WHERE account_id = ? AND (date < ? OR (date = ? AND uid < ?))
                ORDER BY date DESC, uid DESC
                LIMIT ?
                "#,
            )
            .bind(account_id)
            .bind(date)
            .bind(date)
            .bind(uid as i64)
            .bind(limit as i64)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query(
                r#"
                SELECT uid, message_id, subject, from_addr, from_name, to_addr, cc_addr, date, flags, has_attachments, preview, body_cached, in_reply_to, references_list, folder
                FROM emails
                WHERE account_id = ?
                ORDER BY date DESC, uid DESC
                LIMIT ?
                "#,
            )
            .bind(account_id)
            .bind(limit as i64)
            .fetch_all(pool)
            .await?
        }
    };

    Ok(rows.into_iter().map(row_to_email_header).collect())
}

/// Get a single email by UID.
pub async fn get_email(
    pool: &SqlitePool,
    account_id: &str,
    uid: u32,
) -> Result<Option<EmailHeader>> {
    let row = sqlx::query(
        r#"
        SELECT uid, message_id, subject, from_addr, from_name, to_addr, cc_addr, date, flags, has_attachments, preview, body_cached, in_reply_to, references_list, folder
        FROM emails
        WHERE account_id = ? AND uid = ?
        "#,
    )
    .bind(account_id)
    .bind(uid as i64)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_email_header))
}

/// Update email flags.
pub async fn update_flags(
    pool: &SqlitePool,
    account_id: &str,
    uid: u32,
    flags: EmailFlags,
) -> Result<()> {
    sqlx::query("UPDATE emails SET flags = ? WHERE account_id = ? AND uid = ?")
        .bind(flags.bits() as i64)
        .bind(account_id)
        .bind(uid as i64)
        .execute(pool)
        .await?;
    Ok(())
}

/// Get all UIDs and their flags from cache (for flag sync).
pub async fn get_all_uid_flags(
    pool: &SqlitePool,
    account_id: &str,
) -> Result<Vec<(u32, EmailFlags)>> {
    let rows = sqlx::query("SELECT uid, flags FROM emails WHERE account_id = ?")
        .bind(account_id)
        .fetch_all(pool)
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let uid: i64 = row.get("uid");
            let flags_bits: i64 = row.get("flags");
            (
                uid as u32,
                EmailFlags::from_bits_truncate(flags_bits as u32),
            )
        })
        .collect())
}

/// Delete an email by UID.
pub async fn delete_email(pool: &SqlitePool, account_id: &str, uid: u32) -> Result<()> {
    sqlx::query("DELETE FROM emails WHERE account_id = ? AND uid = ?")
        .bind(account_id)
        .bind(uid as i64)
        .execute(pool)
        .await?;
    Ok(())
}

/// Get total email count for an account.
pub async fn get_email_count(pool: &SqlitePool, account_id: &str) -> Result<usize> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM emails WHERE account_id = ?")
        .bind(account_id)
        .fetch_one(pool)
        .await?;
    Ok(count as usize)
}

/// Get unread email count for an account.
pub async fn get_unread_count(pool: &SqlitePool, account_id: &str) -> Result<usize> {
    let seen_flag = EmailFlags::SEEN.bits() as i64;
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM emails WHERE account_id = ? AND (flags & ?) = 0")
            .bind(account_id)
            .bind(seen_flag)
            .fetch_one(pool)
            .await?;
    Ok(count as usize)
}
