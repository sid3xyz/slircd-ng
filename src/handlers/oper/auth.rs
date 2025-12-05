use super::super::{
    Context, Handler, HandlerResult, err_needmoreparams, get_nick_or_star,
    matches_hostmask, server_reply,
};
use async_trait::async_trait;
use slirc_proto::mode::{Mode, UserMode};
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};

/// Handler for OPER command.
///
/// `OPER name password`
///
/// Authenticates a user as an IRC operator.
pub struct OperHandler;

#[async_trait]
impl Handler for OperHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        // OPER <name> <password>
        let name = match msg.arg(0) {
            Some(n) if !n.is_empty() => n,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "OPER"))
                    .await?;
                return Ok(());
            }
        };
        let password = match msg.arg(1) {
            Some(p) if !p.is_empty() => p,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "OPER"))
                    .await?;
                return Ok(());
            }
        };

        let nick = get_nick_or_star(ctx).await;

        const MAX_OPER_ATTEMPTS: u8 = 3;
        const OPER_DELAY_MS: u64 = 3000;
        const LOCKOUT_DELAY_MS: u64 = 30000;

        let now = std::time::Instant::now();

        if ctx.handshake.failed_oper_attempts >= MAX_OPER_ATTEMPTS
            && let Some(last_attempt) = ctx.handshake.last_oper_attempt
        {
            let elapsed = now.duration_since(last_attempt).as_millis() as u64;
            if elapsed < LOCKOUT_DELAY_MS {
                let remaining_sec = (LOCKOUT_DELAY_MS - elapsed) / 1000;
                let reply = server_reply(
                    server_name,
                    Response::ERR_PASSWDMISMATCH,
                    vec![
                        nick.clone(),
                        format!(
                            "Too many failed attempts. Try again in {} seconds.",
                            remaining_sec
                        ),
                    ],
                );
                ctx.sender.send(reply).await?;
                tracing::warn!(nick = %nick, attempts = ctx.handshake.failed_oper_attempts, "OPER brute-force lockout active");
                return Ok(());
            } else {
                ctx.handshake.failed_oper_attempts = 0;
            }
        }

        if let Some(last_attempt) = ctx.handshake.last_oper_attempt {
            let elapsed = now.duration_since(last_attempt).as_millis() as u64;
            if elapsed < OPER_DELAY_MS {
                let remaining_ms = OPER_DELAY_MS - elapsed;
                tokio::time::sleep(tokio::time::Duration::from_millis(remaining_ms)).await;
            }
        }

        ctx.handshake.last_oper_attempt = Some(now);

        let oper_block = ctx
            .matrix
            .config
            .oper_blocks
            .iter()
            .find(|block| block.name == name);

        let Some(oper_block) = oper_block else {
            ctx.handshake.failed_oper_attempts += 1;
            tracing::warn!(
                nick = %nick,
                oper_name = %name,
                attempts = ctx.handshake.failed_oper_attempts,
                "OPER failed: unknown oper name"
            );
            let reply = server_reply(
                server_name,
                Response::ERR_PASSWDMISMATCH,
                vec![nick, "Password incorrect".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        if oper_block.password != password {
            ctx.handshake.failed_oper_attempts += 1;
            tracing::warn!(
                nick = %nick,
                oper_name = %name,
                attempts = ctx.handshake.failed_oper_attempts,
                "OPER failed: incorrect password"
            );
            let reply = server_reply(
                server_name,
                Response::ERR_PASSWDMISMATCH,
                vec![nick, "Password incorrect".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        if let Some(ref required_mask) = oper_block.hostmask {
            let (user_nick, user_user, user_host) = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                (user.nick.clone(), user.user.clone(), user.host.clone())
            } else {
                let hs_nick = ctx.handshake.nick.clone().unwrap_or_else(|| nick.clone());
                let hs_user = ctx.handshake.user.clone().unwrap_or_else(|| "unknown".to_string());
                (hs_nick, hs_user, ctx.remote_addr.ip().to_string())
            };
            let user_mask = format!("{}!{}@{}", user_nick, user_user, user_host);

            if !matches_hostmask(required_mask, &user_mask) {
                ctx.handshake.failed_oper_attempts += 1;
                tracing::warn!(
                    nick = %nick,
                    oper_name = %name,
                    user_mask = %user_mask,
                    required_mask = %required_mask,
                    attempts = ctx.handshake.failed_oper_attempts,
                    "OPER failed: hostmask mismatch"
                );
                let reply = server_reply(
                    server_name,
                    Response::ERR_NOOPERHOST,
                    vec![nick, "No O-lines for your host".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        }

        ctx.handshake.failed_oper_attempts = 0;

        let (user_nick, user_user, user_host) = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let user = user_ref.read().await;
            (user.nick.clone(), user.user.clone(), user.host.clone())
        } else {
            (nick.clone(), "unknown".to_string(), ctx.remote_addr.ip().to_string())
        };

        if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let mut user = user_ref.write().await;
            user.modes.oper = true;
        }

        tracing::info!(nick = %nick, oper_name = %name, "OPER successful");

        let reply = server_reply(
            server_name,
            Response::RPL_YOUREOPER,
            vec![nick.clone(), "You are now an IRC operator".to_string()],
        );
        ctx.sender.send(reply).await?;

        let mode_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(user_nick, user_user, user_host)),
            command: Command::UserMODE(nick, vec![Mode::Plus(UserMode::Oper, None)]),
        };
        ctx.sender.send(mode_msg).await?;

        Ok(())
    }
}
