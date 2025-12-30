#![allow(dead_code)]

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Contact {
    pub id: i64,
    pub email: String,
    pub name: Option<String>,
    pub last_contacted: Option<i64>,
    pub contact_count: i64,
}

impl Contact {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.email)
    }
}

pub struct ContactsDb {
    pool: SqlitePool,
}

impl ContactsDb {
    /// Open the contacts database at the given path
    pub async fn open(path: &Path) -> Result<Self> {
        let db_url = format!("sqlite:{}?mode=rwc", path.display());

        let options = SqliteConnectOptions::from_str(&db_url)?
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_secs(5))
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .context("Failed to create contacts connection pool")?;

        // Initialize contacts schema
        Self::init_schema(&pool).await?;

        Ok(Self { pool })
    }

    /// Open an in-memory database (for testing)
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

        Ok(Self { pool })
    }

    /// Initialize the contacts table schema
    async fn init_schema(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
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

        Ok(())
    }

    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn add_or_update(&self, email: &str, name: Option<&str>) -> Result<()> {
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO contacts (email, name, last_contacted, contact_count)
            VALUES (?, ?, ?, 1)
            ON CONFLICT(email) DO UPDATE SET
                name = COALESCE(excluded.name, contacts.name),
                last_contacted = excluded.last_contacted,
                contact_count = contacts.contact_count + 1
            "#,
        )
        .bind(email)
        .bind(name)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_all(&self) -> Result<Vec<Contact>> {
        let rows = sqlx::query(
            r#"
            SELECT id, email, name, last_contacted, contact_count
            FROM contacts
            ORDER BY contact_count DESC, last_contacted DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let contacts = rows
            .into_iter()
            .map(|row| Contact {
                id: row.get("id"),
                email: row.get("email"),
                name: row.get("name"),
                last_contacted: row.get("last_contacted"),
                contact_count: row.get("contact_count"),
            })
            .collect();

        Ok(contacts)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<Contact>> {
        let pattern = format!("%{}%", query);

        let rows = sqlx::query(
            r#"
            SELECT id, email, name, last_contacted, contact_count
            FROM contacts
            WHERE email LIKE ? OR name LIKE ?
            ORDER BY contact_count DESC
            LIMIT 20
            "#,
        )
        .bind(&pattern)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;

        let contacts = rows
            .into_iter()
            .map(|row| Contact {
                id: row.get("id"),
                email: row.get("email"),
                name: row.get("name"),
                last_contacted: row.get("last_contacted"),
                contact_count: row.get("contact_count"),
            })
            .collect();

        Ok(contacts)
    }

    pub async fn get_by_email(&self, email: &str) -> Result<Option<Contact>> {
        let row = sqlx::query(
            "SELECT id, email, name, last_contacted, contact_count FROM contacts WHERE email = ?",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| Contact {
            id: row.get("id"),
            email: row.get("email"),
            name: row.get("name"),
            last_contacted: row.get("last_contacted"),
            contact_count: row.get("contact_count"),
        }))
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM contacts WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_name(&self, id: i64, name: &str) -> Result<()> {
        let name = if name.is_empty() { None } else { Some(name) };
        sqlx::query("UPDATE contacts SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> ContactsDb {
        ContactsDb::open_in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn test_add_contact() {
        let db = test_db().await;

        db.add_or_update("test@example.com", Some("Test User"))
            .await
            .unwrap();

        let contacts = db.get_all().await.unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].email, "test@example.com");
        assert_eq!(contacts[0].name, Some("Test User".to_string()));
    }

    #[tokio::test]
    async fn test_contact_count_increment() {
        let db = test_db().await;

        db.add_or_update("test@example.com", Some("Test"))
            .await
            .unwrap();
        db.add_or_update("test@example.com", None).await.unwrap();

        let contact = db.get_by_email("test@example.com").await.unwrap().unwrap();
        assert_eq!(contact.contact_count, 2);
    }

    #[tokio::test]
    async fn test_search() {
        let db = test_db().await;

        db.add_or_update("alice@example.com", Some("Alice"))
            .await
            .unwrap();
        db.add_or_update("bob@example.com", Some("Bob"))
            .await
            .unwrap();

        let results = db.search("alice").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].email, "alice@example.com");
    }
}
