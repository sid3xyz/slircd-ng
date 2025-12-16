//! CAP command handler for IRCv3 capability negotiation.
//!
//! Implements CAP LS, LIST, REQ, ACK, NAK, END subcommands.
//! Reference: <https://ircv3.net/specs/extensions/capability-negotiation>

use super::{Context, HandlerResult, PreRegHandler, UniversalHandler};
use crate::config::AccountRegistrationConfig;
use crate::state::{SessionState, UnregisteredState};
use async_trait::async_trait;
use slirc_proto::{CapSubCommand, Command, Message, MessageRef, Prefix, Response, Capability};
use tracing::{debug, info, warn};

/// Capabilities we support (subset of slirc_proto::CAPABILITIES).
const SUPPORTED_CAPS: &[Capability] = &[
    Capability::MultiPrefix,
    Capability::UserhostInNames,
    Capability::ServerTime,
    Capability::EchoMessage,
    Capability::Sasl,
    Capability::Batch,
    Capability::MessageTags,
    Capability::LabeledResponse,
    Capability::SetName,
    Capability::AwayNotify,
    Capability::AccountNotify,
    Capability::ExtendedJoin,
    Capability::InviteNotify,
    Capability::ChgHost,
    Capability::Monitor,
    Capability::CapNotify,
    Capability::AccountTag,
    Capability::Multiline,
    Capability::AccountRegistration,
    Capability::ChatHistory,
    Capability::EventPlayback,
];

/// Maximum bytes allowed in a multiline batch message.
const MULTILINE_MAX_BYTES: u32 = 40000;
/// Maximum lines allowed in a multiline batch.
const MULTILINE_MAX_LINES: u32 = 100;

/// Handler for CAP command.
pub struct CapHandler;

#[async_trait]
impl<S: SessionState> UniversalHandler<S> for CapHandler {
    async fn handle(&self, ctx: &mut Context<'_, S>, msg: &MessageRef<'_>) -> HandlerResult {
        // CAP can be used before and after registration
        // CAP <subcommand> [arg]
        let subcommand_str = msg.arg(0).unwrap_or("");
        let arg = msg.arg(1);

        // Get nick using SessionState trait
        let nick = ctx.state.nick_or_star().to_string();

        // Parse subcommand using slirc-proto's FromStr implementation
        let subcommand: CapSubCommand = match subcommand_str.parse() {
            Ok(cmd) => cmd,
            Err(_) => {
                // Send ERR_INVALIDCAPCMD (410) for unknown subcommand
                let reply = Response::err_invalidcapcmd(&nick, subcommand_str)
                    .with_prefix(ctx.server_prefix());
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        match subcommand {
            CapSubCommand::LS => handle_ls(ctx, &nick, arg).await,
            CapSubCommand::LIST => handle_list(ctx, &nick).await,
            CapSubCommand::REQ => handle_req(ctx, &nick, arg).await,
            CapSubCommand::END => handle_end(ctx, &nick).await,
            _ => {
                // ACK, NAK, NEW, DEL are server-to-client only
                debug!(subcommand = ?subcommand, "Ignoring client->server CAP subcommand");
                Ok(())
            }
        }
    }
}

/// Handle `CAP LS [version]` - list available capabilities.
async fn handle_ls<S: SessionState>(ctx: &mut Context<'_, S>, nick: &str, version_arg: Option<&str>) -> HandlerResult {
    // Parse version (301 default, 302 if specified)
    let version: u32 = version_arg.and_then(|v| v.parse().ok()).unwrap_or(301);

    // Set CAP negotiation flag
    ctx.state.set_cap_negotiating(true);
    ctx.state.set_cap_version(version);

    // CAP LS 302+ implicitly enables cap-notify per IRCv3 spec
    // https://ircv3.net/specs/extensions/capability-negotiation#cap-notify
    if version >= 302 {
        ctx.state.capabilities_mut().insert("cap-notify".to_string());
    }

    let server_name = ctx.server_name();

    // Build capability tokens (include EXTERNAL if TLS with cert)
    let caps = build_cap_list_tokens(
        version,
        ctx.state.is_tls() && ctx.state.certfp().is_some(),
        &ctx.matrix.config.account_registration,
    );

    // CAP LS may need to be split across multiple lines to satisfy the IRC 512-byte limit.
    // We pack space-separated capability tokens into lines using the actual serialized
    // message length as the constraint.
    let cap_lines = pack_cap_ls_lines(server_name, nick, &caps);

    for (i, caps_str) in cap_lines.iter().enumerate() {
        let has_more = i + 1 < cap_lines.len();
        let more_marker = if has_more { Some("*".to_string()) } else { None };

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
async fn handle_list<S: SessionState>(ctx: &mut Context<'_, S>, nick: &str) -> HandlerResult {
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
async fn handle_req<S: SessionState>(ctx: &mut Context<'_, S>, nick: &str, caps_arg: Option<&str>) -> HandlerResult {
    let requested = caps_arg.unwrap_or("");

    let mut accepted = Vec::new();
    let mut rejected = Vec::new();

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
        if ctx.state.is_registered()
            && let Some(user_ref) = ctx.matrix.users.get(ctx.uid)
        {
            let (channels, new_caps) = {
                let mut user = user_ref.write().await;
                user.caps = ctx.state.capabilities().clone();
                (user.channels.iter().cloned().collect::<Vec<_>>(), user.caps.clone())
            };

            // Keep ChannelActor per-user capability caches in sync.
            // Without this, capability-gated broadcasts (e.g., setname, away-notify) can be wrong
            // if the user negotiates capabilities after joining channels.
            for channel_lower in channels {
                if let Some(sender) = ctx.matrix.channels.get(&channel_lower) {
                    let _ = sender
                        .send(crate::state::actor::ChannelEvent::UpdateCaps {
                            uid: ctx.uid.to_string(),
                            caps: new_caps.clone(),
                        })
                        .await;
                }
            }

            debug!(uid = %ctx.uid, caps = ?new_caps, "Synced caps to Matrix user");
        }
    }

    Ok(())
}

/// Handle CAP END - end capability negotiation.
async fn handle_end<S: SessionState>(ctx: &mut Context<'_, S>, nick: &str) -> HandlerResult {
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

/// Build capability list string for CAP LS response.
///
/// `has_cert` indicates whether the client presented a TLS certificate,
/// which enables SASL EXTERNAL.
fn build_cap_list_tokens(
    version: u32,
    has_cert: bool,
    acct_cfg: &AccountRegistrationConfig,
) -> Vec<String> {
    SUPPORTED_CAPS
        .iter()
        .map(|cap| {
            // For CAP 302+, add values for caps that have them
            if version >= 302 {
                match cap {
                    Capability::Sasl => {
                        if has_cert {
                            "sasl=PLAIN,EXTERNAL".to_string()
                        } else {
                            "sasl=PLAIN".to_string()
                        }
                    }
                    Capability::Multiline => {
                        format!(
                            "draft/multiline=max-bytes={},max-lines={}",
                            MULTILINE_MAX_BYTES, MULTILINE_MAX_LINES
                        )
                    }
                    Capability::AccountRegistration => {
                        // Build flags based on server configuration
                        let mut flags = Vec::new();
                        if acct_cfg.custom_account_name {
                            flags.push("custom-account-name");
                        }
                        if acct_cfg.before_connect {
                            flags.push("before-connect");
                        }
                        if acct_cfg.email_required {
                            flags.push("email-required");
                        }
                        if flags.is_empty() {
                            "draft/account-registration".to_string()
                        } else {
                            format!("draft/account-registration={}", flags.join(","))
                        }
                    }
                    _ => cap.as_ref().to_string(),
                }
            } else {
                cap.as_ref().to_string()
            }
        })
        .collect()
}

fn pack_cap_ls_lines(server_name: &str, nick: &str, caps: &[String]) -> Vec<String> {
    // If there are no capabilities, send a single empty line.
    if caps.is_empty() {
        return vec![String::new()];
    }

    // Helper: check whether a CAP LS line fits the IRC 512-byte limit when serialized.
    // For packing we always assume the "*" continuation marker is present, which makes
    // the check stricter than the final line requires.
    fn fits(server_name: &str, nick: &str, caps_str: &str) -> bool {
        let msg = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.to_string())),
            command: Command::CAP(
                Some(nick.to_string()),
                CapSubCommand::LS,
                Some("*".to_string()),
                Some(caps_str.to_string()),
            ),
        };
        msg.to_string().len() <= 512
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for cap in caps {
        let candidate = if current.is_empty() {
            cap.clone()
        } else {
            format!("{} {}", current, cap)
        };

        if fits(server_name, nick, &candidate) {
            current = candidate;
            continue;
        }

        if !current.is_empty() {
            lines.push(current);
            current = String::new();
        }

        // If a single token doesn't fit, we still have to send *something*.
        // This should be practically unreachable with sane capability values.
        if fits(server_name, nick, cap) {
            current = cap.clone();
        } else {
            lines.push(cap.clone());
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
}

/// Handler for AUTHENTICATE command (SASL authentication).
pub struct AuthenticateHandler;

#[async_trait]
impl PreRegHandler for AuthenticateHandler {
    async fn handle(&self, ctx: &mut Context<'_, UnregisteredState>, msg: &MessageRef<'_>) -> HandlerResult {
        // AUTHENTICATE <data>
        let data = msg.arg(0).unwrap_or("");

        // Get nick using SessionState trait
        let nick = ctx.state.nick_or_star().to_string();

        // Check if SASL is enabled
        if !ctx.state.capabilities.contains("sasl") {
            // SASL not enabled, ignore
            debug!(nick = %nick, "AUTHENTICATE received but SASL not enabled");
            return Ok(());
        }

        // Handle SASL flow
        match ctx.state.sasl_state.clone() {
            SaslState::None => {
                // Client is initiating SASL with mechanism name
                if data.eq_ignore_ascii_case("PLAIN") {
                    ctx.state.sasl_state = SaslState::WaitingForData;
                    // Send empty challenge (AUTHENTICATE +)
                    let reply = Message {
                        tags: None,
                        prefix: Some(ctx.server_prefix()),
                        command: Command::AUTHENTICATE("+".to_string()),
                    };
                    ctx.sender.send(reply).await?;
                    debug!(nick = %nick, "SASL PLAIN: sent challenge");
                } else if data.eq_ignore_ascii_case("EXTERNAL") {
                    // EXTERNAL uses TLS client certificate
                    if !ctx.state.is_tls {
                        send_sasl_fail(ctx, &nick, "EXTERNAL requires TLS connection").await?;
                        ctx.state.sasl_state = SaslState::None;
                        return Ok(());
                    }

                    let Some(certfp) = ctx.state.certfp.as_ref() else {
                        send_sasl_fail(ctx, &nick, "No client certificate presented").await?;
                        ctx.state.sasl_state = SaslState::None;
                        return Ok(());
                    };

                    // Send empty challenge to get optional authzid
                    ctx.state.sasl_state = SaslState::WaitingForExternal;
                    let reply = Message {
                        tags: None,
                        prefix: Some(ctx.server_prefix()),
                        command: Command::AUTHENTICATE("+".to_string()),
                    };
                    ctx.sender.send(reply).await?;
                    debug!(nick = %nick, certfp = %certfp, "SASL EXTERNAL: sent challenge");
                } else {
                    // Unsupported mechanism
                    send_sasl_fail(ctx, &nick, "Unsupported SASL mechanism").await?;
                    ctx.state.sasl_state = SaslState::None;
                }
            }
            SaslState::WaitingForExternal => {
                // Client sending empty response or authzid for EXTERNAL
                if data == "*" {
                    // Client aborting
                    send_sasl_fail(ctx, &nick, "SASL authentication aborted").await?;
                    ctx.state.sasl_state = SaslState::None;
                } else {
                    // data is either "+" (empty) or base64-encoded authzid
                    let authzid = if data == "+" {
                        None
                    } else {
                        slirc_proto::sasl::decode_base64(data)
                            .ok()
                            .and_then(|b| String::from_utf8(b).ok())
                    };

                    let certfp = ctx
                        .state
                        .certfp
                        .as_ref()
                        .expect("checked above")
                        .clone();

                    // Look up account by certificate fingerprint
                    match ctx.db.accounts().find_by_certfp(&certfp).await {
                        Ok(Some(account)) => {
                            // If authzid provided, verify it matches
                            if let Some(ref az) = authzid
                                && !az.eq_ignore_ascii_case(&account.name)
                            {
                                warn!(nick = %nick, authzid = %az, account = %account.name, "SASL EXTERNAL authzid mismatch");
                                send_sasl_fail(ctx, &nick, "Authorization identity mismatch")
                                    .await?;
                                ctx.state.sasl_state = SaslState::None;
                                return Ok(());
                            }

                            info!(
                                nick = %nick,
                                account = %account.name,
                                certfp = %certfp,
                                "SASL EXTERNAL authentication successful"
                            );

                            let user = ctx
                                .state
                                .user
                                .clone()
                                .unwrap_or_else(|| "*".to_string());
                            send_sasl_success(ctx, &nick, &user, &account.name).await?;
                            ctx.state.sasl_state = SaslState::Authenticated;
                            ctx.state.account = Some(account.name);
                        }
                        Ok(None) => {
                            warn!(nick = %nick, certfp = %certfp, "SASL EXTERNAL: no account with this certificate");
                            send_sasl_fail(ctx, &nick, "Certificate not registered to any account")
                                .await?;
                            ctx.state.sasl_state = SaslState::None;
                        }
                        Err(e) => {
                            warn!(nick = %nick, certfp = %certfp, error = ?e, "SASL EXTERNAL database error");
                            send_sasl_fail(ctx, &nick, "Authentication failed").await?;
                            ctx.state.sasl_state = SaslState::None;
                        }
                    }
                }
            }
            SaslState::WaitingForData => {
                // Client sending base64-encoded credentials
                // Per SASL 3.1 spec: messages are split into 400-byte chunks
                // - Line == 400 bytes: more data follows
                // - Line < 400 bytes: last chunk
                // - "+" alone: empty final chunk (when prior was exactly 400 bytes)

                if data == "*" {
                    // Client aborting
                    ctx.state.sasl_buffer.clear();
                    send_sasl_fail(ctx, &nick, "SASL authentication aborted").await?;
                    ctx.state.sasl_state = SaslState::None;
                } else {
                    // Accumulate the chunk
                    // "+" alone means empty chunk (final when previous was exactly 400 bytes)
                    if data != "+" {
                        ctx.state.sasl_buffer.push_str(data);
                    }

                    // Check if more data is expected
                    // If this chunk is exactly 400 bytes, wait for more
                    if data.len() == 400 {
                        // More data expected, wait for next AUTHENTICATE
                        debug!(nick = %nick, chunk_len = data.len(), total_len = ctx.state.sasl_buffer.len(), "SASL: accumulated chunk, waiting for more");
                        return Ok(());
                    }

                    // We have the complete payload, process it
                    let full_data = std::mem::take(&mut ctx.state.sasl_buffer);
                    debug!(nick = %nick, total_len = full_data.len(), "SASL: processing complete payload");

                    // Try to decode and validate
                    match validate_sasl_plain(&full_data) {
                        Ok((authzid, authcid, password)) => {
                            // Validate against database
                            let account_name = if authzid.is_empty() {
                                &authcid
                            } else {
                                &authzid
                            };
                            match ctx.db.accounts().identify(account_name, &password).await {
                                Ok(account) => {
                                    info!(
                                        nick = %nick,
                                        account = %account.name,
                                        "SASL PLAIN authentication successful"
                                    );

                                    // Send success
                                    let user = ctx
                                        .state
                                        .user
                                        .clone()
                                        .unwrap_or_else(|| "*".to_string());
                                    send_sasl_success(ctx, &nick, &user, &account.name).await?;
                                    ctx.state.sasl_state = SaslState::Authenticated;
                                    ctx.state.account = Some(account.name);
                                }
                                Err(e) => {
                                    warn!(nick = %nick, account = %account_name, error = ?e, "SASL authentication failed");
                                    send_sasl_fail(ctx, &nick, "Invalid credentials").await?;
                                    ctx.state.sasl_state = SaslState::None;
                                }
                            }
                        }
                        Err(e) => {
                            debug!(nick = %nick, error = %e, "SASL PLAIN decode failed");
                            send_sasl_fail(ctx, &nick, "Invalid SASL credentials").await?;
                            ctx.state.sasl_state = SaslState::None;
                        }
                    }
                }
            }
            SaslState::Authenticated => {
                // Already authenticated
                debug!(nick = %nick, "AUTHENTICATE after already authenticated");
            }
        }

        Ok(())
    }
}

/// SASL authentication state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SaslState {
    #[default]
    None,
    /// Waiting for PLAIN credentials (base64-encoded).
    WaitingForData,
    /// Waiting for EXTERNAL response (empty or authzid).
    WaitingForExternal,
    Authenticated,
}

/// Decode and validate SASL PLAIN credentials.
/// Format: base64(authzid \0 authcid \0 password)
fn validate_sasl_plain(data: &str) -> Result<(String, String, String), &'static str> {
    // Use slirc_proto's decode_base64 helper
    let decoded = slirc_proto::sasl::decode_base64(data).map_err(|_| "Invalid base64")?;

    let parts: Vec<&[u8]> = decoded.split(|&b| b == 0).collect();
    if parts.len() != 3 {
        return Err("Invalid SASL PLAIN format");
    }

    let authzid = String::from_utf8(parts[0].to_vec()).map_err(|_| "Invalid UTF-8")?;
    let authcid = String::from_utf8(parts[1].to_vec()).map_err(|_| "Invalid UTF-8")?;
    let password = String::from_utf8(parts[2].to_vec()).map_err(|_| "Invalid UTF-8")?;

    if authcid.is_empty() {
        return Err("Empty authcid");
    }

    Ok((authzid, authcid, password))
}

/// Send SASL success numerics.
async fn send_sasl_success(
    ctx: &mut Context<'_, UnregisteredState>,
    nick: &str,
    user: &str,
    account: &str,
) -> HandlerResult {
    // Use effective host (WEBIRC/TLS-aware) for prefix
    let host = ctx
        .state
        .webirc_host
        .clone()
        .or(ctx.state.webirc_ip.clone())
        .unwrap_or_else(|| ctx.remote_addr.ip().to_string());

    let mask = format!("{}!{}@{}", nick, user, host);

    // RPL_LOGGEDIN (900)
    let reply = Response::rpl_loggedin(nick, &mask, account)
        .with_prefix(ctx.server_prefix());
    ctx.sender.send(reply).await?;

    // RPL_SASLSUCCESS (903)
    let reply = Response::rpl_saslsuccess(nick)
        .with_prefix(ctx.server_prefix());
    ctx.sender.send(reply).await?;

    Ok(())
}

/// Send SASL failure numerics.
async fn send_sasl_fail(ctx: &mut Context<'_, UnregisteredState>, nick: &str, _reason: &str) -> HandlerResult {
    // ERR_SASLFAIL (904)
    let reply = Response::err_saslfail(nick)
        .with_prefix(ctx.server_prefix());
    ctx.sender.send(reply).await?;

    Ok(())
}


