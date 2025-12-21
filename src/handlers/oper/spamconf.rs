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

use crate::handlers::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use crate::require_oper_cap;
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
        // Check oper status via capability system (Innovation 4)
        let Some(_cap) = require_oper_cap!(ctx, "SPAMCONF", request_spamconf_cap) else {
            return Ok(());
        };

        let subcommand = msg.arg(0).unwrap_or("LIST").to_ascii_uppercase();

        let Some(spam_lock) = &ctx.matrix.security_manager.spam_detector else {
            ctx.send_notice("Spam detection is disabled").await?;
            return Ok(());
        };

        match subcommand.as_str() {
            "LIST" => {
                let spam = spam_lock.read().await;
                ctx.send_notice("*** Spam Detection Settings ***").await?;
                ctx.send_notice(format!("Entropy threshold: {:.2}", spam.entropy_threshold())).await?;
                ctx.send_notice("Use SPAMCONF ENTROPY/REPETITION/ADDKEYWORD").await?;
            }
            "ENTROPY" => {
                let Some(value_str) = msg.arg(1) else {
                    ctx.send_notice("Usage: SPAMCONF ENTROPY <0.0-8.0>").await?;
                    return Ok(());
                };

                let Ok(value) = value_str.parse::<f32>() else {
                    ctx.send_notice("Invalid number").await?;
                    return Ok(());
                };

                let mut spam = spam_lock.write().await;
                spam.set_entropy_threshold(value);
                ctx.send_notice(format!("Entropy threshold set to {:.2}", value)).await?;
            }
            "REPETITION" => {
                let Some(value_str) = msg.arg(1) else {
                    ctx.send_notice("Usage: SPAMCONF REPETITION <1-50>").await?;
                    return Ok(());
                };

                let Ok(value) = value_str.parse::<usize>() else {
                    ctx.send_notice("Invalid number").await?;
                    return Ok(());
                };

                if !(1..=50).contains(&value) {
                    ctx.send_notice("Value must be between 1 and 50").await?;
                    return Ok(());
                }

                let mut spam = spam_lock.write().await;
                spam.set_max_repetition(value);
                ctx.send_notice(format!("Max repetition set to {}", value)).await?;
            }
            "ADDKEYWORD" => {
                let Some(keyword) = msg.arg(1) else {
                    ctx.send_notice("Usage: SPAMCONF ADDKEYWORD <word>").await?;
                    return Ok(());
                };

                let mut spam = spam_lock.write().await;
                spam.add_keyword(keyword.to_string());
                ctx.send_notice(format!("Spam keyword '{}' added", keyword)).await?;
            }
            _ => {
                ctx.send_notice("Unknown subcommand. Use: LIST, ENTROPY, REPETITION, ADDKEYWORD").await?;
            }
        }

        Ok(())
    }
}
