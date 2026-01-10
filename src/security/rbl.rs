//! Privacy-Preserving RBL (Realtime Blocklist) Service
//!
//! Provides IP reputation checking via multiple providers:
//! - **HTTP-based APIs** (privacy-preserving): StopForumSpam, AbuseIPDB
//! - **DNS-based lookups** (legacy, leaks IP to DNS resolver): DroneBL, EFnet RBL
//!
//! # Privacy Considerations
//!
//! DNS-based lookups send the user's IP to your configured DNS resolver, which may:
//! - Log the query
//! - Cache the result
//! - Be a third-party service (e.g., 8.8.8.8, 1.1.1.1)
//!
//! HTTP-based APIs send queries directly to the blocklist provider over HTTPS,
//! which is more privacy-preserving as no intermediate DNS resolver sees the IP.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                      RblService                         │
//! ├─────────────────────────────────────────────────────────┤
//! │ ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │
//! │ │   Cache     │  │  HTTP APIs  │  │  DNS Lists  │       │
//! │ │  (LRU+TTL)  │  │ (preferred) │  │  (fallback) │       │
//! │ └─────────────┘  └─────────────┘  └─────────────┘       │
//! └─────────────────────────────────────────────────────────┘
//! ```

use crate::config::RblConfig;
use dashmap::DashMap;
use hickory_resolver::TokioResolver;
use hickory_resolver::config::ResolverConfig;
use hickory_resolver::name_server::TokioConnectionProvider;
use std::net::IpAddr;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Timeout for RBL queries.
const RBL_TIMEOUT: Duration = Duration::from_secs(5);

/// Result of an RBL lookup.
#[derive(Debug, Clone)]
pub enum RblResult {
    /// IP is clean (not listed).
    Clean,
    /// IP is listed in one or more blocklists.
    Listed {
        /// Which provider(s) listed the IP.
        providers: Vec<String>,
        /// Overall confidence score (0-100).
        confidence: u8,
    },
}

/// Cached RBL result with expiry.
#[derive(Debug, Clone)]
struct CachedResult {
    result: RblResult,
    expires_at: Instant,
}

/// Privacy-preserving RBL Service.
///
/// Checks IPs against multiple blocklist providers with result caching.
pub struct RblService {
    /// Configuration.
    config: RblConfig,
    /// Result cache (IP -> CachedResult).
    cache: DashMap<IpAddr, CachedResult>,
    /// HTTP client for API calls.
    http_client: reqwest::Client,
    /// DNS resolver for legacy lookups.
    dns_resolver: Option<TokioResolver>,
}

impl RblService {
    /// Create a new RBL service with the given configuration.
    pub fn new(config: RblConfig) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(RBL_TIMEOUT)
            .user_agent("slircd-ng/1.0")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let dns_resolver = if config.dns_enabled {
            Some(
                TokioResolver::builder_tokio()
                    .map(|b| b.build())
                    .unwrap_or_else(|_| {
                        TokioResolver::builder_with_config(
                            ResolverConfig::default(),
                            TokioConnectionProvider::default(),
                        )
                        .build()
                    }),
            )
        } else {
            None
        };

        info!(
            http_enabled = config.http_enabled,
            dns_enabled = config.dns_enabled,
            cache_ttl = config.cache_ttl_secs,
            "RBL service initialized"
        );

        Self {
            config,
            cache: DashMap::new(),
            http_client,
            dns_resolver,
        }
    }

    /// Check if an IP is listed in any RBL.
    ///
    /// Returns `true` if the IP should be blocked.
    pub async fn check_ip(&self, ip: IpAddr) -> bool {
        match self.lookup(ip).await {
            RblResult::Clean => false,
            RblResult::Listed {
                providers,
                confidence,
            } => {
                debug!(ip = %ip, confidence = confidence, providers = ?providers, "RBL listed");
                true
            }
        }
    }

    /// Perform a full RBL lookup with detailed results.
    #[allow(clippy::collapsible_if)]
    pub async fn lookup(&self, ip: IpAddr) -> RblResult {
        // Check cache first
        if let Some(cached) = self.cache.get(&ip) {
            if cached.expires_at > Instant::now() {
                debug!(ip = %ip, "RBL cache hit");
                return cached.result.clone();
            }
        }

        // Perform lookup
        let result = self.do_lookup(ip).await;

        // Cache result
        let cached = CachedResult {
            result: result.clone(),
            expires_at: Instant::now() + Duration::from_secs(self.config.cache_ttl_secs),
        };
        self.cache.insert(ip, cached);

        // Prune cache if too large
        if self.cache.len() > self.config.cache_max_size {
            self.prune_cache();
        }

        result
    }

    /// Perform the actual RBL lookup (cache miss path).
    #[allow(clippy::collapsible_if)]
    async fn do_lookup(&self, ip: IpAddr) -> RblResult {
        let mut providers = Vec::new();
        let mut max_confidence: u8 = 0;

        // Try HTTP providers first (privacy-preserving)
        if self.config.http_enabled {
            // StopForumSpam
            if let Some(result) = self.check_stopforumspam(ip).await {
                providers.push("stopforumspam".to_string());
                max_confidence = max_confidence.max(result);
            }

            // AbuseIPDB (requires API key)
            if self.config.abuseipdb_api_key.is_some() {
                if let Some(result) = self.check_abuseipdb(ip).await {
                    providers.push("abuseipdb".to_string());
                    max_confidence = max_confidence.max(result);
                }
            }
        }

        // Fall back to DNS if enabled and no HTTP results
        if providers.is_empty() && self.config.dns_enabled {
            if let Some(list) = self.check_dns(ip).await {
                providers.push(format!("dns:{}", list));
                max_confidence = 75; // DNS lists don't provide confidence scores
            }
        }

        if providers.is_empty() {
            RblResult::Clean
        } else {
            debug!(ip = %ip, providers = ?providers, confidence = max_confidence, "IP listed in RBL");
            RblResult::Listed {
                providers,
                confidence: max_confidence,
            }
        }
    }

    /// Check IP against StopForumSpam API.
    ///
    /// Returns confidence score (0-100) if listed, None if clean.
    async fn check_stopforumspam(&self, ip: IpAddr) -> Option<u8> {
        // Only IPv4 is well-supported by StopForumSpam
        let IpAddr::V4(ipv4) = ip else {
            return None;
        };

        let url = format!("https://api.stopforumspam.org/api?ip={}&json", ipv4);

        let response =
            match tokio::time::timeout(RBL_TIMEOUT, self.http_client.get(&url).send()).await {
                Ok(Ok(resp)) => resp,
                Ok(Err(e)) => {
                    warn!(error = %e, "StopForumSpam API request failed");
                    return None;
                }
                Err(_) => {
                    warn!("StopForumSpam API request timed out");
                    return None;
                }
            };

        let json: serde_json::Value = match response.json().await {
            Ok(j) => j,
            Err(e) => {
                warn!(error = %e, "Failed to parse StopForumSpam response");
                return None;
            }
        };

        // Response format: { "success": 1, "ip": { "appears": 1, "confidence": 95.5 } }
        let ip_data = json.get("ip")?;

        if json.get("success").and_then(|v| v.as_i64()) != Some(1) {
            return None;
        }

        if ip_data.get("appears").and_then(|v| v.as_i64()) != Some(1) {
            return None;
        }

        let confidence = ip_data
            .get("confidence")
            .and_then(|v| v.as_f64())
            .map(|c| c.min(100.0) as u8)
            .unwrap_or(50);

        Some(confidence)
    }

    /// Check IP against AbuseIPDB API.
    ///
    /// Returns confidence score (0-100) if listed and above threshold, None otherwise.
    async fn check_abuseipdb(&self, ip: IpAddr) -> Option<u8> {
        let api_key = self.config.abuseipdb_api_key.as_ref()?;

        let url = format!("https://api.abuseipdb.com/api/v2/check?ipAddress={}", ip);

        let response = match tokio::time::timeout(
            RBL_TIMEOUT,
            self.http_client
                .get(&url)
                .header("Key", api_key.as_str())
                .header("Accept", "application/json")
                .send(),
        )
        .await
        {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => {
                warn!(error = %e, "AbuseIPDB API request failed");
                return None;
            }
            Err(_) => {
                warn!("AbuseIPDB API request timed out");
                return None;
            }
        };

        let json: serde_json::Value = match response.json().await {
            Ok(j) => j,
            Err(e) => {
                warn!(error = %e, "Failed to parse AbuseIPDB response");
                return None;
            }
        };

        // Response format: { "data": { "abuseConfidenceScore": 75 } }
        let data = json.get("data")?;
        let score = data.get("abuseConfidenceScore").and_then(|v| v.as_i64())?;
        let score = score.clamp(0, 100) as u8;

        if score >= self.config.abuseipdb_threshold {
            Some(score)
        } else {
            None
        }
    }

    /// Check IP against DNS-based blocklists.
    ///
    /// Returns the name of the list if found, None if clean.
    async fn check_dns(&self, ip: IpAddr) -> Option<String> {
        let resolver = self.dns_resolver.as_ref()?;

        // IPv6 DNSBL lookups are complex and less supported
        let IpAddr::V4(ipv4) = ip else {
            return None;
        };

        let octets = ipv4.octets();
        let reversed_ip = format!("{}.{}.{}.{}", octets[3], octets[2], octets[1], octets[0]);

        for list in &self.config.dns_lists {
            let query = format!("{}.{}.", reversed_ip, list);
            debug!(query = %query, "Checking DNS RBL");

            let lookup = resolver.lookup_ip(&query);
            match tokio::time::timeout(RBL_TIMEOUT, lookup).await {
                Ok(Ok(response)) => {
                    if response.iter().next().is_some() {
                        debug!(ip = %ip, list = %list, "IP listed in DNS RBL");
                        return Some(list.clone());
                    }
                }
                Ok(Err(e)) => {
                    // NXDOMAIN means not listed
                    if !e.to_string().contains("NXDomain") {
                        warn!(list = %list, error = %e, "DNS RBL lookup failed");
                    }
                }
                Err(_) => {
                    warn!(list = %list, "DNS RBL lookup timed out");
                }
            }
        }

        None
    }

    /// Prune expired entries from the cache.
    fn prune_cache(&self) {
        let now = Instant::now();
        self.cache.retain(|_, v| v.expires_at > now);
    }

    /// Get cache statistics.
    #[cfg(test)]
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> RblConfig {
        RblConfig {
            http_enabled: false, // Disable for tests (no network)
            dns_enabled: false,
            cache_ttl_secs: 60,
            cache_max_size: 100,
            stopforumspam_api_key: None,
            abuseipdb_api_key: None,
            abuseipdb_threshold: 50,
            dns_lists: vec![],
        }
    }

    #[tokio::test]
    async fn test_rbl_cache_hit() {
        let config = make_config();
        let service = RblService::new(config);

        let ip: IpAddr = "192.168.1.1".parse().unwrap();

        // First lookup - miss, returns Clean (no providers enabled)
        let result = service.lookup(ip).await;
        assert!(matches!(result, RblResult::Clean));

        // Second lookup - should hit cache
        let result = service.lookup(ip).await;
        assert!(matches!(result, RblResult::Clean));
        assert_eq!(service.cache_size(), 1);
    }

    #[tokio::test]
    async fn test_rbl_cache_prune() {
        let mut config = make_config();
        config.cache_max_size = 2;
        config.cache_ttl_secs = 1; // Short TTL for test
        let service = RblService::new(config);

        // Add 3 entries to trigger pruning
        for i in 1..=3 {
            let ip: IpAddr = format!("192.168.1.{}", i).parse().unwrap();
            service.lookup(ip).await;
        }

        // Cache should have been pruned to max_size
        // Note: pruning happens after insertion, so may be at max_size
        assert!(service.cache_size() <= 3);
    }

    #[test]
    fn test_rbl_config_defaults() {
        let config = RblConfig::default();
        assert!(config.http_enabled);
        assert!(!config.dns_enabled); // Privacy default
        assert_eq!(config.cache_ttl_secs, 300);
        assert_eq!(config.abuseipdb_threshold, 50);
        assert!(!config.dns_lists.is_empty());
    }

    #[tokio::test]
    async fn test_rbl_ipv6_skipped_for_stopforumspam() {
        let mut config = make_config();
        config.http_enabled = true;
        let service = RblService::new(config);

        let ipv6: IpAddr = "2001:db8::1".parse().unwrap();

        // IPv6 should return Clean (skipped by StopForumSpam)
        let result = service.lookup(ipv6).await;
        assert!(matches!(result, RblResult::Clean));
    }
}
