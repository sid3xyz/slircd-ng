use async_trait::async_trait;
use super::{HistoryProvider, HistoryError, HistoryQuery, StoredMessage};
use redb::{Database, TableDefinition, ReadableTable, ReadableDatabase};
use std::sync::Arc;
use std::time::Duration;
use slirc_proto::irc_to_lower;

const HISTORY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("history");
const MSGID_INDEX: TableDefinition<&str, &[u8]> = TableDefinition::new("msgid_index");

pub struct RedbProvider {
    db: Arc<Database>,
}

impl RedbProvider {
    pub fn new(path: &str) -> Result<Self, HistoryError> {
        let db = Database::create(path).map_err(|e| HistoryError::Database(e.to_string()))?;
        Ok(Self { db: Arc::new(db) })
    }

    fn make_key(target: &str, timestamp: i64, msgid: &str) -> String {
        let target_lower = irc_to_lower(target);
        // Format timestamp as fixed-width string for lexicographical sorting
        format!("{}\0{:020}\0{}", target_lower, timestamp, msgid)
    }
}

#[async_trait]
impl HistoryProvider for RedbProvider {
    async fn store(&self, target: &str, msg: StoredMessage) -> Result<(), HistoryError> {
        let key = Self::make_key(target, msg.nanotime, &msg.msgid);
        let value = serde_json::to_vec(&msg).map_err(|e| HistoryError::Serialization(e.to_string()))?;

        let write_txn = self.db.begin_write().map_err(|e| HistoryError::Database(e.to_string()))?;
        {
            let mut table = write_txn.open_table(HISTORY_TABLE).map_err(|e| HistoryError::Database(e.to_string()))?;
            table.insert(key.as_str(), value.as_slice()).map_err(|e| HistoryError::Database(e.to_string()))?;

            let mut index = write_txn.open_table(MSGID_INDEX).map_err(|e| HistoryError::Database(e.to_string()))?;
            // Value: target\0timestamp
            let index_val = format!("{}\0{}", target, msg.nanotime);
            index.insert(msg.msgid.as_str(), index_val.as_bytes()).map_err(|e| HistoryError::Database(e.to_string()))?;
        }
        write_txn.commit().map_err(|e| HistoryError::Database(e.to_string()))?;
        Ok(())
    }

    async fn query(&self, filter: HistoryQuery) -> Result<Vec<StoredMessage>, HistoryError> {
        let read_txn = self.db.begin_read().map_err(|e| HistoryError::Database(e.to_string()))?;
        let table = read_txn.open_table(HISTORY_TABLE).map_err(|e| HistoryError::Database(e.to_string()))?;

        let target_lower = irc_to_lower(&filter.target);
        let start_key = format!("{}\0{:020}\0", target_lower, filter.start.unwrap_or(0));
        let end_key = format!("{}\0{:020}\0\u{FFFF}", target_lower, filter.end.unwrap_or(i64::MAX));

        let range = table.range(start_key.as_str()..end_key.as_str()).map_err(|e| HistoryError::Database(e.to_string()))?;

        let mut messages = Vec::new();

        if filter.reverse {
             for item in range.rev() {
                 if messages.len() >= filter.limit {
                     break;
                 }
                 let (_k, v) = item.map_err(|e| HistoryError::Database(e.to_string()))?;
                 let msg: StoredMessage = serde_json::from_slice(v.value()).map_err(|e| HistoryError::Serialization(e.to_string()))?;
                 messages.push(msg);
             }
        } else {
             for item in range {
                 if messages.len() >= filter.limit {
                     break;
                 }
                 let (_k, v) = item.map_err(|e| HistoryError::Database(e.to_string()))?;
                 let msg: StoredMessage = serde_json::from_slice(v.value()).map_err(|e| HistoryError::Serialization(e.to_string()))?;
                 messages.push(msg);
             }
        }

        Ok(messages)
    }

    async fn prune(&self, retention: Duration) -> Result<usize, HistoryError> {
        let cutoff = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) - retention.as_nanos() as i64;

        let write_txn = self.db.begin_write().map_err(|e| HistoryError::Database(e.to_string()))?;
        let mut count = 0;
        {
            let mut table = write_txn.open_table(HISTORY_TABLE).map_err(|e| HistoryError::Database(e.to_string()))?;

            let mut to_delete = Vec::new();
            for item in table.iter().map_err(|e| HistoryError::Database(e.to_string()))? {
                let (k, _v) = item.map_err(|e| HistoryError::Database(e.to_string()))?;
                let key_str = k.value();
                let parts: Vec<&str> = key_str.split('\0').collect();
                if parts.len() >= 2
                    && let Ok(ts) = parts[1].parse::<i64>()
                    && ts < cutoff
                {
                    to_delete.push(key_str.to_string());
                }
            }

            for k in to_delete {
                table.remove(k.as_str()).map_err(|e| HistoryError::Database(e.to_string()))?;
                count += 1;
            }
        }
        write_txn.commit().map_err(|e| HistoryError::Database(e.to_string()))?;
        Ok(count)
    }

    async fn purge(&self, target: Option<&str>) -> Result<(), HistoryError> {
        let write_txn = self.db.begin_write().map_err(|e| HistoryError::Database(e.to_string()))?;
        {
            let mut table = write_txn.open_table(HISTORY_TABLE).map_err(|e| HistoryError::Database(e.to_string()))?;
            if let Some(t) = target {
                let target_lower = irc_to_lower(t);
                let start_key = format!("{}\0", target_lower);
                let end_key = format!("{}\0\u{FFFF}", target_lower);

                let mut to_delete = Vec::new();
                for item in table.range(start_key.as_str()..end_key.as_str()).map_err(|e| HistoryError::Database(e.to_string()))? {
                     let (k, _) = item.map_err(|e| HistoryError::Database(e.to_string()))?;
                     to_delete.push(k.value().to_string());
                }
                for k in to_delete {
                    table.remove(k.as_str()).map_err(|e| HistoryError::Database(e.to_string()))?;
                }
            } else {
                 let mut to_delete = Vec::new();
                for item in table.iter().map_err(|e| HistoryError::Database(e.to_string()))? {
                     let (k, _) = item.map_err(|e| HistoryError::Database(e.to_string()))?;
                     to_delete.push(k.value().to_string());
                }
                for k in to_delete {
                    table.remove(k.as_str()).map_err(|e| HistoryError::Database(e.to_string()))?;
                }
            }
        }
        write_txn.commit().map_err(|e| HistoryError::Database(e.to_string()))?;
        Ok(())
    }

    async fn lookup_timestamp(&self, _target: &str, msgid: &str) -> Result<Option<i64>, HistoryError> {
        let read_txn = self.db.begin_read().map_err(|e| HistoryError::Database(e.to_string()))?;
        let index = read_txn.open_table(MSGID_INDEX).map_err(|e| HistoryError::Database(e.to_string()))?;

        if let Some(v) = index.get(msgid).map_err(|e| HistoryError::Database(e.to_string()))? {
            let val_str = std::str::from_utf8(v.value()).map_err(|e| HistoryError::Serialization(e.to_string()))?;
            let parts: Vec<&str> = val_str.split('\0').collect();
            if parts.len() >= 2
                && let Ok(ts) = parts[1].parse::<i64>()
            {
                return Ok(Some(ts));
            }
        }
        Ok(None)
    }

    async fn query_targets(&self, start: i64, end: i64, limit: usize, candidates: Vec<String>) -> Result<Vec<(String, i64)>, HistoryError> {
        let read_txn = self.db.begin_read().map_err(|e| HistoryError::Database(e.to_string()))?;
        let table = read_txn.open_table(HISTORY_TABLE).map_err(|e| HistoryError::Database(e.to_string()))?;

        let mut results = Vec::new();

        for target in candidates {
            let target_lower = irc_to_lower(&target);
            let start_key = format!("{}\0{:020}\0", target_lower, start);
            let end_key = format!("{}\0{:020}\0\u{FFFF}", target_lower, end);

            let mut range = table.range(start_key.as_str()..end_key.as_str()).map_err(|e| HistoryError::Database(e.to_string()))?;

            if let Some(item) = range.next_back() {
                let (k, _) = item.map_err(|e| HistoryError::Database(e.to_string()))?;
                let key_str = k.value();
                let parts: Vec<&str> = key_str.split('\0').collect();
                if parts.len() >= 2
                    && let Ok(ts) = parts[1].parse::<i64>()
                {
                    results.push((target, ts));
                }
            }
        }

        results.sort_by(|a, b| b.1.cmp(&a.1));
        results.truncate(limit);

        Ok(results)
    }
}
