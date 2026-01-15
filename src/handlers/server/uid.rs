#![allow(clippy::collapsible_if)]
use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::ServerState;
use async_trait::async_trait;
use slirc_proto::sync::clock::{HybridTimestamp, ServerId};
use slirc_proto::sync::user::UserCrdt;
use slirc_proto::MessageRef;
use tracing::info;

use crate::handlers::server::source::extract_source_sid;

/// Handler for the UID command (User ID).
///
/// UID introduces a new user to the network.
pub struct UidHandler;

#[async_trait]
impl ServerHandler for UidHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Format: UID <nick> <hopcount> <timestamp> <username> <hostname> <uid> <modes> <realname>

        let nick = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let hopcount_str = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let timestamp_str = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;
        let username = msg.arg(3).ok_or(HandlerError::NeedMoreParams)?;
        let hostname = msg.arg(4).ok_or(HandlerError::NeedMoreParams)?;
        let uid = msg.arg(5).ok_or(HandlerError::NeedMoreParams)?;
        let modes_str = msg.arg(6).ok_or(HandlerError::NeedMoreParams)?;
        let realname = msg.arg(7).ok_or(HandlerError::NeedMoreParams)?;

        let _hopcount = hopcount_str.parse::<u32>().map_err(|_| {
            HandlerError::ProtocolError(format!("Invalid hopcount: {}", hopcount_str))
        })?;

        let timestamp = timestamp_str.parse::<u64>().map_err(|_| {
            HandlerError::ProtocolError(format!("Invalid timestamp: {}", timestamp_str))
        })?;

        let source = extract_source_sid(msg).unwrap_or_else(|| ServerId::new("000".to_string()));

        // Convert TS6 UID to CRDT for lossless merge
        let crdt = uid_to_crdt(
            uid, nick, timestamp, modes_str, username, hostname, realname, &source,
        );

        // Merge user CRDT (handles nick collisions via CRDT semantics)
        ctx.matrix
            .user_manager
            .merge_user_crdt(crdt, Some(source))
            .await;

        info!(uid = %uid, nick = %nick, "Registered remote user via UID CRDT");

        Ok(())
    }
}

//=============================================================================
// CRDT CONVERTERS
//=============================================================================

/// Converts TS6 UID parameters to UserCrdt for lossless CRDT merge.
///
/// Timestamp Layering Strategy:
/// - Base timestamp: User registration timestamp
/// - Incremented: User modes (ensures modes update)
/// - Double-incremented: (reserved for future hierarchical fields)
///
/// This guarantees LWW updates apply when merging concurrent state.
#[allow(clippy::too_many_arguments)] // TS6 protocol has 8 params
fn uid_to_crdt(
    uid: &str,
    nick: &str,
    timestamp: u64,
    modes_str: &str,
    username: &str,
    hostname: &str,
    realname: &str,
    source: &ServerId,
) -> UserCrdt {
    let base_ts = HybridTimestamp::new(timestamp as i64, 0, source);
    let modes_ts = base_ts.increment();

    let mut crdt = UserCrdt::new(
        uid.to_string(),
        nick.to_string(),
        username.to_string(),
        realname.to_string(),
        hostname.to_string(),
        hostname.to_string(), // Assume visible host = hostname
        base_ts,
    );

    // Apply modes with incremented timestamp
    apply_user_modes_to_crdt(&mut crdt, modes_str, modes_ts);

    crdt
}

/// Parses TS6 user mode string (e.g., "+iowZ") and applies to UserCrdt with timestamp.
fn apply_user_modes_to_crdt(crdt: &mut UserCrdt, modes_str: &str, timestamp: HybridTimestamp) {
    for c in modes_str.chars() {
        match c {
            '+' => continue,
            'i' => crdt.modes.invisible.update(true, timestamp),
            'w' => crdt.modes.wallops.update(true, timestamp),
            'o' => crdt.modes.oper.update(true, timestamp),
            'r' => crdt.modes.registered.update(true, timestamp),
            'Z' => crdt.modes.secure.update(true, timestamp),
            'R' => crdt.modes.registered_only.update(true, timestamp),
            'T' => crdt.modes.no_ctcp.update(true, timestamp),
            'B' => crdt.modes.bot.update(true, timestamp),
            'S' => {} // Service mode - not in UserModesCrdt
            _ => {}
        }
    }
}

//=============================================================================
// TESTS
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uid_to_crdt_basic() {
        let source = ServerId::new("001".to_string());
        let crdt = uid_to_crdt(
            "001AAAAAA",
            "Alice",
            1234567890,
            "+i",
            "alice",
            "example.com",
            "Alice Wonderland",
            &source,
        );

        assert_eq!(&crdt.uid, "001AAAAAA");
        assert_eq!(crdt.nick.value(), "Alice");
        assert_eq!(crdt.user.value(), "alice");
        assert_eq!(crdt.host.value(), "example.com");
        assert_eq!(crdt.realname.value(), "Alice Wonderland");
        assert_eq!(*crdt.modes.invisible.value(), true);
    }

    #[test]
    fn test_uid_to_crdt_multiple_modes() {
        let source = ServerId::new("002".to_string());
        let crdt = uid_to_crdt(
            "002BBBBBB",
            "Bob",
            9876543210,
            "+iowrZRTB",
            "bob",
            "test.net",
            "Bob Builder",
            &source,
        );

        assert_eq!(*crdt.modes.invisible.value(), true);
        assert_eq!(*crdt.modes.wallops.value(), true);
        assert_eq!(*crdt.modes.oper.value(), true);
        assert_eq!(*crdt.modes.registered.value(), true);
        assert_eq!(*crdt.modes.secure.value(), true);
        assert_eq!(*crdt.modes.registered_only.value(), true);
        assert_eq!(*crdt.modes.no_ctcp.value(), true);
        assert_eq!(*crdt.modes.bot.value(), true);
    }

    #[test]
    fn test_uid_to_crdt_no_modes() {
        let source = ServerId::new("003".to_string());
        let crdt = uid_to_crdt(
            "003CCCCCC",
            "Charlie",
            5555555555,
            "+",
            "charlie",
            "irc.example",
            "Charlie Chaplin",
            &source,
        );

        assert_eq!(*crdt.modes.invisible.value(), false);
        assert_eq!(*crdt.modes.wallops.value(), false);
        assert_eq!(*crdt.modes.oper.value(), false);
    }

    #[test]
    fn test_uid_to_crdt_timestamp_layering() {
        let source = ServerId::new("004".to_string());
        let crdt = uid_to_crdt(
            "004DDDDDD",
            "Diana",
            1000000000,
            "+i",
            "diana",
            "host.example",
            "Diana Prince",
            &source,
        );

        let base_ts = crdt.nick.timestamp();
        let modes_ts = crdt.modes.invisible.timestamp();

        // Modes timestamp should be incremented relative to base
        assert!(modes_ts > base_ts, "Modes timestamp should be incremented");
    }

    #[test]
    fn test_apply_user_modes_to_crdt_empty() {
        let source = ServerId::new("005".to_string());
        let timestamp = HybridTimestamp::new(2000000000, 0, &source);
        let mut crdt = UserCrdt::new(
            "005EEEEE".to_string(),
            "Emma".to_string(),
            "emma".to_string(),
            "Emma Watson".to_string(),
            "test.org".to_string(),
            "test.org".to_string(),
            timestamp,
        );

        apply_user_modes_to_crdt(&mut crdt, "", timestamp);

        assert_eq!(*crdt.modes.invisible.value(), false);
        assert_eq!(*crdt.modes.oper.value(), false);
    }

    #[test]
    fn test_apply_user_modes_to_crdt_with_plus() {
        let source = ServerId::new("006".to_string());
        let base_ts = HybridTimestamp::new(3000000000, 0, &source);
        let mut crdt = UserCrdt::new(
            "006FFFFFF".to_string(),
            "Frank".to_string(),
            "frank".to_string(),
            "Frank Sinatra".to_string(),
            "irc.test".to_string(),
            "irc.test".to_string(),
            base_ts,
        );

        // Apply modes with incremented timestamp (must be > base_ts for LWW to apply)
        let modes_ts = base_ts.increment();
        apply_user_modes_to_crdt(&mut crdt, "+ow", modes_ts);

        assert_eq!(*crdt.modes.oper.value(), true);
        assert_eq!(*crdt.modes.wallops.value(), true);
        assert_eq!(*crdt.modes.invisible.value(), false);
    }

    #[test]
    fn test_apply_user_modes_to_crdt_unknown_modes() {
        let source = ServerId::new("007".to_string());
        let base_ts = HybridTimestamp::new(4000000000, 0, &source);
        let mut crdt = UserCrdt::new(
            "007GGGGGG".to_string(),
            "Grace".to_string(),
            "grace".to_string(),
            "Grace Hopper".to_string(),
            "cs.mit.edu".to_string(),
            "cs.mit.edu".to_string(),
            base_ts,
        );

        // Apply modes with incremented timestamp
        let modes_ts = base_ts.increment();
        apply_user_modes_to_crdt(&mut crdt, "+iXYZ", modes_ts);

        // Should only set 'i', ignore X, Y, Z
        assert_eq!(*crdt.modes.invisible.value(), true);
        assert_eq!(*crdt.modes.oper.value(), false);
    }
}
