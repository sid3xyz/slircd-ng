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

        // The last argument is the user list
        let user_list_str = msg.arg(arg_count - 1).unwrap();

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
    let hts = HybridTimestamp::new((ts as i64) * 1000, 0, server_id);

    // Create base CRDT
    let mut crdt = ChannelCrdt::new(channel_name.to_string(), hts);

    // Parse modes string and apply to CRDT
    apply_modes_to_crdt(&mut crdt, modes, mode_args, hts);

    // Parse users and add to membership CRDT
    for (prefix, uid) in users {
        crdt.members.join(uid.clone(), hts);
        if let Some(member_modes) = crdt.members.get_modes_mut(uid) {
            apply_prefix_to_member_modes(member_modes, prefix, hts);
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