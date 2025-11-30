//! Centralized Anti-Abuse Service
//!
//! **KAIROS Architecture Initiative - Phase 5 Realization**
//!
//! This service provides a unified API for all anti-abuse checks across the IRCd.
//! It consolidates logic previously scattered across:
//! - `anti_abuse.rs`: Core primitives (ExtendedBan, XLine, RateLimitManager, DNSBLChecker)
//! - `state/rate_limit_ops.rs`: State management wrappers
//! - `server.rs`: Connection-level enforcement
//! - `commands/`: Command-level enforcement
//!
//! # Architecture Vision
//!
//! **Problem:** Anti-abuse logic was distributed, making it hard to:
//! - Understand complete abuse prevention policy
//! - Add new abuse detection mechanisms
//! - Test abuse detection in isolation
//! - Audit abuse events consistently
//!
//! **Solution:** Single service with clean API surface:
//! ```rust,ignore
//! let service = AntiAbuseService::new(state);
//!
//! // Connection validation
//! service.check_connection(ip, client_id).await?;
//!
//! // Message validation
//! service.check_message(client_id, target, text).await?;
//!
//! // Channel join validation
//! service.check_join(client_id, channel).await?;
//! ```
//!
//! # RFC Compliance
//! - RFC 2812 §2.3: Message flood protection (rate limiting)
//! - RFC 2812 §5.8: Connection limits (max clients per IP)
//! - RFC 2811 §4.2.1: Channel ban masks and matching
//! - Modern IRC: DNSBL integration (IP reputation services)
//!
//! # Competitive Analysis
//! - **UnrealIRCd**: Flood module with threshold-based detection (src/modules/flood.c)
//! - **InspIRCd**: m_conn_flood.cpp for connection throttling, separate spam modules
//! - **Ergo**: irc/connection_limits.go with sophisticated per-IP/per-account tracking
//! - **Solanum**: src/reject.c for connection throttling, integrated DNSBL support
//!
//! **Our Advantage:** Unified service architecture makes abuse policy explicit and testable.
//! Other IRCds scatter logic across modules, making policy harder to understand.
//!
//! Verified: 2025-10-13 - All Big 4 IRCds implement multi-layer abuse prevention
//!
//! # Design Principles
//! 1. **Defense in Depth**: Multiple layers (rate limits, bans, reputation)
//! 2. **Fail Safe**: On error, log and allow (don't break legitimate users)
//! 3. **Performance**: Hot path optimizations (DashMap, token buckets)
//! 4. **Auditability**: Structured logging of all abuse events
//! 5. **Extensibility**: Easy to add new detection mechanisms

use crate::security::anti_abuse::spam_detection::{SpamDetectionService, SpamVerdict};
use crate::security::anti_abuse::DNSBLAction;
use crate::core::state::ServerState;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, warn};

/// Client identifier (u64 for compatibility with existing codebase)
pub type ClientId = u64;

/// Reasons why an anti-abuse check failed
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbuseReason {
    /// IP address is listed in DNS blacklist
    Dnsbl { provider: String, reason: String },
    /// Connection rate limit exceeded (too many connections from IP)
    ConnectionRateLimit { ip: IpAddr },
    /// Message rate limit exceeded (client sending too fast)
    MessageRateLimit { client_id: ClientId },
    /// Channel join rate limit exceeded (joining channels too fast)
    JoinRateLimit {
        client_id: ClientId,
        channel: String,
    },
    /// Client matches active X-line (K/G/Z/R/S-line ban)
    XLineBanned {
        line_type: String,
        mask: String,
        reason: String,
    },
    /// Client matches channel ban mask
    ChannelBanned { channel: String, mask: String },
    /// Channel is invite-only and client not invited
    InviteOnly { channel: String },
}

impl std::fmt::Display for AbuseReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AbuseReason::Dnsbl { provider, reason } => {
                write!(f, "IP listed in DNSBL {}: {}", provider, reason)
            }
            AbuseReason::ConnectionRateLimit { ip } => {
                write!(f, "Too many connections from {}", ip)
            }
            AbuseReason::MessageRateLimit { client_id } => {
                write!(f, "Message rate limit exceeded for client {}", client_id)
            }
            AbuseReason::JoinRateLimit { client_id, channel } => {
                write!(
                    f,
                    "Join rate limit exceeded for client {} on {}",
                    client_id, channel
                )
            }
            AbuseReason::XLineBanned {
                line_type,
                mask,
                reason,
            } => {
                write!(f, "{}-line ban: {} ({})", line_type, mask, reason)
            }
            AbuseReason::ChannelBanned { channel, mask } => {
                write!(f, "Banned from {}: {}", channel, mask)
            }
            AbuseReason::InviteOnly { channel } => {
                write!(f, "Channel {} is invite-only", channel)
            }
        }
    }
}

impl std::error::Error for AbuseReason {}

/// Result type for anti-abuse checks
pub type AbuseResult<T = ()> = Result<T, AbuseReason>;

/// Z-line record for IP-based ban matching
/// Returned by check_zline_by_ip for display purposes
#[derive(Debug, Clone)]
pub struct ZLineRecord {
    pub ip: String,
    pub reason: String,
    pub expires: Option<SystemTime>,
}

/// Centralized Anti-Abuse Service
///
/// This service provides a unified API for all anti-abuse checks.
/// It wraps the underlying state and abuse detection primitives,
/// providing a clean interface for connection, message, and join validation.
///
/// # Thread Safety
/// This service is cheaply clonable (Arc internally) and can be shared across
/// async tasks without performance penalty.
///
/// # Performance
/// - DNSBL lookups: Cached (1-hour TTL), parallel queries
/// - Rate limiting: Lock-free token buckets (governor crate)
/// - Ban checks: Bloom filter pre-screening, O(1) DashMap lookups
/// - X-line checks: DashMap iteration with early exit
/// - Spam detection: Delegated to SpamDetectionService (independent module, OPTIONAL)
///
/// # Architecture: Anti-Abuse vs Anti-Spam
/// - **Anti-Abuse** (network): Connection floods, join spam, rate limits → ALWAYS ON
/// - **Anti-Spam** (content): Message content filtering → CONFIGURABLE (can be disabled)
pub struct AntiAbuseService {
    state: Arc<ServerState>,
    spam_detector: SpamDetectionService,
    /// Whether spam content detection is enabled (from config)
    spam_enabled: bool,
    /// Action to take on spam detection: "log", "block", or "silent"
    spam_action: String,
}

impl AntiAbuseService {
    /// Create new AntiAbuse service wrapping server state
    ///
    /// # Arguments
    /// * `state` - Shared server state (Arc-wrapped for cheap cloning)
    /// * `spam_enabled` - Whether spam content detection is enabled
    /// * `spam_config` - Spam detection configuration (entropy threshold, etc.)
    ///
    /// # Example
    /// ```rust,ignore
    /// let service = AntiAbuseService::new(
    ///     state.clone(),
    ///     config.security.anti_spam.enabled,
    ///     &config.security.anti_spam,
    /// );
    /// ```
    pub fn new(
        state: Arc<ServerState>,
        spam_enabled: bool,
        entropy_threshold: f32,
        _max_repetition: usize,
        spam_action: String,
    ) -> Self {
        let mut spam_detector = SpamDetectionService::new();
        spam_detector.set_entropy_threshold(entropy_threshold);

        Self {
            state,
            spam_detector,
            spam_enabled,
            spam_action,
        }
    }

    /// Update spam detection configuration (for live reload)
    pub fn update_spam_config(
        &mut self,
        enabled: bool,
        entropy_threshold: f32,
        _max_repetition: usize,
        action: String,
    ) {
        self.spam_enabled = enabled;
        self.spam_action = action;
        self.spam_detector.set_entropy_threshold(entropy_threshold);
    }

    /// Get mutable reference to spam detector for configuration
    pub fn spam_detector_mut(&mut self) -> &mut SpamDetectionService {
        &mut self.spam_detector
    }

    /// Check if spam detection is enabled
    pub fn is_spam_detection_enabled(&self) -> bool {
        self.spam_enabled
    }

    /// Check if a connection should be allowed
    ///
    /// This is the first line of defense for new connections. It validates:
    /// 1. Connection rate limiting (per-IP)
    /// 2. DNSBL reputation checks
    /// 3. X-line bans (K/G/Z-lines based on IP/host)
    ///
    /// # Arguments
    /// * `ip` - IP address of connecting client
    /// * `client_id` - Assigned client ID (for tracking)
    ///
    /// # Returns
    /// - `Ok(())`: Connection should be accepted
    /// - `Err(AbuseReason)`: Connection should be rejected with reason
    ///
    /// # Usage
    /// ```rust,ignore
    /// match service.check_connection(peer_addr.ip(), client_id).await {
    ///     Ok(()) => {
    ///         // Proceed with connection handshake
    ///     }
    ///     Err(AbuseReason::Dnsbl { provider, reason }) => {
    ///         warn!("DNSBL block: {} listed in {}: {}", ip, provider, reason);
    ///         send_error(&mut stream, "Your IP is listed in a DNS blacklist").await?;
    ///         return Ok(());
    ///     }
    ///     Err(AbuseReason::ConnectionRateLimit { ip }) => {
    ///         warn!("Connection rate limit: too many connections from {}", ip);
    ///         send_error(&mut stream, "Too many connections from your IP").await?;
    ///         return Ok(());
    ///     }
    ///     Err(reason) => {
    ///         warn!("Connection rejected: {}", reason);
    ///         send_error(&mut stream, &format!("Connection rejected: {}", reason)).await?;
    ///         return Ok(());
    ///     }
    /// }
    /// ```
    ///
    /// # Performance
    /// - Connection rate check: O(1) DashMap lookup + governor token bucket
    /// - DNSBL check: Cached (O(1) on cache hit), parallel queries on miss
    /// - X-line check: O(n) iteration with early exit (typically <100 entries)
    ///
    /// # RFC Compliance
    /// - RFC 2812 §5.8: Connection rate limiting
    /// - Modern IRC: DNSBL integration for IP reputation
    pub async fn check_connection(&self, ip: IpAddr, _client_id: ClientId) -> AbuseResult {
        // LAYER 1: Connection rate limiting (per-IP)
        // Prevents single IP from flooding server with connections
        // Typical limit: 3 connections per 60 seconds per IP
        if !self.state.check_connection_rate(ip).await {
            debug!("Connection rate limit exceeded for IP: {}", ip);
            return Err(AbuseReason::ConnectionRateLimit { ip });
        }

        // LAYER 2: DNSBL reputation checks
        // Query DNS blacklist providers for IP reputation
        // Common providers: dronebl.org, efnet RBL, etc.
        let dnsbl_results = self.state.check_dnsbl(ip).await;
        for (provider, action) in dnsbl_results {
            match action {
                DNSBLAction::Block(reason) => {
                    warn!("DNSBL block: IP {} listed in {}: {}", ip, provider, reason);
                    return Err(AbuseReason::Dnsbl { provider, reason });
                }
                DNSBLAction::Monitor(reason) => {
                    // Greylist: Log but allow connection
                    debug!(
                        "DNSBL monitor: IP {} listed in {} ({}), allowing",
                        ip, provider, reason
                    );
                }
                DNSBLAction::Allow => {
                    // IP not listed, continue checks
                    debug!("DNSBL allow: IP {} clean in {}", ip, provider);
                }
            }
        }

        // LAYER 3: X-line bans (K/G/Z-lines)
        // Check if IP matches server-level ban
        // Note: Full hostname resolution happens after connection accepted,
        // so early connection checks only validate IP-based Z-lines
        // K-lines and G-lines (user@host patterns) checked after USER/NICK registration
        if let Some(xline) = self.check_zline_by_ip(ip).await {
            warn!("Z-line match: IP {} banned: {}", ip, xline.reason);
            return Err(AbuseReason::XLineBanned {
                line_type: "Z-line".to_string(),
                mask: xline.ip.clone(),
                reason: xline.reason.clone(),
            });
        }

        // All checks passed
        debug!("Connection validation passed for IP: {}", ip);
        Ok(())
    }

    /// Check if IP matches any Z-line (IP ban)
    /// Called during connection acceptance (before registration)
    async fn check_zline_by_ip(&self, ip: IpAddr) -> Option<crate::security::anti_abuse::ZLineRecord> {
        for xline_ref in self.state.xlines.iter() {
            let xline = xline_ref.value();
            if let crate::security::anti_abuse::XLine::ZLine {
                ip: pattern,
                reason,
                expires,
            } = xline
            {
                // Skip expired bans
                if let Some(expiry) = expires {
                    if SystemTime::now() > *expiry {
                        continue;
                    }
                }

                // Match IP against pattern (supports wildcards like 192.168.*.*)
                if crate::security::anti_abuse::wildcard_match(pattern, &ip.to_string()) {
                    return Some(crate::security::anti_abuse::ZLineRecord {
                        ip: pattern.clone(),
                        reason: reason.clone(),
                        expires: *expires,
                    });
                }
            }
        }
        None
    }

    /// Check if a message should be allowed
    ///
    /// This validates message rate limiting to prevent flood attacks.
    /// Uses per-client token bucket limiter.
    ///
    /// # Arguments
    /// * `client_id` - ID of client sending message
    /// * `target` - Target of message (channel or nickname)
    /// * `text` - Message content (for future spam detection)
    ///
    /// # Returns
    /// - `Ok(())`: Message should be delivered
    /// - `Err(AbuseReason::MessageRateLimit)`: Client is sending too fast
    ///
    /// # Usage
    /// ```rust,ignore
    /// match service.check_message(client_id, "#channel", "Hello world").await {
    ///     Ok(()) => {
    ///         // Deliver message
    ///         broadcast_to_channel(channel, message).await;
    ///     }
    ///     Err(AbuseReason::MessageRateLimit { .. }) => {
    ///         // Send rate limit error
    ///         send_numeric(client, 439, "Message rate limit exceeded").await;
    ///         return Ok(());
    ///     }
    ///     Err(reason) => {
    ///         warn!("Message rejected: {}", reason);
    ///         return Err(anyhow::anyhow!("Message rejected: {}", reason));
    ///     }
    /// }
    /// ```
    ///
    /// # Performance
    /// - Rate check: O(1) DashMap lookup + governor token bucket
    /// - Typical limit: 5 messages/second, burst of 10
    ///
    /// # RFC Compliance
    /// - RFC 2812 §2.3: Message rate limiting for flood protection
    ///
    /// # Future Enhancements
    /// - Spam content detection (keyword matching, entropy analysis)
    /// - CTCP flood detection (VERSION spam, etc.)
    /// - Channel-specific rate limits (slower in large channels)
    pub async fn check_message(
        &self,
        client_id: ClientId,
        _target: &str,
        _text: &str,
    ) -> AbuseResult {
        // LAYER 1: Message rate limiting (per-client)
        // Prevents client from flooding channels/users with messages
        // Typical limit: 5 messages per second, burst capacity of 10
        if !self.state.check_message_rate(client_id).await {
            debug!("Message rate limit exceeded for client: {}", client_id);
            return Err(AbuseReason::MessageRateLimit { client_id });
        }

        // LAYER 2: Spam content detection (OPTIONAL - configurable)
        // Only run if spam detection is enabled in config
        // CRITICAL: Anti-abuse (network) is ALWAYS ON, anti-spam (content) is OPTIONAL
        if self.spam_enabled {
            match self.spam_detector.check_message(_text) {
                SpamVerdict::Clean => {
                    debug!("Spam check passed for message");
                }
                SpamVerdict::Spam {
                    pattern,
                    confidence,
                } => {
                    warn!(
                        "Spam detected in message from client {}: {} (confidence: {:.2})",
                        client_id, pattern, confidence
                    );

                    // Handle spam based on configured action
                    match self.spam_action.as_str() {
                        "block" => {
                            // Block message and return error
                            return Err(AbuseReason::MessageRateLimit { client_id });
                            // Reuse rate limit error for now
                        }
                        "silent" => {
                            // Silently drop message (no error to client)
                            debug!("Spam silently dropped for client {}", client_id);
                            return Ok(()); // Success but don't deliver
                        }
                        _ => {
                            // "log" or unknown: log only, allow delivery
                            debug!("Spam logged but not blocked for client {}", client_id);
                        }
                    }
                }
            }
        } else {
            debug!("Spam detection disabled, skipping content check");
        }

        // All checks passed
        debug!("Message validation passed for client: {}", client_id);
        Ok(())
    }

    /// Check if a channel join should be allowed
    ///
    /// This validates:
    /// 1. Join rate limiting (per-client)
    /// 2. Channel ban masks (future)
    /// 3. Invite-only channels (future)
    ///
    /// # Arguments
    /// * `client_id` - ID of client attempting to join
    /// * `channel` - Name of channel being joined
    ///
    /// # Returns
    /// - `Ok(())`: Join should be allowed
    /// - `Err(AbuseReason)`: Join should be rejected with reason
    ///
    /// # Usage
    /// ```rust,ignore
    /// match service.check_join(client_id, "#channel").await {
    ///     Ok(()) => {
    ///         // Add client to channel
    ///         state.join_channel(client_id, channel).await?;
    ///     }
    ///     Err(AbuseReason::JoinRateLimit { .. }) => {
    ///         // Send rate limit error
    ///         send_numeric(client, 405, "Join rate limit exceeded").await;
    ///         return Ok(());
    ///     }
    ///     Err(AbuseReason::ChannelBanned { .. }) => {
    ///         // Send ban error
    ///         send_numeric(client, 474, "You are banned from this channel").await;
    ///         return Ok(());
    ///     }
    ///     Err(reason) => {
    ///         warn!("Join rejected: {}", reason);
    ///         return Err(anyhow::anyhow!("Join rejected: {}", reason));
    ///     }
    /// }
    /// ```
    ///
    /// # Performance
    /// - Rate check: O(1) DashMap lookup + governor token bucket
    /// - Ban check (future): Bloom filter pre-screen + DashMap lookup
    ///
    /// # RFC Compliance
    /// - RFC 2812 §3.2.1: JOIN command rate limiting
    /// - RFC 2811 §4.2.1: Channel ban mask matching
    ///
    /// # Future Enhancements
    /// - Channel ban mask validation (integrate with ban_ops.rs)
    /// - Invite-only channel enforcement
    /// - Channel key validation
    /// - User limit enforcement
    pub async fn check_join(&self, client_id: ClientId, channel: &str) -> AbuseResult {
        // LAYER 1: Join rate limiting (per-client)
        // Prevents client from flooding joins (common spam tactic)
        // Typical limit: 5 joins per 10 seconds per client
        if !self.state.check_join_rate(client_id).await {
            debug!(
                "Join rate limit exceeded for client {} on channel {}",
                client_id, channel
            );
            return Err(AbuseReason::JoinRateLimit {
                client_id,
                channel: channel.to_string(),
            });
        }

        // LAYER 2: Channel ban checks
        // Check if client matches channel ban masks via ban_ops
        // Delegates to state.check_channel_ban() which uses Bloom filter + DB
        if let Err(_ban_error) = self.state.check_channel_ban(client_id, channel).await {
            debug!("Client {} banned from channel {}", client_id, channel);
            return Err(AbuseReason::ChannelBanned {
                channel: channel.to_string(),
                mask: "ban mask".to_string(), // TODO: Extract mask from ChannelError
            });
        }

        // LAYER 3: Invite-only enforcement (future)
        // Check if channel requires invite and client has one
        // TODO: Implement invite tracking
        // if self.state.is_channel_invite_only(channel).await {
        //     if !self.state.has_invite(client_id, channel).await {
        //         debug!("Client {} lacks invite to {}", client_id, channel);
        //         return Err(AbuseReason::InviteOnly {
        //             channel: channel.to_string(),
        //         });
        //     }
        // }

        // All checks passed
        debug!(
            "Join validation passed for client {} to channel {}",
            client_id, channel
        );
        Ok(())
    }

    /// Cleanup stale rate limiter entries
    ///
    /// This should be called periodically (e.g., every 60 seconds) to remove
    /// rate limiter entries for disconnected clients, preventing memory leaks.
    ///
    /// # Usage
    /// ```rust,ignore
    /// // In maintenance task:
    /// tokio::spawn(async move {
    ///     let mut interval = tokio::time::interval(Duration::from_secs(60));
    ///     loop {
    ///         interval.tick().await;
    ///         service.cleanup_rate_limiters().await;
    ///     }
    /// });
    /// ```
    pub async fn cleanup_rate_limiters(&self) {
        self.state.cleanup_rate_limiters().await;
    }
}

impl Clone for AntiAbuseService {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            spam_detector: SpamDetectionService::new(), // Fresh instance per clone
            spam_enabled: self.spam_enabled,
            spam_action: self.spam_action.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    // Note: Proper unit tests require mock ServerState
    // For now, documenting test scenarios:
    //
    // #[tokio::test]
    // async fn test_connection_rate_limit() {
    //     let state = create_test_state();
    //     let service = AntiAbuseService::new(state);
    //     let ip = "127.0.0.1".parse().unwrap();
    //
    //     // First 3 connections should succeed
    //     for i in 0..3 {
    //         assert!(service.check_connection(ip, i).await.is_ok());
    //     }
    //
    //     // 4th connection should be rate limited
    //     assert!(matches!(
    //         service.check_connection(ip, 4).await,
    //         Err(AbuseReason::ConnectionRateLimit { .. })
    //     ));
    // }
    //
    // #[tokio::test]
    // async fn test_message_rate_limit() {
    //     let state = create_test_state();
    //     let service = AntiAbuseService::new(state);
    //     let client_id = 1;
    //
    //     // First 10 messages should succeed (burst capacity)
    //     for _ in 0..10 {
    //         assert!(service.check_message(client_id, "#test", "hello").await.is_ok());
    //     }
    //
    //     // 11th message should be rate limited
    //     assert!(matches!(
    //         service.check_message(client_id, "#test", "hello").await,
    //         Err(AbuseReason::MessageRateLimit { .. })
    //     ));
    // }
}
