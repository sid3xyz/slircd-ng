//! DNSBL (DNS Blocklist) Service
//!
//! Checks incoming connections against industry-standard blocklists (DroneBL, EFnet RBL).
//! Used during the connection handshake to reject known botnets.

use hickory_resolver::config::ResolverConfig;
use hickory_resolver::TokioResolver;
use hickory_resolver::name_server::TokioConnectionProvider;
use std::net::IpAddr;
use std::time::Duration;
use tracing::{debug, warn};

/// Timeout for DNSBL queries to prevent hanging on slow DNS servers.
const DNSBL_TIMEOUT: Duration = Duration::from_secs(3);

/// DNSBL Service for checking IPs against blocklists.
#[derive(Clone)]
pub struct DnsblService {
    resolver: TokioResolver,
    lists: Vec<String>,
}

impl DnsblService {
    /// Create a new DNSBL service with default lists.
    pub fn new() -> Self {
        // Try system config, fall back to defaults
        let resolver = TokioResolver::builder_tokio()
            .map(|b| b.build())
            .unwrap_or_else(|_| {
                TokioResolver::builder_with_config(
                    ResolverConfig::default(),
                    TokioConnectionProvider::default(),
                )
                .build()
            });

        Self {
            resolver,
            lists: vec![
                "dnsbl.dronebl.org".to_string(),
                "rbl.efnetrbl.org".to_string(),
            ],
        }
    }

    /// Check if an IP is listed in any of the configured DNSBLs.
    /// Returns `Some(list_name)` if listed, `None` if clean.
    pub async fn check_ip(&self, ip: IpAddr) -> Option<String> {
        let reversed_ip = match ip {
            IpAddr::V4(ipv4) => {
                let octets = ipv4.octets();
                format!("{}.{}.{}.{}", octets[3], octets[2], octets[1], octets[0])
            }
            IpAddr::V6(_) => {
                // IPv6 DNSBL lookups are complex and less supported; skipping for now
                return None;
            }
        };

        for list in &self.lists {
            let query = format!("{}.{}.", reversed_ip, list);
            debug!("Checking DNSBL: {}", query);

            // Use timeout to prevent hanging on slow DNS servers
            let lookup = self.resolver.lookup_ip(&query);
            match tokio::time::timeout(DNSBL_TIMEOUT, lookup).await {
                Ok(Ok(response)) => {
                    if response.iter().next().is_some() {
                        debug!("IP {} listed in {}", ip, list);
                        return Some(list.clone());
                    }
                }
                Ok(Err(e)) => {
                    // NXDOMAIN means not listed, other errors are ignored
                    if !e.to_string().contains("NXDomain") {
                        warn!("DNSBL lookup failed for {}: {}", list, e);
                    }
                }
                Err(_) => {
                    // Timeout - log and continue to next list
                    warn!("DNSBL lookup timed out for {} ({}s)", list, DNSBL_TIMEOUT.as_secs());
                }
            }
        }

        None
    }
}
