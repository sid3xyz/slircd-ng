//! Channel creation logic and initial setup.
//!
//! This module contains the core JOIN channel orchestration logic.
//! Channel creation itself is handled by the ChannelActor; this module
//! manages the handshake: checking access, sending events, and handling responses.

use super::super::super::{Context, HandlerError, HandlerResult, user_prefix};
use super::enforcement::{check_akick, check_auto_modes};
use super::responses::{JoinSuccessContext, handle_join_success, send_join_error};
use crate::error::ChannelError;
use crate::handlers::ResponseMiddleware;
use crate::handlers::helpers::fanout::broadcast_to_account;
use crate::security::UserContext;
use crate::state::{RegisteredState, Topic};
use slirc_proto::ircv3::msgid::generate_msgid;
use slirc_proto::ircv3::server_time::format_server_time;
use slirc_proto::{Command, Message, Prefix, irc_to_lower};
use std::sync::Arc;
use tracing::info;

/// Join a single channel.
/// This is the main orchestration function for channel joining.
pub(super) async fn join_channel(
    ctx: &mut Context<'_, RegisteredState>,
    channel_name: &str,
    provided_key: Option<&str>,
) -> HandlerResult {
    // Lookup RAW sender (Arc<Message>)
    let sender = ctx
        .matrix
        .user_manager
        .get_first_sender(ctx.uid)
        .ok_or(HandlerError::Internal("No sender found".into()))?;

    // Attempt to clone matrix as Arc - assuming ctx.matrix creates an Arc reference
    let matrix_arc: Arc<crate::state::Matrix> = ctx.matrix.clone();

    let join_msg = join_channel_internal(
        matrix_arc.clone(),
        ctx.uid,
        &sender,
        ctx.sender.clone(),
        ctx.server_name(),
        ctx.state.is_tls,
        channel_name,
        provided_key,
        ctx.active_batch_id.as_deref(),
        ctx.label.as_deref(),
        Some(ctx.db),
    )
    .await?;

    // Synchronization for sibling sessions (Multiclient)
    if ctx.matrix.config.multiclient.enabled
        && let Some(join_msg) = join_msg
    {
        broadcast_to_account(ctx, join_msg, true).await?;
    }

    Ok(())
}

/// Internal join logic decoupled from Context to support sibling joins.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn join_channel_internal(
    matrix: Arc<crate::state::Matrix>,
    uid: &str,
    sender: &tokio::sync::mpsc::Sender<Arc<Message>>,
    response_sender: ResponseMiddleware<'_>,
    server_name: &str,
    is_tls: bool,
    channel_name: &str,
    provided_key: Option<&str>,
    batch_id: Option<&str>,
    label: Option<&str>,
    db: Option<&crate::db::Database>,
) -> Result<Option<Message>, HandlerError> {
    let channel_lower = irc_to_lower(channel_name);

    // Single user read to capture all needed fields (eliminates redundant lookup)
    let (
        nick,
        user_name,
        visible_host,
        real_host,
        realname,
        session_id,
        account,
        away_message,
        caps,
        is_registered,
        is_oper,
        oper_type,
    ) = {
        let user_ref = matrix
            .user_manager
            .users
            .get(uid)
            .ok_or(HandlerError::NickOrUserMissing)?;
        let user = user_ref.read().await;
        (
            user.nick.clone(),
            user.user.clone(),
            user.visible_host.clone(),
            user.host.clone(),
            user.realname.clone(),
            user.session_id,
            user.account.clone(),
            user.away.clone(),
            user.caps.clone(),
            user.modes.registered,
            user.modes.oper,
            user.modes.oper_type.clone(),
        )
    };

    let user_context = UserContext::for_registration(crate::security::RegistrationParams {
        hostname: real_host.clone(),
        nickname: nick.clone(),
        username: user_name.clone(),
        realname: realname.clone(),
        server: server_name.to_string(),
        account: account.clone(),
        is_tls,
        is_oper,
        oper_type,
    });

    let is_registered_channel = matrix
        .channel_manager
        .registered_channels
        .contains(&channel_lower);

    // Check AKICK before joining (pass pre-fetched host)
    if let Some(db) = db
        && is_registered_channel
        && let Some(akick) = check_akick(db, &channel_lower, &nick, &user_name, &real_host).await
    {
        let reason = akick
            .reason
            .as_deref()
            .unwrap_or("You are banned from this channel");
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::new(
                "ChanServ".to_string(),
                "ChanServ".to_string(),
                "services.".to_string(),
            )),
            command: Command::NOTICE(
                nick.clone(),
                format!(
                    "You are not permitted to be on \x02{}\x02: {}",
                    channel_name, reason
                ),
            ),
        };
        response_sender
            .send(notice)
            .await
            .map_err(|_| HandlerError::Internal("Failed to send AKICK notice".into()))?;
        info!(
            nick = %nick,
            channel = %channel_name,
            "AKICK triggered"
        );
        return Ok(None);
    }

    // Check auto modes if registered
    let initial_modes = if let Some(db) = db
        && is_registered_channel
    {
        check_auto_modes(db, &channel_lower, is_registered, &account).await
    } else {
        None
    };

    // Build JOIN messages
    let account_name = account.as_deref().unwrap_or("*");
    let join_msgid = generate_msgid();
    let join_timestamp = format_server_time();
    let join_nanotime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as i64;
    let make_extended_join_msg = || {
        Message {
            tags: None,
            prefix: Some(user_prefix(&nick, &user_name, &visible_host)),
            command: Command::JOIN(
                channel_name.to_string(),
                Some(account_name.to_string()),
                Some(realname.clone()),
            ),
        }
        .with_tag("msgid", Some(join_msgid.clone()))
        .with_tag("time", Some(join_timestamp.clone()))
    };

    let make_standard_join_msg = || {
        Message {
            tags: None,
            prefix: Some(user_prefix(&nick, &user_name, &visible_host)),
            command: Command::JOIN(channel_name.to_string(), None, None),
        }
        .with_tag("msgid", Some(join_msgid.clone()))
        .with_tag("time", Some(join_timestamp.clone()))
    };

    let mut attempt = 0;

    // Pre-load saved topic for registered channels (passed to actor at spawn)
    let initial_topic = if is_registered_channel {
        if let Some(db) = db {
            db.channels()
                .find_by_name(&channel_lower)
                .await
                .ok()
                .flatten()
                .filter(|r| r.keeptopic)
                .and_then(|r| match (r.topic_text, r.topic_set_by, r.topic_set_at) {
                    (Some(text), Some(set_by), Some(set_at)) => Some(Topic {
                        text,
                        set_by,
                        set_at,
                    }),
                    _ => None,
                })
        } else {
            None
        }
    } else {
        None
    };

    let mailbox_capacity = matrix.config.limits.channel_mailbox_capacity;

    loop {
        let observer = matrix.channel_manager.observer.clone();
        let channel_sender = matrix
            .channel_manager
            .channels
            .entry(channel_lower.clone())
            .or_insert_with(|| {
                if let Some(m) = crate::metrics::ACTIVE_CHANNELS.get() { m.inc(); }
                crate::state::actor::ChannelActor::spawn_with_capacity(
                    channel_name.to_string(),
                    Arc::downgrade(&matrix),
                    initial_topic.clone(),
                    None, // initial_modes
                    None, // created_at
                    mailbox_capacity,
                    observer,
                )
            })
            .clone();

        let extended_join_msg = make_extended_join_msg();
        info!(?extended_join_msg, "Created extended join msg");
        let standard_join_msg = make_standard_join_msg();

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

        let _ = channel_sender
            .send(crate::state::actor::ChannelEvent::Join {
                params: Box::new(crate::state::actor::JoinParams {
                    uid: uid.to_string(),
                    nick: nick.clone(),
                    sender: sender.clone(),
                    caps: caps.clone(),
                    user_context: user_context.clone(),
                    key: provided_key.map(|s| s.to_string()),
                    initial_modes: initial_modes.clone(),
                    join_msg_extended: extended_join_msg.clone(),
                    join_msg_standard: standard_join_msg.clone(),
                    session_id,
                    nanotime: join_nanotime,
                }),
                reply_tx,
            })
            .await;

        match reply_rx.await {
            Ok(Ok(data)) => {
                let self_join_msg = handle_join_success(JoinSuccessContext {
                    response_sender: response_sender.clone(),
                    server_name: server_name.to_string(),
                    active_batch_id: batch_id.map(String::from),
                    label: label.map(String::from),
                    user_manager: &matrix.user_manager,
                    channel_manager: &matrix.channel_manager,
                    client_manager: &matrix.client_manager,
                    config_multiclient: matrix.config.multiclient.enabled,
                    uid_str: uid.to_string(),
                    caps: caps.clone(),

                    channel_sender: &channel_sender,
                    channel_lower: &channel_lower,
                    nick: &nick,
                    user_name: &user_name,
                    visible_host: &visible_host,
                    extended_join_msg: &extended_join_msg,
                    standard_join_msg: &standard_join_msg,
                    away_message: &away_message,
                    data,
                    session_id,
                    account: account.clone(),
                })
                .await?;
                return Ok(self_join_msg);
            }
            Ok(Err(error)) => {
                if matches!(error, ChannelError::ChannelTombstone) && attempt == 0 {
                    if matrix
                        .channel_manager
                        .channels
                        .remove(&channel_lower)
                        .is_some()
                    {
                        if let Some(m) = crate::metrics::ACTIVE_CHANNELS.get() { m.dec(); }
                    }
                    attempt += 1;
                    continue;
                }

                send_join_error(response_sender, server_name, &nick, channel_name, error).await?;
                return Ok(None);
            }
            Err(_) => {
                if attempt == 0 {
                    if matrix
                        .channel_manager
                        .channels
                        .remove(&channel_lower)
                        .is_some()
                    {
                        if let Some(m) = crate::metrics::ACTIVE_CHANNELS.get() { m.dec(); }
                    }
                    attempt += 1;
                    continue;
                }
                send_join_error(
                    response_sender,
                    server_name,
                    &nick,
                    channel_name,
                    ChannelError::ChannelTombstone,
                )
                .await?;
                return Ok(None);
            }
        }
    }
}
