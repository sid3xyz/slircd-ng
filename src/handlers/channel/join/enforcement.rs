//! Channel access enforcement and auto-mode application.

use super::super::super::Context;
use crate::db::ChannelRepository;
use crate::state::{MemberModes, RegisteredState};

/// Check if user should receive auto-op or auto-voice on a registered channel.
/// Returns Some(MemberModes) if the user has access, None otherwise.
/// Takes pre-fetched account info to avoid redundant user lookup.
pub(super) async fn check_auto_modes(
    ctx: &Context<'_, RegisteredState>,
    channel_lower: &str,
    is_registered: bool,
    account: &Option<String>,
) -> Option<MemberModes> {
    // Early return if user is not registered
    if !is_registered {
        return None;
    }

    let account_name = account.as_ref()?;

    let account_record = ctx.db.accounts().find_by_name(account_name).await.ok()??;
    let channel_record = ctx.db.channels().find_by_name(channel_lower).await.ok()??;

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

    let access = ctx
        .db
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
    ctx: &Context<'_, RegisteredState>,
    channel_lower: &str,
    nick: &str,
    user: &str,
    host: &str,
) -> Option<crate::db::ChannelAkick> {
    let channel_record = ctx.db.channels().find_by_name(channel_lower).await.ok()??;

    ctx.db
        .channels()
        .check_akick(channel_record.id, nick, user, host)
        .await
        .ok()?
}
