//! Redb-backed persistent history storage.
//!
//! Implements [`HistoryProvider`] using the redb embedded database for
//! durable message history with efficient range queries by target and time.

use super::{HistoryError, HistoryProvider, HistoryQuery, StoredMessage, types::{HistoryItem, StoredEvent}};
use async_trait::async_trait;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use slirc_proto::irc_to_lower;
use std::sync::Arc;
use std::time::Duration;

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

    /// Get a clone of the underlying database Arc for sharing with other components.
    pub fn database(&self) -> Arc<Database> {
        Arc::clone(&self.db)
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
        self.store_item(target, HistoryItem::Message(msg)).await
    }

    async fn store_item(&self, target: &str, item: HistoryItem) -> Result<(), HistoryError> {
        let (nanotime, id) = match &item {
            HistoryItem::Message(m) => (m.nanotime, &m.msgid),
            HistoryItem::Event(e) => (e.nanotime, &e.id),
        };

        let key = Self::make_key(target, nanotime, id);
        let value =
            serde_json::to_vec(&item).map_err(|e| HistoryError::Serialization(e.to_string()))?;

        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| HistoryError::Database(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(HISTORY_TABLE)
                .map_err(|e| HistoryError::Database(e.to_string()))?;
            table
                .insert(key.as_str(), value.as_slice())
                .map_err(|e| HistoryError::Database(e.to_string()))?;

            let mut index = write_txn
                .open_table(MSGID_INDEX)
                .map_err(|e| HistoryError::Database(e.to_string()))?;
            // Value: target\0timestamp
            let index_val = format!("{}\0{}", target, nanotime);
            index
                .insert(id.as_str(), index_val.as_bytes())
                .map_err(|e| HistoryError::Database(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| HistoryError::Database(e.to_string()))?;
        Ok(())
    }

    async fn query(&self, filter: HistoryQuery) -> Result<Vec<HistoryItem>, HistoryError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| HistoryError::Database(e.to_string()))?;
        let table = read_txn
            .open_table(HISTORY_TABLE)
            .map_err(|e| HistoryError::Database(e.to_string()))?;

        let target_lower = irc_to_lower(&filter.target);

        // Precise paging logic:
        // Key format: target\0timestamp\0msgid
        //
        // If start_id/end_id are provided, use them to form a precise boundary key.
        // Otherwise, use the default boundary (start of timestamp / end of timestamp).

        let start_key = if let (Some(ts), Some(id)) = (filter.start, &filter.start_id) {
            // Precise start: target\0timestamp\0msgid
            format!("{}\0{:020}\0{}", target_lower, ts, id)
        } else {
            // Loose start: target\0timestamp\0 (start of this millisecond)
            format!("{}\0{:020}\0", target_lower, filter.start.unwrap_or(0))
        };

        let end_key = if let (Some(ts), Some(id)) = (filter.end, &filter.end_id) {
            // Precise end: target\0timestamp\0msgid
            format!("{}\0{:020}\0{}", target_lower, ts, id)
        } else {
            // Loose end: target\0timestamp\0 (end of global)
            format!(
                "{}\0{:020}\0",
                target_lower,
                filter.end.unwrap_or(i64::MAX)
            )
        };

        let range = table
            .range(start_key.as_str()..end_key.as_str())
            .map_err(|e| HistoryError::Database(e.to_string()))?;

        // Diagnostic: log the actual first key in range if exists
        #[cfg(debug_assertions)]
        {
            if filter.start_id.is_some() {
                tracing::error!(
                    target: "history_debug",
                    start_key = %start_key,
                    end_key = %end_key,
                    start_ts = ?filter.start,
                    start_id = ?filter.start_id,
                    "Query range bounds"
                );
            }
        }

        let mut messages = Vec::with_capacity(filter.limit);

        if filter.reverse {
            for item in range.rev() {
                if messages.len() >= filter.limit {
                    break;
                }
                let (_k, v) = item.map_err(|e| HistoryError::Database(e.to_string()))?;
                
                // Try deserializing as generic HistoryItem first
                let item: HistoryItem = match serde_json::from_slice(v.value()) {
                    Ok(item) => item,
                    Err(_) => {
                        // Fallback: try legacy StoredMessage and wrap it
                        let msg: StoredMessage = serde_json::from_slice(v.value())
                            .map_err(|e| HistoryError::Serialization(format!("Corrupt or unknown history format: {}", e)))?;
                        HistoryItem::Message(msg)
                    }
                };
                messages.push(item);
            }
        } else {
            for item in range {
                if messages.len() >= filter.limit {
                    break;
                }
                let (_k, v) = item.map_err(|e| HistoryError::Database(e.to_string()))?;
                
                // Try deserializing as generic HistoryItem first
                let item: HistoryItem = match serde_json::from_slice(v.value()) {
                    Ok(item) => item,
                    Err(_) => {
                        // Fallback: try legacy StoredMessage and wrap it
                        let msg: StoredMessage = serde_json::from_slice(v.value())
                            .map_err(|e| HistoryError::Serialization(format!("Corrupt or unknown history format: {}", e)))?;
                        HistoryItem::Message(msg)
                    }
                };
                messages.push(item);
            }
        }

        Ok(messages)
    }

    async fn prune(&self, retention: Duration) -> Result<usize, HistoryError> {
        let cutoff =
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) - retention.as_nanos() as i64;

        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| HistoryError::Database(e.to_string()))?;
        let mut count = 0;
        {
            let mut table = write_txn
                .open_table(HISTORY_TABLE)
                .map_err(|e| HistoryError::Database(e.to_string()))?;

            let mut to_delete = Vec::new();
            for item in table
                .iter()
                .map_err(|e| HistoryError::Database(e.to_string()))?
            {
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
                table
                    .remove(k.as_str())
                    .map_err(|e| HistoryError::Database(e.to_string()))?;
                count += 1;
            }
        }
        write_txn
            .commit()
            .map_err(|e| HistoryError::Database(e.to_string()))?;
        Ok(count)
    }

    async fn lookup_timestamp(
        &self,
        _target: &str,
        msgid: &str,
    ) -> Result<Option<i64>, HistoryError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| HistoryError::Database(e.to_string()))?;
        let index = read_txn
            .open_table(MSGID_INDEX)
            .map_err(|e| HistoryError::Database(e.to_string()))?;

        if let Some(v) = index
            .get(msgid)
            .map_err(|e| HistoryError::Database(e.to_string()))?
        {
            let val_str = std::str::from_utf8(v.value())
                .map_err(|e| HistoryError::Serialization(e.to_string()))?;
            let parts: Vec<&str> = val_str.split('\0').collect();
            if parts.len() >= 2
                && let Ok(ts) = parts[1].parse::<i64>()
            {
                return Ok(Some(ts));
            }
        }
        Ok(None)
    }

    async fn query_targets(
        &self,
        start: i64,
        end: i64,
        limit: usize,
        nick: String,
        channels: Vec<String>,
    ) -> Result<Vec<(String, i64)>, HistoryError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| HistoryError::Database(e.to_string()))?;
        let table = read_txn
            .open_table(HISTORY_TABLE)
            .map_err(|e| HistoryError::Database(e.to_string()))?;

        let mut results = Vec::with_capacity(limit);
        let nick_lower = irc_to_lower(&nick);
        let channels_set: std::collections::HashSet<String> =
            channels.iter().map(|c| irc_to_lower(c)).collect();

        let mut cursor = "".to_string();

        loop {
            let mut range = table
                .range(cursor.as_str()..)
                .map_err(|e| HistoryError::Database(e.to_string()))?;
            let next_item = range.next();

            let (target_key, next_cursor) = match next_item {
                Some(item) => {
                    let (k, _) = item.map_err(|e| HistoryError::Database(e.to_string()))?;
                    let key_str = k.value();
                    let parts: Vec<&str> = key_str.split('\0').collect();

                    if parts.is_empty() {
                        break;
                    }

                    let target = parts[0];

                    // Next cursor skips all messages for this target
                    (target.to_string(), format!("{}\0\u{FFFF}", target))
                }
                None => break,
            };

            // Check relevance and extract display name
            let display_target = if target_key.starts_with('#') || target_key.starts_with('&') {
                if channels_set.contains(&target_key) {
                    Some(target_key.clone())
                } else {
                    None
                }
            } else if target_key.starts_with("dm:") {
                // Parse DM: dm:a:u1:a:u2
                let parts: Vec<&str> = target_key.split(':').collect();

                if parts.len() >= 5 {
                    let u1 = parts[2];
                    let u2 = parts[4];
                    if u1 == nick_lower {
                        Some(u2.to_string())
                    } else if u2 == nick_lower {
                        Some(u1.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(display_name) = display_target {
                // Get latest message for this target to check timestamp
                let prefix_start = format!("{}\0", target_key);
                let prefix_end = format!("{}\0\u{FFFF}", target_key);

                let mut sub_range = table
                    .range(prefix_start.as_str()..prefix_end.as_str())
                    .map_err(|e| HistoryError::Database(e.to_string()))?;

                if let Some(item) = sub_range.next_back() {
                    let (k, _) = item.map_err(|e| HistoryError::Database(e.to_string()))?;
                    let key_str = k.value();
                    let parts: Vec<&str> = key_str.split('\0').collect();

                    // Key format: target\0timestamp\0msgid
                    // So timestamp is always at index 1
                    if parts.len() >= 2
                        && let Ok(ts) = parts[1].parse::<i64>()
                        && ts >= start
                        && ts < end
                    {
                        results.push((display_name, ts));
                    }
                }
            }

            cursor = next_cursor;
        }

        // Sort by timestamp ascending (earliest first) per IRCv3 spec
        results.sort_by(|a, b| a.1.cmp(&b.1));
        results.truncate(limit);

        Ok(results)
    }
}
