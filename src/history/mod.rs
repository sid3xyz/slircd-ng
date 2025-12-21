//! History provider abstraction.

use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;

pub mod noop;
pub mod redb;
pub mod types;

pub use types::{MessageEnvelope, StoredMessage};

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[derive(Debug, Clone)]
pub struct HistoryQuery {
    pub target: String,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub limit: usize,
    pub reverse: bool,
}

#[async_trait]
pub trait HistoryProvider: Send + Sync {
    /// Store a message. Returns immediately (fire-and-forget).
    async fn store(&self, target: &str, msg: StoredMessage) -> Result<(), HistoryError>;

    /// Retrieve messages (Range Query).
    async fn query(&self, filter: HistoryQuery) -> Result<Vec<StoredMessage>, HistoryError>;

    /// Prune old messages (Maintenance).
    async fn prune(&self, retention: Duration) -> Result<usize, HistoryError>;

    /// Lookup timestamp for a message ID.
    async fn lookup_timestamp(
        &self,
        target: &str,
        msgid: &str,
    ) -> Result<Option<i64>, HistoryError>;

    /// Query targets with activity.
    async fn query_targets(
        &self,
        start: i64,
        end: i64,
        limit: usize,
        nick: String,
        channels: Vec<String>,
    ) -> Result<Vec<(String, i64)>, HistoryError>;
}
