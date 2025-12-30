//! Database schema - now managed by sqlx in db.rs
//! This file is kept for backwards compatibility and tests

#[cfg(test)]
mod tests {
    use super::super::Cache;

    #[tokio::test]
    async fn test_schema_creation() {
        let cache = Cache::open_in_memory().await.unwrap();

        // Verify tables exist by querying them
        let emails_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM emails")
            .fetch_one(cache.pool())
            .await
            .unwrap();
        assert_eq!(emails_count, 0);

        let bodies_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM email_bodies")
            .fetch_one(cache.pool())
            .await
            .unwrap();
        assert_eq!(bodies_count, 0);

        let contacts_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts")
            .fetch_one(cache.pool())
            .await
            .unwrap();
        assert_eq!(contacts_count, 0);
    }

    #[tokio::test]
    async fn test_schema_version() {
        // Schema version 2 supports multi-account with account_id columns
        // This test verifies the schema is at version 2 by checking for account_id column
        let cache = Cache::open_in_memory().await.unwrap();
        // If this query works, schema is v2+ (has account_id column)
        sqlx::query("SELECT account_id FROM emails LIMIT 0")
            .execute(cache.pool())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_account_id_columns() {
        let cache = Cache::open_in_memory().await.unwrap();

        // Verify account_id columns exist by inserting test data
        sqlx::query("INSERT INTO emails (uid, account_id, subject, from_addr, date) VALUES (1, 'test@example.com', 'Test', 'sender@example.com', 1000)")
            .execute(cache.pool())
            .await
            .unwrap();

        let result: (i64, String) =
            sqlx::query_as("SELECT uid, account_id FROM emails WHERE uid = 1")
                .fetch_one(cache.pool())
                .await
                .unwrap();
        assert_eq!(result.0, 1);
        assert_eq!(result.1, "test@example.com");
    }

    #[tokio::test]
    async fn test_multi_account_emails() {
        let cache = Cache::open_in_memory().await.unwrap();

        // Insert emails for two different accounts with same UID
        sqlx::query("INSERT INTO emails (uid, account_id, subject, from_addr, date) VALUES (1, 'user1@example.com', 'Test 1', 'sender@example.com', 1000)")
            .execute(cache.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO emails (uid, account_id, subject, from_addr, date) VALUES (1, 'user2@example.com', 'Test 2', 'sender@example.com', 2000)")
            .execute(cache.pool())
            .await
            .unwrap();

        // Same UID can exist for different accounts
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM emails WHERE uid = 1")
            .fetch_one(cache.pool())
            .await
            .unwrap();
        assert_eq!(count, 2);
    }
}
