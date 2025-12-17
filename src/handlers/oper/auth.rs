//! OPER command handler for operator authentication.
//!
//! Authenticates users as IRC operators using password verification
//! and hostmask matching from the server configuration.

use super::super::{Context,
    HandlerResult, PostRegHandler, matches_hostmask,
    server_reply,
};
use crate::require_arg_or_reply;
use crate::state::RegisteredState;
use crate::state::actor::validation::format_user_mask;
use async_trait::async_trait;
use rand::Rng;
use slirc_proto::mode::{Mode, UserMode};
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};
use std::time::Duration;

/// Baseline delay for all OPER responses to prevent timing side-channels.
/// All paths (success, failure, lockout) take approximately this long.
const OPER_BASELINE_DELAY_MS: u64 = 500;
/// Random jitter added to baseline delay (0..OPER_JITTER_MS).
const OPER_JITTER_MS: u64 = 200;

/// Handler for OPER command.
///
/// `OPER name password`
///
/// Authenticates a user as an IRC operator.
pub struct OperHandler;

#[async_trait]
impl PostRegHandler for OperHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name().to_string();

        let Some(name) = require_arg_or_reply!(ctx, msg, 0, "OPER") else { return Ok(()); };
        let Some(password) = require_arg_or_reply!(ctx, msg, 1, "OPER") else { return Ok(()); };

        let nick = ctx.nick().to_string();

        const MAX_OPER_ATTEMPTS: u8 = 3;
        const OPER_DELAY_MS: u64 = 3000;
        const LOCKOUT_DELAY_MS: u64 = 30000;

        // Helper to apply timing normalization to prevent side-channel attacks.
        // All OPER responses (success, failure, lockout) will take approximately
        // the same amount of time, making it impossible to distinguish valid
        // usernames from invalid ones via timing analysis.
        async fn apply_timing_delay() {
            let jitter = rand::thread_rng().gen_range(0..OPER_JITTER_MS);
            tokio::time::sleep(Duration::from_millis(OPER_BASELINE_DELAY_MS + jitter)).await;
        }

        let now = std::time::Instant::now();

        if ctx.state.failed_oper_attempts >= MAX_OPER_ATTEMPTS
            && let Some(last_attempt) = ctx.state.last_oper_attempt
        {
            let elapsed = now.duration_since(last_attempt).as_millis() as u64;
            if elapsed < LOCKOUT_DELAY_MS {
                // Apply timing normalization even for lockout responses
                apply_timing_delay().await;

                let remaining_sec = (LOCKOUT_DELAY_MS - elapsed) / 1000;
                let reply = server_reply(
                    &server_name,
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
                tracing::warn!(nick = %nick, attempts = ctx.state.failed_oper_attempts, "OPER brute-force lockout active");
                return Ok(());
            } else {
                ctx.state.failed_oper_attempts = 0;
            }
        }

        // Rate limiting between attempts (this is separate from timing normalization)
        if let Some(last_attempt) = ctx.state.last_oper_attempt {
            let elapsed = now.duration_since(last_attempt).as_millis() as u64;
            if elapsed < OPER_DELAY_MS {
                let remaining_ms = OPER_DELAY_MS - elapsed;
                tokio::time::sleep(tokio::time::Duration::from_millis(remaining_ms)).await;
            }
        }

        ctx.state.last_oper_attempt = Some(now);

        let oper_block = ctx
            .matrix
            .config
            .oper_blocks
            .iter()
            .find(|block| block.name == name);

        let Some(oper_block) = oper_block else {
            // Apply timing normalization before responding
            apply_timing_delay().await;

            ctx.state.failed_oper_attempts += 1;
            tracing::warn!(
                nick = %nick,
                oper_name = %name,
                attempts = ctx.state.failed_oper_attempts,
                "OPER failed: unknown oper name"
            );
            let reply = server_reply(
                &server_name,
                Response::ERR_PASSWDMISMATCH,
                vec![nick, "Password incorrect".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        if !oper_block.verify_password(password) {
            // Apply timing normalization before responding
            apply_timing_delay().await;

            ctx.state.failed_oper_attempts += 1;
            tracing::warn!(
                nick = %nick,
                oper_name = %name,
                attempts = ctx.state.failed_oper_attempts,
                "OPER failed: incorrect password"
            );
            let reply = server_reply(
                &server_name,
                Response::ERR_PASSWDMISMATCH,
                vec![nick, "Password incorrect".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        if let Some(ref required_mask) = oper_block.hostmask {
            let (user_nick, user_user, user_host) =
                if let Some(user_arc) = ctx.matrix.users.get(ctx.uid).map(|u| u.value().clone()) {
                    let user = user_arc.read().await;
                    (user.nick.clone(), user.user.clone(), user.host.clone())
                } else {
                    let hs_nick = ctx.state.nick.clone();
                    let hs_user = ctx.state.user.clone();
                    (hs_nick, hs_user, ctx.remote_addr.ip().to_string())
                };
            let user_mask = format_user_mask(&user_nick, &user_user, &user_host);

            if !matches_hostmask(required_mask, &user_mask) {
                // Apply timing normalization before responding
                apply_timing_delay().await;

                ctx.state.failed_oper_attempts += 1;
                tracing::warn!(
                    nick = %nick,
                    oper_name = %name,
                    user_mask = %user_mask,
                    required_mask = %required_mask,
                    attempts = ctx.state.failed_oper_attempts,
                    "OPER failed: hostmask mismatch"
                );
                let reply = server_reply(
                    &server_name,
                    Response::ERR_NOOPERHOST,
                    vec![nick, "No O-lines for your host".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        }

        // Apply timing normalization for success path too
        // This ensures success and failure take the same amount of time
        apply_timing_delay().await;

        ctx.state.failed_oper_attempts = 0;

        let (user_nick, user_user, user_host) =
            if let Some(user_arc) = ctx.matrix.users.get(ctx.uid).map(|u| u.value().clone()) {
                let user = user_arc.read().await;
                (user.nick.clone(), user.user.clone(), user.host.clone())
            } else {
                (
                    nick.clone(),
                    "unknown".to_string(),
                    ctx.remote_addr.ip().to_string(),
                )
            };

        if let Some(user_arc) = ctx.matrix.users.get(ctx.uid).map(|u| u.value().clone()) {
            let mut user = user_arc.write().await;
            user.modes.oper = true;
        }

        tracing::info!(nick = %nick, oper_name = %name, "OPER successful");

        // Send snomask 'o'
        ctx.matrix.send_snomask('o', &format!("OPER: {} ({}) is now an IRC operator", nick, name)).await;

        let reply = server_reply(
            &server_name,
            Response::RPL_YOUREOPER,
            vec![nick.clone(), "You are now an IRC operator".to_string()],
        );
        ctx.sender.send(reply).await?;

        let mode_msg = Message {
            tags: None,
            prefix: Some(Prefix::new(user_nick, user_user, user_host)),
            command: Command::UserMODE(nick, vec![Mode::Plus(UserMode::Oper, None)]),
        };
        ctx.sender.send(mode_msg).await?;

        Ok(())
    }
}
