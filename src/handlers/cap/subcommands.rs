use super::helpers::{CapListParams, build_cap_list_tokens, pack_cap_ls_lines};
use super::types::SUPPORTED_CAPS;
use crate::handlers::{Context, HandlerResult};
use crate::state::SessionState;
use crate::state::dashmap_ext::DashMapExt;
use slirc_proto::{CapSubCommand, Command, Message};
use tracing::{debug, info};

/// Handle `CAP LS [version]` - list available capabilities.
pub async fn handle_ls<S: SessionState>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    version_arg: Option<&str>,
) -> HandlerResult {
    // Parse version (301 default, 302 if specified)
    let version: u32 = version_arg.and_then(|v| v.parse().ok()).unwrap_or(301);

    // Set CAP negotiation flag
    ctx.state.set_cap_negotiating(true);
    ctx.state.set_cap_version(version);

    // CAP LS 302+ implicitly enables cap-notify per IRCv3 spec
    // https://ircv3.net/specs/extensions/capability-negotiation#cap-notify
    if version >= 302 {
        ctx.state
            .capabilities_mut()
            .insert("cap-notify".to_string());
    }

    let server_name = ctx.server_name();

    // Get STS config if TLS is configured
    let sts_cfg = ctx
        .matrix
        .config
        .tls
        .as_ref()
        .and_then(|tls| tls.sts.as_ref());

    // Build capability tokens (include EXTERNAL if TLS with cert)
    let caps = build_cap_list_tokens(&CapListParams {
        version,
        is_tls: ctx.state.is_tls(),
        has_cert: ctx.state.is_tls() && ctx.state.certfp().is_some(),
        acct_cfg: &ctx.matrix.config.account_registration,
        sts_cfg,
    });

    // CAP LS may need to be split across multiple lines to satisfy the IRC 512-byte limit.
    // We pack space-separated capability tokens into lines using the actual serialized
    // message length as the constraint.
    let cap_lines = pack_cap_ls_lines(server_name, nick, &caps);

    for (i, caps_str) in cap_lines.iter().enumerate() {
        let has_more = i + 1 < cap_lines.len();
        let more_marker = if has_more {
            Some("*".to_string())
        } else {
            None
        };

        let reply = Message {
            tags: None,
            prefix: Some(ctx.server_prefix()),
            command: Command::CAP(
                Some(nick.to_string()),
                CapSubCommand::LS,
                more_marker,
                Some(caps_str.clone()),
            ),
        };

        ctx.sender.send(reply).await?;
    }

    debug!(nick = %nick, version = %version, "CAP LS sent");
    Ok(())
}

/// Handle CAP LIST - list currently enabled capabilities.
pub async fn handle_list<S: SessionState>(ctx: &mut Context<'_, S>, nick: &str) -> HandlerResult {
    let enabled: String = ctx
        .state
        .capabilities()
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    let reply = Message {
        tags: None,
        prefix: Some(ctx.server_prefix()),
        command: Command::CAP(
            Some(nick.to_string()),
            CapSubCommand::LIST,
            None,
            Some(enabled),
        ),
    };
    ctx.sender.send(reply).await?;

    debug!(nick = %nick, "CAP LIST sent");
    Ok(())
}

/// Handle `CAP REQ :<capabilities>` - request capabilities.
pub async fn handle_req<S: SessionState>(
    ctx: &mut Context<'_, S>,
    nick: &str,
    caps_arg: Option<&str>,
) -> HandlerResult {
    let requested = caps_arg.unwrap_or("");

    let mut accepted = Vec::with_capacity(8); // Typical CAP REQ has 5-10 capabilities
    let mut rejected = Vec::with_capacity(4);

    for cap in requested.split_whitespace() {
        // Check for removal prefix
        let (is_removal, cap_name) = if let Some(name) = cap.strip_prefix('-') {
            (true, name)
        } else {
            (false, cap)
        };

        // Strip any value suffix (cap=value) - split always returns at least one element
        let cap_base = cap_name.split('=').next().unwrap_or(cap_name);

        let is_supported = SUPPORTED_CAPS.iter().any(|c| c.as_ref() == cap_base);

        if is_supported {
            if is_removal {
                ctx.state.capabilities_mut().remove(cap_base);
                accepted.push(format!("-{}", cap_base));
            } else {
                ctx.state.capabilities_mut().insert(cap_base.to_string());
                accepted.push(cap_base.to_string());
            }
        } else {
            rejected.push(cap_base.to_string());
        }
    }

    // If any were rejected, send NAK for entire request (per spec)
    if !rejected.is_empty() {
        let reply = Message {
            tags: None,
            prefix: Some(ctx.server_prefix()),
            command: Command::CAP(
                Some(nick.to_string()),
                CapSubCommand::NAK,
                None,
                Some(requested.to_string()),
            ),
        };
        ctx.sender.send(reply).await?;
        debug!(nick = %nick, rejected = ?rejected, "CAP REQ NAK");
    } else {
        // All accepted
        let reply = Message {
            tags: None,
            prefix: Some(ctx.server_prefix()),
            command: Command::CAP(
                Some(nick.to_string()),
                CapSubCommand::ACK,
                None,
                Some(accepted.join(" ")),
            ),
        };
        ctx.sender.send(reply).await?;
        debug!(nick = %nick, accepted = ?accepted, "CAP REQ ACK");

        // If user is registered, sync capabilities to their User in Matrix
        // This enables mid-session CAP REQ (e.g., requesting message-tags after registration)
        if ctx.state.is_registered() {
            let user_arc = ctx.matrix.user_manager.users.get_cloned(ctx.uid);
            if let Some(user_arc) = user_arc {
                let (channels, new_caps) = {
                    let mut user = user_arc.write().await;
                    user.caps = ctx.state.capabilities().clone();
                    (
                        user.channels.iter().cloned().collect::<Vec<_>>(),
                        user.caps.clone(),
                    )
                };

                // Keep ChannelActor per-user capability caches in sync.
                // Without this, capability-gated broadcasts (e.g., setname, away-notify) can be wrong
                // if the user negotiates capabilities after joining channels.
                for channel_lower in channels {
                    if let Some(sender) = ctx
                        .matrix
                        .channel_manager
                        .channels
                        .get_cloned(&channel_lower)
                    {
                        let _ = sender
                            .send(crate::state::actor::ChannelEvent::UpdateCaps {
                                uid: ctx.uid.to_string(),
                                caps: new_caps.clone(),
                            })
                            .await;
                    }
                }

                debug!(uid = %ctx.uid, caps = ?new_caps, "Synced caps to Matrix user");

                // Update per-session capabilities for mid-session CAP changes
                // so per-session fanout uses the latest negotiated caps.
                ctx.matrix
                    .user_manager
                    .update_session_caps(ctx.state.session_id(), new_caps.clone());
            }
        }
    }

    Ok(())
}

/// Handle CAP END - end capability negotiation.
pub async fn handle_end<S: SessionState>(ctx: &mut Context<'_, S>, nick: &str) -> HandlerResult {
    ctx.state.set_cap_negotiating(false);

    info!(
        nick = %nick,
        capabilities = ?ctx.state.capabilities(),
        "CAP negotiation complete"
    );

    // Note: Registration check is handled by the connection loop, not here.
    // The connection loop checks can_register() after each command dispatch.

    Ok(())
}
