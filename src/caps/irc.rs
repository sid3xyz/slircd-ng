//! IRC-specific capability types.
//!
//! Defines the concrete capability types for IRC operations:
//! - Channel capabilities (kick, ban, topic, etc.)
//! - Oper capabilities (kill, kline, rehash, etc.)
//! - Special capabilities (bypass flood, global notice, etc.)

use super::tokens::Capability;

// ============================================================================
// Channel Capabilities
// ============================================================================

/// Capability to kick a user from a channel.
///
/// Scope: Channel name (lowercase).
/// Required: Channel operator (+o) or higher.
pub struct KickCap;

impl Capability for KickCap {
    type Scope = String;
    const NAME: &'static str = "channel:kick";
}

/// Capability to set/unset bans on a channel.
///
/// Scope: Channel name (lowercase).
/// Required: Channel operator (+o) or higher.
pub struct BanCap;

impl Capability for BanCap {
    type Scope = String;
    const NAME: &'static str = "channel:ban";
}

/// Capability to set the channel topic.
///
/// Scope: Channel name (lowercase).
/// Required: Channel operator (+o) or higher (if +t is set).
pub struct TopicCap;

impl Capability for TopicCap {
    type Scope = String;
    const NAME: &'static str = "channel:topic";
}

/// Capability to modify channel modes.
///
/// Scope: Channel name (lowercase).
/// Required: Channel operator (+o) or higher.
pub struct ChannelModeCap;

impl Capability for ChannelModeCap {
    type Scope = String;
    const NAME: &'static str = "channel:mode";
}

/// Capability to give/take voice (+v) on a channel.
///
/// Scope: Channel name (lowercase).
/// Required: Channel operator (+o) or higher, or halfop (+h).
pub struct VoiceCap;

impl Capability for VoiceCap {
    type Scope = String;
    const NAME: &'static str = "channel:voice";
}

/// Capability to invite users to a channel.
///
/// Scope: Channel name (lowercase).
/// Required: Channel operator (+o) or higher for +i channels.
pub struct InviteCap;

impl Capability for InviteCap {
    type Scope = String;
    const NAME: &'static str = "channel:invite";
}

// ============================================================================
// IRC Operator Capabilities
// ============================================================================

/// Capability to KILL a user (disconnect from network).
///
/// Scope: Unit (global).
/// Required: IRC operator (+o user mode).
pub struct KillCap;

impl Capability for KillCap {
    type Scope = ();
    const NAME: &'static str = "oper:kill";
}

/// Capability to set K-lines (server bans by user@host).
///
/// Scope: Unit (global).
/// Required: IRC operator.
pub struct KlineCap;

impl Capability for KlineCap {
    type Scope = ();
    const NAME: &'static str = "oper:kline";
}

/// Capability to set D-lines (bans by IP, connection rejected).
///
/// Scope: Unit (global).
/// Required: IRC operator.
pub struct DlineCap;

impl Capability for DlineCap {
    type Scope = ();
    const NAME: &'static str = "oper:dline";
}

/// Capability to set G-lines (network-wide bans).
///
/// Scope: Unit (global).
/// Required: IRC operator with gline privilege.
pub struct GlineCap;

impl Capability for GlineCap {
    type Scope = ();
    const NAME: &'static str = "oper:gline";
}

/// Capability to set Z-lines (global IP bans, skips DNS).
///
/// Scope: Unit (global).
/// Required: IRC operator with zline privilege.
pub struct ZlineCap;

impl Capability for ZlineCap {
    type Scope = ();
    const NAME: &'static str = "oper:zline";
}

/// Capability to set R-lines (bans by realname/GECOS).
///
/// Scope: Unit (global).
/// Required: IRC operator with rline privilege.
pub struct RlineCap;

impl Capability for RlineCap {
    type Scope = ();
    const NAME: &'static str = "oper:rline";
}

/// Capability to SHUN users (silent ignore without disconnect).
///
/// Scope: Unit (global).
/// Required: IRC operator with shun privilege.
pub struct ShunCap;

impl Capability for ShunCap {
    type Scope = ();
    const NAME: &'static str = "oper:shun";
}

/// Capability for SA* admin commands (SAJOIN, SAPART, SAMODE, SANICK).
///
/// Scope: Unit (global).
/// Required: IRC operator with admin privilege.
pub struct AdminCap;

impl Capability for AdminCap {
    type Scope = ();
    const NAME: &'static str = "oper:admin";
}

/// Capability to REHASH the server configuration.
///
/// Scope: Unit (global).
/// Required: IRC operator.
pub struct RehashCap;

impl Capability for RehashCap {
    type Scope = ();
    const NAME: &'static str = "oper:rehash";
}

/// Capability to DIE (shut down the server).
///
/// Scope: Unit (global).
/// Required: IRC operator with die privilege.
pub struct DieCap;

impl Capability for DieCap {
    type Scope = ();
    const NAME: &'static str = "oper:die";
}

/// Capability to RESTART the server.
///
/// Scope: Unit (global).
/// Required: IRC operator with restart privilege.
pub struct RestartCap;

impl Capability for RestartCap {
    type Scope = ();
    const NAME: &'static str = "oper:restart";
}

// ============================================================================
// Special Capabilities
// ============================================================================

/// Capability to bypass flood protection.
///
/// Scope: Unit (global).
/// Required: IRC operator or services.
pub struct BypassFloodCap;

impl Capability for BypassFloodCap {
    type Scope = ();
    const NAME: &'static str = "special:bypass_flood";
}

/// Capability to bypass mode restrictions (e.g., +m, +n).
///
/// Scope: Channel name.
/// Required: IRC operator with override privilege.
pub struct BypassModeCap;

impl Capability for BypassModeCap {
    type Scope = String;
    const NAME: &'static str = "special:bypass_mode";
}

/// Capability to send global notices (WALLOPS, GLOBOPS).
///
/// Scope: Unit (global).
/// Required: IRC operator.
pub struct GlobalNoticeCap;

impl Capability for GlobalNoticeCap {
    type Scope = ();
    const NAME: &'static str = "special:global_notice";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_capabilities_have_string_scope() {
        assert_eq!(KickCap::NAME, "channel:kick");
        assert_eq!(BanCap::NAME, "channel:ban");
        assert_eq!(TopicCap::NAME, "channel:topic");
    }

    #[test]
    fn oper_capabilities_have_unit_scope() {
        assert_eq!(KillCap::NAME, "oper:kill");
        assert_eq!(RehashCap::NAME, "oper:rehash");
        assert_eq!(DieCap::NAME, "oper:die");
    }
}
