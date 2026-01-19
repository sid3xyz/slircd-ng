//! No-op history provider that discards all messages.
//!
//! Used when history storage is disabled or unavailable.
//! All operations succeed but store nothing.

use super::{HistoryError, HistoryProvider, HistoryQuery, StoredMessage, types::HistoryItem};
use async_trait::async_trait;
use std::time::Duration;

pub struct NoOpProvider;

#[async_trait]
impl HistoryProvider for NoOpProvider {
    async fn store(&self, _target: &str, _msg: StoredMessage) -> Result<(), HistoryError> {
        Ok(())
    }

    async fn store_item(&self, _target: &str, _item: HistoryItem) -> Result<(), HistoryError> {
        Ok(())
    }

    async fn query(&self, _filter: HistoryQuery) -> Result<Vec<HistoryItem>, HistoryError> {
        Ok(vec![])
    }

    async fn prune(&self, _retention: Duration) -> Result<usize, HistoryError> {
        Ok(0)
    }

    async fn lookup_timestamp(
        &self,
        _target: &str,
        _msgid: &str,
    ) -> Result<Option<i64>, HistoryError> {
        Ok(None)
    }

    async fn query_targets(
        &self,
        _start: i64,
        _end: i64,
        _limit: usize,
        _nick: String,
        _channels: Vec<String>,
    ) -> Result<Vec<(String, i64)>, HistoryError> {
        Ok(vec![])
    }
}
