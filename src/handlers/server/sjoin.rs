use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::ServerState;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_crdt::channel::{ChannelCrdt, MemberModesCrdt};
use slirc_crdt::clock::{HybridTimestamp, ServerId};
use slirc_proto::MessageRef;
use tracing::warn;

/// Handler for the SJOIN command (Safe Join).
///
/// SJOIN is used to sync channel state (modes, topic, members) during bursts
/// and netsplit merges. It handles TS conflict resolution.
pub struct SJoinHandler;

#[async_trait]
impl ServerHandler for SJoinHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Format: SJOIN <ts> <channel> <modes> [args...] :<users>

        let ts_str = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let channel_name = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let modes = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;

        let ts = ts_str.parse::<u64>().map_err(|_| {
            HandlerError::ProtocolError(format!("Invalid timestamp: {}", ts_str))
        })?;

        let arg_count = msg.args().len();
        if arg_count < 4 {
             return Err(HandlerError::NeedMoreParams);
        }

        // The last argument is the user list (validated by arg_count check above)
        let user_list_str = msg.arg(arg_count - 1).ok_or(HandlerError::NeedMoreParams)?;

        // Arguments between modes and the last argument are mode args
        let mut mode_args = Vec::new();
        for i in 3..(arg_count - 1) {
            if let Some(arg) = msg.arg(i) {
                mode_args.push(arg.to_string());
            }
        }

        // Parse user list
        let mut users = Vec::new();
        for user_token in user_list_str.split_whitespace() {
            let mut prefix = String::new();
            let mut uid = String::new();

            for (i, c) in user_token.char_indices() {
                if c.is_alphanumeric() {
                    uid = user_token[i..].to_string();
                    break;
                } else {
                    prefix.push(c);
                }
            }

            if uid.is_empty() {
                uid = user_token.to_string();
            }

            users.push((prefix, uid));
        }

        // Get or create channel actor
        let tx = ctx.matrix.channel_manager.get_or_create_actor(
            channel_name.to_string(),
            std::sync::Arc::downgrade(ctx.matrix),
        ).await;

        // Convert TS6 SJOIN to CRDT for lossless merge semantics
        // Extract source SID from message prefix (e.g., ":00A SJOIN ...")
        let source_sid = msg
            .prefix
            .as_ref()
            .and_then(|p| {
                if p.is_server() {
                    p.raw.split('.').next() // Get SID portion if server name
                } else {
                    // For server messages, raw prefix is often just the SID (e.g., "00A")
                    Some(p.raw)
                }
            })
            .map(|s| s.to_string())
            .unwrap_or_else(|| "000".to_string());
        let source = ServerId::new(source_sid);

        let crdt = sjoin_to_crdt(
            channel_name,
            ts,
            modes,
            &mode_args,
            &users,
            &source,
        );

        let event = ChannelEvent::MergeCrdt {
            crdt: Box::new(crdt),
            source: Some(source),
        };

        if let Err(e) = tx.send(event).await {
            warn!(channel = %channel_name, error = %e, "Failed to send SJOIN to channel actor");
        }

        Ok(())
    }
}
// =============================================================================
// CRDT Conversion Functions
// =============================================================================

/// Convert SJOIN data into a CRDT for merging.
///
/// This function creates a `ChannelCrdt` from TS6 SJOIN parameters,
/// enabling lossless merge semantics instead of "lower TS wins" data loss.
fn sjoin_to_crdt(
    channel_name: &str,
    ts: u64,
    modes: &str,
    mode_args: &[String],
    users: &[(String, String)], // (prefix, uid)
    server_id: &ServerId,
) -> ChannelCrdt {
    // Create HybridTimestamp from Unix TS and server_id
    // Note: TS is seconds, HybridTimestamp wants millis
    let base_hts = HybridTimestamp::new((ts as i64) * 1000, 0, server_id);

    // Create base CRDT with the base timestamp
    let mut crdt = ChannelCrdt::new(channel_name.to_string(), base_hts);

    // Use incremented timestamp for mode values to ensure they override defaults
    let mode_hts = base_hts.increment();

    // Parse modes string and apply to CRDT
    apply_modes_to_crdt(&mut crdt, modes, mode_args, mode_hts);

    // Parse users and add to membership CRDT
    // Use double-incremented timestamp for prefix modes to ensure they override
    // the default member modes created by join()
    let prefix_hts = mode_hts.increment();
    for (prefix, uid) in users {
        crdt.members.join(uid.clone(), mode_hts);
        if let Some(member_modes) = crdt.members.get_modes_mut(uid) {
            apply_prefix_to_member_modes(member_modes, prefix, prefix_hts);
        }
    }

    crdt
}

/// Apply SJOIN mode string to CRDT.
///
/// Parses a mode string like "+ntsk" with corresponding arguments and
/// applies each mode to the appropriate CRDT register.
fn apply_modes_to_crdt(
    crdt: &mut ChannelCrdt,
    modes: &str,
    mode_args: &[String],
    hts: HybridTimestamp,
) {
    let mut arg_idx = 0;

    // Skip leading '+' if present (modes in SJOIN are always positive)
    let mode_chars: Vec<char> = modes.chars().filter(|&c| c != '+').collect();

    for c in mode_chars {
        match c {
            'n' => crdt.modes.no_external.update(true, hts),
            't' => crdt.modes.topic_ops_only.update(true, hts),
            'm' => crdt.modes.moderated.update(true, hts),
            'i' => crdt.modes.invite_only.update(true, hts),
            's' => crdt.modes.secret.update(true, hts),
            'p' => crdt.modes.private.update(true, hts),
            'R' => crdt.modes.registered_only.update(true, hts),
            'c' => crdt.modes.no_colors.update(true, hts),
            'C' => crdt.modes.no_ctcp.update(true, hts),
            'z' | 'S' => crdt.modes.ssl_only.update(true, hts),
            'k' => {
                // Key mode requires an argument
                if arg_idx < mode_args.len() {
                    crdt.key.update(Some(mode_args[arg_idx].clone()), hts);
                    arg_idx += 1;
                }
            }
            'l' => {
                // Limit mode requires a numeric argument
                if arg_idx < mode_args.len() {
                    if let Ok(limit) = mode_args[arg_idx].parse::<u32>() {
                        crdt.limit.update(Some(limit), hts);
                    }
                    arg_idx += 1;
                }
            }
            _ => {
                // Unknown mode - skip any argument it might consume
                // (paranoid handling for forward compatibility)
            }
        }
    }
}

/// Apply SJOIN prefix characters to member modes CRDT.
///
/// Prefix mapping:
/// - `~` → owner (+q)
/// - `&` → admin (+a)
/// - `@` → op (+o)
/// - `%` → halfop (+h)
/// - `+` → voice (+v)
fn apply_prefix_to_member_modes(
    m_crdt: &mut MemberModesCrdt,
    prefix: &str,
    hts: HybridTimestamp,
) {
    for c in prefix.chars() {
        match c {
            '~' => m_crdt.owner.update(true, hts),
            '&' => m_crdt.admin.update(true, hts),
            '@' => m_crdt.op.update(true, hts),
            '%' => m_crdt.halfop.update(true, hts),
            '+' => m_crdt.voice.update(true, hts),
            _ => {} // Ignore unknown prefixes
        }
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_server_id() -> ServerId {
        ServerId::new("00A".to_string())
    }

    #[test]
    fn test_sjoin_to_crdt_empty_channel() {
        let crdt = sjoin_to_crdt(
            "#test",
            1700000000,
            "+nt",
            &[],
            &[],
            &test_server_id(),
        );

        assert_eq!(crdt.name, "#test");
        assert!(*crdt.modes.no_external.value());
        assert!(*crdt.modes.topic_ops_only.value());
        assert!(!*crdt.modes.moderated.value());
        assert!(crdt.members.is_empty());
    }

    #[test]
    fn test_sjoin_to_crdt_with_users() {
        let users = vec![
            ("@".to_string(), "00AAAAAAA".to_string()),
            ("+".to_string(), "00AAAAAAB".to_string()),
            ("".to_string(), "00AAAAAAC".to_string()),
        ];

        let crdt = sjoin_to_crdt(
            "#channel",
            1700000000,
            "+nt",
            &[],
            &users,
            &test_server_id(),
        );

        assert_eq!(crdt.members.len(), 3);
        assert!(crdt.members.contains("00AAAAAAA"));
        assert!(crdt.members.contains("00AAAAAAB"));
        assert!(crdt.members.contains("00AAAAAAC"));

        // Check op user
        let op_modes = crdt.members.get_modes("00AAAAAAA").unwrap();
        assert!(*op_modes.op.value());
        assert!(!*op_modes.voice.value());

        // Check voiced user
        let voice_modes = crdt.members.get_modes("00AAAAAAB").unwrap();
        assert!(!*voice_modes.op.value());
        assert!(*voice_modes.voice.value());

        // Check regular user
        let reg_modes = crdt.members.get_modes("00AAAAAAC").unwrap();
        assert!(!*reg_modes.op.value());
        assert!(!*reg_modes.voice.value());
    }

    #[test]
    fn test_sjoin_to_crdt_with_key() {
        let crdt = sjoin_to_crdt(
            "#secret",
            1700000000,
            "+ntk",
            &["secretkey".to_string()],
            &[],
            &test_server_id(),
        );

        assert_eq!(crdt.key.value(), &Some("secretkey".to_string()));
    }

    #[test]
    fn test_sjoin_to_crdt_with_limit() {
        let crdt = sjoin_to_crdt(
            "#limited",
            1700000000,
            "+ntl",
            &["50".to_string()],
            &[],
            &test_server_id(),
        );

        assert_eq!(crdt.limit.value(), &Some(50));
    }

    #[test]
    fn test_sjoin_to_crdt_with_key_and_limit() {
        let crdt = sjoin_to_crdt(
            "#both",
            1700000000,
            "+ntkl",
            &["password".to_string(), "100".to_string()],
            &[],
            &test_server_id(),
        );

        assert_eq!(crdt.key.value(), &Some("password".to_string()));
        assert_eq!(crdt.limit.value(), &Some(100));
    }

    #[test]
    fn test_sjoin_to_crdt_multiple_prefixes() {
        // User with @+ (op AND voice)
        let users = vec![("@+".to_string(), "00AAAAAAA".to_string())];

        let crdt = sjoin_to_crdt(
            "#test",
            1700000000,
            "+nt",
            &[],
            &users,
            &test_server_id(),
        );

        let modes = crdt.members.get_modes("00AAAAAAA").unwrap();
        assert!(*modes.op.value());
        assert!(*modes.voice.value());
        assert!(!*modes.owner.value());
    }

    #[test]
    fn test_sjoin_to_crdt_all_prefixes() {
        // User with all prefixes ~&@%+
        let users = vec![("~&@%+".to_string(), "00AAAAAAA".to_string())];

        let crdt = sjoin_to_crdt(
            "#test",
            1700000000,
            "+nt",
            &[],
            &users,
            &test_server_id(),
        );

        let modes = crdt.members.get_modes("00AAAAAAA").unwrap();
        assert!(*modes.owner.value());
        assert!(*modes.admin.value());
        assert!(*modes.op.value());
        assert!(*modes.halfop.value());
        assert!(*modes.voice.value());
    }

    #[test]
    fn test_sjoin_to_crdt_all_boolean_modes() {
        let crdt = sjoin_to_crdt(
            "#allmodes",
            1700000000,
            "+ntmispcCzR",
            &[],
            &[],
            &test_server_id(),
        );

        assert!(*crdt.modes.no_external.value());
        assert!(*crdt.modes.topic_ops_only.value());
        assert!(*crdt.modes.moderated.value());
        assert!(*crdt.modes.invite_only.value());
        assert!(*crdt.modes.secret.value());
        assert!(*crdt.modes.private.value());
        assert!(*crdt.modes.no_colors.value());
        assert!(*crdt.modes.no_ctcp.value());
        assert!(*crdt.modes.ssl_only.value());
        assert!(*crdt.modes.registered_only.value());
    }

    #[test]
    fn test_sjoin_to_crdt_timestamp_conversion() {
        let ts = 1700000000_u64; // Unix timestamp in seconds
        let crdt = sjoin_to_crdt(
            "#test",
            ts,
            "+n",
            &[],
            &[],
            &test_server_id(),
        );

        // HybridTimestamp should have millis = ts * 1000
        assert_eq!(crdt.created_at.millis, (ts as i64) * 1000);
    }

    #[test]
    fn test_sjoin_to_crdt_mode_with_leading_plus() {
        // Some servers send "+nt", some send "nt"
        let crdt1 = sjoin_to_crdt("#test", 1700000000, "+nt", &[], &[], &test_server_id());
        let crdt2 = sjoin_to_crdt("#test", 1700000000, "nt", &[], &[], &test_server_id());

        assert!(*crdt1.modes.no_external.value());
        assert!(*crdt1.modes.topic_ops_only.value());
        assert!(*crdt2.modes.no_external.value());
        assert!(*crdt2.modes.topic_ops_only.value());
    }

    #[test]
    fn test_apply_modes_to_crdt_invalid_limit() {
        let server_id = test_server_id();
        let hts = HybridTimestamp::new(1700000000000, 0, &server_id);
        let mut crdt = ChannelCrdt::new("#test".to_string(), hts);

        // Invalid limit (not a number) should be ignored
        apply_modes_to_crdt(&mut crdt, "+l", &["notanumber".to_string()], hts);
        assert_eq!(crdt.limit.value(), &None);
    }
}
