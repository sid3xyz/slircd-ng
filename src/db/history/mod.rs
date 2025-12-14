//! Message history storage for CHATHISTORY command (IRCv3 draft/chathistory).
//!
//! Provides persistent message storage for channel history retrieval.
//!
//! # Reference
//! - IRCv3 chathistory: <https://ircv3.net/specs/extensions/chathistory>
//!
//! # Architecture
//! - Uses sqlx async SQLite (consistent with rest of db layer)
//! - JSON message envelope for flexible schema evolution
//! - Nanosecond timestamps for precise ordering
//! - Storage is async and non-blocking

mod types;
mod storage;
mod queries;

pub use types::{MessageEnvelope, StoreMessageParams, StoredMessage};

use crate::db::DbError;
use sqlx::SqlitePool;

/// History repository for message storage and retrieval.
pub struct HistoryRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> HistoryRepository<'a> {
    /// Create a new history repository.
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Store a channel message in history.
    ///
    /// Uses nanosecond timestamps for precise ordering.
    /// Idempotent: duplicate msgids are ignored.
    pub async fn store_message(&self, params: StoreMessageParams<'_>) -> Result<(), DbError> {
        storage::store_message(self.pool, params).await
    }

    /// Fetch a single message by ID.
    pub async fn get_message_by_id(&self, msgid: &str) -> Result<Option<StoredMessage>, DbError> {
        storage::get_message_by_id(self.pool, msgid).await
    }

    /// Query targets (channels and DMs) with activity between start and end.
    /// Returns list of (target_name, last_timestamp).
    pub async fn query_targets(
        &self,
        nick: &str,
        channels: &[String],
        start: i64,
        end: i64,
        limit: usize,
    ) -> Result<Vec<(String, i64)>, DbError> {
        storage::query_targets(self.pool, nick, channels, start, end, limit).await
    }

    /// Prune old messages based on retention policy.
    ///
    /// Called by scheduled maintenance task in main.rs (runs daily).
    pub async fn prune_old_messages(&self, retention_days: u32) -> Result<u64, DbError> {
        storage::prune_old_messages(self.pool, retention_days).await
    }

    /// Query most recent N messages (CHATHISTORY LATEST).
    pub async fn query_latest(
        &self,
        target: &str,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        queries::query_latest(self.pool, target, limit).await
    }

    /// Query most recent N messages after a timestamp (CHATHISTORY LATEST with lower bound).
    pub async fn query_latest_after(
        &self,
        target: &str,
        after_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        queries::query_latest_after(self.pool, target, after_nanos, limit).await
    }

    /// Query messages before a timestamp (CHATHISTORY BEFORE).
    pub async fn query_before(
        &self,
        target: &str,
        before_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        queries::query_before(self.pool, target, before_nanos, limit).await
    }

    /// Query messages after a timestamp (CHATHISTORY AFTER).
    pub async fn query_after(
        &self,
        target: &str,
        after_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        queries::query_after(self.pool, target, after_nanos, limit).await
    }

    /// Query messages between two timestamps (CHATHISTORY BETWEEN).
    pub async fn query_between(
        &self,
        target: &str,
        start_nanos: i64,
        end_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        queries::query_between(self.pool, target, start_nanos, end_nanos, limit).await
    }

    /// Query messages between two timestamps (CHATHISTORY BETWEEN) in reverse order.
    pub async fn query_between_desc(
        &self,
        target: &str,
        start_nanos: i64,
        end_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        queries::query_between_desc(self.pool, target, start_nanos, end_nanos, limit).await
    }

    /// Query DM history between two users (LATEST).
    pub async fn query_dm_latest(
        &self,
        user1: &str,
        user1_account: Option<&str>,
        user2: &str,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        queries::query_dm_latest(self.pool, user1, user1_account, user2, limit).await
    }

    /// Query DM history between two users (LATEST with lower bound).
    pub async fn query_dm_latest_after(
        &self,
        user1: &str,
        user1_account: Option<&str>,
        user2: &str,
        after_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        queries::query_dm_latest_after(self.pool, user1, user1_account, user2, after_nanos, limit).await
    }

    /// Query DM history between two users (BEFORE).
    pub async fn query_dm_before(
        &self,
        user1: &str,
        user1_account: Option<&str>,
        user2: &str,
        before_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        queries::query_dm_before(self.pool, user1, user1_account, user2, before_nanos, limit).await
    }

    /// Query DM history between two users (AFTER).
    pub async fn query_dm_after(
        &self,
        user1: &str,
        user1_account: Option<&str>,
        user2: &str,
        after_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        queries::query_dm_after(self.pool, user1, user1_account, user2, after_nanos, limit).await
    }

    /// Query DM history between two users (BETWEEN).
    pub async fn query_dm_between(
        &self,
        user1: &str,
        user1_account: Option<&str>,
        user2: &str,
        start_nanos: i64,
        end_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        queries::query_dm_between(self.pool, user1, user1_account, user2, start_nanos, end_nanos, limit).await
    }

    /// Query DM history between two users (BETWEEN) in reverse order.
    pub async fn query_dm_between_desc(
        &self,
        user1: &str,
        user1_account: Option<&str>,
        user2: &str,
        start_nanos: i64,
        end_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        queries::query_dm_between_desc(self.pool, user1, user1_account, user2, start_nanos, end_nanos, limit).await
    }

    /// Lookup msgid and return its nanotime.
    pub async fn lookup_msgid_nanotime(
        &self,
        target: &str,
        msgid: &str,
    ) -> Result<Option<i64>, DbError> {
        queries::lookup_msgid_nanotime(self.pool, target, msgid).await
    }

    /// Lookup msgid for DM and return its nanotime.
    pub async fn lookup_dm_msgid_nanotime(
        &self,
        user1: &str,
        user2: &str,
        msgid: &str,
    ) -> Result<Option<i64>, DbError> {
        queries::lookup_dm_msgid_nanotime(self.pool, user1, user2, msgid).await
    }
}
