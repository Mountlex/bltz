//! Database schema initialization and migrations.

use anyhow::Result;
use sqlx::SqlitePool;

/// Initialize database schema with all tables, indexes, and FTS5.
pub async fn init_schema(pool: &SqlitePool) -> Result<()> {
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
            folder TEXT,
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

    // Migration: Add folder column if it doesn't exist (for existing databases)
    sqlx::query("ALTER TABLE emails ADD COLUMN folder TEXT")
        .execute(pool)
        .await
        .ok(); // Ignore error if column already exists

    // Index on folder for cross-folder queries
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_emails_folder ON emails(folder)")
        .execute(pool)
        .await?;

    Ok(())
}
