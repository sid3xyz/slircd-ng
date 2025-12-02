//! Database query methods for ban operations.

pub mod kline;
pub mod dline;
pub mod gline;
pub mod zline;
pub mod rline;
pub mod shun;

use crate::db::DbError;
use sqlx::SqlitePool;

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
        kline::add_kline(self.pool, mask, reason, set_by, duration).await
    }

    /// Remove a K-line.
    pub async fn remove_kline(&self, mask: &str) -> Result<bool, DbError> {
        kline::remove_kline(self.pool, mask).await
    }

    /// Check if a user@host matches any active K-line.
    pub async fn matches_kline(&self, user_host: &str) -> Result<Option<super::models::Kline>, DbError> {
        kline::matches_kline(self.pool, user_host).await
    }

    /// Get all active K-lines (not expired).
    pub async fn get_active_klines(&self) -> Result<Vec<super::models::Kline>, DbError> {
        kline::get_active_klines(self.pool).await
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
        dline::add_dline(self.pool, mask, reason, set_by, duration).await
    }

    /// Remove a D-line.
    pub async fn remove_dline(&self, mask: &str) -> Result<bool, DbError> {
        dline::remove_dline(self.pool, mask).await
    }

    /// Check if an IP matches any active D-line.
    pub async fn matches_dline(&self, ip: &str) -> Result<Option<super::models::Dline>, DbError> {
        dline::matches_dline(self.pool, ip).await
    }

    /// Get all active D-lines (not expired).
    pub async fn get_active_dlines(&self) -> Result<Vec<super::models::Dline>, DbError> {
        dline::get_active_dlines(self.pool).await
    }

    // ========== G-line operations ==========

    /// Add a G-line.
    pub async fn add_gline(
        &self,
        mask: &str,
        reason: Option<&str>,
        set_by: &str,
        duration: Option<i64>,
    ) -> Result<(), DbError> {
        gline::add_gline(self.pool, mask, reason, set_by, duration).await
    }

    /// Remove a G-line.
    pub async fn remove_gline(&self, mask: &str) -> Result<bool, DbError> {
        gline::remove_gline(self.pool, mask).await
    }

    /// Check if a user@host matches any active G-line.
    pub async fn matches_gline(&self, user_host: &str) -> Result<Option<super::models::Gline>, DbError> {
        gline::matches_gline(self.pool, user_host).await
    }

    /// Get all active G-lines (not expired).
    pub async fn get_active_glines(&self) -> Result<Vec<super::models::Gline>, DbError> {
        gline::get_active_glines(self.pool).await
    }

    // ========== Z-line operations ==========

    /// Add a Z-line.
    pub async fn add_zline(
        &self,
        mask: &str,
        reason: Option<&str>,
        set_by: &str,
        duration: Option<i64>,
    ) -> Result<(), DbError> {
        zline::add_zline(self.pool, mask, reason, set_by, duration).await
    }

    /// Remove a Z-line.
    pub async fn remove_zline(&self, mask: &str) -> Result<bool, DbError> {
        zline::remove_zline(self.pool, mask).await
    }

    /// Check if an IP matches any active Z-line.
    pub async fn matches_zline(&self, ip: &str) -> Result<Option<super::models::Zline>, DbError> {
        zline::matches_zline(self.pool, ip).await
    }

    /// Get all active Z-lines (not expired).
    pub async fn get_active_zlines(&self) -> Result<Vec<super::models::Zline>, DbError> {
        zline::get_active_zlines(self.pool).await
    }

    // ========== R-line operations ==========

    /// Add an R-line.
    pub async fn add_rline(
        &self,
        mask: &str,
        reason: Option<&str>,
        set_by: &str,
        duration: Option<i64>,
    ) -> Result<(), DbError> {
        rline::add_rline(self.pool, mask, reason, set_by, duration).await
    }

    /// Remove an R-line.
    pub async fn remove_rline(&self, mask: &str) -> Result<bool, DbError> {
        rline::remove_rline(self.pool, mask).await
    }

    /// Check if a realname matches any active R-line.
    pub async fn matches_rline(&self, realname: &str) -> Result<Option<super::models::Rline>, DbError> {
        rline::matches_rline(self.pool, realname).await
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
        shun::add_shun(self.pool, mask, reason, set_by, duration).await
    }

    /// Remove a shun.
    pub async fn remove_shun(&self, mask: &str) -> Result<bool, DbError> {
        shun::remove_shun(self.pool, mask).await
    }

    /// Check if a user@host matches any active shun.
    pub async fn matches_shun(&self, user_host: &str) -> Result<Option<super::models::Shun>, DbError> {
        shun::matches_shun(self.pool, user_host).await
    }

    /// Check if an IP matches any active shun.
    #[allow(dead_code)] // Will be used for connection-time shun checks
    pub async fn matches_shun_ip(&self, ip: &str) -> Result<Option<super::models::Shun>, DbError> {
        shun::matches_shun_ip(self.pool, ip).await
    }

    /// Get all active shuns (not expired).
    pub async fn get_active_shuns(&self) -> Result<Vec<super::models::Shun>, DbError> {
        shun::get_active_shuns(self.pool).await
    }

    // ========== Combined check operations ==========

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
}
