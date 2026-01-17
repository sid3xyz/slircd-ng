//! Multiclient and Bouncer support helpers.
//!
//! Handles echoing messages to other sessions of the same user (self-echo).

use super::delivery::build_local_recipient_message;
use super::types::SenderSnapshot;
use crate::handlers::core::Context;
use slirc_proto::Message;
use std::sync::Arc;
use tracing::debug;

/// Helper to echo a message to other sessions of the same sender (for bouncer/multiclient).
pub async fn echo_to_other_sessions(
    ctx: &Context<'_, crate::state::RegisteredState>,
    msg: &Message,
    snapshot: &SenderSnapshot,
    timestamp: &str,
    msgid: &str,
) {
    debug!("Attempting self-echo for uid={}", ctx.uid);
    if !ctx.matrix.config.multiclient.enabled {
        debug!("Multiclient disabled in config");
        return;
    }

    if let Some(sessions) = ctx.matrix.user_manager.get_senders_cloned(ctx.uid) {
        debug!("Found {} sessions for uid {}", sessions.len(), ctx.uid);
        let mut any_sent = false;

        for sess in sessions {
            // Skip the current session (sender)
            if sess.session_id == ctx.state.session_id {
                continue;
            }

            let caps = ctx
                .matrix
                .user_manager
                .get_session_caps(sess.session_id)
                .unwrap_or_default();

            let msg_for_session = build_local_recipient_message(
                msg,
                &caps,
                snapshot,
                msgid,
                timestamp,
                None, // Self-echo copies never carry labels logic handled separately or N/A
            );

            let _ = sess.tx.send(Arc::new(msg_for_session)).await;
            any_sent = true;
            crate::metrics::MESSAGES_SENT.inc();
        }
        if any_sent {
            debug!(uid = %ctx.uid, "Echoed message to other bouncer sessions");
        }
    }
}
