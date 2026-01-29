//! Redb-backed persistence for always-on clients.
//!
//! Stores Client state in Redb for:
//! - Restoration on server restart
//! - Dirty-bit writeback for efficient persistence
//!
//! # Schema
//!
//! ```text
//! ALWAYS_ON_CLIENTS: account_lower -> StoredClient (serde_json)
//! DEVICE_STATE: "account_lower\0device_id" -> DeviceInfo (serde_json)
//! ```

use crate::state::client::{ChannelMembership, Client, DeviceInfo};
use chrono::{DateTime, Utc};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use slirc_proto::irc_to_lower;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Redb table for always-on client state.
const ALWAYS_ON_CLIENTS: TableDefinition<&str, &[u8]> = TableDefinition::new("always_on_clients");

/// Redb table for device state.
const DEVICE_STATE: TableDefinition<&str, &[u8]> = TableDefinition::new("device_state");

/// Redb table for read markers.
/// Key: "account_lower\0target_lower"
/// Value: Timestamp (nanoseconds)
const READ_MARKERS: TableDefinition<&str, i64> = TableDefinition::new("read_markers");

/// Errors from always-on persistence.
#[derive(Debug, Error)]
pub enum AlwaysOnError {
    #[error("redb error: {0}")]
    Redb(#[from] redb::Error),

    #[error("table error: {0}")]
    Table(#[from] redb::TableError),

    #[error("storage error: {0}")]
    Storage(#[from] redb::StorageError),

    #[error("transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),

    #[error("commit error: {0}")]
    Commit(#[from] redb::CommitError),

    #[error("serialization error: {0}")]
    Serialization(String),
}

/// Serialized client state for persistence.
#[derive(Debug, Serialize, Deserialize)]
pub struct StoredClient {
    /// Account name (primary key).
    pub account: String,

    /// Current nickname.
    pub nick: String,

    /// User modes as string (e.g., "+iwx").
    pub modes: String,

    /// Channels and membership info.
    pub channels: Vec<StoredChannelMembership>,

    /// Per-device last-seen timestamps (Unix epoch).
    pub last_seen: HashMap<String, i64>,

    /// When this client was created (Unix epoch).
    pub created_at: i64,

    /// Whether always-on is enabled.
    pub always_on: bool,

    /// Whether auto-away is enabled.
    pub auto_away: bool,

    /// Current away message (if set).
    pub away: Option<String>,
}

/// Serialized channel membership.
#[derive(Debug, Serialize, Deserialize)]
pub struct StoredChannelMembership {
    /// Channel name (case-preserved).
    pub channel: String,

    /// Channel modes for this user.
    pub modes: String,

    /// Join time (Unix epoch).
    pub join_time: i64,
}

/// Serialized device info for persistence.
#[derive(Debug, Serialize, Deserialize)]
pub struct StoredDeviceInfo {
    /// Device identifier.
    pub id: String,

    /// Human-readable name.
    pub name: Option<String>,

    /// TLS certificate fingerprint.
    pub certfp: Option<String>,

    /// When first seen (Unix epoch).
    pub created_at: i64,

    /// When last seen (Unix epoch).
    pub last_seen: i64,
}

/// Redb-backed always-on client persistence.
pub struct AlwaysOnStore {
    db: Arc<Database>,
}

impl AlwaysOnStore {
    /// Create a new AlwaysOnStore using an existing Redb database.
    ///
    /// The database is shared with the history provider.
    pub fn new(db: Arc<Database>) -> Result<Self, AlwaysOnError> {
        // Ensure tables exist
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(ALWAYS_ON_CLIENTS)?;
            let _ = write_txn.open_table(DEVICE_STATE)?;
            let _ = write_txn.open_table(READ_MARKERS)?;
        }
        write_txn.commit()?;

        info!("AlwaysOn store initialized");
        Ok(Self { db })
    }

    /// Save a client to persistent storage.
    pub fn save_client(&self, client: &Client) -> Result<(), AlwaysOnError> {
        let stored = StoredClient::from_client(client);
        let key = irc_to_lower(&client.account);
        let value =
            serde_json::to_vec(&stored).map_err(|e| AlwaysOnError::Serialization(e.to_string()))?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(ALWAYS_ON_CLIENTS)?;
            table.insert(key.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;

        // Persist per-device metadata separately so names/certfps survive restarts
        for device in client.devices.values() {
            self.save_device(&client.account, device)?;
        }

        debug!(account = %client.account, "Saved always-on client state");
        Ok(())
    }

    /// Load all always-on clients from storage.
    pub fn load_all_clients(&self) -> Result<Vec<StoredClient>, AlwaysOnError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(ALWAYS_ON_CLIENTS)?;

        let mut clients = Vec::new();
        for item in table.iter()? {
            let (_key, value) = item?;
            match serde_json::from_slice::<StoredClient>(value.value()) {
                Ok(stored) => {
                    if stored.always_on {
                        clients.push(stored);
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to deserialize stored client, skipping");
                }
            }
        }

        info!(
            count = clients.len(),
            "Loaded always-on clients from storage"
        );
        Ok(clients)
    }

    /// Delete a client from persistent storage.
    pub fn delete_client(&self, account: &str) -> Result<bool, AlwaysOnError> {
        let key = irc_to_lower(account);
        let write_txn = self.db.begin_write()?;
        let deleted = {
            let mut table = write_txn.open_table(ALWAYS_ON_CLIENTS)?;
            table.remove(key.as_str())?.is_some()
        };
        write_txn.commit()?;

        if deleted {
            // Remove any per-device metadata associated with this account
            for device in self.load_devices(account)? {
                let _ = self.delete_device(account, &device.id)?;
            }

            debug!(account = %account, "Deleted always-on client from storage");
        }
        Ok(deleted)
    }

    /// Save device info for an account.
    pub fn save_device(&self, account: &str, device: &DeviceInfo) -> Result<(), AlwaysOnError> {
        let key = Self::device_key(account, &device.id);
        let stored = StoredDeviceInfo::from_device(device);
        let value =
            serde_json::to_vec(&stored).map_err(|e| AlwaysOnError::Serialization(e.to_string()))?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DEVICE_STATE)?;
            table.insert(key.as_str(), value.as_slice())?;
        }
        write_txn.commit()?;

        debug!(account = %account, device_id = %device.id, "Saved device state");
        Ok(())
    }

    /// Load all devices for an account.
    pub fn load_devices(&self, account: &str) -> Result<Vec<StoredDeviceInfo>, AlwaysOnError> {
        let prefix = format!("{}\0", irc_to_lower(account));
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DEVICE_STATE)?;

        let mut devices = Vec::new();
        for item in table.iter()? {
            let (key, value) = item?;
            if key.value().starts_with(&prefix) {
                match serde_json::from_slice::<StoredDeviceInfo>(value.value()) {
                    Ok(stored) => devices.push(stored),
                    Err(e) => {
                        warn!(key = %key.value(), error = %e, "Failed to deserialize device");
                    }
                }
            }
        }
        Ok(devices)
    }

    /// Delete a device.
    pub fn delete_device(&self, account: &str, device_id: &str) -> Result<bool, AlwaysOnError> {
        let key = Self::device_key(account, device_id);
        let write_txn = self.db.begin_write()?;
        let deleted = {
            let mut table = write_txn.open_table(DEVICE_STATE)?;
            table.remove(key.as_str())?.is_some()
        };
        write_txn.commit()?;
        Ok(deleted)
    }

    /// Save a read marker.
    pub fn save_read_marker(
        &self,
        account: &str,
        target: &str,
        timestamp: i64,
    ) -> Result<(), AlwaysOnError> {
        let key = Self::marker_key(account, target);
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(READ_MARKERS)?;
            table.insert(key.as_str(), timestamp)?;
        }
        write_txn.commit()?;
        debug!(account = %account, target = %target, ts = %timestamp, "Saved read marker");
        Ok(())
    }

    /// Get a read marker.
    pub fn get_read_marker(&self, account: &str, target: &str) -> Result<Option<i64>, AlwaysOnError> {
        let key = Self::marker_key(account, target);
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(READ_MARKERS)?;
        let result = table.get(key.as_str())?.map(|v| v.value());
        Ok(result)
    }

    /// Delete all markers for an account.
    pub fn delete_markers(&self, account: &str) -> Result<usize, AlwaysOnError> {
        let prefix = format!("{}\0", irc_to_lower(account));
        let write_txn = self.db.begin_write()?;
        let mut count = 0;
        {
            let mut table = write_txn.open_table(READ_MARKERS)?;
            // Redb doesn't support prefix delete directly efficiently without iteration,
            // but we can iterate and collect keys to delete.
            // Note: Mutating while iterating is not allowed, so we collect first.
            let keys: Vec<String> = table
                .iter()?
                .filter_map(|r| {
                    if let Ok((k, _)) = r {
                        if k.value().starts_with(&prefix) {
                            return Some(k.value().to_string());
                        }
                    }
                    None
                })
                .collect();

            for key in keys {
                if table.remove(key.as_str())?.is_some() {
                    count += 1;
                }
            }
        }
        write_txn.commit()?;
        debug!(account = %account, count = %count, "Deleted read markers");
        Ok(count)
    }

    fn marker_key(account: &str, target: &str) -> String {
        format!("{}\0{}", irc_to_lower(account), irc_to_lower(target))
    }

    /// Expire clients that haven't been seen within the cutoff time.
    pub fn expire_clients(&self, cutoff: DateTime<Utc>) -> Result<Vec<String>, AlwaysOnError> {
        let cutoff_ts = cutoff.timestamp();
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(ALWAYS_ON_CLIENTS)?;

        // Find expired accounts
        let mut expired = Vec::new();
        for item in table.iter()? {
            let (key, value) = item?;
            if let Ok(stored) = serde_json::from_slice::<StoredClient>(value.value()) {
                // A client is expired if ALL devices are older than the cutoff
                let all_expired = stored.last_seen.is_empty()
                    || stored.last_seen.values().all(|&ts| ts < cutoff_ts);
                if all_expired {
                    expired.push(key.value().to_string());
                }
            }
        }
        drop(table);
        drop(read_txn);

        // Delete expired clients
        if !expired.is_empty() {
            let write_txn = self.db.begin_write()?;
            {
                let mut table = write_txn.open_table(ALWAYS_ON_CLIENTS)?;
                for account in &expired {
                    table.remove(account.as_str())?;
                }
            }
            write_txn.commit()?;
            info!(count = expired.len(), "Expired stale always-on clients");
        }

        Ok(expired)
    }

    fn device_key(account: &str, device_id: &str) -> String {
        format!("{}\0{}", irc_to_lower(account), device_id)
    }
}

impl StoredClient {
    /// Convert a Client to StoredClient.
    pub fn from_client(client: &Client) -> Self {
        Self {
            account: client.account.clone(),
            nick: client.nick.clone(),
            modes: client.modes.as_mode_string(),
            channels: client
                .channels
                .iter()
                .map(|(ch, mem)| StoredChannelMembership {
                    channel: ch.clone(),
                    modes: mem.modes.clone(),
                    join_time: mem.join_time,
                })
                .collect(),
            last_seen: client
                .last_seen
                .iter()
                .map(|(k, v)| (k.clone(), v.timestamp()))
                .collect(),
            created_at: client.created_at.timestamp(),
            always_on: client.always_on,
            auto_away: client.auto_away,
            away: client.away.clone(),
        }
    }

    /// Convert StoredClient to Client.
    pub fn to_client(&self) -> Client {
        use crate::state::UserModes;

        let mut client = Client::new(self.account.clone(), self.nick.clone());
        client.always_on = self.always_on;
        client.auto_away = self.auto_away;
        client.away = self.away.clone();
        client.created_at = DateTime::from_timestamp(self.created_at, 0).unwrap_or_else(Utc::now);

        // Restore channel memberships
        for stored_mem in &self.channels {
            client.channels.insert(
                stored_mem.channel.clone(),
                ChannelMembership {
                    modes: stored_mem.modes.clone(),
                    join_time: stored_mem.join_time,
                },
            );
        }

        // Restore last-seen timestamps
        for (device_id, ts) in &self.last_seen {
            if let Some(dt) = DateTime::from_timestamp(*ts, 0) {
                client.last_seen.insert(device_id.clone(), dt);
            }
        }

        // Parse and set user modes
        client.modes = UserModes::default();
        // NOTE: Mode parsing will be added when UserModes supports parsing from a string.

        client
    }
}

impl StoredDeviceInfo {
    /// Convert DeviceInfo to StoredDeviceInfo.
    pub fn from_device(device: &DeviceInfo) -> Self {
        Self {
            id: device.id.clone(),
            name: device.name.clone(),
            certfp: device.certfp.clone(),
            created_at: device.created_at.timestamp(),
            last_seen: device.last_seen.timestamp(),
        }
    }

    /// Convert StoredDeviceInfo to DeviceInfo.
    pub fn to_device(&self) -> DeviceInfo {
        DeviceInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            certfp: self.certfp.clone(),
            created_at: DateTime::from_timestamp(self.created_at, 0).unwrap_or_else(Utc::now),
            last_seen: DateTime::from_timestamp(self.last_seen, 0).unwrap_or_else(Utc::now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_db() -> (Arc<Database>, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let db = Database::create(db_path).unwrap();
        (Arc::new(db), dir)
    }

    #[test]
    fn test_save_and_load_client() {
        let (db, _dir) = create_test_db();
        let store = AlwaysOnStore::new(db).unwrap();

        let mut client = Client::new("alice".to_string(), "Alice".to_string());
        client.set_always_on(true);
        client.set_auto_away(true);
        client.join_channel("#test", "ov");
        client.update_last_seen(&"phone".to_string());

        store.save_client(&client).unwrap();

        let loaded = store
            .load_all_clients()
            .unwrap()
            .into_iter()
            .find(|client| client.account == "alice")
            .unwrap();
        assert_eq!(loaded.account, "alice");
        assert_eq!(loaded.nick, "Alice");
        assert!(loaded.always_on);
        assert!(loaded.auto_away);
        assert_eq!(loaded.channels.len(), 1);
        assert_eq!(loaded.channels[0].channel, "#test");
        assert_eq!(loaded.channels[0].modes, "ov");
        assert!(loaded.last_seen.contains_key("phone"));
    }

    #[test]
    fn test_load_all_always_on() {
        let (db, _dir) = create_test_db();
        let store = AlwaysOnStore::new(db).unwrap();

        // Save two clients, one always-on, one not
        let mut alice = Client::new("alice".to_string(), "Alice".to_string());
        alice.set_always_on(true);
        store.save_client(&alice).unwrap();

        let bob = Client::new("bob".to_string(), "Bob".to_string());
        store.save_client(&bob).unwrap();

        // Only alice should be loaded
        let clients = store.load_all_clients().unwrap();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].account, "alice");
    }

    #[test]
    fn test_delete_client() {
        let (db, _dir) = create_test_db();
        let store = AlwaysOnStore::new(db).unwrap();

        let mut client = Client::new("alice".to_string(), "Alice".to_string());
        client.set_always_on(true);
        store.save_client(&client).unwrap();

        let clients = store.load_all_clients().unwrap();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].account, "alice");

        let deleted = store.delete_client("alice").unwrap();
        assert!(deleted);

        assert!(store.load_all_clients().unwrap().is_empty());
    }

    #[test]
    fn test_device_operations() {
        let (db, _dir) = create_test_db();
        let store = AlwaysOnStore::new(db).unwrap();

        let device = DeviceInfo {
            id: "phone".to_string(),
            name: Some("iPhone".to_string()),
            certfp: None,
            created_at: Utc::now(),
            last_seen: Utc::now(),
        };

        store.save_device("alice", &device).unwrap();

        let devices = store.load_devices("alice").unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "phone");
        assert_eq!(devices[0].name, Some("iPhone".to_string()));

        let deleted = store.delete_device("alice", "phone").unwrap();
        assert!(deleted);

        let devices = store.load_devices("alice").unwrap();
        assert!(devices.is_empty());
    }

    #[test]
    fn test_expire_clients() {
        let (db, _dir) = create_test_db();
        let store = AlwaysOnStore::new(db).unwrap();

        // Create a client with old last_seen
        let mut client = Client::new("alice".to_string(), "Alice".to_string());
        client.set_always_on(true);
        client.last_seen.insert(
            "phone".to_string(),
            DateTime::from_timestamp(0, 0).unwrap(), // Very old
        );
        store.save_client(&client).unwrap();

        // Expire with recent cutoff
        let cutoff = Utc::now();
        let expired = store.expire_clients(cutoff).unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], "alice");

        // Client should be gone
        assert!(store.load_all_clients().unwrap().is_empty());
    }
}
