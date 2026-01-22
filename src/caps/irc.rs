//! IRC-specific capability types.
//!
//! Defines the concrete capability types for IRC operations:
//! - Channel capabilities (kick, ban, topic, etc.)
//! - Oper capabilities (kill, kline, rehash, etc.)
//! - Special capabilities (bypass flood, global notice, etc.)

use super::tokens::Capability;

// ============================================================================
// Capability Definition Macro
// ============================================================================

/// Macro to define capability types with minimal boilerplate.
///
/// # Variants
///
/// - `channel($name, $cap_name)` - Channel-scoped capability (Scope = String)
/// - `oper($name, $cap_name)` - Global oper capability (Scope = ())
/// - `special($name, $cap_name, $scope)` - Custom scope capability
macro_rules! define_capability {
    // Channel capability (String scope)
    (channel $name:ident, $cap_name:literal, $doc:literal) => {
        #[doc = $doc]
        ///
        /// Scope: Channel name (lowercase).
        pub struct $name;

        impl Capability for $name {
            type Scope = String;
            const NAME: &'static str = $cap_name;
        }
    };

    // Oper capability (unit scope)
    (oper $name:ident, $cap_name:literal, $doc:literal) => {
        #[doc = $doc]
        ///
        /// Scope: Unit (global).
        pub struct $name;

        impl Capability for $name {
            type Scope = ();
            const NAME: &'static str = $cap_name;
        }
    };

    // Special capability with custom scope
    (special $name:ident, $cap_name:literal, $scope:ty, $doc:literal) => {
        #[doc = $doc]
        pub struct $name;

        impl Capability for $name {
            type Scope = $scope;
            const NAME: &'static str = $cap_name;
        }
    };
}

// ============================================================================
// Channel Capabilities
// ============================================================================

define_capability!(channel KickCap, "channel:kick",
    "Capability to kick a user from a channel. Required: Channel operator (+o) or higher.");

define_capability!(channel BanCap, "channel:ban",
    "Capability to set/unset bans on a channel. Required: Channel operator (+o) or higher.");

define_capability!(channel TopicCap, "channel:topic",
    "Capability to set the channel topic. Required: Channel operator (+o) or higher (if +t is set).");

define_capability!(channel ChannelModeCap, "channel:mode",
    "Capability to modify channel modes. Required: Channel operator (+o) or higher.");

define_capability!(channel VoiceCap, "channel:voice",
    "Capability to give/take voice (+v) on a channel. Required: Channel operator or halfop (+h).");

define_capability!(channel InviteCap, "channel:invite",
    "Capability to invite users to a channel. Required: Channel operator (+o) or higher for +i channels.");

// ============================================================================
// IRC Operator Capabilities
// ============================================================================

define_capability!(oper KillCap, "oper:kill",
    "Capability to KILL a user (disconnect from network). Required: IRC operator (+o user mode).");

define_capability!(oper KlineCap, "oper:kline",
    "Capability to set K-lines (server bans by user@host). Required: IRC operator.");

define_capability!(oper DlineCap, "oper:dline",
    "Capability to set D-lines (bans by IP, connection rejected). Required: IRC operator.");

define_capability!(oper GlineCap, "oper:gline",
    "Capability to set G-lines (network-wide bans). Required: IRC operator with gline privilege.");

define_capability!(oper ZlineCap, "oper:zline",
    "Capability to set Z-lines (global IP bans, skips DNS). Required: IRC operator with zline privilege.");

define_capability!(oper RlineCap, "oper:rline",
    "Capability to set R-lines (bans by realname/GECOS). Required: IRC operator with rline privilege.");

define_capability!(oper ShunCap, "oper:shun",
    "Capability to SHUN users (silent ignore without disconnect). Required: IRC operator with shun privilege.");

define_capability!(oper AdminCap, "oper:admin",
    "Capability for SA* admin commands (SAJOIN, SAPART, SAMODE, SANICK). Required: IRC operator with admin privilege.");

define_capability!(oper RehashCap, "oper:rehash",
    "Capability to REHASH the server configuration. Required: IRC operator.");

define_capability!(oper DieCap, "oper:die",
    "Capability to DIE (shut down the server). Required: IRC operator with die privilege.");

define_capability!(oper RestartCap, "oper:restart",
    "Capability to RESTART the server. Required: IRC operator with restart privilege.");

define_capability!(oper ChgHostCap, "oper:chghost",
    "Capability to change user hosts (CHGHOST). Required: IRC operator with chghost privilege.");

define_capability!(oper ChgIdentCap, "oper:chgident",
    "Capability to change user idents (CHGIDENT). Required: IRC operator with chgident privilege.");

define_capability!(oper VhostCap, "oper:vhost",
    "Capability to set VHOSTs. Required: IRC operator with vhost privilege.");

define_capability!(oper WallopsCap, "oper:wallops",
    "Capability to send WALLOPS. Required: IRC operator with wallops privilege.");

define_capability!(oper GlobOpsCap, "oper:globops",
    "Capability to send GLOBOPS. Required: IRC operator with globops privilege.");

define_capability!(oper ClearChanCap, "oper:clearchan",
    "Capability to clear channel state (CLEARCHAN). Required: IRC operator with clearchan privilege.");

define_capability!(oper ConnectCap, "oper:connect",
    "Capability to CONNECT to a remote server (initiate S2S link). Required: IRC operator.");

define_capability!(oper SquitCap, "oper:squit",
    "Capability to SQUIT a server (terminate S2S link). Required: IRC operator.");

// ============================================================================
// Special Capabilities
// ============================================================================

define_capability!(oper BypassFloodCap, "special:bypass_flood",
    "Capability to bypass flood protection. Required: IRC operator or services.");

define_capability!(special BypassModeCap, "special:bypass_mode", String,
    "Capability to bypass mode restrictions (e.g., +m, +n). Scope: Channel name. Required: IRC operator with override privilege.");

define_capability!(oper GlobalNoticeCap, "special:global_notice",
    "Capability to send global notices (WALLOPS, GLOBOPS). Required: IRC operator.");

define_capability!(oper SpamConfCap, "oper:spamconf",
    "Capability to configure spam detection at runtime. Required: IRC operator.");

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
