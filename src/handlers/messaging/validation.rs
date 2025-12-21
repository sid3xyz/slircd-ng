//! Message validation shared between PRIVMSG and NOTICE.
//!
//! Both message commands share identical validation logic (shun checks, spam detection),
//! differing only in error handling strategy:
//! - PRIVMSG sends error replies to user
//! - NOTICE silently drops (per RFC 2812)
//!
//! Note: Rate limiting is handled at the connection level in lifecycle.rs,
//! NOT here. This prevents double-checking which would consume tokens twice
//! and create bypass opportunities.

use crate::handlers::{Context, HandlerError, server_reply};
use crate::state::RegisteredState;
use slirc_proto::Response;
use tracing::debug;
use super::common::SenderSnapshot;

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

/// Validate a message send operation (shun, spam detection).
///
/// NOTE: Rate limiting is NOT checked here - it's handled at the connection
/// level in lifecycle.rs with proper strike tracking and disconnect logic.
/// Checking here would double-consume rate limit tokens.
///
/// Returns:
/// - `Ok(ValidationResult::Ok)` if message passes all checks
/// - `Ok(ValidationResult::Blocked)` if blocked but handler should return Ok
/// - `Err(HandlerError)` for actual errors (nick/user missing)
pub async fn validate_message_send(
    ctx: &mut Context<'_, RegisteredState>,
    target: &str,
    text: &str,
    strategy: ErrorStrategy,
    snapshot: &SenderSnapshot,
) -> Result<ValidationResult, HandlerError> {
    // Check shun first - always silent
    if super::common::is_shunned_with_snapshot(ctx, snapshot).await {
        return Ok(ValidationResult::Blocked);
    }

    let uid_string = ctx.uid.to_string();
    let nick = &ctx.state.nick; // Guaranteed present in RegisteredState

    // NOTE: Rate limiting is handled at connection level (lifecycle.rs)
    // with proper strike tracking. Do NOT check here to avoid double-penalty.

    // Check for repetition spam
    if let Some(detector_lock) = &ctx.matrix.security_manager.spam_detector {
        let detector = detector_lock.read().await;
        if let crate::security::spam::SpamVerdict::Spam { pattern, .. } =
            detector.check_message_repetition(&uid_string, text)
        {
            // Record violation
            if let Ok(ip) = snapshot.ip.parse() {
                detector.record_violation(ip, "repetition").await;
            }

            debug!(
                uid = %uid_string,
                pattern = %pattern,
                "Message blocked by spam detector (repetition)"
            );
            match strategy {
                ErrorStrategy::SendError => {
                    let reply = server_reply(
                        ctx.server_name(),
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
    }

    // Check for content spam (skip for trusted users)
    let is_trusted = snapshot.is_oper || snapshot.account.is_some();
    let is_private = !target.starts_with('#') && !target.starts_with('&');

    if !is_trusted
        && let Some(detector_lock) = &ctx.matrix.security_manager.spam_detector
    {
        let detector = detector_lock.read().await;
        if let crate::security::spam::SpamVerdict::Spam { pattern, .. } =
            detector.check_message(&uid_string, &snapshot.ip, text, is_private).await
        {
            // Record violation
            if let Ok(ip) = snapshot.ip.parse() {
                detector.record_violation(ip, &pattern).await;
            }

            debug!(
                uid = %uid_string,
                pattern = %pattern,
                "Message blocked by spam detector (content)"
            );
            match strategy {
                ErrorStrategy::SendError => {
                    let reply = server_reply(
                        ctx.server_name(),
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
    }

    // Rate-limit CTCP floods
    if slirc_proto::ctcp::Ctcp::is_ctcp(text)
        && !ctx.matrix.security_manager.rate_limiter.check_ctcp_rate(&uid_string)
    {
        match strategy {
            ErrorStrategy::SendError => {
                let reply = server_reply(
                    ctx.server_name(),
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
