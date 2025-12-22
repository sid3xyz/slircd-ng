//! Helper utilities for CHATHISTORY.
//!
//! Includes timestamp resolution and query parameter handling.

use crate::handlers::{Context, HandlerError};
use crate::state::RegisteredState;
use slirc_proto::{MessageReference, parse_server_time};

/// Maximum messages per CHATHISTORY request.
pub const MAX_HISTORY_LIMIT: u32 = 100;

/// 1 millisecond in nanoseconds.
/// IRC timestamps use millisecond precision (ISO8601 with `.3f`), but we store nanoseconds.
/// When excluding a boundary timestamp, we need to add a full millisecond to ensure
/// messages at the same millisecond but different sub-millisecond times are excluded.
pub const ONE_MILLISECOND_NS: i64 = 1_000_000;

// ============================================================================
// Error Messages (DRY)
// ============================================================================

/// Standard FAIL message for unknown CHATHISTORY subcommand.
pub const FAIL_UNKNOWN_SUBCOMMAND: &str = "Unknown subcommand";
/// Standard FAIL message for invalid target.
pub const FAIL_NOT_IN_CHANNEL: &str = "You are not in that channel";
/// Standard FAIL message for query errors.
pub const FAIL_QUERY_ERROR: &str = "Failed to retrieve history";

// ============================================================================
// Message Reference Parsing (DRY helper)
// ============================================================================

/// Parsed timestamp from a message reference with precision info.
pub struct ResolvedTimestamp {
    /// The timestamp in nanoseconds.
    pub timestamp: i64,
    /// Whether the original reference was a timestamp (ms precision) vs msgid (ns precision).
    pub is_timestamp: bool,
}

/// Parameters for execute_query (reduces argument count).
pub struct QueryParams {
    pub target: String,
    pub nick: String,
    pub limit: u32,
    pub is_dm: bool,
    /// Message reference argument (arg 2 for most subcommands)
    pub msgref: String,
    /// Second message reference (for BETWEEN, TARGETS)
    pub msgref2: Option<String>,
}

/// Resolve a message reference string to a timestamp.
/// Returns None if the reference is "*" or cannot be resolved.
pub async fn resolve_msgref(
    ctx: &Context<'_, RegisteredState>,
    query_target: &str,
    msgref_str: &str,
) -> Result<Option<ResolvedTimestamp>, HandlerError> {
    if msgref_str == "*" {
        return Ok(None);
    }

    match MessageReference::parse(msgref_str) {
        Ok(MessageReference::MsgId(id)) => {
            let ts = ctx
                .matrix
                .service_manager
                .history
                .lookup_timestamp(query_target, &id)
                .await
                .map_err(|e| HandlerError::Internal(e.to_string()))?;
            Ok(ts.map(|t| ResolvedTimestamp {
                timestamp: t,
                is_timestamp: false,
            }))
        }
        Ok(MessageReference::Timestamp(ts)) => Ok(Some(ResolvedTimestamp {
            timestamp: parse_server_time(&ts),
            is_timestamp: true,
        })),
        _ => Ok(None),
    }
}

/// Get the exclusivity offset based on precision.
/// Timestamps have millisecond precision, msgids have nanosecond precision.
pub fn exclusivity_offset(resolved: &ResolvedTimestamp) -> i64 {
    if resolved.is_timestamp {
        ONE_MILLISECOND_NS
    } else {
        1
    }
}

/// Resolve the DM key for history storage.
/// DMs are stored under a canonical key that combines both participants.
pub async fn resolve_dm_key(
    ctx: &Context<'_, RegisteredState>,
    nick: &str,
    target: &str,
) -> String {
    // Resolve self (nick) to account
    // Prefix with 'a:' for account, 'u:' for unregistered nick to avoid collisions
    let sender_key_part = if let Some(acct) = &ctx.state.account {
        format!("a:{}", slirc_proto::irc_to_lower(acct))
    } else {
        // This should not happen for RegisteredState context, but fallback to nick
        format!("u:{}", slirc_proto::irc_to_lower(nick))
    };

    // Resolve target to account
    let target_lower = slirc_proto::irc_to_lower(target);
    let target_account = if let Some(uid_ref) = ctx.matrix.user_manager.nicks.get(&target_lower) {
        let uid = uid_ref.value();
        if let Some(user) = ctx.matrix.user_manager.users.get(uid) {
            let u = user.read().await;
            u.account.clone()
        } else {
            None
        }
    } else {
        None
    };

    let target_key_part = if let Some(acct) = target_account {
        format!("a:{}", slirc_proto::irc_to_lower(&acct))
    } else {
        format!("u:{}", target_lower)
    };

    let mut users = [sender_key_part, target_key_part];
    users.sort();
    format!("dm:{}:{}", users[0], users[1])
}
