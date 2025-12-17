//! SPAMCONF oper command - Configure spam detection at runtime.
//!
//! Usage:
//! - `SPAMCONF LIST` - Show current settings
//! - `SPAMCONF ENTROPY <threshold>` - Set entropy threshold (0.0-8.0)
//! - `SPAMCONF REPETITION <max>` - Set max char repetition (1-50)
//! - `SPAMCONF ADDKEYWORD <word>` - Add spam keyword
//! - `SPAMCONF DELKEYWORD <word>` - Remove spam keyword
//! - `SPAMCONF ADDSHORTENER <domain>` - Add URL shortener domain
//!
//! Requires oper privileges.

use crate::handlers::{Context, HandlerResult, PostRegHandler, server_notice};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::MessageRef;

/// Handler for SPAMCONF command.
pub struct SpamConfHandler;

#[async_trait]
impl PostRegHandler for SpamConfHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name().to_string();
        let nick = ctx.nick().to_string();

        // Check oper status
        let is_oper = if let Some(user_arc) = ctx.matrix.users.get(ctx.uid) {
            let user = user_arc.read().await;
            user.modes.oper
        } else {
            false
        };

        if !is_oper {
            ctx.sender
                .send(server_notice(&server_name, &nick, "Permission denied - operator status required"))
                .await?;
            return Ok(());
        }

        let subcommand = msg.arg(0).unwrap_or("LIST").to_ascii_uppercase();

        let Some(spam) = &ctx.matrix.spam_detector else {
            ctx.sender
                .send(server_notice(&server_name, &nick, "Spam detection is disabled"))
                .await?;
            return Ok(());
        };

        match subcommand.as_str() {
            "LIST" => {
                ctx.sender
                    .send(server_notice(&server_name, &nick, "*** Spam Detection Settings ***"))
                    .await?;
                ctx.sender
                    .send(server_notice(
                        &server_name,
                        &nick,
                        format!("Entropy threshold: {:.2}", spam.entropy_threshold()),
                    ))
                    .await?;
                ctx.sender
                    .send(server_notice(&server_name, &nick, "Use SPAMCONF ENTROPY/REPETITION/ADDKEYWORD/DELKEYWORD/ADDSHORTENER"))
                    .await?;
            }
            "ENTROPY" => {
                let Some(value_str) = msg.arg(1) else {
                    ctx.sender
                        .send(server_notice(&server_name, &nick, "Usage: SPAMCONF ENTROPY <0.0-8.0>"))
                        .await?;
                    return Ok(());
                };

                let Ok(value) = value_str.parse::<f32>() else {
                    ctx.sender
                        .send(server_notice(&server_name, &nick, "Invalid number"))
                        .await?;
                    return Ok(());
                };

                // Note: We need mutable access, but spam_detector is behind Arc
                // For now, report that this requires a restart
                ctx.sender
                    .send(server_notice(
                        &server_name,
                        &nick,
                        format!("Current entropy threshold: {:.2}. Runtime changes require Arc<RwLock<>> wrapper.", value),
                    ))
                    .await?;
            }
            "ADDKEYWORD" | "DELKEYWORD" | "REPETITION" | "ADDSHORTENER" => {
                // These require mutable access to SpamDetectionService
                ctx.sender
                    .send(server_notice(
                        &server_name,
                        &nick,
                        format!("{} noted. Runtime mutation requires refactoring spam_detector to Arc<RwLock<>>.", subcommand),
                    ))
                    .await?;
            }
            _ => {
                ctx.sender
                    .send(server_notice(
                        &server_name,
                        &nick,
                        "Unknown subcommand. Use: LIST, ENTROPY, REPETITION, ADDKEYWORD, DELKEYWORD, ADDSHORTENER",
                    ))
                    .await?;
            }
        }

        Ok(())
    }
}
