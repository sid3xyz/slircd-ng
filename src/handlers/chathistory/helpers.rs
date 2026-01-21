//! Helper utilities for CHATHISTORY.
//!
//! Includes timestamp resolution and query parameter handling.

use crate::handlers::{Context, HandlerError};
use crate::state::RegisteredState;
use crate::state::dashmap_ext::DashMapExt;
use crate::history::types::{EventKind, HistoryItem};
use slirc_proto::{Command, Message, MessageReference, Prefix, Tag, parse_server_time};
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
    let target_account = if let Some(uid) = ctx.matrix.user_manager.get_first_uid(&target_lower) {
        if let Some(user_arc) = ctx.matrix.user_manager.users.get_cloned(&uid) {
            let u = user_arc.read().await;
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


/// Convert a HistoryItem to a protocol Message with appropriate tags.
///
/// Handles filtering based on `event-playback` capability.
/// Returns `None` if the item should be filtered out.
pub fn history_item_to_message(
    item: &HistoryItem,
    batch_id: &str,
    target: &str,
    has_event_playback: bool,
) -> Option<Message> {
    // Determine if we should skip this item based on capabilities
    match item {
        HistoryItem::Message(msg) => {
            let cmd = msg.envelope.command.as_str();
            if (cmd == "TOPIC" || cmd == "TAGMSG") && !has_event_playback {
                return None;
            }
        }
        HistoryItem::Event(_) => {
            if !has_event_playback {
                return None;
            }
        }
    }

    // Common tags
    let (nanotime, msgid) = match item {
        HistoryItem::Message(m) => (m.nanotime, m.msgid.clone()),
        HistoryItem::Event(e) => (e.nanotime, e.id.clone()),
    };

    // Timestamp ISO string
    let time_iso = {
        let secs = nanotime / 1_000_000_000;
        let nanos = (nanotime % 1_000_000_000) as u32;
        if let Some(dt) = chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos) {
            dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        } else {
            "1970-01-01T00:00:00.000Z".to_string()
        }
    };

    let mut tags = vec![
        Tag::new("batch", Some(batch_id.to_string())),
        Tag::new("time", Some(time_iso)),
        Tag::new("msgid", Some(msgid.clone())),
    ];

    // Construct command
    let (prefix, command) = match item {
        HistoryItem::Message(msg) => {
            if let Some(account) = &msg.account {
                tags.push(Tag::new("account", Some(account.clone())));
            }
            if let Some(_status) = msg.status_prefix {
                // Not standard IRCv3 tag yet, but useful for internal or custom clients
                // For now, we don't send it as a tag unless specified by spec
            }

            // Add preserved client-only tags for TAGMSG
            if let Some(env_tags) = &msg.envelope.tags {
                for env_tag in env_tags {
                    if env_tag.key.starts_with('+') {
                        tags.push(Tag::new(&env_tag.key, env_tag.value.clone()));
                    }
                }
            }

            let cmd = match msg.envelope.command.as_str() {
                "PRIVMSG" => {
                    Command::PRIVMSG(msg.envelope.target.clone(), msg.envelope.text.clone())
                }
                "NOTICE" => {
                    Command::NOTICE(msg.envelope.target.clone(), msg.envelope.text.clone())
                }
                "TAGMSG" => Command::TAGMSG(msg.envelope.target.clone()),
                "TOPIC" => Command::TOPIC(
                    msg.envelope.target.clone(),
                    Some(msg.envelope.text.clone()),
                ),
                _ => return None,
            };
            (Some(Prefix::new_from_str(&msg.envelope.prefix)), cmd)
        }
        HistoryItem::Event(evt) => {
            let cmd = match &evt.kind {
                EventKind::Join => Command::JOIN(target.to_string(), None, None),
                EventKind::Part(reason) => Command::PART(target.to_string(), reason.clone()),
                EventKind::Quit(reason) => Command::QUIT(reason.clone()),
                EventKind::Kick {
                    target: kicked,
                    reason,
                } => Command::KICK(target.to_string(), kicked.clone(), reason.clone()),
                EventKind::Mode { diff } => {
                    Command::Raw("MODE".to_string(), vec![target.to_string(), diff.clone()])
                }
                EventKind::Topic { new_topic, .. } => {
                    Command::TOPIC(target.to_string(), Some(new_topic.clone()))
                }
                EventKind::Nick { new_nick } => Command::NICK(new_nick.clone()),
            };
            (Some(Prefix::new_from_str(&evt.source)), cmd)
        }
    };

    Some(Message {
        tags: Some(tags),
        prefix,
        command,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exclusivity_offset_for_timestamp() {
        // Timestamps have millisecond precision, so offset should be 1ms in ns
        let resolved = ResolvedTimestamp {
            timestamp: 1_700_000_000_000_000_000,
            is_timestamp: true,
        };
        assert_eq!(exclusivity_offset(&resolved), ONE_MILLISECOND_NS);
        assert_eq!(exclusivity_offset(&resolved), 1_000_000);
    }

    #[test]
    fn test_exclusivity_offset_for_msgid() {
        // Message IDs have nanosecond precision, so offset should be 1 ns
        let resolved = ResolvedTimestamp {
            timestamp: 1_700_000_000_000_000_000,
            is_timestamp: false,
        };
        assert_eq!(exclusivity_offset(&resolved), 1);
    }

    #[test]
    fn test_one_millisecond_ns_constant() {
        // Verify the constant is correctly defined
        assert_eq!(ONE_MILLISECOND_NS, 1_000_000);
    }

    #[test]
    fn test_max_history_limit_constant() {
        // Verify the max limit constant
        assert_eq!(MAX_HISTORY_LIMIT, 100);
    }
}
