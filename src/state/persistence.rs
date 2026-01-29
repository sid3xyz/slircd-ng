//! Channel state persistence.
//!
//! Saves and restores channel state across server restarts.

use crate::db::DbError;
use sqlx::SqlitePool;

/// A channel's persistent state.
#[derive(Debug, Clone)]
pub struct ChannelState {
    pub name: String,
    pub modes: String,
    pub topic: Option<String>,
    pub topic_set_by: Option<String>,
    pub topic_set_at: Option<i64>,
    pub created_at: i64,
    pub key: Option<String>,
    pub user_limit: Option<i32>,
}

/// Repository for channel state persistence.
pub struct ChannelStateRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> ChannelStateRepository<'a> {
    /// Create a new channel state repository.
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Save channel state to database.
    pub async fn save(&self, state: &ChannelState) -> Result<(), DbError> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO channel_state
            (name, modes, topic, topic_set_by, topic_set_at, created_at, key, user_limit)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&state.name)
        .bind(&state.modes)
        .bind(&state.topic)
        .bind(&state.topic_set_by)
        .bind(state.topic_set_at)
        .bind(state.created_at)
        .bind(&state.key)
        .bind(state.user_limit)
        .execute(self.pool)
        .await?;
        Ok(())
    }

    /// Delete channel state from database.
    pub async fn delete(&self, name: &str) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM channel_state WHERE name = ?")
            .bind(name)
            .execute(self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Load all channel states from database.
    pub async fn load_all(&self) -> Result<Vec<ChannelState>, DbError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                Option<String>,
                Option<String>,
                Option<i64>,
                i64,
                Option<String>,
                Option<i32>,
            ),
        >(
            r#"
            SELECT name, modes, topic, topic_set_by, topic_set_at, created_at, key, user_limit
            FROM channel_state
            "#,
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(name, modes, topic, topic_set_by, topic_set_at, created_at, key, user_limit)| {
                    ChannelState {
                        name,
                        modes,
                        topic,
                        topic_set_by,
                        topic_set_at,
                        created_at,
                        key,
                        user_limit,
                    }
                },
            )
            .collect())
    }

    /// Find channel state by name.
    #[allow(dead_code)]
    pub async fn find_by_name(&self, name: &str) -> Result<Option<ChannelState>, DbError> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                Option<String>,
                Option<String>,
                Option<i64>,
                i64,
                Option<String>,
                Option<i32>,
            ),
        >(
            r#"
            SELECT name, modes, topic, topic_set_by, topic_set_at, created_at, key, user_limit
            FROM channel_state
            WHERE name = ? COLLATE NOCASE
            "#,
        )
        .bind(name)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(
            |(name, modes, topic, topic_set_by, topic_set_at, created_at, key, user_limit)| {
                ChannelState {
                    name,
                    modes,
                    topic,
                    topic_set_by,
                    topic_set_at,
                    created_at,
                    key,
                    user_limit,
                }
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn test_channel_persistence_cycle() {
        // Setup in-memory DB
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // Run migration manually for test
        sqlx::query(
            "CREATE TABLE channel_state (
                name TEXT PRIMARY KEY NOT NULL,
                modes TEXT NOT NULL DEFAULT '',
                topic TEXT,
                topic_set_by TEXT,
                topic_set_at INTEGER,
                created_at INTEGER NOT NULL DEFAULT 0,
                key TEXT,
                user_limit INTEGER
            );",
        )
        .execute(&pool)
        .await
        .unwrap();

        let repo = ChannelStateRepository::new(&pool);
        let now = chrono::Utc::now().timestamp();

        let state = ChannelState {
            name: "#test".to_string(),
            modes: "+ntk".to_string(),
            topic: Some("Hello World".to_string()),
            topic_set_by: Some("Admin".to_string()),
            topic_set_at: Some(now),
            created_at: now,
            key: Some("secret".to_string()),
            user_limit: Some(10),
        };

        // Test Save
        repo.save(&state).await.expect("Failed to save state");

        // Test Find
        let found = repo
            .find_by_name("#test")
            .await
            .expect("Failed to find state")
            .expect("State not found");

        assert_eq!(found.name, "#test");
        assert_eq!(found.modes, "+ntk");
        assert_eq!(found.topic.as_deref(), Some("Hello World"));
        assert_eq!(found.key.as_deref(), Some("secret"));
        assert_eq!(found.user_limit, Some(10));

        // Test Load All
        let all = repo.load_all().await.expect("Failed to load all");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "#test");

        // Test Delete
        let deleted = repo.delete("#test").await.expect("Failed to delete");
        assert!(deleted);

        let missing = repo
            .find_by_name("#test")
            .await
            .expect("Failed to check missing");
        assert!(missing.is_none());
    }
}
