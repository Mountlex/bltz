use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use super::sync::SyncState;
use crate::mail::types::{EmailBody, EmailFlags, EmailHeader};

/// Connection pool size - allows concurrent reads
const POOL_SIZE: u32 = 4;

/// Moka cache settings for hot data
const BODY_CACHE_MAX_CAPACITY: u64 = 100; // Max cached bodies
const BODY_CACHE_TTL_SECS: u64 = 300; // 5 minutes TTL

/// Cache key for body cache: (account_id, uid)
type BodyCacheKey = (String, u32);

pub struct Cache {
    pool: SqlitePool,
    /// L1 hot cache for email bodies - instant access without DB query
    body_cache: moka::future::Cache<BodyCacheKey, EmailBody>,
}

impl Cache {
    /// Get a reference to the connection pool (for tests and advanced usage)
    #[cfg(test)]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

impl Cache {
    /// Create the moka body cache with configured TTL and capacity
    fn create_body_cache() -> moka::future::Cache<BodyCacheKey, EmailBody> {
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
        Self::init_schema(&pool).await?;

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

        Self::init_schema(&pool).await?;

        Ok(Self {
            pool,
            body_cache: Self::create_body_cache(),
        })
    }

    /// Initialize database schema
    async fn init_schema(pool: &SqlitePool) -> Result<()> {
        // Create tables
        sqlx::query(
            r#"
            -- Schema version tracking
            CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY
            );

            -- Email headers
            CREATE TABLE IF NOT EXISTS emails (
                uid INTEGER NOT NULL,
                account_id TEXT NOT NULL DEFAULT '',
                message_id TEXT,
                subject TEXT NOT NULL DEFAULT '',
                from_addr TEXT NOT NULL DEFAULT '',
                from_name TEXT,
                to_addr TEXT,
                cc_addr TEXT,
                date INTEGER NOT NULL,
                flags INTEGER NOT NULL DEFAULT 0,
                has_attachments INTEGER NOT NULL DEFAULT 0,
                preview TEXT,
                body_cached INTEGER NOT NULL DEFAULT 0,
                in_reply_to TEXT,
                references_list TEXT,
                PRIMARY KEY (account_id, uid)
            );

            CREATE INDEX IF NOT EXISTS idx_emails_date ON emails(date DESC);
            CREATE INDEX IF NOT EXISTS idx_emails_flags ON emails(flags);
            CREATE INDEX IF NOT EXISTS idx_emails_message_id ON emails(message_id);
            CREATE INDEX IF NOT EXISTS idx_emails_in_reply_to ON emails(in_reply_to);
            CREATE INDEX IF NOT EXISTS idx_emails_account ON emails(account_id);
            CREATE INDEX IF NOT EXISTS idx_emails_account_date ON emails(account_id, date DESC);
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            -- Email bodies
            CREATE TABLE IF NOT EXISTS email_bodies (
                uid INTEGER NOT NULL,
                account_id TEXT NOT NULL DEFAULT '',
                text_body TEXT,
                html_body TEXT,
                raw_message BLOB,
                PRIMARY KEY (account_id, uid)
            );

            CREATE INDEX IF NOT EXISTS idx_bodies_account_uid ON email_bodies(account_id, uid);

            -- Attachments
            CREATE TABLE IF NOT EXISTS attachments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id TEXT NOT NULL DEFAULT '',
                email_uid INTEGER NOT NULL,
                filename TEXT,
                mime_type TEXT,
                size INTEGER,
                content_id TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_attachments_email ON attachments(account_id, email_uid);

            -- Sync state
            CREATE TABLE IF NOT EXISTS sync_state (
                account_id TEXT PRIMARY KEY,
                uid_validity INTEGER,
                uid_next INTEGER,
                last_sync INTEGER
            );

            -- Contacts
            CREATE TABLE IF NOT EXISTS contacts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email TEXT UNIQUE NOT NULL,
                name TEXT,
                last_contacted INTEGER,
                contact_count INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_contacts_email ON contacts(email);
            CREATE INDEX IF NOT EXISTS idx_contacts_name ON contacts(name);
            "#,
        )
        .execute(pool)
        .await?;

        // FTS5 virtual table for body full-text search
        // Note: FTS5 tables must be created separately (can't use IF NOT EXISTS in same batch)
        sqlx::query(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS body_fts USING fts5(
                text_body,
                content='email_bodies',
                content_rowid='rowid',
                tokenize='unicode61 remove_diacritics 1'
            );
            "#,
        )
        .execute(pool)
        .await?;

        // Triggers to keep FTS in sync with email_bodies
        // Using INSERT OR IGNORE pattern since triggers can't use IF NOT EXISTS
        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS body_fts_insert AFTER INSERT ON email_bodies BEGIN
                INSERT INTO body_fts(rowid, text_body)
                VALUES (NEW.rowid, NEW.text_body);
            END;
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS body_fts_delete AFTER DELETE ON email_bodies BEGIN
                INSERT INTO body_fts(body_fts, rowid, text_body)
                VALUES ('delete', OLD.rowid, OLD.text_body);
            END;
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TRIGGER IF NOT EXISTS body_fts_update AFTER UPDATE ON email_bodies BEGIN
                INSERT INTO body_fts(body_fts, rowid, text_body)
                VALUES ('delete', OLD.rowid, OLD.text_body);
                INSERT INTO body_fts(rowid, text_body)
                VALUES (NEW.rowid, NEW.text_body);
            END;
            "#,
        )
        .execute(pool)
        .await?;

        // Populate FTS for any existing data (idempotent - uses INSERT OR IGNORE semantics via content table)
        sqlx::query(
            r#"
            INSERT INTO body_fts(body_fts) VALUES('rebuild');
            "#,
        )
        .execute(pool)
        .await
        .ok(); // Ignore error if already populated

        Ok(())
    }

    /// Get sync state for an account
    pub async fn get_sync_state(&self, account_id: &str) -> Result<SyncState> {
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

    /// Set sync state for an account
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

    /// Clear all emails for an account
    pub async fn clear_emails(&self, account_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM emails WHERE account_id = ?")
            .bind(account_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Clear all cached data for an account (including moka L1 cache)
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
        self.invalidate_body_cache_for_account(account_id);

        Ok(())
    }

    /// Invalidate all body cache entries for a given account
    fn invalidate_body_cache_for_account(&self, account_id: &str) {
        // moka doesn't have a prefix-based invalidation, so we need to iterate
        // This is rare (only on cache clear) so iteration is acceptable
        let account_id = account_id.to_string();
        let _ = self
            .body_cache
            .invalidate_entries_if(move |key, _value| key.0 == account_id);
    }

    #[cfg(test)]
    pub async fn insert_email(&self, account_id: &str, header: &EmailHeader) -> Result<()> {
        let references_str = if header.references.is_empty() {
            None
        } else {
            Some(header.references.join(" "))
        };
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO emails
            (uid, account_id, message_id, subject, from_addr, from_name, to_addr, cc_addr, date, flags, has_attachments, preview, body_cached, in_reply_to, references_list)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_emails(&self, account_id: &str, headers: &[EmailHeader]) -> Result<()> {
        // Use a transaction for batch insert
        let mut tx = self.pool.begin().await?;

        for header in headers {
            let references_str = if header.references.is_empty() {
                None
            } else {
                Some(header.references.join(" "))
            };
            sqlx::query(
                r#"
                INSERT OR REPLACE INTO emails
                (uid, account_id, message_id, subject, from_addr, from_name, to_addr, cc_addr, date, flags, has_attachments, preview, body_cached, in_reply_to, references_list)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Get emails with OFFSET pagination (legacy, kept for compatibility)
    pub async fn get_emails(
        &self,
        account_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<EmailHeader>> {
        let rows = sqlx::query(
            r#"
            SELECT uid, message_id, subject, from_addr, from_name, to_addr, cc_addr, date, flags, has_attachments, preview, body_cached, in_reply_to, references_list
            FROM emails
            WHERE account_id = ?
            ORDER BY date DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(account_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Self::row_to_email_header).collect())
    }

    /// Keyset pagination: get emails before a given date (O(1) vs O(offset))
    pub async fn get_emails_before_date(
        &self,
        account_id: &str,
        before_date: Option<i64>,
        limit: usize,
    ) -> Result<Vec<EmailHeader>> {
        let rows = match before_date {
            Some(date) => {
                sqlx::query(
                    r#"
                    SELECT uid, message_id, subject, from_addr, from_name, to_addr, cc_addr, date, flags, has_attachments, preview, body_cached, in_reply_to, references_list
                    FROM emails
                    WHERE account_id = ? AND date < ?
                    ORDER BY date DESC
                    LIMIT ?
                    "#,
                )
                .bind(account_id)
                .bind(date)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query(
                    r#"
                    SELECT uid, message_id, subject, from_addr, from_name, to_addr, cc_addr, date, flags, has_attachments, preview, body_cached, in_reply_to, references_list
                    FROM emails
                    WHERE account_id = ?
                    ORDER BY date DESC
                    LIMIT ?
                    "#,
                )
                .bind(account_id)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
        };

        Ok(rows.into_iter().map(Self::row_to_email_header).collect())
    }

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
        }
    }

    pub async fn get_email(&self, account_id: &str, uid: u32) -> Result<Option<EmailHeader>> {
        let row = sqlx::query(
            r#"
            SELECT uid, message_id, subject, from_addr, from_name, to_addr, cc_addr, date, flags, has_attachments, preview, body_cached, in_reply_to, references_list
            FROM emails
            WHERE account_id = ? AND uid = ?
            "#,
        )
        .bind(account_id)
        .bind(uid as i64)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(Self::row_to_email_header))
    }

    pub async fn update_flags(&self, account_id: &str, uid: u32, flags: EmailFlags) -> Result<()> {
        sqlx::query("UPDATE emails SET flags = ? WHERE account_id = ? AND uid = ?")
            .bind(flags.bits() as i64)
            .bind(account_id)
            .bind(uid as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get all UIDs and their flags from cache (for flag sync)
    pub async fn get_all_uid_flags(&self, account_id: &str) -> Result<Vec<(u32, EmailFlags)>> {
        let rows = sqlx::query("SELECT uid, flags FROM emails WHERE account_id = ?")
            .bind(account_id)
            .fetch_all(&self.pool)
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

    pub async fn delete_email(&self, account_id: &str, uid: u32) -> Result<()> {
        sqlx::query("DELETE FROM emails WHERE account_id = ? AND uid = ?")
            .bind(account_id)
            .bind(uid as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_email_body(&self, account_id: &str, uid: u32) -> Result<Option<EmailBody>> {
        let key = (account_id.to_string(), uid);

        // L1: Check moka hot cache first (instant, no DB query)
        if let Some(body) = self.body_cache.get(&key).await {
            return Ok(Some(body));
        }

        // L2: Query SQLite database
        let row = sqlx::query(
            "SELECT text_body, html_body FROM email_bodies WHERE account_id = ? AND uid = ?",
        )
        .bind(account_id)
        .bind(uid as i64)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(ref r) = row {
            let body = EmailBody {
                text: r.get("text_body"),
                html: r.get("html_body"),
            };
            // Populate L1 cache for future reads
            self.body_cache.insert(key, body.clone()).await;
            return Ok(Some(body));
        }

        Ok(None)
    }

    /// Batch check which UIDs have cached bodies (checks moka L1 first, then SQLite L2)
    pub async fn get_cached_body_uids(
        &self,
        account_id: &str,
        uids: &[u32],
    ) -> Result<std::collections::HashSet<u32>> {
        use std::collections::HashSet;

        if uids.is_empty() {
            return Ok(HashSet::new());
        }

        let mut cached = HashSet::new();
        let mut uids_to_check_db: Vec<u32> = Vec::new();

        // L1: Check moka hot cache first
        for &uid in uids {
            let key = (account_id.to_string(), uid);
            if self.body_cache.contains_key(&key) {
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

            let results = query.fetch_all(&self.pool).await?;
            cached.extend(results.into_iter().map(|u| u as u32));
        }

        Ok(cached)
    }

    pub async fn insert_email_body(
        &self,
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
        .execute(&self.pool)
        .await?;

        sqlx::query("UPDATE emails SET body_cached = 1 WHERE account_id = ? AND uid = ?")
            .bind(account_id)
            .bind(uid as i64)
            .execute(&self.pool)
            .await?;

        // Also populate moka L1 hot cache
        let key = (account_id.to_string(), uid);
        self.body_cache.insert(key, body.clone()).await;

        Ok(())
    }

    /// Search email bodies using FTS5 - returns UIDs of matching emails
    /// Uses prefix matching for instant-as-you-type results
    pub async fn search_body_fts(
        &self,
        account_id: &str,
        query: &str,
    ) -> Result<std::collections::HashSet<u32>> {
        use std::collections::HashSet;

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

        let rows: Vec<(i64,)> = sqlx::query_as(
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
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        Ok(rows.into_iter().map(|(uid,)| uid as u32).collect())
    }

    pub async fn get_email_count(&self, account_id: &str) -> Result<usize> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM emails WHERE account_id = ?")
            .bind(account_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(count as usize)
    }

    pub async fn get_unread_count(&self, account_id: &str) -> Result<usize> {
        let seen_flag = EmailFlags::SEEN.bits() as i64;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM emails WHERE account_id = ? AND (flags & ?) = 0",
        )
        .bind(account_id)
        .bind(seen_flag)
        .fetch_one(&self.pool)
        .await?;
        Ok(count as usize)
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
