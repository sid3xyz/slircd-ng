//! Channel mode builder for ergonomic mode construction.
//!
//! This module provides a fluent API for building channel mode changes.
//! Per the slirc-proto design notes, the builder generates `Vec<Mode<ChannelMode>>`
//! while metadata (set_by, set_at) is passed separately to the application layer.
//!
//! # Design Philosophy
//!
//! slirc-proto keeps the Mode API minimal (parse/serialize only). This builder
//! provides ISUPPORT-aware ergonomics in slircd-ng where runtime context is available.
//!
//! # Example
//!
//! ```ignore
//! let modes = ChannelModeBuilder::new()
//!     .add_op("nick1")
//!     .add_voice("nick2")
//!     .add_ban("*!*@bad.host")
//!     .set_secret()
//!     .build();
//!
//! // modes is Vec<Mode<ChannelMode>>, pass to apply_channel_modes_typed()
//! ```

use slirc_proto::{ChannelMode, Mode};

/// Builder for constructing channel mode changes.
///
/// Generates a `Vec<Mode<ChannelMode>>` that can be passed to the mode application layer.
/// Metadata (set_by, set_at) should be passed separately when applying the modes.
#[allow(dead_code)] // Ready for use by ChanServ batch mode commands
#[derive(Debug, Clone, Default)]
pub struct ChannelModeBuilder {
    modes: Vec<Mode<ChannelMode>>,
}

/// Result from building modes - the modes vector.
#[allow(dead_code)] // Used by ChannelModeBuilder
pub type ModeChangeResult = Vec<Mode<ChannelMode>>;

#[allow(dead_code)] // Ready for ChanServ batch mode commands
impl ChannelModeBuilder {
    /// Create a new empty mode builder.
    pub fn new() -> Self {
        Self { modes: Vec::new() }
    }

    /// Build and return the collected modes.
    pub fn build(self) -> ModeChangeResult {
        self.modes
    }

    /// Check if any modes have been added.
    pub fn is_empty(&self) -> bool {
        self.modes.is_empty()
    }

    /// Get the number of modes added.
    pub fn len(&self) -> usize {
        self.modes.len()
    }

    // === Prefix modes (user privileges) ===

    /// Add operator status to a user (+o nick).
    pub fn add_op(mut self, nick: impl Into<String>) -> Self {
        self.modes
            .push(Mode::plus(ChannelMode::Oper, Some(&nick.into())));
        self
    }

    /// Remove operator status from a user (-o nick).
    pub fn remove_op(mut self, nick: impl Into<String>) -> Self {
        self.modes
            .push(Mode::minus(ChannelMode::Oper, Some(&nick.into())));
        self
    }

    /// Add voice status to a user (+v nick).
    pub fn add_voice(mut self, nick: impl Into<String>) -> Self {
        self.modes
            .push(Mode::plus(ChannelMode::Voice, Some(&nick.into())));
        self
    }

    /// Remove voice status from a user (-v nick).
    pub fn remove_voice(mut self, nick: impl Into<String>) -> Self {
        self.modes
            .push(Mode::minus(ChannelMode::Voice, Some(&nick.into())));
        self
    }

    // === List modes (Type A) ===

    /// Add a ban mask (+b mask).
    pub fn add_ban(mut self, mask: impl Into<String>) -> Self {
        self.modes
            .push(Mode::plus(ChannelMode::Ban, Some(&mask.into())));
        self
    }

    /// Remove a ban mask (-b mask).
    pub fn remove_ban(mut self, mask: impl Into<String>) -> Self {
        self.modes
            .push(Mode::minus(ChannelMode::Ban, Some(&mask.into())));
        self
    }

    /// Add a ban exception mask (+e mask).
    pub fn add_except(mut self, mask: impl Into<String>) -> Self {
        self.modes
            .push(Mode::plus(ChannelMode::Exception, Some(&mask.into())));
        self
    }

    /// Remove a ban exception mask (-e mask).
    pub fn remove_except(mut self, mask: impl Into<String>) -> Self {
        self.modes
            .push(Mode::minus(ChannelMode::Exception, Some(&mask.into())));
        self
    }

    /// Add an invite exception mask (+I mask).
    pub fn add_invex(mut self, mask: impl Into<String>) -> Self {
        self.modes
            .push(Mode::plus(ChannelMode::InviteException, Some(&mask.into())));
        self
    }

    /// Remove an invite exception mask (-I mask).
    pub fn remove_invex(mut self, mask: impl Into<String>) -> Self {
        self.modes.push(Mode::minus(
            ChannelMode::InviteException,
            Some(&mask.into()),
        ));
        self
    }

    /// Add a quiet mask (+q mask).
    pub fn add_quiet(mut self, mask: impl Into<String>) -> Self {
        self.modes
            .push(Mode::plus(ChannelMode::Quiet, Some(&mask.into())));
        self
    }

    /// Remove a quiet mask (-q mask).
    pub fn remove_quiet(mut self, mask: impl Into<String>) -> Self {
        self.modes
            .push(Mode::minus(ChannelMode::Quiet, Some(&mask.into())));
        self
    }

    // === Parameter modes (Type B/C) ===

    /// Set the channel key (+k key).
    pub fn set_key(mut self, key: impl Into<String>) -> Self {
        self.modes
            .push(Mode::plus(ChannelMode::Key, Some(&key.into())));
        self
    }

    /// Remove the channel key (-k).
    pub fn unset_key(mut self) -> Self {
        self.modes.push(Mode::minus(ChannelMode::Key, None));
        self
    }

    /// Set the user limit (+l limit).
    pub fn set_limit(mut self, limit: u32) -> Self {
        self.modes
            .push(Mode::plus(ChannelMode::Limit, Some(&limit.to_string())));
        self
    }

    /// Remove the user limit (-l).
    pub fn unset_limit(mut self) -> Self {
        self.modes.push(Mode::minus(ChannelMode::Limit, None));
        self
    }

    // === Simple flags (Type D - no parameters) ===

    /// Set invite-only mode (+i).
    pub fn set_invite_only(mut self) -> Self {
        self.modes.push(Mode::plus(ChannelMode::InviteOnly, None));
        self
    }

    /// Unset invite-only mode (-i).
    pub fn unset_invite_only(mut self) -> Self {
        self.modes.push(Mode::minus(ChannelMode::InviteOnly, None));
        self
    }

    /// Set moderated mode (+m).
    pub fn set_moderated(mut self) -> Self {
        self.modes.push(Mode::plus(ChannelMode::Moderated, None));
        self
    }

    /// Unset moderated mode (-m).
    pub fn unset_moderated(mut self) -> Self {
        self.modes.push(Mode::minus(ChannelMode::Moderated, None));
        self
    }

    /// Set no-external-messages mode (+n).
    pub fn set_no_external(mut self) -> Self {
        self.modes
            .push(Mode::plus(ChannelMode::NoExternalMessages, None));
        self
    }

    /// Unset no-external-messages mode (-n).
    pub fn unset_no_external(mut self) -> Self {
        self.modes
            .push(Mode::minus(ChannelMode::NoExternalMessages, None));
        self
    }

    /// Set secret mode (+s).
    pub fn set_secret(mut self) -> Self {
        self.modes.push(Mode::plus(ChannelMode::Secret, None));
        self
    }

    /// Unset secret mode (-s).
    pub fn unset_secret(mut self) -> Self {
        self.modes.push(Mode::minus(ChannelMode::Secret, None));
        self
    }

    /// Set topic-lock mode (+t).
    pub fn set_topic_lock(mut self) -> Self {
        self.modes
            .push(Mode::plus(ChannelMode::ProtectedTopic, None));
        self
    }

    /// Unset topic-lock mode (-t).
    pub fn unset_topic_lock(mut self) -> Self {
        self.modes
            .push(Mode::minus(ChannelMode::ProtectedTopic, None));
        self
    }

    /// Set registered-only mode (+r).
    pub fn set_registered_only(mut self) -> Self {
        self.modes
            .push(Mode::plus(ChannelMode::RegisteredOnly, None));
        self
    }

    /// Unset registered-only mode (-r).
    pub fn unset_registered_only(mut self) -> Self {
        self.modes
            .push(Mode::minus(ChannelMode::RegisteredOnly, None));
        self
    }

    // === Raw mode access ===

    /// Add a raw mode (for advanced use or future ISUPPORT-aware extensions).
    pub fn add_mode(mut self, mode: Mode<ChannelMode>) -> Self {
        self.modes.push(mode);
        self
    }

    /// Extend with multiple modes.
    pub fn extend(mut self, modes: impl IntoIterator<Item = Mode<ChannelMode>>) -> Self {
        self.modes.extend(modes);
        self
    }
}

/// Parse an MLOCK string like "+nt-s" or "+ntk-il secretkey" into a Mode vector.
///
/// MLOCK format:
/// - Flags switch between add (+) and remove (-)
/// - Parameter modes (k, l) consume arguments from the remaining string
///
/// # Example
/// ```ignore
/// let modes = parse_mlock("+nt-s");
/// // Returns [+n, +t, -s]
///
/// let modes = parse_mlock("+ntk secretkey");
/// // Returns [+n, +t, +k secretkey]
/// ```
pub fn parse_mlock(mlock: &str) -> Vec<Mode<ChannelMode>> {
    let trimmed = mlock.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut adding = true; // Start assuming + if no prefix given

    // Split into mode chars and remaining args
    let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
    let mode_chars = parts[0];
    let args: Vec<&str> = if parts.len() > 1 {
        parts[1].split_whitespace().collect()
    } else {
        Vec::new()
    };
    let mut arg_idx = 0;

    for ch in mode_chars.chars() {
        match ch {
            '+' => adding = true,
            '-' => adding = false,
            'n' => {
                let mode = if adding {
                    Mode::plus(ChannelMode::NoExternalMessages, None)
                } else {
                    Mode::minus(ChannelMode::NoExternalMessages, None)
                };
                result.push(mode);
            }
            't' => {
                let mode = if adding {
                    Mode::plus(ChannelMode::ProtectedTopic, None)
                } else {
                    Mode::minus(ChannelMode::ProtectedTopic, None)
                };
                result.push(mode);
            }
            's' => {
                let mode = if adding {
                    Mode::plus(ChannelMode::Secret, None)
                } else {
                    Mode::minus(ChannelMode::Secret, None)
                };
                result.push(mode);
            }
            'i' => {
                let mode = if adding {
                    Mode::plus(ChannelMode::InviteOnly, None)
                } else {
                    Mode::minus(ChannelMode::InviteOnly, None)
                };
                result.push(mode);
            }
            'm' => {
                let mode = if adding {
                    Mode::plus(ChannelMode::Moderated, None)
                } else {
                    Mode::minus(ChannelMode::Moderated, None)
                };
                result.push(mode);
            }
            'r' => {
                let mode = if adding {
                    Mode::plus(ChannelMode::RegisteredOnly, None)
                } else {
                    Mode::minus(ChannelMode::RegisteredOnly, None)
                };
                result.push(mode);
            }
            'c' => {
                let mode = if adding {
                    Mode::plus(ChannelMode::NoColors, None)
                } else {
                    Mode::minus(ChannelMode::NoColors, None)
                };
                result.push(mode);
            }
            'C' => {
                let mode = if adding {
                    Mode::plus(ChannelMode::NoCTCP, None)
                } else {
                    Mode::minus(ChannelMode::NoCTCP, None)
                };
                result.push(mode);
            }
            'N' => {
                let mode = if adding {
                    Mode::plus(ChannelMode::NoNickChange, None)
                } else {
                    Mode::minus(ChannelMode::NoNickChange, None)
                };
                result.push(mode);
            }
            'k' => {
                // Key mode requires a parameter when adding
                if adding {
                    if arg_idx < args.len() {
                        let key = args[arg_idx].to_string();
                        arg_idx += 1;
                        result.push(Mode::plus(ChannelMode::Key, Some(&key)));
                    }
                    // If no param, skip (graceful handling)
                } else {
                    result.push(Mode::minus(ChannelMode::Key, None));
                }
            }
            'l' => {
                // Limit mode requires a parameter when adding
                if adding {
                    if arg_idx < args.len() {
                        if let Ok(limit) = args[arg_idx].parse::<u32>() {
                            arg_idx += 1;
                            result.push(Mode::plus(ChannelMode::Limit, Some(&limit.to_string())));
                        } else {
                            arg_idx += 1; // Consume invalid arg, skip mode
                        }
                    }
                    // If no param, skip (graceful handling)
                } else {
                    result.push(Mode::minus(ChannelMode::Limit, None));
                }
            }
            _ => {
                // Unknown mode char, skip gracefully
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_basic() {
        let modes = ChannelModeBuilder::new()
            .add_op("nick1")
            .add_voice("nick2")
            .build();

        assert_eq!(modes.len(), 2);
        assert!(modes[0].is_plus());
        assert_eq!(modes[0].arg(), Some("nick1"));
        assert_eq!(*modes[0].mode(), ChannelMode::Oper);
    }

    #[test]
    fn test_builder_mixed() {
        let modes = ChannelModeBuilder::new()
            .set_secret()
            .set_no_external()
            .add_ban("*!*@bad.host")
            .remove_op("badop")
            .build();

        assert_eq!(modes.len(), 4);
        // +s
        assert!(modes[0].is_plus());
        assert_eq!(*modes[0].mode(), ChannelMode::Secret);
        // +n
        assert!(modes[1].is_plus());
        assert_eq!(*modes[1].mode(), ChannelMode::NoExternalMessages);
        // +b
        assert!(modes[2].is_plus());
        assert_eq!(*modes[2].mode(), ChannelMode::Ban);
        assert_eq!(modes[2].arg(), Some("*!*@bad.host"));
        // -o
        assert!(modes[3].is_minus());
        assert_eq!(*modes[3].mode(), ChannelMode::Oper);
        assert_eq!(modes[3].arg(), Some("badop"));
    }

    #[test]
    fn test_builder_empty() {
        let modes = ChannelModeBuilder::new().build();
        assert!(modes.is_empty());
    }

    #[test]
    fn test_builder_is_empty() {
        let builder = ChannelModeBuilder::new();
        assert!(builder.is_empty());

        let builder = builder.set_secret();
        assert!(!builder.is_empty());
    }

    #[test]
    fn test_parse_mlock_simple() {
        let modes = super::parse_mlock("+nt-s");
        assert_eq!(modes.len(), 3);
        // +n
        assert!(modes[0].is_plus());
        assert_eq!(*modes[0].mode(), ChannelMode::NoExternalMessages);
        // +t
        assert!(modes[1].is_plus());
        assert_eq!(*modes[1].mode(), ChannelMode::ProtectedTopic);
        // -s
        assert!(modes[2].is_minus());
        assert_eq!(*modes[2].mode(), ChannelMode::Secret);
    }

    #[test]
    fn test_parse_mlock_with_key() {
        let modes = super::parse_mlock("+ntk secretkey");
        assert_eq!(modes.len(), 3);
        // +n
        assert!(modes[0].is_plus());
        assert_eq!(*modes[0].mode(), ChannelMode::NoExternalMessages);
        // +t
        assert!(modes[1].is_plus());
        assert_eq!(*modes[1].mode(), ChannelMode::ProtectedTopic);
        // +k with param
        assert!(modes[2].is_plus());
        assert_eq!(*modes[2].mode(), ChannelMode::Key);
        assert_eq!(modes[2].arg(), Some("secretkey"));
    }

    #[test]
    fn test_parse_mlock_with_limit() {
        let modes = super::parse_mlock("+ntl 50");
        assert_eq!(modes.len(), 3);
        // +l with param
        assert!(modes[2].is_plus());
        assert_eq!(*modes[2].mode(), ChannelMode::Limit);
        assert_eq!(modes[2].arg(), Some("50"));
    }

    #[test]
    fn test_parse_mlock_remove_key_limit() {
        let modes = super::parse_mlock("-kl");
        assert_eq!(modes.len(), 2);
        // -k (no param needed for removal)
        assert!(modes[0].is_minus());
        assert_eq!(*modes[0].mode(), ChannelMode::Key);
        assert!(modes[0].arg().is_none());
        // -l
        assert!(modes[1].is_minus());
        assert_eq!(*modes[1].mode(), ChannelMode::Limit);
    }

    #[test]
    fn test_parse_mlock_empty() {
        let modes = super::parse_mlock("");
        assert!(modes.is_empty());

        let modes = super::parse_mlock("   ");
        assert!(modes.is_empty());
    }

    #[test]
    fn test_parse_mlock_complex() {
        let modes = super::parse_mlock("+ntk-il secretkey 100");
        assert_eq!(modes.len(), 5);
        // +n, +t, +k secretkey, -i, -l
        assert!(modes[0].is_plus());
        assert_eq!(*modes[0].mode(), ChannelMode::NoExternalMessages);
        assert!(modes[1].is_plus());
        assert_eq!(*modes[1].mode(), ChannelMode::ProtectedTopic);
        assert!(modes[2].is_plus());
        assert_eq!(*modes[2].mode(), ChannelMode::Key);
        assert_eq!(modes[2].arg(), Some("secretkey"));
        assert!(modes[3].is_minus());
        assert_eq!(*modes[3].mode(), ChannelMode::InviteOnly);
        assert!(modes[4].is_minus());
        assert_eq!(*modes[4].mode(), ChannelMode::Limit);
    }

    #[test]
    fn test_parse_mlock_all_simple_flags() {
        let modes = super::parse_mlock("+ntsimrcCN");
        assert_eq!(modes.len(), 9);
        assert_eq!(*modes[0].mode(), ChannelMode::NoExternalMessages);
        assert_eq!(*modes[1].mode(), ChannelMode::ProtectedTopic);
        assert_eq!(*modes[2].mode(), ChannelMode::Secret);
        assert_eq!(*modes[3].mode(), ChannelMode::InviteOnly);
        assert_eq!(*modes[4].mode(), ChannelMode::Moderated);
        assert_eq!(*modes[5].mode(), ChannelMode::RegisteredOnly);
        assert_eq!(*modes[6].mode(), ChannelMode::NoColors);
        assert_eq!(*modes[7].mode(), ChannelMode::NoCTCP);
        assert_eq!(*modes[8].mode(), ChannelMode::NoNickChange);
    }
}
