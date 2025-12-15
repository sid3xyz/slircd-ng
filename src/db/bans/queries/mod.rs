//! Database query methods for ban operations.

pub mod dline;
pub mod generic;
pub mod gline;
pub mod kline;
pub mod rline;
pub mod shun;
pub mod zline;

use crate::db::DbError;
use sqlx::SqlitePool;

/// Macro to generate repository wrapper methods for ban operations.
///
/// This eliminates ~150 lines of boilerplate delegation code by automatically
/// generating wrapper methods that forward to module-level functions.
macro_rules! ban_repository_methods {
    (
        $(
            $(#[$meta:meta])*
            fn $method_name:ident($($arg:ident: $arg_ty:ty),*) -> $ret_ty:ty
                => $module:ident::$fn_name:ident;
        )*
    ) => {
        $(
            $(#[$meta])*
            pub async fn $method_name(&self, $($arg: $arg_ty),*) -> $ret_ty {
                $module::$fn_name(self.pool, $($arg),*).await
            }
        )*
    };
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

    // Generate all ban query wrapper methods using the macro
    ban_repository_methods! {
        // ========== K-line operations ==========

        /// Add a K-line.
        fn add_kline(mask: &str, reason: Option<&str>, set_by: &str, duration: Option<i64>) -> Result<(), DbError>
            => kline::add_kline;

        /// Remove a K-line.
        fn remove_kline(mask: &str) -> Result<bool, DbError>
            => kline::remove_kline;

        /// Check if a user@host matches any active K-line.
        fn matches_kline(user_host: &str) -> Result<Option<super::models::Kline>, DbError>
            => kline::matches_kline;

        /// Get all active K-lines (not expired).
        fn get_active_klines() -> Result<Vec<super::models::Kline>, DbError>
            => kline::get_active_klines;

        // ========== D-line operations ==========

        /// Add a D-line.
        fn add_dline(mask: &str, reason: Option<&str>, set_by: &str, duration: Option<i64>) -> Result<(), DbError>
            => dline::add_dline;

        /// Remove a D-line.
        fn remove_dline(mask: &str) -> Result<bool, DbError>
            => dline::remove_dline;

        /// Get all active D-lines (not expired).
        fn get_active_dlines() -> Result<Vec<super::models::Dline>, DbError>
            => dline::get_active_dlines;

        // ========== G-line operations ==========

        /// Add a G-line.
        fn add_gline(mask: &str, reason: Option<&str>, set_by: &str, duration: Option<i64>) -> Result<(), DbError>
            => gline::add_gline;

        /// Remove a G-line.
        fn remove_gline(mask: &str) -> Result<bool, DbError>
            => gline::remove_gline;

        /// Check if a user@host matches any active G-line.
        fn matches_gline(user_host: &str) -> Result<Option<super::models::Gline>, DbError>
            => gline::matches_gline;

        /// Get all active G-lines (not expired).
        fn get_active_glines() -> Result<Vec<super::models::Gline>, DbError>
            => gline::get_active_glines;

        // ========== Z-line operations ==========

        /// Add a Z-line.
        fn add_zline(mask: &str, reason: Option<&str>, set_by: &str, duration: Option<i64>) -> Result<(), DbError>
            => zline::add_zline;

        /// Remove a Z-line.
        fn remove_zline(mask: &str) -> Result<bool, DbError>
            => zline::remove_zline;

        /// Get all active Z-lines (not expired).
        fn get_active_zlines() -> Result<Vec<super::models::Zline>, DbError>
            => zline::get_active_zlines;

        // ========== R-line operations ==========

        /// Add an R-line.
        fn add_rline(mask: &str, reason: Option<&str>, set_by: &str, duration: Option<i64>) -> Result<(), DbError>
            => rline::add_rline;

        /// Remove an R-line.
        fn remove_rline(mask: &str) -> Result<bool, DbError>
            => rline::remove_rline;

        /// Check if a realname matches any active R-line.
        fn matches_rline(realname: &str) -> Result<Option<super::models::Rline>, DbError>
            => rline::matches_rline;

        /// Get all active R-lines (not expired).
        fn get_active_rlines() -> Result<Vec<super::models::Rline>, DbError>
            => rline::get_active_rlines;

        // ========== Shun operations ==========

        /// Add a shun.
        fn add_shun(mask: &str, reason: Option<&str>, set_by: &str, duration: Option<i64>) -> Result<(), DbError>
            => shun::add_shun;

        /// Remove a shun.
        fn remove_shun(mask: &str) -> Result<bool, DbError>
            => shun::remove_shun;

        /// Check if a user@host matches any active shun.
        fn matches_shun(user_host: &str) -> Result<Option<super::models::Shun>, DbError>
            => shun::matches_shun;

        /// Get all active shuns (not expired).
        fn get_active_shuns() -> Result<Vec<super::models::Shun>, DbError>
            => shun::get_active_shuns;
    }

    // ========== Combined check operations ==========

    /// Check for user@host bans (G-lines and K-lines only).
    ///
    /// This is an optimized version of `check_all_bans()` that skips IP-based
    /// ban checks (Z-lines and D-lines) since those are handled at connection
    /// time by `IpDenyList` with O(1) Roaring Bitmap lookups.
    ///
    /// Use this for registration-time checks after the connection has already
    /// passed gateway IP filtering.
    pub async fn check_user_host_bans(
        &self,
        user: &str,
        host: &str,
    ) -> Result<Option<String>, DbError> {
        let user_host = format!("{}@{}", user, host);

        // Check G-lines (global user@host)
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
