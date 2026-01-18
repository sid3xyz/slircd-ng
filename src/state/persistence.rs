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
        .bind(&state.topic_set_at)
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
