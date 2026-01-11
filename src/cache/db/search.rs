//! Full-text search using SQLite FTS5.

use anyhow::Result;
use sqlx::SqlitePool;
use std::collections::HashSet;

/// Search email bodies using FTS5 - returns UIDs of matching emails.
/// Uses prefix matching for instant-as-you-type results.
pub async fn search_body_fts(
    pool: &SqlitePool,
    account_id: &str,
    query: &str,
) -> Result<HashSet<u32>> {
    if query.is_empty() {
        return Ok(HashSet::new());
    }

    // Escape special FTS5 characters and add prefix matching
    // FTS5 special chars: " * ^ : OR AND NOT NEAR
    let escaped = query
        .replace('\\', "\\\\")
        .replace('"', "\"\"")
        .replace('*', "\\*")
        .replace('^', "\\^");
    let fts_query = format!("\"{}\"*", escaped);

    let rows: Vec<(i64,)> = match sqlx::query_as(
        r#"
        SELECT b.uid
        FROM email_bodies b
        JOIN body_fts fts ON b.rowid = fts.rowid
        WHERE b.account_id = ? AND body_fts MATCH ?
        ORDER BY rank
        LIMIT 500
        "#,
    )
    .bind(account_id)
    .bind(&fts_query)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("FTS5 search failed for query '{}': {}", query, e);
            return Ok(HashSet::new());
        }
    };

    Ok(rows.into_iter().map(|(uid,)| uid as u32).collect())
}
