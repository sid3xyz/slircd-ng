//! Channel access enforcement and auto-mode application.

use super::super::super::Context;
use crate::db::ChannelRepository;
use crate::state::{MemberModes, RegisteredState};

/// Check if user should receive auto-op or auto-voice on a registered channel.
/// Returns Some(MemberModes) if the user has access, None otherwise.
/// Takes pre-fetched account info to avoid redundant user lookup.
pub(super) async fn check_auto_modes(
    db: &crate::db::Database,
    channel_lower: &str,
    is_registered: bool,
    account: &Option<String>,
) -> Option<MemberModes> {
    // Early return if user is not registered
    if !is_registered {
        return None;
    }

    let account_name = account.as_ref()?;

    let account_record = db.accounts().find_by_name(account_name).await.ok()??;
    let channel_record = db.channels().find_by_name(channel_lower).await.ok()??;

    // Check if user is founder
    if account_record.id == channel_record.founder_account_id {
        return Some(MemberModes {
            owner: false,
            owner_ts: None,
            admin: false,
            admin_ts: None,
            op: true,
            op_ts: None,
            halfop: false,
            halfop_ts: None,
            voice: false,
            voice_ts: None,
            join_time: None,
        });
    }

    let access = db
        .channels()
        .get_access(channel_record.id, account_record.id)
        .await
        .ok()??;

    let op = ChannelRepository::has_op_access(&access.flags);
    let voice = ChannelRepository::has_voice_access(&access.flags);

    if op || voice {
        Some(MemberModes {
            owner: false,
            owner_ts: None,
            admin: false,
            admin_ts: None,
            op,
            op_ts: None,
            halfop: false,
            halfop_ts: None,
            voice,
            voice_ts: None,
            join_time: None,
        })
    } else {
        None
    }
}

/// Check if user is on the AKICK list for a channel.
/// Returns the matching AKICK entry if found.
/// Takes pre-fetched host to avoid redundant user lookup.
pub(super) async fn check_akick(
    db: &crate::db::Database,
    channel_lower: &str,
    nick: &str,
    user: &str,
    host: &str,
) -> Option<crate::db::ChannelAkick> {
    let channel_record = db.channels().find_by_name(channel_lower).await.ok()??;

    db
        .channels()
        .check_akick(channel_record.id, nick, user, host)
        .await
        .ok()?
}
/// Check if a channel should forward on the given error and return the forward target if so.
/// Returns the forward target channel name if forwarding should occur.
pub(super) async fn check_forward(
    ctx: &Context<'_, RegisteredState>,
    channel_lower: &str,
    error: &crate::error::ChannelError,
) -> Option<String> {
    use crate::error::ChannelError;
    use crate::state::actor::ChannelMode;

    // Forwarding only applies to invite-only and full channel errors
    let should_forward = matches!(
        error,
        ChannelError::InviteOnlyChan | ChannelError::ChannelIsFull
    );
    if !should_forward {
        return None;
    }

    // Get the channel and query its modes
    if let Some(channel_sender) = ctx.matrix.channel_manager.channels.get(channel_lower) {
        let (modes_tx, modes_rx) = tokio::sync::oneshot::channel();
        let _ = channel_sender
            .send(crate::state::actor::ChannelEvent::GetModes { reply_tx: modes_tx })
            .await;

        if let Ok(modes) = modes_rx.await {
            // Look for Forward mode
            for mode in modes {
                if let ChannelMode::Forward(target, _) = mode {
                    return Some(target);
                }
            }
        }
    }

    None
}
