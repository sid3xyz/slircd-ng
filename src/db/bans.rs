//! Repository for K-line and D-line bans.

use super::DbError;
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
    pub async fn check_ban(&self, ip: &str, user: &str, host: &str) -> Result<Option<String>, DbError> {
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

/// Simple wildcard matching (* and ?).
#[allow(dead_code)] // Used by matches_kline/matches_dline
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let text = text.to_lowercase();

    let mut p_chars = pattern.chars().peekable();
    let mut t_chars = text.chars().peekable();

    while let Some(p) = p_chars.next() {
        match p {
            '*' => {
                // Consume consecutive *
                while p_chars.peek() == Some(&'*') {
                    p_chars.next();
                }
                // If * is at end, match rest
                if p_chars.peek().is_none() {
                    return true;
                }
                // Try matching from each position
                while t_chars.peek().is_some() {
                    let remaining_pattern: String = std::iter::once(p_chars.clone())
                        .flatten()
                        .collect();
                    let remaining_text: String = t_chars.clone().collect();
                    if wildcard_match(&remaining_pattern, &remaining_text) {
                        return true;
                    }
                    t_chars.next();
                }
                return wildcard_match(&p_chars.collect::<String>(), "");
            }
            '?' => {
                if t_chars.next().is_none() {
                    return false;
                }
            }
            c => {
                if t_chars.next() != Some(c) {
                    return false;
                }
            }
        }
    }

    t_chars.next().is_none()
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
    let network_parts: Vec<u8> = network
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
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
