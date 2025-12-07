//! Message validation shared between PRIVMSG and NOTICE.
//!
//! Both message commands share identical validation logic (shun checks, rate limiting,
//! spam detection), differing only in error handling strategy:
//! - PRIVMSG sends error replies to user
//! - NOTICE silently drops (per RFC 2812)

use crate::handlers::{Context, HandlerError, server_reply};
use slirc_proto::Response;
use tracing::debug;

/// Error handling strategy for message validation failures.
#[derive(Debug, Clone, Copy)]
pub enum ErrorStrategy {
    /// Send error reply to user (PRIVMSG behavior).
    SendError,
    /// Silently drop message (NOTICE behavior per RFC 2812).
    SilentDrop,
}

/// Result of message validation.
#[derive(Debug)]
pub enum ValidationResult {
    /// Message passed all validations, proceed with routing.
    Ok,
    /// Message blocked, but handler should return Ok (silent drop or error sent).
    Blocked,
}

/// Validate a message send operation (shun, rate limits, spam detection).
///
/// Returns:
/// - `Ok(ValidationResult::Ok)` if message passes all checks
/// - `Ok(ValidationResult::Blocked)` if blocked but handler should return Ok
/// - `Err(HandlerError)` for actual errors (nick/user missing)
pub async fn validate_message_send(
    ctx: &mut Context<'_>,
    target: &str,
    text: &str,
    strategy: ErrorStrategy,
) -> Result<ValidationResult, HandlerError> {
    // Check shun first - always silent
    if super::common::is_shunned(ctx).await {
        return Ok(ValidationResult::Blocked);
    }

    let uid_string = ctx.uid.to_string();
    let nick = ctx
        .state
        .nick
        .as_ref()
        .ok_or(HandlerError::NickOrUserMissing)?;

    // Check message rate limit
    if !ctx.matrix.rate_limiter.check_message_rate(&uid_string) {
        match strategy {
            ErrorStrategy::SendError => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_TOOMANYTARGETS,
                    vec![
                        nick.to_string(),
                        "*".to_string(),
                        "You are sending messages too quickly. Please wait.".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
            }
            ErrorStrategy::SilentDrop => {
                // NOTICE errors silently dropped
            }
        }
        return Ok(ValidationResult::Blocked);
    }

    // Check for repetition spam
    if let Some(detector) = &ctx.matrix.spam_detector
        && let crate::security::spam::SpamVerdict::Spam { pattern, .. } =
            detector.check_message_repetition(&uid_string, text)
    {
        debug!(
            uid = %uid_string,
            pattern = %pattern,
            "Message blocked by spam detector (repetition)"
        );
        match strategy {
            ErrorStrategy::SendError => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_TOOMANYTARGETS,
                    vec![
                        nick.to_string(),
                        target.to_string(),
                        "Message blocked: repetition detected.".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
            }
            ErrorStrategy::SilentDrop => {}
        }
        return Ok(ValidationResult::Blocked);
    }

    // Check for content spam (skip for trusted users)
    let is_trusted = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        let user = user_ref.read().await;
        user.modes.oper || user.account.is_some()
    } else {
        false
    };

    if !is_trusted
        && let Some(detector) = &ctx.matrix.spam_detector
        && let crate::security::spam::SpamVerdict::Spam { pattern, .. } =
            detector.check_message(text)
    {
        debug!(
            uid = %uid_string,
            pattern = %pattern,
            "Message blocked by spam detector (content)"
        );
        match strategy {
            ErrorStrategy::SendError => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_TOOMANYTARGETS,
                    vec![
                        nick.to_string(),
                        target.to_string(),
                        "Message blocked: spam pattern detected.".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
            }
            ErrorStrategy::SilentDrop => {}
        }
        return Ok(ValidationResult::Blocked);
    }

    // Rate-limit CTCP floods
    if slirc_proto::ctcp::Ctcp::is_ctcp(text)
        && !ctx.matrix.rate_limiter.check_ctcp_rate(&uid_string)
    {
        match strategy {
            ErrorStrategy::SendError => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_TOOMANYTARGETS,
                    vec![
                        nick.to_string(),
                        target.to_string(),
                        "CTCP flood detected. Please slow down.".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
            }
            ErrorStrategy::SilentDrop => {}
        }
        return Ok(ValidationResult::Blocked);
    }

    // All checks passed
    Ok(ValidationResult::Ok)
}
