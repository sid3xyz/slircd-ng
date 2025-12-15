use async_trait::async_trait;
use super::{HistoryProvider, HistoryError, HistoryQuery, StoredMessage};
use std::time::Duration;

pub struct NoOpProvider;

#[async_trait]
impl HistoryProvider for NoOpProvider {
    async fn store(&self, _target: &str, _msg: StoredMessage) -> Result<(), HistoryError> {
        Ok(())
    }

    async fn query(&self, _filter: HistoryQuery) -> Result<Vec<StoredMessage>, HistoryError> {
        Ok(vec![])
    }

    async fn prune(&self, _retention: Duration) -> Result<usize, HistoryError> {
        Ok(0)
    }

    async fn purge(&self, _target: Option<&str>) -> Result<(), HistoryError> {
        Ok(())
    }

    async fn lookup_timestamp(&self, _target: &str, _msgid: &str) -> Result<Option<i64>, HistoryError> {
        Ok(None)
    }

    async fn query_targets(&self, _start: i64, _end: i64, _limit: usize, _candidates: Vec<String>) -> Result<Vec<(String, i64)>, HistoryError> {
        Ok(vec![])
    }
}
