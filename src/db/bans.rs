//! Repository for K-line and D-line bans.

use super::DbError;
use slirc_proto::wildcard_match;
use sqlx::SqlitePool;

/// A K-line (user@host ban).
#[derive(Debug, Clone)]
#[allow(dead_code)] // TODO: Use for connection-time ban checks
pub struct Kline {
    pub mask: String,
    pub reason: Option<String>,
    pub set_by: String,
    pub set_at: i64,
    pub expires_at: Option<i64>,
}

/// A D-line (IP ban).
#[derive(Debug, Clone)]
#[allow(dead_code)] // TODO: Use for connection-time ban checks
pub struct Dline {
    pub mask: String,
    pub reason: Option<String>,
    pub set_by: String,
    pub set_at: i64,
    pub expires_at: Option<i64>,
}

/// Repository for ban operations.
pub struct BanRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> BanRepository<'a> {
    /// Create a new ban repository.
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    // ========== K-line operations ==========

    /// Add a K-line.
    pub async fn add_kline(
        &self,
        mask: &str,
        reason: Option<&str>,
        set_by: &str,
        duration: Option<i64>,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().timestamp();
        let expires_at = duration.map(|d| now + d);

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO klines (mask, reason, set_by, set_at, expires_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(mask)
        .bind(reason)
        .bind(set_by)
        .bind(now)
        .bind(expires_at)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Remove a K-line.
    pub async fn remove_kline(&self, mask: &str) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM klines WHERE mask = ?")
            .bind(mask)
            .execute(self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all active K-lines (not expired).
    #[allow(dead_code)] // TODO: Use for connection-time ban checks
    pub async fn get_active_klines(&self) -> Result<Vec<Kline>, DbError> {
        let now = chrono::Utc::now().timestamp();

        let rows = sqlx::query_as::<_, (String, Option<String>, String, i64, Option<i64>)>(
            r#"
            SELECT mask, reason, set_by, set_at, expires_at
            FROM klines
            WHERE expires_at IS NULL OR expires_at > ?
            "#,
        )
        .bind(now)
        .fetch_all(self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(mask, reason, set_by, set_at, expires_at)| Kline {
                mask,
                reason,
                set_by,
                set_at,
                expires_at,
            })
            .collect())
    }

    /// Check if a user@host matches any active K-line.
    pub async fn matches_kline(&self, user_host: &str) -> Result<Option<Kline>, DbError> {
        let klines = self.get_active_klines().await?;

        for kline in klines {
            if wildcard_match(&kline.mask, user_host) {
                return Ok(Some(kline));
            }
        }

        Ok(None)
    }

    // ========== D-line operations ==========

    /// Add a D-line.
    pub async fn add_dline(
        &self,
        mask: &str,
        reason: Option<&str>,
        set_by: &str,
        duration: Option<i64>,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().timestamp();
        let expires_at = duration.map(|d| now + d);

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO dlines (mask, reason, set_by, set_at, expires_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(mask)
        .bind(reason)
        .bind(set_by)
        .bind(now)
        .bind(expires_at)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Remove a D-line.
    pub async fn remove_dline(&self, mask: &str) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM dlines WHERE mask = ?")
            .bind(mask)
            .execute(self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all active D-lines (not expired).
    #[allow(dead_code)] // TODO: Use for connection-time ban checks
    pub async fn get_active_dlines(&self) -> Result<Vec<Dline>, DbError> {
        let now = chrono::Utc::now().timestamp();

        let rows = sqlx::query_as::<_, (String, Option<String>, String, i64, Option<i64>)>(
            r#"
            SELECT mask, reason, set_by, set_at, expires_at
            FROM dlines
            WHERE expires_at IS NULL OR expires_at > ?
            "#,
        )
        .bind(now)
        .fetch_all(self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(mask, reason, set_by, set_at, expires_at)| Dline {
                mask,
                reason,
                set_by,
                set_at,
                expires_at,
            })
            .collect())
    }

    /// Check if an IP matches any active D-line.
    pub async fn matches_dline(&self, ip: &str) -> Result<Option<Dline>, DbError> {
        let dlines = self.get_active_dlines().await?;

        for dline in dlines {
            if wildcard_match(&dline.mask, ip) || cidr_match(&dline.mask, ip) {
                return Ok(Some(dline));
            }
        }

        Ok(None)
    }

    /// Check if a connection should be banned.
    ///
    /// Checks both K-lines (user@host bans) and D-lines (IP bans).
    /// Returns the ban reason if banned, None if allowed.
    /// Deprecated: Use check_all_bans() for full X-line support.
    #[allow(dead_code)]
    pub async fn check_ban(
        &self,
        ip: &str,
        user: &str,
        host: &str,
    ) -> Result<Option<String>, DbError> {
        // Check D-lines first (IP ban takes precedence)
        if let Some(dline) = self.matches_dline(ip).await? {
            let reason = dline.reason.unwrap_or_else(|| "Banned".to_string());
            return Ok(Some(format!("D-lined: {}", reason)));
        }

        // Check K-lines (user@host)
        let user_host = format!("{}@{}", user, host);
        if let Some(kline) = self.matches_kline(&user_host).await? {
            let reason = kline.reason.unwrap_or_else(|| "Banned".to_string());
            return Ok(Some(format!("K-lined: {}", reason)));
        }

        Ok(None)
    }
}

/// Basic CIDR matching for IP addresses.
#[allow(dead_code)] // Used by matches_dline
fn cidr_match(cidr: &str, ip: &str) -> bool {
    // Parse CIDR notation (e.g., "192.168.1.0/24")
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return false;
    }

    let network = parts[0];
    let prefix_len: u32 = match parts[1].parse() {
        Ok(p) if p <= 32 => p,
        _ => return false,
    };

    // Parse network IP
    let network_parts: Vec<u8> = network.split('.').filter_map(|s| s.parse().ok()).collect();
    if network_parts.len() != 4 {
        return false;
    }

    // Parse target IP
    let ip_parts: Vec<u8> = ip.split('.').filter_map(|s| s.parse().ok()).collect();
    if ip_parts.len() != 4 {
        return false;
    }

    // Convert to u32
    let network_u32 = u32::from_be_bytes([
        network_parts[0],
        network_parts[1],
        network_parts[2],
        network_parts[3],
    ]);
    let ip_u32 = u32::from_be_bytes([ip_parts[0], ip_parts[1], ip_parts[2], ip_parts[3]]);

    // Create mask and compare
    let mask = if prefix_len == 0 {
        0
    } else {
        !0u32 << (32 - prefix_len)
    };

    (network_u32 & mask) == (ip_u32 & mask)
}

// ========== G-Line Types and Operations ==========

/// A G-line (global hostmask ban).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by admin commands in Phase 3b
pub struct Gline {
    pub mask: String,
    pub reason: Option<String>,
    pub set_by: String,
    pub set_at: i64,
    pub expires_at: Option<i64>,
}

/// A Z-line (IP ban that skips DNS lookup).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by admin commands in Phase 3b
pub struct Zline {
    pub mask: String,
    pub reason: Option<String>,
    pub set_by: String,
    pub set_at: i64,
    pub expires_at: Option<i64>,
}

/// An R-line (realname/GECOS ban).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by admin commands in Phase 3b
pub struct Rline {
    pub mask: String,
    pub reason: Option<String>,
    pub set_by: String,
    pub set_at: i64,
    pub expires_at: Option<i64>,
}

impl<'a> BanRepository<'a> {
    // ========== G-line operations ==========

    /// Add a G-line.
    #[allow(dead_code)] // Phase 3b: Admin commands
    pub async fn add_gline(
        &self,
        mask: &str,
        reason: Option<&str>,
        set_by: &str,
        duration: Option<i64>,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().timestamp();
        let expires_at = duration.map(|d| now + d);

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO glines (mask, reason, set_by, set_at, expires_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(mask)
        .bind(reason)
        .bind(set_by)
        .bind(now)
        .bind(expires_at)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Remove a G-line.
    #[allow(dead_code)] // Phase 3b: Admin commands
    pub async fn remove_gline(&self, mask: &str) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM glines WHERE mask = ?")
            .bind(mask)
            .execute(self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all active G-lines (not expired).
    pub async fn get_active_glines(&self) -> Result<Vec<Gline>, DbError> {
        let now = chrono::Utc::now().timestamp();

        let rows = sqlx::query_as::<_, (String, Option<String>, String, i64, Option<i64>)>(
            r#"
            SELECT mask, reason, set_by, set_at, expires_at
            FROM glines
            WHERE expires_at IS NULL OR expires_at > ?
            "#,
        )
        .bind(now)
        .fetch_all(self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(mask, reason, set_by, set_at, expires_at)| Gline {
                mask,
                reason,
                set_by,
                set_at,
                expires_at,
            })
            .collect())
    }

    /// Check if a user@host matches any active G-line.
    pub async fn matches_gline(&self, user_host: &str) -> Result<Option<Gline>, DbError> {
        let glines = self.get_active_glines().await?;

        for gline in glines {
            if wildcard_match(&gline.mask, user_host) {
                return Ok(Some(gline));
            }
        }

        Ok(None)
    }

    // ========== Z-line operations ==========

    /// Add a Z-line.
    #[allow(dead_code)] // Phase 3b: Admin commands
    pub async fn add_zline(
        &self,
        mask: &str,
        reason: Option<&str>,
        set_by: &str,
        duration: Option<i64>,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().timestamp();
        let expires_at = duration.map(|d| now + d);

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO zlines (mask, reason, set_by, set_at, expires_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(mask)
        .bind(reason)
        .bind(set_by)
        .bind(now)
        .bind(expires_at)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Remove a Z-line.
    #[allow(dead_code)] // Phase 3b: Admin commands
    pub async fn remove_zline(&self, mask: &str) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM zlines WHERE mask = ?")
            .bind(mask)
            .execute(self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all active Z-lines (not expired).
    pub async fn get_active_zlines(&self) -> Result<Vec<Zline>, DbError> {
        let now = chrono::Utc::now().timestamp();

        let rows = sqlx::query_as::<_, (String, Option<String>, String, i64, Option<i64>)>(
            r#"
            SELECT mask, reason, set_by, set_at, expires_at
            FROM zlines
            WHERE expires_at IS NULL OR expires_at > ?
            "#,
        )
        .bind(now)
        .fetch_all(self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(mask, reason, set_by, set_at, expires_at)| Zline {
                mask,
                reason,
                set_by,
                set_at,
                expires_at,
            })
            .collect())
    }

    /// Check if an IP matches any active Z-line.
    pub async fn matches_zline(&self, ip: &str) -> Result<Option<Zline>, DbError> {
        let zlines = self.get_active_zlines().await?;

        for zline in zlines {
            if wildcard_match(&zline.mask, ip) || cidr_match(&zline.mask, ip) {
                return Ok(Some(zline));
            }
        }

        Ok(None)
    }

    // ========== R-line operations ==========

    /// Add an R-line (realname ban).
    pub async fn add_rline(
        &self,
        mask: &str,
        reason: Option<&str>,
        set_by: &str,
        duration: Option<i64>,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().timestamp();
        let expires_at = duration.map(|d| now + d);

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO rlines (mask, reason, set_by, set_at, expires_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(mask)
        .bind(reason)
        .bind(set_by)
        .bind(now)
        .bind(expires_at)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Remove an R-line.
    pub async fn remove_rline(&self, mask: &str) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM rlines WHERE mask = ?")
            .bind(mask)
            .execute(self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all active R-lines (not expired).
    pub async fn get_active_rlines(&self) -> Result<Vec<Rline>, DbError> {
        let now = chrono::Utc::now().timestamp();

        let rows = sqlx::query_as::<_, (String, Option<String>, String, i64, Option<i64>)>(
            r#"
            SELECT mask, reason, set_by, set_at, expires_at
            FROM rlines
            WHERE expires_at IS NULL OR expires_at > ?
            "#,
        )
        .bind(now)
        .fetch_all(self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(mask, reason, set_by, set_at, expires_at)| Rline {
                mask,
                reason,
                set_by,
                set_at,
                expires_at,
            })
            .collect())
    }

    /// Check if a realname matches any active R-line.
    pub async fn matches_rline(&self, realname: &str) -> Result<Option<Rline>, DbError> {
        let rlines = self.get_active_rlines().await?;

        for rline in rlines {
            if wildcard_match(&rline.mask, realname) {
                return Ok(Some(rline));
            }
        }

        Ok(None)
    }

    /// Check if a connection should be banned (extended to include G-lines and Z-lines).
    ///
    /// Checks in order: Z-line (IP), D-line (IP), G-line (user@host), K-line (user@host).
    /// Returns the ban reason if banned, None if allowed.
    pub async fn check_all_bans(
        &self,
        ip: &str,
        user: &str,
        host: &str,
    ) -> Result<Option<String>, DbError> {
        // Check Z-lines first (IP ban, skips DNS)
        if let Some(zline) = self.matches_zline(ip).await? {
            let reason = zline.reason.unwrap_or_else(|| "Banned".to_string());
            return Ok(Some(format!("Z-lined: {}", reason)));
        }

        // Check D-lines (IP ban)
        if let Some(dline) = self.matches_dline(ip).await? {
            let reason = dline.reason.unwrap_or_else(|| "Banned".to_string());
            return Ok(Some(format!("D-lined: {}", reason)));
        }

        // Check G-lines (global user@host)
        let user_host = format!("{}@{}", user, host);
        if let Some(gline) = self.matches_gline(&user_host).await? {
            let reason = gline.reason.unwrap_or_else(|| "Banned".to_string());
            return Ok(Some(format!("G-lined: {}", reason)));
        }

        // Check K-lines (local user@host)
        if let Some(kline) = self.matches_kline(&user_host).await? {
            let reason = kline.reason.unwrap_or_else(|| "Banned".to_string());
            return Ok(Some(format!("K-lined: {}", reason)));
        }

        Ok(None)
    }

    /// Check if a realname is banned (R-line check).
    /// This is typically called during registration after USER command is received.
    pub async fn check_realname_ban(&self, realname: &str) -> Result<Option<String>, DbError> {
        if let Some(rline) = self.matches_rline(realname).await? {
            let reason = rline.reason.unwrap_or_else(|| "Banned".to_string());
            return Ok(Some(format!("R-lined: {}", reason)));
        }
        Ok(None)
    }

    // ========== Shun operations ==========

    /// Add a shun.
    pub async fn add_shun(
        &self,
        mask: &str,
        reason: Option<&str>,
        set_by: &str,
        duration: Option<i64>,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().timestamp();
        let expires_at = duration.map(|d| now + d);

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO shuns (mask, reason, set_by, set_at, expires_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(mask)
        .bind(reason)
        .bind(set_by)
        .bind(now)
        .bind(expires_at)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Remove a shun.
    pub async fn remove_shun(&self, mask: &str) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM shuns WHERE mask = ?")
            .bind(mask)
            .execute(self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Get all active shuns (not expired).
    pub async fn get_active_shuns(&self) -> Result<Vec<Shun>, DbError> {
        let now = chrono::Utc::now().timestamp();

        let rows = sqlx::query_as::<_, (String, Option<String>, String, i64, Option<i64>)>(
            r#"
            SELECT mask, reason, set_by, set_at, expires_at
            FROM shuns
            WHERE expires_at IS NULL OR expires_at > ?
            "#,
        )
        .bind(now)
        .fetch_all(self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(mask, reason, set_by, set_at, expires_at)| Shun {
                mask,
                reason,
                set_by,
                set_at,
                expires_at,
            })
            .collect())
    }

    /// Check if a user@host matches any active shun.
    pub async fn matches_shun(&self, user_host: &str) -> Result<Option<Shun>, DbError> {
        let shuns = self.get_active_shuns().await?;

        for shun in shuns {
            if wildcard_match(&shun.mask, user_host) {
                return Ok(Some(shun));
            }
        }

        Ok(None)
    }

    /// Check if an IP matches any active shun.
    #[allow(dead_code)] // Will be used for connection-time shun checks
    pub async fn matches_shun_ip(&self, ip: &str) -> Result<Option<Shun>, DbError> {
        let shuns = self.get_active_shuns().await?;

        for shun in shuns {
            if wildcard_match(&shun.mask, ip) || cidr_match(&shun.mask, ip) {
                return Ok(Some(shun));
            }
        }

        Ok(None)
    }
}

/// A shun (silent ban - user stays connected but commands are ignored).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by admin for stats/inspection
pub struct Shun {
    pub mask: String,
    pub reason: Option<String>,
    pub set_by: String,
    pub set_at: i64,
    pub expires_at: Option<i64>,
}
