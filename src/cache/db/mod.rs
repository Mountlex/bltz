//! SQLite cache for email headers, bodies, and sync state.
//!
//! This module is split into:
//! - `mod.rs` - Cache struct, connection pool, sync state operations
//! - `schema.rs` - Database schema initialization and migrations
//! - `email.rs` - Email header CRUD operations
//! - `body.rs` - Email body caching with L1 (moka) and L2 (SQLite)
//! - `attachment.rs` - Attachment metadata caching
//! - `search.rs` - Full-text search using FTS5

mod attachment;
mod body;
mod email;
mod schema;
mod search;

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use super::sync::SyncState;
use crate::mail::types::{Attachment, EmailBody, EmailFlags, EmailHeader};

/// Connection pool size - allows concurrent reads and writes.
/// Sized for multi-account usage with concurrent operations:
/// - Each IMAP actor may hold a connection during sync/flag operations
/// - Prefetch operations run in parallel with user actions
/// - FTS search queries can run alongside cache updates
const POOL_SIZE: u32 = 16;

/// Moka cache settings for hot data.
const BODY_CACHE_MAX_CAPACITY: u64 = 600; // Max cached bodies (slightly above page size of 500)
const BODY_CACHE_TTL_SECS: u64 = 1800; // 30 minutes TTL (email bodies are immutable)

pub struct Cache {
    pool: SqlitePool,
    /// L1 hot cache for email bodies - instant access without DB query.
    body_cache: body::BodyCache,
}

impl Cache {
    /// Get a reference to the connection pool (for tests and advanced usage).
    #[cfg(test)]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

impl Cache {
    /// Create the moka body cache with configured TTL and capacity.
    fn create_body_cache() -> body::BodyCache {
        moka::future::Cache::builder()
            .max_capacity(BODY_CACHE_MAX_CAPACITY)
            .time_to_live(Duration::from_secs(BODY_CACHE_TTL_SECS))
            .build()
    }

    pub async fn open(path: &Path) -> Result<Self> {
        let db_url = format!("sqlite:{}?mode=rwc", path.display());

        let options = SqliteConnectOptions::from_str(&db_url)?
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_secs(5))
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(POOL_SIZE)
            .connect_with(options)
            .await
            .context("Failed to create connection pool")?;

        // Initialize schema
        schema::init_schema(&pool).await?;

        Ok(Self {
            pool,
            body_cache: Self::create_body_cache(),
        })
    }

    #[cfg(test)]
    pub async fn open_in_memory() -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .context("Failed to create in-memory connection pool")?;

        schema::init_schema(&pool).await?;

        Ok(Self {
            pool,
            body_cache: Self::create_body_cache(),
        })
    }

    //
    // Sync State Operations
    //

    /// Get sync state for an account.
    pub async fn get_sync_state(&self, account_id: &str) -> Result<SyncState> {
        use sqlx::Row;

        let row = sqlx::query(
            "SELECT uid_validity, uid_next, last_sync FROM sync_state WHERE account_id = ?",
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(match row {
            Some(row) => SyncState {
                uid_validity: row.get::<Option<i64>, _>("uid_validity").map(|v| v as u32),
                uid_next: row.get::<Option<i64>, _>("uid_next").map(|v| v as u32),
                last_sync: row.get("last_sync"),
            },
            None => SyncState::default(),
        })
    }

    /// Set sync state for an account.
    pub async fn set_sync_state(&self, account_id: &str, state: &SyncState) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO sync_state (account_id, uid_validity, uid_next, last_sync) VALUES (?, ?, ?, ?)",
        )
        .bind(account_id)
        .bind(state.uid_validity.map(|v| v as i64))
        .bind(state.uid_next.map(|v| v as i64))
        .bind(state.last_sync)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    //
    // Clear Operations
    //

    /// Clear all emails for an account.
    #[allow(dead_code)]
    pub async fn clear_emails(&self, account_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM emails WHERE account_id = ?")
            .bind(account_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Clear all cached data for an account (including moka L1 cache).
    pub async fn clear_all(&self, account_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM emails WHERE account_id = ?")
            .bind(account_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM email_bodies WHERE account_id = ?")
            .bind(account_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM sync_state WHERE account_id = ?")
            .bind(account_id)
            .execute(&self.pool)
            .await?;

        // Invalidate moka L1 cache entries for this account
        body::invalidate_body_cache_for_account(&self.body_cache, account_id);

        Ok(())
    }

    //
    // Email Header Operations (delegated to email module)
    //

    #[cfg(test)]
    pub async fn insert_email(&self, account_id: &str, header: &EmailHeader) -> Result<()> {
        email::insert_email(&self.pool, account_id, header).await
    }

    pub async fn insert_emails(&self, account_id: &str, headers: &[EmailHeader]) -> Result<()> {
        email::insert_emails(&self.pool, account_id, headers).await
    }

    pub async fn get_emails(
        &self,
        account_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<EmailHeader>> {
        email::get_emails(&self.pool, account_id, limit, offset).await
    }

    pub async fn get_emails_before_cursor(
        &self,
        account_id: &str,
        cursor: Option<(i64, u32)>,
        limit: usize,
    ) -> Result<Vec<EmailHeader>> {
        email::get_emails_before_cursor(&self.pool, account_id, cursor, limit).await
    }

    #[allow(dead_code)]
    pub async fn get_email(&self, account_id: &str, uid: u32) -> Result<Option<EmailHeader>> {
        email::get_email(&self.pool, account_id, uid).await
    }

    pub async fn update_flags(&self, account_id: &str, uid: u32, flags: EmailFlags) -> Result<()> {
        email::update_flags(&self.pool, account_id, uid, flags).await
    }

    /// Atomically add a flag (avoids read-modify-write race).
    pub async fn add_flag(
        &self,
        account_id: &str,
        uid: u32,
        flag: EmailFlags,
    ) -> Result<EmailFlags> {
        email::add_flag(&self.pool, account_id, uid, flag).await
    }

    /// Atomically remove a flag (avoids read-modify-write race).
    pub async fn remove_flag(
        &self,
        account_id: &str,
        uid: u32,
        flag: EmailFlags,
    ) -> Result<EmailFlags> {
        email::remove_flag(&self.pool, account_id, uid, flag).await
    }

    pub async fn get_all_uid_flags(&self, account_id: &str) -> Result<Vec<(u32, EmailFlags)>> {
        email::get_all_uid_flags(&self.pool, account_id).await
    }

    pub async fn delete_email(&self, account_id: &str, uid: u32) -> Result<()> {
        email::delete_email(&self.pool, account_id, uid).await
    }

    /// Delete emails that are NOT in the given UID list (safer than clear_emails for full sync).
    pub async fn delete_emails_not_in(&self, account_id: &str, keep_uids: &[u32]) -> Result<usize> {
        email::delete_emails_not_in(&self.pool, account_id, keep_uids).await
    }

    pub async fn get_email_count(&self, account_id: &str) -> Result<usize> {
        email::get_email_count(&self.pool, account_id).await
    }

    pub async fn get_unread_count(&self, account_id: &str) -> Result<usize> {
        email::get_unread_count(&self.pool, account_id).await
    }

    //
    // Email Body Operations (delegated to body module)
    //

    pub async fn get_email_body(&self, account_id: &str, uid: u32) -> Result<Option<EmailBody>> {
        body::get_email_body(&self.pool, &self.body_cache, account_id, uid).await
    }

    pub async fn get_cached_body_uids(
        &self,
        account_id: &str,
        uids: &[u32],
    ) -> Result<HashSet<u32>> {
        body::get_cached_body_uids(&self.pool, &self.body_cache, account_id, uids).await
    }

    pub async fn insert_email_body(
        &self,
        account_id: &str,
        uid: u32,
        body: &EmailBody,
    ) -> Result<()> {
        body::insert_email_body(&self.pool, &self.body_cache, account_id, uid, body).await
    }

    //
    // Search Operations (delegated to search module)
    //

    pub async fn search_body_fts(&self, account_id: &str, query: &str) -> Result<HashSet<u32>> {
        search::search_body_fts(&self.pool, account_id, query).await
    }

    //
    // Attachment Operations (delegated to attachment module)
    //

    pub async fn insert_attachments(
        &self,
        account_id: &str,
        email_uid: u32,
        attachments: &[Attachment],
    ) -> Result<()> {
        attachment::insert_attachments(&self.pool, account_id, email_uid, attachments).await
    }

    pub async fn get_attachments(
        &self,
        account_id: &str,
        email_uid: u32,
    ) -> Result<Vec<Attachment>> {
        attachment::get_attachments(&self.pool, account_id, email_uid).await
    }

    pub async fn get_raw_message(&self, account_id: &str, uid: u32) -> Result<Option<Vec<u8>>> {
        attachment::get_raw_message(&self.pool, account_id, uid).await
    }

    //
    // Email Body with Raw Message (extended operations)
    //

    pub async fn insert_email_body_with_raw(
        &self,
        account_id: &str,
        uid: u32,
        body: &EmailBody,
        raw_message: &[u8],
    ) -> Result<()> {
        body::insert_email_body_with_raw(
            &self.pool,
            &self.body_cache,
            account_id,
            uid,
            body,
            raw_message,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_ACCOUNT: &str = "test@example.com";

    #[tokio::test]
    async fn test_cache_operations() {
        let cache = Cache::open_in_memory().await.unwrap();

        let header = EmailHeader {
            uid: 1,
            message_id: Some("msg@example.com".to_string()),
            subject: "Test Subject".to_string(),
            from_addr: "sender@example.com".to_string(),
            from_name: Some("Sender".to_string()),
            to_addr: Some("recipient@example.com".to_string()),
            cc_addr: None,
            date: 1234567890,
            flags: EmailFlags::empty(),
            has_attachments: false,
            preview: Some("Preview text".to_string()),
            body_cached: false,
            in_reply_to: None,
            references: Vec::new(),
            folder: None,
        };

        cache.insert_email(TEST_ACCOUNT, &header).await.unwrap();

        let emails = cache.get_emails(TEST_ACCOUNT, 100, 0).await.unwrap();
        assert_eq!(emails.len(), 1);
        assert_eq!(emails[0].subject, "Test Subject");

        // Test flags update
        cache
            .update_flags(TEST_ACCOUNT, 1, EmailFlags::SEEN | EmailFlags::FLAGGED)
            .await
            .unwrap();
        let updated = cache.get_email(TEST_ACCOUNT, 1).await.unwrap().unwrap();
        assert!(updated.flags.contains(EmailFlags::SEEN));
        assert!(updated.flags.contains(EmailFlags::FLAGGED));

        // Test delete
        cache.delete_email(TEST_ACCOUNT, 1).await.unwrap();
        assert!(cache.get_email(TEST_ACCOUNT, 1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_multi_account_isolation() {
        let cache = Cache::open_in_memory().await.unwrap();

        let header1 = EmailHeader {
            uid: 1,
            message_id: Some("msg1@example.com".to_string()),
            subject: "Account 1 Email".to_string(),
            from_addr: "sender@example.com".to_string(),
            from_name: None,
            to_addr: None,
            cc_addr: None,
            date: 1000,
            flags: EmailFlags::empty(),
            has_attachments: false,
            preview: None,
            body_cached: false,
            in_reply_to: None,
            references: Vec::new(),
            folder: None,
        };

        let header2 = EmailHeader {
            uid: 1,
            message_id: Some("msg2@example.com".to_string()),
            subject: "Account 2 Email".to_string(),
            from_addr: "sender@example.com".to_string(),
            from_name: None,
            to_addr: None,
            cc_addr: None,
            date: 2000,
            flags: EmailFlags::SEEN,
            has_attachments: false,
            preview: None,
            body_cached: false,
            in_reply_to: None,
            references: Vec::new(),
            folder: None,
        };

        cache
            .insert_email("account1@example.com", &header1)
            .await
            .unwrap();
        cache
            .insert_email("account2@example.com", &header2)
            .await
            .unwrap();

        let account1_emails = cache
            .get_emails("account1@example.com", 100, 0)
            .await
            .unwrap();
        let account2_emails = cache
            .get_emails("account2@example.com", 100, 0)
            .await
            .unwrap();

        assert_eq!(account1_emails.len(), 1);
        assert_eq!(account2_emails.len(), 1);
        assert_eq!(account1_emails[0].subject, "Account 1 Email");
        assert_eq!(account2_emails[0].subject, "Account 2 Email");

        assert_eq!(
            cache.get_email_count("account1@example.com").await.unwrap(),
            1
        );
        assert_eq!(
            cache.get_email_count("account2@example.com").await.unwrap(),
            1
        );
        assert_eq!(
            cache
                .get_unread_count("account1@example.com")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            cache
                .get_unread_count("account2@example.com")
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn test_sync_state_per_account() {
        let cache = Cache::open_in_memory().await.unwrap();

        let state1 = SyncState {
            uid_validity: Some(100),
            uid_next: Some(50),
            last_sync: Some(1000),
        };

        let state2 = SyncState {
            uid_validity: Some(200),
            uid_next: Some(100),
            last_sync: Some(2000),
        };

        cache
            .set_sync_state("account1@example.com", &state1)
            .await
            .unwrap();
        cache
            .set_sync_state("account2@example.com", &state2)
            .await
            .unwrap();

        let retrieved1 = cache.get_sync_state("account1@example.com").await.unwrap();
        let retrieved2 = cache.get_sync_state("account2@example.com").await.unwrap();

        assert_eq!(retrieved1.uid_validity, Some(100));
        assert_eq!(retrieved2.uid_validity, Some(200));
        assert_eq!(retrieved1.uid_next, Some(50));
        assert_eq!(retrieved2.uid_next, Some(100));
    }
}
