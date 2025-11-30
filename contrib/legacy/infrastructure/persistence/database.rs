//! SQLite access layer. (Reviewed for deployment comments: none found)
//!
//! - Connection pooling via `deadpool-sqlite` (size from `SLIRCD_DB_POOL_SIZE`, default 10).
//! - WAL mode enabled; busy timeout from `SLIRCD_DB_BUSY_MS` (default 5000ms).
//! - Schema bootstrap: creates minimal `users` table.
//! - Keep DB I/O behind this module; favor short transactions to avoid `SQLITE_BUSY`.
//! - Errors are contextualized with `anyhow::Context` for precise troubleshooting.
use anyhow::{anyhow, Context, Result};
use deadpool_sqlite::PoolConfig;
use deadpool_sqlite::{Config, Pool, Runtime};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Macro to wrap database operations with metrics tracking
/// Automatically tracks operation count and duration for all DB operations
macro_rules! track_db_operation {
    ($operation:expr) => {{
        let start = Instant::now();
        let result = $operation;
        crate::infrastructure::observability::metrics::METRICS.database_operations.inc();
        crate::infrastructure::observability::metrics::METRICS
            .database_duration
            .observe(start.elapsed());
        result
    }};
}

#[derive(Clone)]
pub struct Database {
    path: PathBuf,
    pool: Pool,
}

/// User quit record for WHOWAS history - groups parameters for record_user_quit
pub struct UserQuitRecord<'a> {
    pub nickname: &'a str,
    pub username: &'a str,
    pub realname: &'a str,
    pub hostname: &'a str,
    pub account: Option<&'a str>,
    pub quit_message: Option<&'a str>,
    pub server_name: &'a str,
}

impl Database {
    pub async fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();

        // Configure connection pool with tunable parameters
        let max_size = std::env::var("SLIRCD_DB_POOL_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);

        let busy_ms = std::env::var("SLIRCD_DB_BUSY_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5000);

        let mut config = Config::new(path.clone());
        // Apply pool sizing
        config.pool = Some(PoolConfig {
            max_size,
            ..Default::default()
        });
        let pool = config
            .create_pool(Runtime::Tokio1)
            .context("creating database connection pool")?;

        let database = Database { path, pool };
        database.initialize(busy_ms).await?;
        Ok(database)
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    async fn initialize(&self, busy_ms: u64) -> Result<()> {
        let conn = self
            .pool
            .get()
            .await
            .context("getting connection from pool for initialization")?;

        conn.interact(move |conn| {
            // Configure busy timeout
            conn.busy_timeout(Duration::from_millis(busy_ms))
                .context("configuring sqlite busy_timeout")?;

            conn.execute_batch(
                r"
                PRAGMA journal_mode=WAL;

                -- Core user registration table
                CREATE TABLE IF NOT EXISTS users (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    nickname TEXT UNIQUE,
                    username TEXT,
                    realname TEXT,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                );

                -- SASL authentication accounts
                CREATE TABLE IF NOT EXISTS accounts (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    username TEXT UNIQUE NOT NULL,
                    password_hash TEXT NOT NULL,
                    email TEXT,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    last_login TIMESTAMP,
                    is_active BOOLEAN DEFAULT 1
                );

                -- Channel persistence for permanent channels
                CREATE TABLE IF NOT EXISTS channels (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT UNIQUE NOT NULL,
                    topic TEXT,
                    topic_setter TEXT,
                    topic_set_at TIMESTAMP,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    modes TEXT DEFAULT '',
                    key_hash TEXT,  -- hashed channel password for +k mode
                    member_limit INTEGER,  -- +l mode limit
                    is_permanent BOOLEAN DEFAULT 0
                );

                -- Channel ban lists (+b mode)
                CREATE TABLE IF NOT EXISTS channel_bans (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    channel_name TEXT NOT NULL,
                    ban_mask TEXT NOT NULL,
                    setter TEXT NOT NULL,
                    set_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    expires_at TIMESTAMP,
                    reason TEXT,
                    UNIQUE(channel_name, ban_mask)
                );

                -- Unified ban system (K-line, G-line, Z-line, SQ-line, D-line)
                -- RFC: Server-specific operator tools (not in RFC2812)
                -- COMPETITIVE ANALYSIS: UnrealIRCd TKL, InspIRCd XLine, Ergo oper_reason, Solanum network_wide
                -- Reference: reference-docs/competitors/analysis/ban-system.md
                CREATE TABLE IF NOT EXISTS bans (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    ban_type TEXT NOT NULL,                 -- 'kline', 'gline', 'zline', 'sqline', 'dline'
                    mask TEXT NOT NULL,                     -- user@host (K/G), IP/CIDR (Z/D), nick/#channel (SQ)
                    reason TEXT NOT NULL,                   -- Public reason shown to banned user
                    oper_reason TEXT,                       -- Private reason for operators (Ergo pattern)
                    set_by TEXT NOT NULL,                   -- Operator nickname
                    set_at INTEGER NOT NULL,                -- Unix timestamp
                    expires_at INTEGER,                     -- Unix timestamp (NULL = permanent)
                    soft_ban INTEGER NOT NULL DEFAULT 0,    -- 1 = require SASL instead of hard block (D-line)
                    network_wide INTEGER NOT NULL DEFAULT 0,-- 1 = propagates in S2S (G-line, SQ-line)
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    UNIQUE(ban_type, mask)
                );

                -- Legacy global_bans table (deprecated, will be dropped in future release)
                -- Kept temporarily for migration compatibility
                CREATE TABLE IF NOT EXISTS global_bans (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    ban_type TEXT NOT NULL,
                    ban_mask TEXT NOT NULL,
                    setter TEXT NOT NULL,
                    set_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    expires_at TIMESTAMP,
                    reason TEXT NOT NULL,
                    is_active BOOLEAN DEFAULT 1,
                    UNIQUE(ban_type, ban_mask)
                );

                -- User history for WHOWAS command
                CREATE TABLE IF NOT EXISTS user_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    nickname TEXT NOT NULL,
                    username TEXT NOT NULL,
                    realname TEXT NOT NULL,
                    hostname TEXT NOT NULL,
                    account TEXT,  -- IRCv3 account if authenticated
                    quit_message TEXT,
                    quit_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    server_name TEXT NOT NULL
                );

                -- Message history for CHATHISTORY command (IRCv3 draft/chathistory)
                -- RFC: https://ircv3.net/specs/extensions/chathistory
                --
                -- IMPLEMENTATION NOTES:
                --   - SQLite-optimized indexes (DESC for time range queries)
                --   - Privacy-first: separate PM table with clear opt-in semantics
                --   - Nanosecond timestamps for precise ordering
                --   - JSON BLOB for message envelope (schema evolution flexibility)
                --   - Rate limiting enforced in command handler
                CREATE TABLE IF NOT EXISTS message_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    msgid TEXT NOT NULL UNIQUE,          -- IRCv3 msgid tag (base64)
                    target TEXT NOT NULL,                -- Channel name (normalized, lowercase)
                    sender TEXT NOT NULL,                -- Nickname at send time
                    message_data BLOB NOT NULL,          -- Serialized message envelope (JSON)
                    nanotime INTEGER NOT NULL,           -- Unix nanoseconds (server-time tag)
                    account TEXT,                        -- account-tag if authenticated
                    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
                );

                CREATE TABLE IF NOT EXISTS private_message_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    msgid TEXT NOT NULL UNIQUE,
                    sender TEXT NOT NULL,                -- Normalized nickname
                    recipient TEXT NOT NULL,             -- Normalized nickname
                    message_data BLOB NOT NULL,          -- Serialized message envelope (JSON)
                    nanotime INTEGER NOT NULL,           -- Unix nanoseconds
                    account TEXT,                        -- Sender's account if authenticated
                    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
                );

                -- Operator definitions for OPER command
                CREATE TABLE IF NOT EXISTS operators (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    username TEXT UNIQUE NOT NULL,
                    password_hash TEXT NOT NULL,
                    hostmask TEXT,  -- optional host restriction
                    flags TEXT DEFAULT '',  -- operator privilege flags
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    is_active BOOLEAN DEFAULT 1
                );

                -- Create indexes for performance
                CREATE INDEX IF NOT EXISTS idx_channel_bans_channel ON channel_bans(channel_name);
                CREATE INDEX IF NOT EXISTS idx_channel_bans_expires ON channel_bans(expires_at);

                -- Ban system indexes (COMPETITIVE: InspIRCd XLine query patterns)
                CREATE INDEX IF NOT EXISTS idx_bans_type ON bans(ban_type);
                CREATE INDEX IF NOT EXISTS idx_bans_expires ON bans(expires_at) WHERE expires_at IS NOT NULL;
                CREATE INDEX IF NOT EXISTS idx_bans_network_wide ON bans(network_wide) WHERE network_wide = 1;
                CREATE INDEX IF NOT EXISTS idx_bans_mask ON bans(mask);

                -- Legacy global_bans indexes (deprecated)
                CREATE INDEX IF NOT EXISTS idx_global_bans_type_active ON global_bans(ban_type, is_active);
                CREATE INDEX IF NOT EXISTS idx_global_bans_expires ON global_bans(expires_at);

                CREATE INDEX IF NOT EXISTS idx_user_history_nick ON user_history(nickname);
                CREATE INDEX IF NOT EXISTS idx_user_history_quit_at ON user_history(quit_at);

                -- Message history indexes (CHATHISTORY query optimization)
                -- Compound index on (target, nanotime DESC) for efficient BEFORE/AFTER queries
                -- DESC order optimizes backward scans (LATEST, BEFORE subcommands)
                CREATE INDEX IF NOT EXISTS idx_message_history_target_time ON message_history(target, nanotime DESC);
                CREATE INDEX IF NOT EXISTS idx_message_history_msgid ON message_history(msgid);
                CREATE INDEX IF NOT EXISTS idx_message_history_sender ON message_history(sender);
                CREATE INDEX IF NOT EXISTS idx_message_history_created ON message_history(created_at);

                -- Private message indexes for CHATHISTORY TARGETS subcommand
                -- Participant-based index for DM conversation discovery
                CREATE INDEX IF NOT EXISTS idx_pm_history_participants ON private_message_history(sender, recipient, nanotime DESC);
                CREATE INDEX IF NOT EXISTS idx_pm_history_msgid ON private_message_history(msgid);
                CREATE INDEX IF NOT EXISTS idx_pm_history_created ON private_message_history(created_at);

                -- IRC Services: Registered nicknames (NickServ)
                -- ARCHITECTURE: Embedded services (Ergo pattern) - pseudo-client bots sharing ServerState
                -- SECURITY: bcrypt password hashing (cost 12), rate limiting (1 reg/IP/min)
                -- COMPETITIVE: Matches Ergo embedded model (simpler than Anope/Atheme separate process)
                CREATE TABLE IF NOT EXISTS registered_nicks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    nickname TEXT UNIQUE NOT NULL,       -- Canonical nickname (normalized)
                    password_hash TEXT NOT NULL,         -- bcrypt hash ($2b$12$...)
                    email TEXT,                          -- Optional contact email
                    registered_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    last_seen TIMESTAMP,                 -- Last IDENTIFY or connection
                    enforced BOOLEAN DEFAULT 0,          -- Enforce nickname ownership (kick unauthed)
                    owner_account_id INTEGER,            -- Link to accounts table if SASL-registered
                    FOREIGN KEY (owner_account_id) REFERENCES accounts(id)
                );

                -- IRC Services: Registered channels (ChanServ)
                -- ARCHITECTURE: Channel persistence + founder control (Ergo cs_register pattern)
                -- FEATURES: Topic protection, auto-op, access lists (Phase 2/3)
                CREATE TABLE IF NOT EXISTS registered_channels (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    channel_name TEXT UNIQUE NOT NULL,   -- Canonical channel name (normalized, lowercase)
                    founder_account_id INTEGER NOT NULL, -- Must be registered nick owner
                    founder_nick TEXT NOT NULL,          -- Display name of founder
                    topic TEXT,                          -- Protected topic (ChanServ enforces)
                    registered_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    flags TEXT DEFAULT '',               -- Channel registration flags (GUARD, TOPICLOCK, etc)
                    FOREIGN KEY (founder_account_id) REFERENCES registered_nicks(id)
                );

                -- Services indexes for fast lookup (NickServ IDENTIFY, ChanServ registration checks)
                CREATE INDEX IF NOT EXISTS idx_registered_nicks_nick ON registered_nicks(nickname);
                CREATE INDEX IF NOT EXISTS idx_registered_nicks_email ON registered_nicks(email);
                CREATE INDEX IF NOT EXISTS idx_registered_channels_name ON registered_channels(channel_name);
                CREATE INDEX IF NOT EXISTS idx_registered_channels_founder ON registered_channels(founder_account_id);
                ",
            )
            .context("initializing database schema")?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .map_err(|e| anyhow!("database initialization interaction failed: {}", e))?
    }

    // WHOWAS command support: record user quit for history
    pub async fn record_user_quit(&self, record: UserQuitRecord<'_>) -> Result<()> {
        let nickname = record.nickname.to_string();
        let username = record.username.to_string();
        let realname = record.realname.to_string();
        let hostname = record.hostname.to_string();
        let account = record.account.map(|s| s.to_string());
        let quit_message = record.quit_message.map(|s| s.to_string());
        let server_name = record.server_name.to_string();

        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;
        track_db_operation!(
            conn.interact(move |conn| {
                conn.execute(
                    "INSERT INTO user_history (nickname, username, realname, hostname, account, quit_message, server_name)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![nickname, username, realname, hostname, account, quit_message, server_name],
                ).context("inserting user history record")?;
                Ok::<_, anyhow::Error>(())
            })
            .await
            .map_err(|e| anyhow!("user history interaction failed: {}", e))?
        )
    }

    // WHOWAS command support: retrieve user history
    pub async fn get_user_history(
        &self,
        nickname: &str,
        limit: Option<usize>,
    ) -> Result<Vec<UserHistoryRecord>> {
        let nickname = nickname.to_string();
        let limit = limit.unwrap_or(10); // Default limit

        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;
        track_db_operation!(
            conn.interact(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT nickname, username, realname, hostname, account, quit_message, quit_at, server_name
                     FROM user_history WHERE nickname = ?1 ORDER BY quit_at DESC LIMIT ?2"
                ).context("preparing user history query")?;

                let records = stmt.query_map(rusqlite::params![nickname, limit], |row| {
                    Ok(UserHistoryRecord {
                        nickname: row.get(0)?,
                        username: row.get(1)?,
                        realname: row.get(2)?,
                        hostname: row.get(3)?,
                        account: row.get(4)?,
                        quit_message: row.get(5)?,
                        quit_at: row.get(6)?,
                        server_name: row.get(7)?,
                    })
                }).context("executing user history query")?
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("collecting user history results")?;

                Ok(records)
            })
            .await
            .map_err(|e| anyhow!("user history interaction failed: {}", e))?
        )
    }

    // Channel ban management
    pub async fn add_channel_ban(
        &self,
        channel_name: &str,
        ban_mask: &str,
        setter: &str,
        expires_at: Option<i64>,
        reason: Option<&str>,
    ) -> Result<()> {
        let channel_name = channel_name.to_string();
        let ban_mask = ban_mask.to_string();
        let setter = setter.to_string();
        let reason = reason.map(|s| s.to_string());

        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;
        track_db_operation!(
            conn.interact(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO channel_bans (channel_name, ban_mask, setter, expires_at, reason)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![channel_name, ban_mask, setter, expires_at, reason],
                ).context("inserting channel ban")?;
                Ok::<_, anyhow::Error>(())
            })
            .await
            .map_err(|e| anyhow!("channel ban interaction failed: {}", e))?
        )
    }

    // Get active channel bans
    pub async fn get_channel_bans(&self, channel_name: &str) -> Result<Vec<ChannelBanRecord>> {
        let channel_name = channel_name.to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;
        track_db_operation!(conn
            .interact(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT ban_mask, setter, set_at, expires_at, reason
                     FROM channel_bans
                     WHERE channel_name = ?1 AND (expires_at IS NULL OR expires_at > ?2)
                     ORDER BY set_at ASC",
                    )
                    .context("preparing channel bans query")?;

                let records = stmt
                    .query_map(rusqlite::params![channel_name, now], |row| {
                        Ok(ChannelBanRecord {
                            ban_mask: row.get(0)?,
                            setter: row.get(1)?,
                            set_at: row.get(2)?,
                            expires_at: row.get(3)?,
                            reason: row.get(4)?,
                        })
                    })
                    .context("executing channel bans query")?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .context("collecting channel bans results")?;

                Ok(records)
            })
            .await
            .map_err(|e| anyhow!("channel bans interaction failed: {}", e))?)
    }

    // Remove channel ban
    pub async fn remove_channel_ban(&self, channel_name: &str, ban_mask: &str) -> Result<bool> {
        let channel_name = channel_name.to_string();
        let ban_mask = ban_mask.to_string();

        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;
        track_db_operation!(conn
            .interact(move |conn| {
                let rows_affected = conn
                    .execute(
                        "DELETE FROM channel_bans WHERE channel_name = ?1 AND ban_mask = ?2",
                        rusqlite::params![channel_name, ban_mask],
                    )
                    .context("removing channel ban")?;
                Ok(rows_affected > 0)
            })
            .await
            .map_err(|e| anyhow!("channel ban removal interaction failed: {}", e))?)
    }

    // Global ban management (X-lines)
    pub async fn add_global_ban(
        &self,
        ban_type: &str,
        ban_mask: &str,
        setter: &str,
        expires_at: Option<i64>,
        reason: &str,
    ) -> Result<()> {
        let ban_type = ban_type.to_string();
        let ban_mask = ban_mask.to_string();
        let setter = setter.to_string();
        let reason = reason.to_string();

        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;
        track_db_operation!(
            conn.interact(move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO global_bans (ban_type, ban_mask, setter, expires_at, reason)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![ban_type, ban_mask, setter, expires_at, reason],
                ).context("inserting global ban")?;
                Ok::<_, anyhow::Error>(())
            })
            .await
            .map_err(|e| anyhow!("global ban interaction failed: {}", e))?
        )
    }

    // Check if user matches any active global bans
    pub async fn check_global_bans(
        &self,
        nickname: &str,
        username: &str,
        hostname: &str,
        realname: &str,
    ) -> Result<Option<GlobalBanRecord>> {
        let nickname = nickname.to_string();
        let username = username.to_string();
        let hostname = hostname.to_string();
        let realname = realname.to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;
        track_db_operation!(conn
            .interact(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT ban_type, ban_mask, setter, set_at, expires_at, reason
                     FROM global_bans
                     WHERE is_active = 1 AND (expires_at IS NULL OR expires_at > ?1)
                     ORDER BY set_at ASC",
                    )
                    .context("preparing global bans query")?;

                let records = stmt
                    .query_map(rusqlite::params![now], |row| {
                        Ok(GlobalBanRecord {
                            ban_type: row.get(0)?,
                            ban_mask: row.get(1)?,
                            setter: row.get(2)?,
                            set_at: row.get(3)?,
                            expires_at: row.get(4)?,
                            reason: row.get(5)?,
                        })
                    })
                    .context("executing global bans query")?;

                // Check each ban to see if it matches the user
                for record_result in records {
                    let record = record_result.context("processing global ban record")?;
                    if matches_ban_mask(
                        &record.ban_type,
                        &record.ban_mask,
                        &nickname,
                        &username,
                        &hostname,
                        &realname,
                    ) {
                        return Ok(Some(record));
                    }
                }
                Ok(None)
            })
            .await
            .map_err(|e| anyhow!("global ban check interaction failed: {}", e))?)
    }

    // ========================================================================
    // Ban System Methods (Stage 2: Database Persistence)
    // Reference: reference-docs/competitors/analysis/ban-system.md
    // COMPETITIVE ANALYSIS: UnrealIRCd TKL, InspIRCd XLine, Ergo duration patterns
    // ========================================================================

    /// Add a ban to the database
    /// Returns the database ID of the newly inserted ban
    /// COMPETITIVE: InspIRCd XLineManager::AddLine pattern
    pub async fn add_ban(&self, ban: &crate::security::bans::BanInfo) -> Result<i64> {
        let ban_type = ban.ban_type.to_string();
        let mask = ban.mask.clone();
        let reason = ban.reason.clone();
        let oper_reason = ban.oper_reason.clone();
        let set_by = ban.set_by.clone();
        let set_at = ban.set_at;
        let expires_at = ban.expires_at;
        let soft_ban = if ban.soft_ban { 1 } else { 0 };
        let network_wide = if ban.network_wide { 1 } else { 0 };
        let created_at = ban.created_at;

        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;

        track_db_operation!(conn
            .interact(move |conn| {
                conn.execute(
                    "INSERT INTO bans (ban_type, mask, reason, oper_reason, set_by, set_at, expires_at, soft_ban, network_wide, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     ON CONFLICT(ban_type, mask) DO UPDATE SET
                         reason = excluded.reason,
                         oper_reason = excluded.oper_reason,
                         set_by = excluded.set_by,
                         set_at = excluded.set_at,
                         expires_at = excluded.expires_at,
                         soft_ban = excluded.soft_ban,
                         network_wide = excluded.network_wide",
                    rusqlite::params![ban_type, mask, reason, oper_reason, set_by, set_at, expires_at, soft_ban, network_wide, created_at],
                ).context("inserting ban")?;

                let id = conn.last_insert_rowid();
                Ok::<_, anyhow::Error>(id)
            })
            .await
            .map_err(|e| anyhow!("ban insertion interaction failed: {}", e))?)
    }

    /// Get a specific ban by ID
    /// COMPETITIVE: InspIRCd XLineManager::GetLine pattern
    pub async fn get_ban(&self, id: i64) -> Result<Option<crate::security::bans::BanInfo>> {
        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;

        track_db_operation!(conn
            .interact(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, ban_type, mask, reason, oper_reason, set_by, set_at, expires_at, soft_ban, network_wide, created_at
                     FROM bans WHERE id = ?1"
                ).context("preparing get ban query")?;

                let result = stmt.query_row(rusqlite::params![id], |row| {
                    let ban_type_str: String = row.get(1)?;
                    let ban_type = crate::security::bans::BanType::from_string(&ban_type_str)
                        .ok_or_else(|| rusqlite::Error::InvalidQuery)?;

                    Ok(crate::security::bans::BanInfo {
                        id: Some(row.get(0)?),
                        ban_type,
                        mask: row.get(2)?,
                        reason: row.get(3)?,
                        oper_reason: row.get(4)?,
                        set_by: row.get(5)?,
                        set_at: row.get(6)?,
                        expires_at: row.get(7)?,
                        soft_ban: row.get::<_, i64>(8)? != 0,
                        network_wide: row.get::<_, i64>(9)? != 0,
                        created_at: row.get(10)?,
                    })
                });

                match result {
                    Ok(ban) => Ok(Some(ban)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(anyhow::anyhow!("failed to get ban: {}", e)),
                }
            })
            .await
            .map_err(|e| anyhow!("get ban interaction failed: {}", e))?)
    }

    /// List all bans, optionally filtered by type
    /// COMPETITIVE: UnrealIRCd tkl_list_all pattern
    pub async fn list_bans(
        &self,
        ban_type: Option<crate::security::bans::BanType>,
    ) -> Result<Vec<crate::security::bans::BanInfo>> {
        let ban_type_filter = ban_type.as_ref().map(|t| t.to_string());

        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;

        track_db_operation!(conn
            .interact(move |conn| {
                let (query, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(filter) = ban_type_filter {
                    (
                        "SELECT id, ban_type, mask, reason, oper_reason, set_by, set_at, expires_at, soft_ban, network_wide, created_at
                         FROM bans WHERE ban_type = ?1 ORDER BY set_at DESC".to_string(),
                        vec![Box::new(filter)]
                    )
                } else {
                    (
                        "SELECT id, ban_type, mask, reason, oper_reason, set_by, set_at, expires_at, soft_ban, network_wide, created_at
                         FROM bans ORDER BY set_at DESC".to_string(),
                        vec![]
                    )
                };

                let mut stmt = conn.prepare(&query).context("preparing list bans query")?;

                let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
                let bans = stmt.query_map(&params_refs[..], |row| {
                    let ban_type_str: String = row.get(1)?;
                    let ban_type = crate::security::bans::BanType::from_string(&ban_type_str)
                        .ok_or_else(|| rusqlite::Error::InvalidQuery)?;

                    Ok(crate::security::bans::BanInfo {
                        id: Some(row.get(0)?),
                        ban_type,
                        mask: row.get(2)?,
                        reason: row.get(3)?,
                        oper_reason: row.get(4)?,
                        set_by: row.get(5)?,
                        set_at: row.get(6)?,
                        expires_at: row.get(7)?,
                        soft_ban: row.get::<_, i64>(8)? != 0,
                        network_wide: row.get::<_, i64>(9)? != 0,
                        created_at: row.get(10)?,
                    })
                }).context("executing list bans query")?;

                let mut result = Vec::new();
                for ban in bans {
                    result.push(ban.context("processing ban record")?);
                }
                Ok::<_, anyhow::Error>(result)
            })
            .await
            .map_err(|e| anyhow!("list bans interaction failed: {}", e))?)
    }

    /// Remove a ban by ID
    /// Returns true if a ban was removed, false if ID not found
    /// COMPETITIVE: InspIRCd XLineManager::DelLine pattern
    pub async fn remove_ban(&self, id: i64) -> Result<bool> {
        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;

        track_db_operation!(conn
            .interact(move |conn| {
                let rows_affected = conn
                    .execute("DELETE FROM bans WHERE id = ?1", rusqlite::params![id])
                    .context("deleting ban")?;

                Ok::<_, anyhow::Error>(rows_affected > 0)
            })
            .await
            .map_err(|e| anyhow!("remove ban interaction failed: {}", e))?)
    }

    /// Check if a target matches any active bans of a specific type
    /// Returns the first matching ban (by set_at ASC = oldest first)
    /// COMPETITIVE: Ergo/Solanum CheckBans pattern
    pub async fn check_for_ban_match(
        &self,
        target: &str,
        ban_type: crate::security::bans::BanType,
    ) -> Result<Option<crate::security::bans::BanInfo>> {
        let target = target.to_string();
        let ban_type_str = ban_type.to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;

        track_db_operation!(conn
            .interact(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, ban_type, mask, reason, oper_reason, set_by, set_at, expires_at, soft_ban, network_wide, created_at
                     FROM bans
                     WHERE ban_type = ?1 AND (expires_at IS NULL OR expires_at > ?2)
                     ORDER BY set_at ASC"
                ).context("preparing ban match query")?;

                let bans = stmt.query_map(rusqlite::params![ban_type_str, now], |row| {
                    let ban_type_str: String = row.get(1)?;
                    let ban_type = crate::security::bans::BanType::from_string(&ban_type_str)
                        .ok_or_else(|| rusqlite::Error::InvalidQuery)?;

                    Ok(crate::security::bans::BanInfo {
                        id: Some(row.get(0)?),
                        ban_type,
                        mask: row.get(2)?,
                        reason: row.get(3)?,
                        oper_reason: row.get(4)?,
                        set_by: row.get(5)?,
                        set_at: row.get(6)?,
                        expires_at: row.get(7)?,
                        soft_ban: row.get::<_, i64>(8)? != 0,
                        network_wide: row.get::<_, i64>(9)? != 0,
                        created_at: row.get(10)?,
                    })
                }).context("executing ban match query")?;

                // Check each ban to see if it matches the target
                for ban_result in bans {
                    let ban = ban_result.context("processing ban record")?;
                    if ban.matches(&target) {
                        return Ok(Some(ban));
                    }
                }
                Ok(None)
            })
            .await
            .map_err(|e| anyhow!("ban match check interaction failed: {}", e))?)
    }

    /// Clean up expired bans (called by background task)
    /// Returns the number of bans removed
    /// COMPETITIVE: UnrealIRCd tkl_expire pattern (60-second intervals)
    pub async fn expire_bans(&self) -> Result<usize> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;

        track_db_operation!(conn
            .interact(move |conn| {
                let rows_affected = conn
                    .execute(
                        "DELETE FROM bans WHERE expires_at IS NOT NULL AND expires_at <= ?1",
                        rusqlite::params![now],
                    )
                    .context("deleting expired bans")?;

                Ok::<_, anyhow::Error>(rows_affected)
            })
            .await
            .map_err(|e| anyhow!("expire bans interaction failed: {}", e))?)
    }

    /// Clean up old WHOWAS history (called by background task)
    /// Returns the number of records removed
    /// RFC2812 §3.6.3: WHOWAS history retention is server-defined
    /// Default: 7 days (168 hours), configurable via server.whowas_retention_days
    pub async fn cleanup_whowas_history(&self, retention_days: u32) -> Result<usize> {
        let retention_seconds = (retention_days as i64) * 86400; // days to seconds
        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
            - retention_seconds;

        let conn = self
            .pool
            .get()
            .await
            .context("getting database connection")?;

        track_db_operation!(conn
            .interact(move |conn| {
                // SQLite CURRENT_TIMESTAMP is UTC seconds since epoch
                // Compare against quit_at which is TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                let rows_affected = conn
                    .execute(
                        "DELETE FROM user_history WHERE unixepoch(quit_at) < ?1",
                        rusqlite::params![cutoff],
                    )
                    .context("deleting old WHOWAS history")?;

                Ok::<_, anyhow::Error>(rows_affected)
            })
            .await
            .map_err(|e| anyhow!("cleanup WHOWAS history interaction failed: {}", e))?)
    }
}

// Data structures for database records
#[derive(Debug, Clone)]
pub struct UserHistoryRecord {
    pub nickname: String,
    pub username: String,
    pub realname: String,
    pub hostname: String,
    pub account: Option<String>,
    pub quit_message: Option<String>,
    pub quit_at: String, // ISO timestamp from SQLite
    pub server_name: String,
}

#[derive(Debug, Clone)]
pub struct ChannelBanRecord {
    pub ban_mask: String,
    pub setter: String,
    pub set_at: String, // ISO timestamp from SQLite
    pub expires_at: Option<i64>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GlobalBanRecord {
    pub ban_type: String,
    pub ban_mask: String,
    pub setter: String,
    pub set_at: String, // ISO timestamp from SQLite
    pub expires_at: Option<i64>,
    pub reason: String,
}

// Helper function to check if a user matches a ban mask
fn matches_ban_mask(
    ban_type: &str,
    ban_mask: &str,
    nickname: &str,
    username: &str,
    hostname: &str,
    realname: &str,
) -> bool {
    match ban_type {
        "nick" => crate::util::wildcard_match(ban_mask, nickname),
        "user" => crate::util::wildcard_match(ban_mask, username),
        "host" => crate::util::wildcard_match(ban_mask, hostname),
        "gecos" => crate::util::wildcard_match(ban_mask, realname),
        "ip" => crate::util::wildcard_match(ban_mask, hostname), // IP bans check hostname
        _ => false,
    }
}
