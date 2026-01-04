//! Email body caching with L1 (moka) and L2 (SQLite) layers.

use anyhow::Result;
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;

use crate::mail::types::EmailBody;

/// Cache key for body cache: (account_id, uid)
pub type BodyCacheKey = (String, u32);

/// L1 hot cache type alias.
pub type BodyCache = moka::future::Cache<BodyCacheKey, EmailBody>;

/// Get an email body, checking L1 cache first then L2.
pub async fn get_email_body(
    pool: &SqlitePool,
    body_cache: &BodyCache,
    account_id: &str,
    uid: u32,
) -> Result<Option<EmailBody>> {
    let key = (account_id.to_string(), uid);

    // L1: Check moka hot cache first (instant, no DB query)
    if let Some(body) = body_cache.get(&key).await {
        return Ok(Some(body));
    }

    // L2: Query SQLite database
    let row = sqlx::query(
        "SELECT text_body, html_body FROM email_bodies WHERE account_id = ? AND uid = ?",
    )
    .bind(account_id)
    .bind(uid as i64)
    .fetch_optional(pool)
    .await?;

    if let Some(ref r) = row {
        let body = EmailBody {
            text: r.get("text_body"),
            html: r.get("html_body"),
        };
        // Populate L1 cache for future reads
        body_cache.insert(key, body.clone()).await;
        return Ok(Some(body));
    }

    Ok(None)
}

/// Batch check which UIDs have cached bodies (checks moka L1 first, then SQLite L2).
pub async fn get_cached_body_uids(
    pool: &SqlitePool,
    body_cache: &BodyCache,
    account_id: &str,
    uids: &[u32],
) -> Result<HashSet<u32>> {
    if uids.is_empty() {
        return Ok(HashSet::new());
    }

    let mut cached = HashSet::new();
    let mut uids_to_check_db: Vec<u32> = Vec::new();

    // L1: Check moka hot cache first
    for &uid in uids {
        let key = (account_id.to_string(), uid);
        if body_cache.contains_key(&key) {
            cached.insert(uid);
        } else {
            uids_to_check_db.push(uid);
        }
    }

    // L2: Check SQLite for remaining UIDs
    if !uids_to_check_db.is_empty() {
        let placeholders = uids_to_check_db
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT uid FROM email_bodies WHERE account_id = ? AND uid IN ({})",
            placeholders
        );

        let mut query = sqlx::query_scalar::<_, i64>(&sql).bind(account_id);
        for uid in &uids_to_check_db {
            query = query.bind(*uid as i64);
        }

        let results = query.fetch_all(pool).await?;
        cached.extend(results.into_iter().map(|u| u as u32));
    }

    Ok(cached)
}

/// Insert an email body into both L1 and L2 caches.
pub async fn insert_email_body(
    pool: &SqlitePool,
    body_cache: &BodyCache,
    account_id: &str,
    uid: u32,
    body: &EmailBody,
) -> Result<()> {
    // Write to SQLite L2
    sqlx::query(
        "INSERT OR REPLACE INTO email_bodies (account_id, uid, text_body, html_body) VALUES (?, ?, ?, ?)",
    )
    .bind(account_id)
    .bind(uid as i64)
    .bind(&body.text)
    .bind(&body.html)
    .execute(pool)
    .await?;

    sqlx::query("UPDATE emails SET body_cached = 1 WHERE account_id = ? AND uid = ?")
        .bind(account_id)
        .bind(uid as i64)
        .execute(pool)
        .await?;

    // Also populate moka L1 hot cache
    let key = (account_id.to_string(), uid);
    body_cache.insert(key, body.clone()).await;

    Ok(())
}

/// Insert an email body with raw message into both L1 and L2 caches.
/// The raw message is stored for later attachment extraction.
pub async fn insert_email_body_with_raw(
    pool: &SqlitePool,
    body_cache: &BodyCache,
    account_id: &str,
    uid: u32,
    body: &EmailBody,
    raw_message: &[u8],
) -> Result<()> {
    // Write to SQLite L2 with raw message
    sqlx::query(
        "INSERT OR REPLACE INTO email_bodies (account_id, uid, text_body, html_body, raw_message) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(account_id)
    .bind(uid as i64)
    .bind(&body.text)
    .bind(&body.html)
    .bind(raw_message)
    .execute(pool)
    .await?;

    sqlx::query("UPDATE emails SET body_cached = 1 WHERE account_id = ? AND uid = ?")
        .bind(account_id)
        .bind(uid as i64)
        .execute(pool)
        .await?;

    // Also populate moka L1 hot cache
    let key = (account_id.to_string(), uid);
    body_cache.insert(key, body.clone()).await;

    Ok(())
}

/// Invalidate all body cache entries for a given account.
pub fn invalidate_body_cache_for_account(body_cache: &BodyCache, account_id: &str) {
    // moka doesn't have a prefix-based invalidation, so we need to iterate
    // This is rare (only on cache clear) so iteration is acceptable
    let account_id = account_id.to_string();
    let _ = body_cache.invalidate_entries_if(move |key, _value| key.0 == account_id);
}
