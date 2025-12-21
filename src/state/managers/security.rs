//! Security management state.
//!
//! This module contains the `SecurityManager` struct, which isolates all
//! security-related state from the main Matrix struct.

use crate::config::SecurityConfig;
use crate::db::{Database, Dline, Gline, Kline, Shun, Zline};
use crate::security::ip_deny::IpDenyList;
use crate::security::spam::SpamDetectionService;
use crate::security::{BanCache, RateLimitManager};
use dashmap::DashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Security management state.
///
/// The SecurityManager holds all security-related state, including:
/// - Rate limiting for flood protection
/// - Spam detection service
/// - Active shuns (temporary bans)
/// - Ban cache for K-lines and G-lines
/// - IP deny list for D-lines and Z-lines
pub struct SecurityManager {
    /// Global rate limiter for flood protection.
    pub rate_limiter: RateLimitManager,

    /// Spam detection service for content analysis (wrapped in RwLock for runtime config).
    pub spam_detector: Option<Arc<RwLock<SpamDetectionService>>>,

    /// Active shuns cached in memory for fast lookup.
    /// Key is the mask pattern, value is the Shun record.
    pub shuns: DashMap<String, Shun>,

    /// In-memory ban cache for fast connection-time ban checks (K-lines and G-lines).
    pub ban_cache: BanCache,

    /// High-performance IP deny list (Roaring Bitmap engine).
    /// Used for nanosecond-scale IP rejection in the gateway accept loop.
    pub ip_deny_list: std::sync::RwLock<IpDenyList>,
}

/// Parameters for creating a new SecurityManager.
pub struct SecurityManagerParams<'a> {
    pub security_config: &'a SecurityConfig,
    pub db: Option<Database>,
    pub data_dir: Option<&'a Path>,
    pub shuns: Vec<Shun>,
    pub klines: Vec<Kline>,
    pub dlines: Vec<Dline>,
    pub glines: Vec<Gline>,
    pub zlines: Vec<Zline>,
}

impl SecurityManager {
    /// Create a new SecurityManager with the given configuration.
    pub fn new(params: SecurityManagerParams<'_>) -> Self {
        let SecurityManagerParams {
            security_config,
            db,
            data_dir,
            shuns,
            klines,
            dlines,
            glines,
            zlines,
        } = params;

        // Build the shuns map
        let shuns_map = DashMap::with_capacity(shuns.len());
        for shun in shuns {
            shuns_map.insert(shun.mask.clone(), shun);
        }

        // Initialize spam detector if enabled
        let spam_detector = if security_config.spam_detection_enabled {
            Some(Arc::new(RwLock::new(SpamDetectionService::new(
                db,
                security_config.clone(),
            ))))
        } else {
            None
        };

        // Load IP deny list from data directory
        let ip_deny_path = data_dir
            .map(|d| d.join("ip_bans.msgpack"))
            .unwrap_or_else(|| std::path::PathBuf::from("ip_bans.msgpack"));
        let mut ip_deny_list = IpDenyList::load(&ip_deny_path);

        // Sync IpDenyList with database D-lines and Z-lines
        ip_deny_list.sync_from_database_bans(&dlines, &zlines);

        // Build the ban cache (K-lines and G-lines only; IP bans handled by IpDenyList)
        let ban_cache = BanCache::load(klines, glines);

        Self {
            rate_limiter: RateLimitManager::new(security_config.rate_limits.clone()),
            spam_detector,
            shuns: shuns_map,
            ban_cache,
            ip_deny_list: std::sync::RwLock::new(ip_deny_list),
        }
    }
}
