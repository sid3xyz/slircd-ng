//! CAP command handler for IRCv3 capability negotiation.
//!
//! Implements CAP LS, LIST, REQ, ACK, NAK, END subcommands.
//! Reference: <https://ircv3.net/specs/extensions/capability-negotiation>

use super::{Context, Handler, HandlerResult, server_reply};
use async_trait::async_trait;
use slirc_proto::{CapSubCommand, Command, Message, MessageRef, Prefix, Response};
use tracing::{debug, info, warn};

/// Capabilities we support (subset of slirc_proto::CAPABILITIES).
const SUPPORTED_CAPS: &[&str] = &[
    "multi-prefix",
    "userhost-in-names",
    "server-time",
    "echo-message",
    "sasl",
    "batch",
    "message-tags",
    "labeled-response",
    "setname",
    "away-notify",
    "account-notify",
    "extended-join",
    "invite-notify",
    "chghost",
    "monitor",
    "cap-notify",
];

/// Handler for CAP command.
pub struct CapHandler;

#[async_trait]
impl Handler for CapHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // CAP can be used before and after registration
        // CAP <subcommand> [arg]
        let subcommand_str = msg.arg(0).unwrap_or("");
        let arg = msg.arg(1);

        // Clone nick upfront to avoid borrowing issues
        let nick = ctx
            .handshake
            .nick
            .clone()
            .unwrap_or_else(|| "*".to_string());

        // Parse subcommand using slirc-proto's FromStr implementation
        let subcommand: CapSubCommand = match subcommand_str.parse() {
            Ok(cmd) => cmd,
            Err(_) => {
                debug!(subcommand = subcommand_str, "Unknown CAP subcommand");
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

/// Handle CAP LS [version] - list available capabilities.
async fn handle_ls(ctx: &mut Context<'_>, nick: &str, version_arg: Option<&str>) -> HandlerResult {
    // Parse version (301 default, 302 if specified)
    let version: u32 = version_arg.and_then(|v| v.parse().ok()).unwrap_or(301);

    // Set CAP negotiation flag
    ctx.handshake.cap_negotiating = true;
    ctx.handshake.cap_version = version;

    // CAP LS 302+ implicitly enables cap-notify per IRCv3 spec
    // https://ircv3.net/specs/extensions/capability-negotiation#cap-notify
    if version >= 302 {
        ctx.handshake.capabilities.insert("cap-notify".to_string());
    }

    // Build capability list
    let cap_list = build_cap_list(version);

    // Send CAP LS response
    // Format: CAP <nick> LS [* ] :<caps>
    // The * indicates more lines follow (multiline for >510 bytes)
    let reply = Message {
        tags: None,
        prefix: Some(Prefix::ServerName(ctx.matrix.server_info.name.clone())),
        command: Command::CAP(
            Some(nick.to_string()),
            CapSubCommand::LS,
            None,
            Some(cap_list),
        ),
    };
    ctx.sender.send(reply).await?;

    debug!(nick = %nick, version = %version, "CAP LS sent");
    Ok(())
}

/// Handle CAP LIST - list currently enabled capabilities.
async fn handle_list(ctx: &mut Context<'_>, nick: &str) -> HandlerResult {
    let enabled: String = ctx
        .handshake
        .capabilities
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    let reply = Message {
        tags: None,
        prefix: Some(Prefix::ServerName(ctx.matrix.server_info.name.clone())),
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

/// Handle CAP REQ :<capabilities> - request capabilities.
async fn handle_req(ctx: &mut Context<'_>, nick: &str, caps_arg: Option<&str>) -> HandlerResult {
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

        if SUPPORTED_CAPS.contains(&cap_base) {
            if is_removal {
                ctx.handshake.capabilities.remove(cap_base);
                accepted.push(format!("-{}", cap_base));
            } else {
                ctx.handshake.capabilities.insert(cap_base.to_string());
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
            prefix: Some(Prefix::ServerName(ctx.matrix.server_info.name.clone())),
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
            prefix: Some(Prefix::ServerName(ctx.matrix.server_info.name.clone())),
            command: Command::CAP(
                Some(nick.to_string()),
                CapSubCommand::ACK,
                None,
                Some(accepted.join(" ")),
            ),
        };
        ctx.sender.send(reply).await?;
        debug!(nick = %nick, accepted = ?accepted, "CAP REQ ACK");
    }

    Ok(())
}

/// Handle CAP END - end capability negotiation.
async fn handle_end(ctx: &mut Context<'_>, nick: &str) -> HandlerResult {
    ctx.handshake.cap_negotiating = false;

    info!(
        nick = %nick,
        capabilities = ?ctx.handshake.capabilities,
        "CAP negotiation complete"
    );

    // If registration is pending, complete it now
    // The connection handler will send welcome burst
    // We just mark that CAP is done

    Ok(())
}

/// Build capability list string for CAP LS response.
fn build_cap_list(version: u32) -> String {
    let caps: Vec<String> = SUPPORTED_CAPS
        .iter()
        .map(|&cap| {
            // For CAP 302+, add values for caps that have them
            if version >= 302 {
                match cap {
                    "sasl" => "sasl=PLAIN".to_string(),
                    _ => cap.to_string(),
                }
            } else {
                cap.to_string()
            }
        })
        .collect();

    caps.join(" ")
}

/// Handler for AUTHENTICATE command (SASL authentication).
pub struct AuthenticateHandler;

#[async_trait]
impl Handler for AuthenticateHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // AUTHENTICATE <data>
        let data = msg.arg(0).unwrap_or("");

        // Clone nick upfront to avoid borrowing issues
        let nick = ctx
            .handshake
            .nick
            .clone()
            .unwrap_or_else(|| "*".to_string());

        // Check if SASL is enabled
        if !ctx.handshake.capabilities.contains("sasl") {
            // SASL not enabled, ignore
            debug!(nick = %nick, "AUTHENTICATE received but SASL not enabled");
            return Ok(());
        }

        // Handle SASL flow
        match ctx.handshake.sasl_state.clone() {
            SaslState::None => {
                // Client is initiating SASL with mechanism name
                if data.eq_ignore_ascii_case("PLAIN") {
                    ctx.handshake.sasl_state = SaslState::WaitingForData;
                    // Send empty challenge (AUTHENTICATE +)
                    let reply = Message {
                        tags: None,
                        prefix: Some(Prefix::ServerName(ctx.matrix.server_info.name.clone())),
                        command: Command::AUTHENTICATE("+".to_string()),
                    };
                    ctx.sender.send(reply).await?;
                    debug!(nick = %nick, "SASL PLAIN: sent challenge");
                } else {
                    // Unsupported mechanism
                    send_sasl_fail(ctx, &nick, "Unsupported SASL mechanism").await?;
                    ctx.handshake.sasl_state = SaslState::None;
                }
            }
            SaslState::WaitingForData => {
                // Client sending base64-encoded credentials
                if data == "*" {
                    // Client aborting
                    send_sasl_fail(ctx, &nick, "SASL authentication aborted").await?;
                    ctx.handshake.sasl_state = SaslState::None;
                } else {
                    // Try to decode and validate
                    match validate_sasl_plain(data) {
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
                                        .handshake
                                        .user
                                        .clone()
                                        .unwrap_or_else(|| "*".to_string());
                                    send_sasl_success(ctx, &nick, &user, &account.name).await?;
                                    ctx.handshake.sasl_state = SaslState::Authenticated;
                                    ctx.handshake.account = Some(account.name);
                                }
                                Err(e) => {
                                    warn!(nick = %nick, account = %account_name, error = ?e, "SASL authentication failed");
                                    send_sasl_fail(ctx, &nick, "Invalid credentials").await?;
                                    ctx.handshake.sasl_state = SaslState::None;
                                }
                            }
                        }
                        Err(e) => {
                            debug!(nick = %nick, error = %e, "SASL PLAIN decode failed");
                            send_sasl_fail(ctx, &nick, "Invalid SASL credentials").await?;
                            ctx.handshake.sasl_state = SaslState::None;
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
    WaitingForData,
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
    ctx: &mut Context<'_>,
    nick: &str,
    user: &str,
    account: &str,
) -> HandlerResult {
    // RPL_LOGGEDIN (900)
    let reply = server_reply(
        &ctx.matrix.server_info.name,
        Response::RPL_LOGGEDIN,
        vec![
            nick.to_string(),
            format!("{}!{}@{}", nick, user, "localhost"),
            account.to_string(),
            format!("You are now logged in as {}", account),
        ],
    );
    ctx.sender.send(reply).await?;

    // RPL_SASLSUCCESS (903)
    let reply = server_reply(
        &ctx.matrix.server_info.name,
        Response::RPL_SASLSUCCESS,
        vec![
            nick.to_string(),
            "SASL authentication successful".to_string(),
        ],
    );
    ctx.sender.send(reply).await?;

    Ok(())
}

/// Send SASL failure numerics.
async fn send_sasl_fail(ctx: &mut Context<'_>, nick: &str, reason: &str) -> HandlerResult {
    // ERR_SASLFAIL (904)
    let reply = server_reply(
        &ctx.matrix.server_info.name,
        Response::ERR_SASLFAIL,
        vec![nick.to_string(), reason.to_string()],
    );
    ctx.sender.send(reply).await?;

    Ok(())
}
