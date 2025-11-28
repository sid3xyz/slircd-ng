//! Channel repository for ChanServ functionality.
//!
//! Handles channel registration, access lists, and settings.

use super::DbError;
use sqlx::SqlitePool;

/// A registered ChanServ channel.
#[derive(Debug, Clone)]
pub struct ChannelRecord {
    pub id: i64,
    pub name: String,
    pub founder_account_id: i64,
    pub registered_at: i64,
    pub last_used_at: i64,
    pub description: Option<String>,
    pub mlock: Option<String>,
    pub keeptopic: bool,
}

/// Channel access entry.
#[derive(Debug, Clone)]
pub struct ChannelAccess {
    #[allow(dead_code)] // Stored for potential future use in access queries
    pub channel_id: i64,
    pub account_id: i64,
    pub flags: String,
    pub added_by: String,
    pub added_at: i64,
}

/// Repository for channel operations.
pub struct ChannelRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> ChannelRepository<'a> {
    /// Create a new channel repository.
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Register a new channel.
    pub async fn register(
        &self,
        name: &str,
        founder_account_id: i64,
        description: Option<&str>,
    ) -> Result<ChannelRecord, DbError> {
        // Check if channel is already registered
        if self.find_by_name(name).await?.is_some() {
            return Err(DbError::ChannelExists(name.to_string()));
        }

        let now = chrono::Utc::now().timestamp();

        let result = sqlx::query(
            r#"
            INSERT INTO channels (name, founder_account_id, registered_at, last_used_at, description)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(name)
        .bind(founder_account_id)
        .bind(now)
        .bind(now)
        .bind(description)
        .execute(self.pool)
        .await?;

        let channel_id = result.last_insert_rowid();

        // Grant founder full access (+F)
        sqlx::query(
            r#"
            INSERT INTO channel_access (channel_id, account_id, flags, added_by, added_at)
            VALUES (?, ?, '+F', 'ChanServ', ?)
            "#,
        )
        .bind(channel_id)
        .bind(founder_account_id)
        .bind(now)
        .execute(self.pool)
        .await?;

        Ok(ChannelRecord {
            id: channel_id,
            name: name.to_string(),
            founder_account_id,
            registered_at: now,
            last_used_at: now,
            description: description.map(String::from),
            mlock: None,
            keeptopic: true,
        })
    }

    /// Find channel by name.
    pub async fn find_by_name(&self, name: &str) -> Result<Option<ChannelRecord>, DbError> {
        let row = sqlx::query_as::<_, (i64, String, i64, i64, i64, Option<String>, Option<String>, bool)>(
            r#"
            SELECT id, name, founder_account_id, registered_at, last_used_at, description, mlock, keeptopic
            FROM channels
            WHERE name = ? COLLATE NOCASE
            "#,
        )
        .bind(name)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(|(id, name, founder_account_id, registered_at, last_used_at, description, mlock, keeptopic)| {
            ChannelRecord {
                id,
                name,
                founder_account_id,
                registered_at,
                last_used_at,
                description,
                mlock,
                keeptopic,
            }
        }))
    }

    /// Find channel by ID.
    #[allow(dead_code)]
    pub async fn find_by_id(&self, id: i64) -> Result<Option<ChannelRecord>, DbError> {
        let row = sqlx::query_as::<_, (i64, String, i64, i64, i64, Option<String>, Option<String>, bool)>(
            r#"
            SELECT id, name, founder_account_id, registered_at, last_used_at, description, mlock, keeptopic
            FROM channels
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(|(id, name, founder_account_id, registered_at, last_used_at, description, mlock, keeptopic)| {
            ChannelRecord {
                id,
                name,
                founder_account_id,
                registered_at,
                last_used_at,
                description,
                mlock,
                keeptopic,
            }
        }))
    }

    /// Get all channels registered by an account.
    #[allow(dead_code)]
    pub async fn find_by_founder(&self, founder_account_id: i64) -> Result<Vec<ChannelRecord>, DbError> {
        let rows = sqlx::query_as::<_, (i64, String, i64, i64, i64, Option<String>, Option<String>, bool)>(
            r#"
            SELECT id, name, founder_account_id, registered_at, last_used_at, description, mlock, keeptopic
            FROM channels
            WHERE founder_account_id = ?
            "#,
        )
        .bind(founder_account_id)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|(id, name, founder_account_id, registered_at, last_used_at, description, mlock, keeptopic)| {
            ChannelRecord {
                id,
                name,
                founder_account_id,
                registered_at,
                last_used_at,
                description,
                mlock,
                keeptopic,
            }
        }).collect())
    }

    /// Get access flags for an account on a channel.
    pub async fn get_access(
        &self,
        channel_id: i64,
        account_id: i64,
    ) -> Result<Option<ChannelAccess>, DbError> {
        let row = sqlx::query_as::<_, (i64, i64, String, String, i64)>(
            r#"
            SELECT channel_id, account_id, flags, added_by, added_at
            FROM channel_access
            WHERE channel_id = ? AND account_id = ?
            "#,
        )
        .bind(channel_id)
        .bind(account_id)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.map(|(channel_id, account_id, flags, added_by, added_at)| ChannelAccess {
            channel_id,
            account_id,
            flags,
            added_by,
            added_at,
        }))
    }

    /// Get all access entries for a channel.
    pub async fn list_access(&self, channel_id: i64) -> Result<Vec<ChannelAccess>, DbError> {
        let rows = sqlx::query_as::<_, (i64, i64, String, String, i64)>(
            r#"
            SELECT channel_id, account_id, flags, added_by, added_at
            FROM channel_access
            WHERE channel_id = ?
            ORDER BY added_at ASC
            "#,
        )
        .bind(channel_id)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().map(|(channel_id, account_id, flags, added_by, added_at)| {
            ChannelAccess {
                channel_id,
                account_id,
                flags,
                added_by,
                added_at,
            }
        }).collect())
    }

    /// Set access flags for an account on a channel.
    pub async fn set_access(
        &self,
        channel_id: i64,
        account_id: i64,
        flags: &str,
        added_by: &str,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().timestamp();

        // Use REPLACE to upsert
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO channel_access (channel_id, account_id, flags, added_by, added_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(channel_id)
        .bind(account_id)
        .bind(flags)
        .bind(added_by)
        .bind(now)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Remove access for an account on a channel.
    pub async fn remove_access(
        &self,
        channel_id: i64,
        account_id: i64,
    ) -> Result<bool, DbError> {
        let result = sqlx::query(
            r#"
            DELETE FROM channel_access
            WHERE channel_id = ? AND account_id = ?
            "#,
        )
        .bind(channel_id)
        .bind(account_id)
        .execute(self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Update channel settings.
    pub async fn set_option(
        &self,
        channel_id: i64,
        option: &str,
        value: &str,
    ) -> Result<(), DbError> {
        match option.to_lowercase().as_str() {
            "description" | "desc" => {
                sqlx::query("UPDATE channels SET description = ? WHERE id = ?")
                    .bind(value)
                    .bind(channel_id)
                    .execute(self.pool)
                    .await?;
            }
            "mlock" => {
                sqlx::query("UPDATE channels SET mlock = ? WHERE id = ?")
                    .bind(value)
                    .bind(channel_id)
                    .execute(self.pool)
                    .await?;
            }
            "keeptopic" => {
                let keep = matches!(value.to_lowercase().as_str(), "on" | "true" | "1" | "yes");
                sqlx::query("UPDATE channels SET keeptopic = ? WHERE id = ?")
                    .bind(keep)
                    .bind(channel_id)
                    .execute(self.pool)
                    .await?;
            }
            _ => {
                return Err(DbError::UnknownOption(option.to_string()));
            }
        }
        Ok(())
    }

    /// Update last used timestamp.
    #[allow(dead_code)]
    pub async fn touch(&self, channel_id: i64) -> Result<(), DbError> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE channels SET last_used_at = ? WHERE id = ?")
            .bind(now)
            .bind(channel_id)
            .execute(self.pool)
            .await?;
        Ok(())
    }

    /// Drop (unregister) a channel.
    pub async fn drop_channel(&self, channel_id: i64) -> Result<bool, DbError> {
        // Access entries are deleted via CASCADE
        let result = sqlx::query("DELETE FROM channels WHERE id = ?")
            .bind(channel_id)
            .execute(self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Check if account has specific flag on channel.
    pub fn has_flag(flags: &str, flag: char) -> bool {
        flags.contains(flag)
    }

    /// Check if account is founder (+F).
    pub fn is_founder(flags: &str) -> bool {
        Self::has_flag(flags, 'F')
    }

    /// Check if account has operator access (+o or +F).
    #[allow(dead_code)]
    pub fn has_op_access(flags: &str) -> bool {
        Self::has_flag(flags, 'o') || Self::has_flag(flags, 'F')
    }

    /// Check if account has voice access (+v, +o, or +F).
    #[allow(dead_code)]
    pub fn has_voice_access(flags: &str) -> bool {
        Self::has_flag(flags, 'v') || Self::has_op_access(flags)
    }
}
