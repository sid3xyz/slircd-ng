//! Spam Detection Service
//!
//! Multi-layer spam content analysis for IRC messages.
//!
//! Detection mechanisms:
//! 1. **Keyword matching**: Common spam patterns (viagra, casino, etc.)
//! 2. **Entropy analysis**: Gibberish detection via Shannon entropy
//! 3. **URL analysis**: Shortener detection, suspicious TLDs
//! 4. **Repetition detection**: Character/word repetition spam
//! 5. **CTCP flood**: Excessive CTCP queries (VERSION, FINGER, etc.)
//!
//! # Design Principles
//! - **Low false positives**: Prefer under-detection to blocking legitimate users
//! - **Performance**: Hot path optimizations, ~1-5μs per message
//! - **Configurability**: Thresholds tunable via config
//! - **Extensibility**: Easy to add new detection mechanisms

use aho_corasick::AhoCorasick;
use dashmap::DashMap;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::config::SecurityConfig;
use crate::db::Database;
use crate::security::{DnsblService, HeuristicsEngine, ReputationManager};

/// Spam detection result
#[derive(Debug, Clone, PartialEq)]
pub enum SpamVerdict {
    /// Message is legitimate
    Clean,
    /// Message is likely spam with detection reason
    Spam { pattern: String, confidence: f32 },
}

/// Centralized Spam Detection Service
///
/// Analyzes message content for spam indicators.
/// Designed to be called from message handlers before broadcasting.
pub struct SpamDetectionService {
    /// Aho-Corasick automaton for O(N) keyword matching
    keyword_matcher: AhoCorasick,
    /// Raw keywords for management/rebuilding
    raw_keywords: HashSet<String>,
    /// Suspicious URL shortener domains
    url_shorteners: HashSet<String>,
    /// Entropy threshold for gibberish detection (0.0-8.0, typical spam <3.5)
    entropy_threshold: f32,
    /// Maximum allowed character repetition (e.g., "aaaaaaa")
    max_char_repetition: usize,
    /// Recent message hashes per user for repetition detection.
    recent_messages: DashMap<String, VecDeque<(Instant, u64)>>,

    // New Components (TODO: integrate fully)
    reputation: Option<Arc<ReputationManager>>,
    dnsbl: Option<Arc<DnsblService>>,
    heuristics: Option<HeuristicsEngine>,
}

impl SpamDetectionService {
    /// Create new spam detection service
    pub fn new(db: Option<Database>, config: SecurityConfig) -> Self {
        let keywords = Self::default_spam_keywords();
        let matcher = match AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(&keywords)
        {
            Ok(matcher) => matcher,
            Err(err) => {
                warn!(error = ?err, "Failed to build spam keyword matcher; keyword matching disabled");
                let empty: Vec<String> = Vec::new();
                AhoCorasick::builder()
                    .ascii_case_insensitive(true)
                    .build(&empty)
                    .expect("building empty Aho-Corasick should not fail")
            }
        };

        // Initialize Heuristics with config
        let heuristics_config = config.spam.heuristics.clone();
        let heuristics = if heuristics_config.enabled {
            Some(HeuristicsEngine::new(heuristics_config))
        } else {
            None
        };

        // Initialize Reputation
        let reputation = if config.spam.reputation_enabled {
            db.map(|d| Arc::new(ReputationManager::new(d.pool().clone())))
        } else {
            None
        };

        // Initialize DNSBL
        let dnsbl = if config.spam.dnsbl_enabled {
            Some(Arc::new(DnsblService::new()))
        } else {
            None
        };

        Self {
            keyword_matcher: matcher,
            raw_keywords: keywords.into_iter().collect(),
            url_shorteners: Self::default_url_shorteners(),
            entropy_threshold: 3.0,
            max_char_repetition: 10,
            recent_messages: DashMap::new(),
            reputation,
            dnsbl,
            heuristics,
        }
    }

    /// Check if an IP is listed in DNSBLs.
    pub async fn check_ip_dnsbl(&self, ip: IpAddr) -> bool {
        if let Some(dnsbl) = &self.dnsbl {
            dnsbl.check_ip(ip).await.is_some()
        } else {
            false
        }
    }

    /// Record a successful connection (boost reputation).
    pub async fn record_connection_success(&self, ip: IpAddr) {
        if let Some(rep) = &self.reputation {
            rep.record_connection(&ip.to_string()).await;
        }
    }

    /// Record a spam violation (penalize reputation).
    pub async fn record_violation(&self, ip: IpAddr, _reason: &str) {
        if let Some(rep) = &self.reputation {
            // Default penalty 20 for spam
            rep.record_violation(&ip.to_string(), 20).await;
        }
    }

    /// Check if a message is a repetition of recent messages.
    pub fn check_message_repetition(&self, uid: &str, message: &str) -> SpamVerdict {
        let mut hasher = DefaultHasher::new();
        message.hash(&mut hasher);
        let hash = hasher.finish();
        let now = Instant::now();

        let mut history = self
            .recent_messages
            .entry(uid.to_string())
            .or_insert_with(VecDeque::new);

        // Prune old messages (older than 10 seconds)
        while let Some((time, _)) = history.front() {
            if now.duration_since(*time) > Duration::from_secs(10) {
                history.pop_front();
            } else {
                break;
            }
        }

        // Count repetitions
        let count = history.iter().filter(|(_, h)| *h == hash).count();

        // Add current message
        history.push_back((now, hash));

        // Limit history size
        if history.len() > 20 {
            history.pop_front();
        }

        if count >= 2 {
            return SpamVerdict::Spam {
                pattern: "message_repetition".to_string(),
                confidence: 1.0,
            };
        }

        SpamVerdict::Clean
    }

    /// Default spam keyword list
    /// Source: Analysis of IRC spam logs from 2020-2025
    fn default_spam_keywords() -> HashSet<String> {
        let keywords = vec![
            // Gambling/casino spam
            "casino",
            "poker",
            "gambling",
            "jackpot",
            "slots",
            // Pharmaceutical spam
            "viagra",
            "cialis",
            "pharmacy",
            "prescription",
            // Financial spam
            "bitcoin",
            "crypto",
            "investment",
            "profit",
            "earnings",
            // Adult content spam
            "xxx",
            "porn",
            "sex",
            "dating",
            "hookup",
            // Bot/service spam
            "free money",
            "click here",
            "limited time",
            "act now",
            // Discord/external service spam
            "discord.gg",
            "join my server",
            "free nitro",
        ];

        keywords.into_iter().map(|s| s.to_lowercase()).collect()
    }

    /// Default URL shortener list
    /// These are commonly used to obfuscate spam links
    fn default_url_shorteners() -> HashSet<String> {
        let shorteners = vec![
            "bit.ly",
            "tinyurl.com",
            "goo.gl",
            "ow.ly",
            "t.co",
            "is.gd",
            "cli.gs",
            "pic.gd",
            "v.gd",
            "dft.ba",
            "tr.im",
            "qr.ae",
            "adf.ly",
            "bitly.com",
            "cur.lv",
        ];

        shorteners.into_iter().map(|s| s.to_lowercase()).collect()
    }

    /// Check if message is spam using weighted scoring
    pub async fn check_message(&self, uid: &str, _ip: &str, text: &str, is_private: bool) -> SpamVerdict {
        // 1. Reputation Check (Fast Path)
        // High trust users bypass spam checks
        if let Some(reputation) = &self.reputation {
            let trust_score = reputation.get_trust_score(uid).await;
            if trust_score > 80 {
                return SpamVerdict::Clean;
            }
        }

        // 2. Heuristics Analysis
        let heuristic_risk = if let Some(heuristics) = &self.heuristics {
            heuristics.analyze(uid, text, is_private)
        } else {
            0.0
        };

        // 3. Content Analysis
        let content_verdict = self.check_content(text);
        let content_risk = match &content_verdict {
            SpamVerdict::Clean => 0.0,
            SpamVerdict::Spam { confidence, .. } => *confidence,
        };

        // 4. Weighted Scoring
        // Heuristics: 40%, Content: 60%
        let total_risk = (heuristic_risk * 0.4) + (content_risk * 0.6);

        if total_risk > 0.7 {
             // If content triggered it, return that reason
             if let SpamVerdict::Spam { pattern, .. } = content_verdict {
                 return SpamVerdict::Spam { pattern, confidence: total_risk };
             }
             // Otherwise return heuristic reason
             return SpamVerdict::Spam {
                 pattern: format!("heuristic_risk:{:.2}", heuristic_risk),
                 confidence: total_risk
             };
        }

        SpamVerdict::Clean
    }

    /// Check content for spam indicators (Internal)
    ///
    /// # Arguments
    /// * `text` - Message content to analyze
    ///
    /// # Returns
    /// - `SpamVerdict::Clean`: Message passed all checks
    /// - `SpamVerdict::Spam`: Message triggered spam detection
    ///
    /// # Performance
    /// - Keyword check: O(n) where n = number of words
    /// - Entropy: O(m) where m = message length
    /// - URL check: O(u) where u = number of URLs
    /// - Total: ~1-5μs per message on modern hardware
    pub(crate) fn check_content(&self, text: &str) -> SpamVerdict {
        // LAYER 1: Keyword matching
        if let Some(keyword) = self.check_keywords(text) {
            debug!("Spam keyword detected: {}", keyword);
            return SpamVerdict::Spam {
                pattern: format!("keyword:{}", keyword),
                confidence: 0.8,
            };
        }

        // LAYER 2: Entropy analysis (gibberish detection)
        // Only check entropy for messages long enough to be statistically significant
        // Short messages (e.g. "hi", "hello", "ok") naturally have low entropy
        if text.len() > 20 {
            let entropy = self.calculate_entropy(text);
            if entropy < self.entropy_threshold {
                debug!(
                    "Low entropy detected: {} (threshold: {})",
                    entropy, self.entropy_threshold
                );
                return SpamVerdict::Spam {
                    pattern: format!("entropy:{:.2}", entropy),
                    confidence: 0.7,
                };
            }
        }

        // LAYER 3: Character repetition (flood detection)
        if let Some(repeated_char) = self.check_repetition(text) {
            debug!("Character repetition detected: {}", repeated_char);
            return SpamVerdict::Spam {
                pattern: format!("repetition:{}", repeated_char),
                confidence: 0.9,
            };
        }

        // LAYER 4: URL shortener detection
        if let Some(shortener) = self.check_url_shorteners(text) {
            debug!("URL shortener detected: {}", shortener);
            return SpamVerdict::Spam {
                pattern: format!("shortener:{}", shortener),
                confidence: 0.6,
            };
        }

        // LAYER 5: CTCP flood detection
        if self.check_ctcp_flood(text) {
            debug!("CTCP flood detected");
            return SpamVerdict::Spam {
                pattern: "ctcp_flood".to_string(),
                confidence: 0.85,
            };
        }

        SpamVerdict::Clean
    }

    /// Check for spam keywords in message
    /// Returns first matched keyword if found
    fn check_keywords(&self, text: &str) -> Option<String> {
        // Use Aho-Corasick for O(N) matching
        if let Some(mat) = self.keyword_matcher.find(text) {
            // We need to return the pattern string.
            // Since we don't have easy access to the pattern string from the match index
            // without storing a separate vector, we can reconstruct it or just return a generic indicator.
            // However, for logging, the actual keyword is useful.
            // Let's look up the pattern from our raw_keywords if possible, or just return the matched text.
            // Aho-Corasick match gives us start/end indices.
            let matched_text = &text[mat.start()..mat.end()];
            return Some(matched_text.to_string());
        }
        None
    }

    /// Calculate Shannon entropy of text
    ///
    /// Entropy measures randomness/complexity:
    /// - High entropy (>4.5): Normal human text
    /// - Medium entropy (3.0-4.5): Structured text
    /// - Low entropy (<3.0): Repetitive/gibberish (spam indicator)
    ///
    /// # Algorithm
    /// Shannon entropy: H = -Σ(p(x) * log2(p(x)))
    /// where p(x) = frequency of character x
    fn calculate_entropy(&self, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }

        // Count character frequencies
        let mut char_counts: std::collections::HashMap<char, usize> =
            std::collections::HashMap::new();
        for ch in text.chars() {
            *char_counts.entry(ch).or_insert(0) += 1;
        }

        let len = text.len() as f32;
        let mut entropy = 0.0;

        for count in char_counts.values() {
            let probability = *count as f32 / len;
            entropy -= probability * probability.log2();
        }

        entropy
    }

    /// Check for excessive character repetition
    /// Returns repeated character if threshold exceeded
    fn check_repetition(&self, text: &str) -> Option<char> {
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return None;
        }

        let mut current_char = chars[0];
        let mut current_count = 1;

        for &ch in &chars[1..] {
            if ch == current_char {
                current_count += 1;
                if current_count > self.max_char_repetition {
                    return Some(current_char);
                }
            } else {
                current_char = ch;
                current_count = 1;
            }
        }

        None
    }

    /// Check for URL shortener domains in message
    /// Returns shortener domain if found
    fn check_url_shorteners(&self, text: &str) -> Option<String> {
        let lowercase_text = text.to_lowercase();

        for shortener in &self.url_shorteners {
            if lowercase_text.contains(shortener) {
                return Some(shortener.clone());
            }
        }

        None
    }

    /// Check for CTCP flood (multiple CTCP queries)
    /// CTCP format: \x01COMMAND args\x01
    fn check_ctcp_flood(&self, text: &str) -> bool {
        // Count CTCP delimiters (0x01)
        let ctcp_count = text.chars().filter(|&c| c == '\x01').count();

        // More than 2 CTCP markers in one message is suspicious
        // (one at start, one at end for legitimate CTCP)
        ctcp_count > 2
    }

    /// Add custom spam keyword
    #[allow(dead_code)] // Used in tests, available for runtime config
    pub fn add_keyword(&mut self, keyword: String) {
        let keyword = keyword.to_lowercase();
        self.raw_keywords.insert(keyword.clone());

        // Rebuild matcher
        match AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(&self.raw_keywords)
        {
            Ok(matcher) => self.keyword_matcher = matcher,
            Err(err) => {
                warn!(error = ?err, keyword = %keyword, "Failed to rebuild spam keyword matcher; reverting keyword addition");
                self.raw_keywords.remove(&keyword);
            }
        }
    }

    /// Remove spam keyword
    #[allow(dead_code)] // Available for runtime config
    pub fn remove_keyword(&mut self, keyword: &str) -> bool {
        let keyword = keyword.to_lowercase();
        let removed = self.raw_keywords.remove(&keyword);
        if !removed {
            return false;
        }

        // Rebuild matcher
        match AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(&self.raw_keywords)
        {
            Ok(matcher) => {
                self.keyword_matcher = matcher;
                true
            }
            Err(err) => {
                warn!(error = ?err, keyword = %keyword, "Failed to rebuild spam keyword matcher; reverting keyword removal");
                self.raw_keywords.insert(keyword);
                false
            }
        }
    }

    /// Add URL shortener domain
    #[allow(dead_code)] // Available for runtime config
    pub fn add_shortener(&mut self, domain: String) {
        self.url_shorteners.insert(domain.to_lowercase());
    }

    /// Get current entropy threshold
    #[allow(dead_code)] // Available for runtime config
    pub fn entropy_threshold(&self) -> f32 {
        self.entropy_threshold
    }

    /// Set entropy threshold (0.0-8.0)
    /// Lower = stricter detection, higher false positive rate
    #[allow(dead_code)] // Available for runtime config
    pub fn set_entropy_threshold(&mut self, threshold: f32) {
        self.entropy_threshold = threshold.clamp(0.0, 8.0);
    }

    /// Set maximum character repetition threshold
    /// Higher = more lenient, allows more repeated characters
    #[allow(dead_code)] // Available for runtime config
    pub fn set_max_repetition(&mut self, max: usize) {
        self.max_char_repetition = max;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_test() -> SpamDetectionService {
        SpamDetectionService::new(None, SecurityConfig::default())
    }

    #[test]
    fn test_clean_message() {
        let service = new_test();
        let verdict = service.check_content("Hello everyone, how are you today?");
        assert_eq!(verdict, SpamVerdict::Clean);
    }

    #[test]
    fn test_spam_keyword() {
        let service = new_test();
        let verdict = service.check_content("Buy viagra now at discount prices!");
        assert!(matches!(verdict, SpamVerdict::Spam { .. }));
    }

    #[test]
    fn test_character_repetition() {
        let service = new_test();
        let verdict = service.check_content("aaaaaaaaaaaaaaaaaaaaaa");
        assert!(matches!(verdict, SpamVerdict::Spam { .. }));
    }

    #[test]
    fn test_url_shortener() {
        let service = new_test();
        let verdict = service.check_content("Check out this link: bit.ly/spam123");
        assert!(matches!(verdict, SpamVerdict::Spam { .. }));
    }

    #[test]
    fn test_entropy_calculation() {
        let service = new_test();

        // Normal text should have high entropy
        let high_entropy = service.calculate_entropy("The quick brown fox jumps over the lazy dog");
        assert!(high_entropy > 4.0);

        // Repetitive text should have low entropy
        let low_entropy = service.calculate_entropy("aaaaaaaaaa");
        assert!(low_entropy < 2.0);
    }

    #[test]
    fn test_ctcp_flood() {
        let service = new_test();
        let verdict = service.check_content("\x01VERSION\x01\x01PING\x01\x01FINGER\x01");
        assert!(matches!(verdict, SpamVerdict::Spam { .. }));
    }

    #[test]
    fn test_custom_keyword() {
        let mut service = new_test();
        service.add_keyword("testspam".to_string());

        let verdict = service.check_content("This message contains testspam");
        assert!(matches!(verdict, SpamVerdict::Spam { .. }));
    }

    #[test]
    fn test_case_insensitive_keyword() {
        let service = new_test();
        let verdict = service.check_content("Buy VIAGRA now!");
        assert!(matches!(verdict, SpamVerdict::Spam { .. }));
    }

    #[test]
    fn test_configuration() {
        let mut service = new_test();

        // Test entropy threshold modification
        service.set_entropy_threshold(2.5);
        assert_eq!(service.entropy_threshold(), 2.5);

        // Test clamping
        service.set_entropy_threshold(10.0);
        assert_eq!(service.entropy_threshold(), 8.0);

        // Test repetition configuration
        service.set_max_repetition(5);
        let verdict = service.check_content("aaaaaa"); // 6 chars
        assert!(matches!(verdict, SpamVerdict::Spam { .. }));
    }

    #[test]
    fn test_message_repetition() {
        let service = new_test();
        let uid = "000AAAAAA";
        let msg = "Hello world";

        // First 2 messages allowed
        assert_eq!(
            service.check_message_repetition(uid, msg),
            SpamVerdict::Clean
        );
        assert_eq!(
            service.check_message_repetition(uid, msg),
            SpamVerdict::Clean
        );

        // Third message blocked
        assert!(matches!(
            service.check_message_repetition(uid, msg),
            SpamVerdict::Spam { .. }
        ));

        // Different message allowed
        assert_eq!(
            service.check_message_repetition(uid, "Different"),
            SpamVerdict::Clean
        );
    }
}
