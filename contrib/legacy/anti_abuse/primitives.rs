//! Advanced Anti-Abuse Features for IRC Server (Reviewed for deployment comments: none found)
//!
//! This module provides comprehensive anti-abuse functionality including:
//! - Extended bans (EXTBAN) with pattern matching
//! - Rate limiting for messages, connections, and joins
//! - DNS blacklist (DNSBL) checking
//! - X-lines (K/G/Z/R/S-line) server-level bans
//!
//! Modern IRC networks require sophisticated abuse prevention beyond simple
//! nick!user@host matching. Extended bans provide powerful pattern matching
//! for account names, realnames, servers, and certificates.

use crate::core::state::ClientId;
use dashmap::DashMap;
use governor::{Quota, RateLimiter};
use regex::Regex;
use std::collections::HashMap;
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::time::{Duration, Instant, SystemTime};
use tokio::net::lookup_host;
use tracing::{debug, warn};

// Use the type alias for cleaner code
type DirectRateLimiter = governor::DefaultDirectRateLimiter;

/// Extended Ban Types for advanced pattern matching
/// Extends beyond simple nick!user@host to match on various user attributes
#[derive(Debug, Clone)]
pub enum ExtendedBan {
    Account(String),     // $a:account - matches users logged into account
    Realname(String),    // $r:pattern - matches realname field
    Server(String),      // $s:server - matches user's server
    Channel(String),     // $c:channel - matches users in channel
    Oper(String),        // $o:type - matches IRCops of given type
    Certificate(String), // $x:fp - matches SSL certificate fingerprint
    Unregistered,        // $U - matches unregistered users
    SASL(String),        // $z:pattern - matches SASL authentication mechanism
    Join(String),        // $j:pattern - matches channel join patterns
}

impl ExtendedBan {
    /// Parse extended ban from string format like "$a:nickname" or "$r:*bot*"
    pub fn parse(ban_string: &str) -> Option<Self> {
        if !ban_string.starts_with('$') {
            return None;
        }

        let parts: Vec<&str> = ban_string.splitn(2, ':').collect();
        if parts.len() < 2 {
            // Handle special cases like $U (unregistered)
            return match ban_string {
                "$U" => Some(ExtendedBan::Unregistered),
                _ => None,
            };
        }

        let ban_type = parts[0];
        let pattern = parts[1].to_string();

        match ban_type {
            "$a" => Some(ExtendedBan::Account(pattern)),
            "$r" => Some(ExtendedBan::Realname(pattern)),
            "$s" => Some(ExtendedBan::Server(pattern)),
            "$c" => Some(ExtendedBan::Channel(pattern)),
            "$o" => Some(ExtendedBan::Oper(pattern)),
            "$x" => Some(ExtendedBan::Certificate(pattern)),
            "$z" => Some(ExtendedBan::SASL(pattern)),
            "$j" => Some(ExtendedBan::Join(pattern)),
            _ => None,
        }
    }
}

/// User context for evaluating extended bans
#[derive(Debug, Clone)]
pub struct UserContext {
    pub nickname: String,
    pub username: String,
    pub hostname: String,
    pub realname: String,
    pub account: Option<String>,
    pub server: String,
    pub channels: Vec<String>,
    pub is_oper: bool,
    pub oper_type: Option<String>,
    pub certificate_fp: Option<String>,
    pub sasl_mechanism: Option<String>,
    pub ip_address: IpAddr,
    pub is_registered: bool,
}

/// X-line ban types following traditional IRC server conventions
#[derive(Debug, Clone)]
pub enum XLine {
    KLine {
        mask: String,
        reason: String,
        expires: Option<SystemTime>,
    }, // Local user bans
    GLine {
        mask: String,
        reason: String,
        expires: Option<SystemTime>,
    }, // Global user bans
    ZLine {
        ip: String,
        reason: String,
        expires: Option<SystemTime>,
    }, // IP address bans
    RLine {
        regex: String,
        reason: String,
        expires: Option<SystemTime>,
    }, // Regex-based bans
    SLine {
        mask: String,
        reason: String,
        expires: Option<SystemTime>,
    }, // Server bans
}

impl XLine {
    /// Check if X-line has expired
    pub fn is_expired(&self) -> bool {
        let expires = match self {
            XLine::KLine { expires, .. }
            | XLine::GLine { expires, .. }
            | XLine::ZLine { expires, .. }
            | XLine::RLine { expires, .. }
            | XLine::SLine { expires, .. } => *expires,
        };

        if let Some(expiry) = expires {
            SystemTime::now() > expiry
        } else {
            false // Permanent ban
        }
    }

    /// Get the pattern/mask for this X-line (for indexing purposes)
    pub fn pattern(&self) -> &str {
        match self {
            XLine::KLine { mask, .. } | XLine::GLine { mask, .. } => mask,
            XLine::ZLine { ip, .. } => ip,
            XLine::RLine { regex, .. } => regex,
            XLine::SLine { mask, .. } => mask,
        }
    }
}

/// DNS Blacklist checker for IP reputation
#[derive(Debug)]
pub struct DNSBLChecker {
    providers: Vec<String>,
    timeout: Duration,
    cache: DashMap<IpAddr, DNSBLCacheEntry>,
    cache_ttl: Duration,
    /// Whitelisted IPs that bypass DNSBL checks (loaded from SLIRCD_DNSBL_WHITELIST env)
    whitelist: Vec<IpAddr>,
}

/// Cached DNSBL lookup result with timestamp
#[derive(Debug, Clone)]
struct DNSBLCacheEntry {
    result: bool,
    timestamp: Instant,
    reason: Option<String>,
}

/// DNSBL check result action
#[derive(Debug, Clone, PartialEq)]
pub enum DNSBLAction {
    Block(String),   // Block with reason
    Allow,           // Allow connection
    Monitor(String), // Allow but log for monitoring
}

/// DNSBL configuration for a blacklist
#[derive(Debug, Clone)]
pub struct DNSBLConfig {
    pub hostname: String,
    pub action: DNSBLAction,
    pub enabled: bool,
}

impl DNSBLChecker {
    /// Create new DNSBL checker with specified providers and cache TTL
    pub fn new(providers: Vec<String>, timeout: Duration) -> Self {
        Self {
            providers,
            timeout,
            cache: DashMap::new(),
            cache_ttl: Duration::from_secs(3600), // 1 hour default
            whitelist: Self::load_whitelist_from_env(),
        }
    }

    /// Load whitelist IPs from SLIRCD_DNSBL_WHITELIST environment variable
    /// Format: comma-separated IPs (e.g., "1.2.3.4,5.6.7.8")
    fn load_whitelist_from_env() -> Vec<IpAddr> {
        std::env::var("SLIRCD_DNSBL_WHITELIST")
            .ok()
            .map(|s| {
                s.split(',')
                    .filter_map(|ip_str| ip_str.trim().parse::<IpAddr>().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Create with custom cache TTL
    pub fn with_cache_ttl(providers: Vec<String>, timeout: Duration, cache_ttl: Duration) -> Self {
        Self {
            providers,
            timeout,
            cache: DashMap::new(),
            cache_ttl,
            whitelist: Self::load_whitelist_from_env(),
        }
    }

    /// Create with default providers
    pub fn with_defaults() -> Self {
        Self::new(
            vec![
                "zen.spamhaus.org".to_string(),
                "bl.spamcop.net".to_string(),
                "dnsbl.sorbs.net".to_string(),
                "cbl.abuseat.org".to_string(),
            ],
            Duration::from_secs(2),
        )
    }

    /// Get cached result if valid (not expired)
    fn get_cached(&self, ip: IpAddr) -> Option<(bool, Option<String>)> {
        if let Some(entry) = self.cache.get(&ip) {
            if entry.timestamp.elapsed() < self.cache_ttl {
                debug!(ip = %ip, cached_result = entry.result, "using cached DNSBL result");
                return Some((entry.result, entry.reason.clone()));
            }
            debug!(ip = %ip, "cached DNSBL result expired");
        }
        None
    }

    /// Cache a DNSBL lookup result
    fn cache_result(&self, ip: IpAddr, is_listed: bool, reason: Option<String>) {
        self.cache.insert(
            ip,
            DNSBLCacheEntry {
                result: is_listed,
                timestamp: Instant::now(),
                reason,
            },
        );
    }

    /// Cleanup expired cache entries
    pub fn cleanup_cache(&self) {
        self.cache
            .retain(|_, entry| entry.timestamp.elapsed() < self.cache_ttl);
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        let total = self.cache.len();
        let expired = self
            .cache
            .iter()
            .filter(|entry| entry.timestamp.elapsed() >= self.cache_ttl)
            .count();
        (total, expired)
    }

    /// Reverse IP address for DNSBL lookup
    /// Example: 192.0.2.1 becomes 1.2.0.192
    fn reverse_ip(ip: IpAddr) -> String {
        match ip {
            IpAddr::V4(ipv4) => {
                let octets = ipv4.octets();
                format!("{}.{}.{}.{}", octets[3], octets[2], octets[1], octets[0])
            }
            IpAddr::V6(_) => {
                // IPv6 DNSBL support is rare, return as-is
                // Production implementations would convert to reverse nibble format
                ip.to_string()
            }
        }
    }

    /// Check if IP is listed in a specific DNSBL
    /// Returns Some(reason) if listed, None if not listed or on error
    async fn check_single_dnsbl(&self, ip: IpAddr, provider: &str) -> Option<String> {
        let reversed_ip = Self::reverse_ip(ip);
        let query_host = format!("{}.{}", reversed_ip, provider);

        debug!(ip = %ip, provider = %provider, query = %query_host, "performing DNSBL lookup");

        // Perform async DNS lookup with timeout
        match tokio::time::timeout(self.timeout, lookup_host(&format!("{}:0", query_host))).await {
            Ok(Ok(mut addrs)) => {
                // If we get any result, the IP is listed
                if let Some(addr) = addrs.next() {
                    let listed_code = addr.ip();
                    debug!(
                        ip = %ip,
                        provider = %provider,
                        return_code = %listed_code,
                        "IP is listed in DNSBL"
                    );
                    Some(format!("Listed in {} ({})", provider, listed_code))
                } else {
                    None
                }
            }
            Ok(Err(e)) => {
                // DNS error usually means not listed (NXDOMAIN)
                debug!(ip = %ip, provider = %provider, error = %e, "DNSBL lookup returned error (likely not listed)");
                None
            }
            Err(_) => {
                // Timeout
                warn!(ip = %ip, provider = %provider, "DNSBL lookup timeout");
                None
            }
        }
    }

    /// Check if IP is listed in any DNSBL (async DNS lookup)
    /// Uses cache to avoid repeated lookups
    pub async fn check_ip(&self, ip: IpAddr) -> bool {
        // Check whitelist first - whitelisted IPs always pass
        if self.whitelist.contains(&ip) {
            debug!(ip = %ip, "IP is whitelisted, bypassing DNSBL check");
            return false;
        }

        // Check cache first
        if let Some((is_listed, _reason)) = self.get_cached(ip) {
            return is_listed;
        }

        // Check all providers
        for provider in &self.providers {
            if let Some(reason) = self.check_single_dnsbl(ip, provider).await {
                // Cache the positive result
                self.cache_result(ip, true, Some(reason));
                return true;
            }
        }

        // Cache the negative result
        self.cache_result(ip, false, None);
        false
    }

    /// Check all configured DNSBLs and return detailed actions
    /// Uses cache to avoid repeated lookups
    pub async fn check_all_dnsbls(&self, ip: IpAddr) -> Vec<(String, DNSBLAction)> {
        // Check whitelist first - whitelisted IPs always pass
        if self.whitelist.contains(&ip) {
            debug!(ip = %ip, "IP is whitelisted, bypassing DNSBL check");
            return vec![("whitelisted".to_string(), DNSBLAction::Allow)];
        }

        // Check cache first
        if let Some((is_listed, reason)) = self.get_cached(ip) {
            if is_listed {
                let reason_str = reason.unwrap_or_else(|| "Listed in DNSBL".to_string());
                return vec![("cached".to_string(), DNSBLAction::Block(reason_str))];
            }
            return vec![("cached_clean".to_string(), DNSBLAction::Allow)];
        }

        let mut results = Vec::new();

        for provider in &self.providers {
            if let Some(reason) = self.check_single_dnsbl(ip, provider).await {
                results.push((provider.clone(), DNSBLAction::Block(reason.clone())));
                // Cache first positive result
                if results.len() == 1 {
                    self.cache_result(ip, true, Some(reason));
                }
            }
        }

        if results.is_empty() {
            results.push(("clean".to_string(), DNSBLAction::Allow));
            // Cache the negative result
            self.cache_result(ip, false, None);
        }

        results
    }
}

/// Rate limiting manager using governor crate
#[derive(Debug)]
pub struct RateLimitManager {
    // Per-client message rate limiters
    message_limiters: HashMap<u64, DirectRateLimiter>,
    // Per-IP connection rate limiters
    connection_limiters: HashMap<IpAddr, DirectRateLimiter>,
    // Per-client channel join rate limiters
    join_limiters: HashMap<u64, DirectRateLimiter>,

    // LOW-2 COMPLETE: Configurable rate limits (message/connection/join all use config)
    message_rate_per_second: u32,
    connection_burst_per_ip: u32,
    join_burst_per_client: u32,
}

impl Default for RateLimitManager {
    fn default() -> Self {
        Self::new(2, 3, 5)
    }
}

impl RateLimitManager {
    /// Create new RateLimitManager with configurable thresholds (LOW-2 COMPLETE)
    ///
    /// # Arguments
    /// * `message_rate_per_second` - Messages allowed per client per second (default: 2)
    /// * `connection_burst_per_ip` - Connection burst per IP per 10s (default: 3)
    /// * `join_burst_per_client` - Join burst per client per 10s (default: 5)
    pub fn new(
        message_rate_per_second: u32,
        connection_burst_per_ip: u32,
        join_burst_per_client: u32,
    ) -> Self {
        Self {
            message_limiters: HashMap::new(),
            connection_limiters: HashMap::new(),
            join_limiters: HashMap::new(),
            message_rate_per_second,
            connection_burst_per_ip,
            join_burst_per_client,
        }
    }

    /// Update the rate limit values on-the-fly during a REHASH.
    ///
    /// This updates the base rates used to create *new* limiters.
    /// It does not affect existing rate limiters for currently connected clients.
    /// See Issue #2 for full discussion. This is the first safe step.
    pub fn update_limits(
        &mut self,
        message_rate_per_second: u32,
        connection_burst_per_ip: u32,
        join_burst_per_client: u32,
    ) {
        self.message_rate_per_second = message_rate_per_second;
        self.connection_burst_per_ip = connection_burst_per_ip;
        self.join_burst_per_client = join_burst_per_client;
    }

    /// Check if client can send message (rate limiting)
    pub fn check_message_rate(&mut self, client_id: ClientId) -> bool {
        let rate = self.message_rate_per_second;
        let limiter = self.message_limiters.entry(client_id).or_insert_with(|| {
            // SECURITY EXPERT: Configurable message rate (LOW-2 COMPLETE)
            // Config validation ensures > 0; fallback to 2 if somehow zero
            let rate_nz = NonZeroU32::new(rate).unwrap_or(NonZeroU32::new(2).unwrap());
            RateLimiter::direct(Quota::per_second(rate_nz))
        });

        limiter.check().is_ok()
    }

    /// Check if IP can make new connection (rate limiting)
    pub fn check_connection_rate(&mut self, ip: IpAddr) -> bool {
        let burst = self.connection_burst_per_ip;
        let limiter = self.connection_limiters.entry(ip).or_insert_with(|| {
            // SECURITY EXPERT: Configurable connection burst per IP (LOW-2 COMPLETE)
            // Config validation ensures > 0; fallback to 3 if somehow zero
            let burst_nz = NonZeroU32::new(burst).unwrap_or(NonZeroU32::new(3).unwrap());
            RateLimiter::direct(
                Quota::per_second(NonZeroU32::new(1).unwrap())
                    .allow_burst(burst_nz),
            )
        });

        limiter.check().is_ok()
    }

    /// Check if client can join a channel (rate limiting)
    pub fn check_join_rate(&mut self, client_id: u64) -> bool {
        let burst = self.join_burst_per_client;
        let limiter = self.join_limiters.entry(client_id).or_insert_with(|| {
            // SECURITY EXPERT: Configurable join burst per client (LOW-2 COMPLETE)
            // Config validation ensures > 0; fallback to 5 if somehow zero
            let burst_nz = NonZeroU32::new(burst).unwrap_or(NonZeroU32::new(5).unwrap());
            RateLimiter::direct(
                Quota::per_second(NonZeroU32::new(1).unwrap())
                    .allow_burst(burst_nz),
            )
        });

        limiter.check().is_ok()
    }

    /// Cleanup old rate limiters to prevent memory growth
    pub fn cleanup(&mut self) {
        // Remove limiters if we have too many (simple cleanup strategy)
        if self.message_limiters.len() > 1000 {
            self.message_limiters.clear();
        }
        if self.connection_limiters.len() > 1000 {
            self.connection_limiters.clear();
        }
        if self.join_limiters.len() > 1000 {
            self.join_limiters.clear();
        }
    }
}

/// Check if extended ban matches user context
pub fn matches_extended_ban(ban: &ExtendedBan, context: &UserContext) -> bool {
    match ban {
        ExtendedBan::Account(pattern) => {
            if let Some(account) = &context.account {
                wildcard_match(pattern, account)
            } else {
                false
            }
        }
        ExtendedBan::Realname(pattern) => wildcard_match(pattern, &context.realname),
        ExtendedBan::Server(pattern) => wildcard_match(pattern, &context.server),
        ExtendedBan::Channel(pattern) => context
            .channels
            .iter()
            .any(|chan| wildcard_match(pattern, chan)),
        ExtendedBan::Oper(pattern) => {
            if context.is_oper {
                if let Some(oper_type) = &context.oper_type {
                    wildcard_match(pattern, oper_type)
                } else {
                    pattern == "*" // Match any oper if no specific type
                }
            } else {
                false
            }
        }
        ExtendedBan::Certificate(pattern) => {
            if let Some(cert_fp) = &context.certificate_fp {
                wildcard_match(pattern, cert_fp)
            } else {
                false
            }
        }
        ExtendedBan::Unregistered => !context.is_registered,
        ExtendedBan::SASL(pattern) => {
            if let Some(sasl) = &context.sasl_mechanism {
                wildcard_match(pattern, sasl)
            } else {
                false
            }
        }
        ExtendedBan::Join(pattern) => {
            // This would match against recent join patterns - simplified for now
            wildcard_match(pattern, &context.nickname)
        }
    }
}

/// Check if X-line matches user context
pub fn matches_xline(xline: &XLine, context: &UserContext) -> bool {
    if xline.is_expired() {
        return false;
    }

    match xline {
        XLine::KLine { mask, .. } | XLine::GLine { mask, .. } => {
            let user_mask = format!(
                "{}!{}@{}",
                context.nickname, context.username, context.ip_address
            );
            wildcard_match(mask, &user_mask)
        }
        XLine::ZLine { ip, .. } => wildcard_match(ip, &context.ip_address.to_string()),
        XLine::RLine { regex, .. } => {
            if let Ok(re) = Regex::new(regex) {
                let user_string = format!(
                    "{}!{}@{} {}",
                    context.nickname, context.username, context.ip_address, context.realname
                );
                re.is_match(&user_string)
            } else {
                false
            }
        }
        XLine::SLine { mask, .. } => crate::util::wildcard_match_regex(mask, &context.server),
    }
}

/// Simple wildcard matching (* and ? support)
///
/// Re-exports the centralized wildcard matching from util module.
/// Uses regex-based matching with proper escaping for hostmask patterns.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    crate::util::wildcard_match_regex(pattern, text)
}
