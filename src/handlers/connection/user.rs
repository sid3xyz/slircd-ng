//! USER command handler for connection registration.

use super::super::{Context, HandlerError, HandlerResult, PreRegHandler};
use crate::state::UnregisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};
use tracing::debug;

/// Handler for USER command.
///
/// Sets the username and realname for the connection. Registration is NOT
/// triggered here - it happens in the connection loop after the handler returns,
/// using `WelcomeBurstWriter` to write directly to transport.
/// # RFC 2812 §3.1.3
///
/// User message - Specifies username, hostname, and real name of a new user.
///
/// **Specification:** [RFC 2812 §3.1.3](https://datatracker.ietf.org/doc/html/rfc2812#section-3.1.3)
///
/// **Compliance:** 11/11 irctest pass
/// Parses and validates USER command parameters.
///
/// Expects: USER <username> <mode> <unused> <realname>
#[allow(clippy::result_large_err)]
fn parse_user_params<'a>(msg: &MessageRef<'a>) -> Result<(&'a str, &'a str), HandlerError> {
    let username = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
    // arg(1) is mode, arg(2) is unused
    let realname = msg.arg(3).unwrap_or("");

    if username.is_empty() || realname.is_empty() {
        return Err(HandlerError::NeedMoreParams);
    }

    Ok((username, realname))
}

pub struct UserHandler;

#[async_trait]
impl PreRegHandler for UserHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // USER cannot be resent after already set
        if ctx.state.user.is_some() {
            let nick = ctx.state.nick.as_deref().unwrap_or("*");
            let reply = Response::err_alreadyregistred(nick).with_prefix(ctx.server_prefix());
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let (username, realname) = parse_user_params(msg)?;

        ctx.state.user = Some(username.to_string());
        ctx.state.realname = Some(realname.to_string());

        debug!(user = %username, realname = %realname, uid = %ctx.uid, "User set");

        // Registration check is deferred to the connection loop, which uses
        // WelcomeBurstWriter to write directly to transport (avoiding channel deadlock).

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::MessageRef;

    #[test]
    fn test_parse_user_valid() {
        let msg = MessageRef::parse("USER nick 0 * :Real Name").unwrap();
        let (user, real) = parse_user_params(&msg).expect("Should parse valid USER");
        assert_eq!(user, "nick");
        assert_eq!(real, "Real Name");
    }

    #[test]
    fn test_parse_user_missing_username() {
        let msg = MessageRef::parse("USER").unwrap();
        let err = parse_user_params(&msg).unwrap_err();
        assert!(matches!(err, HandlerError::NeedMoreParams));
    }

    #[test]
    fn test_parse_user_missing_realname() {
        let msg = MessageRef::parse("USER nick 0 *").unwrap();
        let err = parse_user_params(&msg).unwrap_err();
        assert!(matches!(err, HandlerError::NeedMoreParams));
    }

    #[test]
    fn test_parse_user_empty_username() {
        let msg = MessageRef::parse("USER : 0 * :Real Name").unwrap();
        let err = parse_user_params(&msg).unwrap_err();
        assert!(matches!(err, HandlerError::NeedMoreParams));
    }

    #[test]
    fn test_parse_user_empty_realname() {
        let msg = MessageRef::parse("USER nick 0 * :").unwrap();
        let err = parse_user_params(&msg).unwrap_err();
        assert!(matches!(err, HandlerError::NeedMoreParams));
    }

    #[test]
    fn test_parse_user_realname_with_spaces() {
        let msg = MessageRef::parse("USER nick 0 * :Real Name With Spaces").unwrap();
        let (user, real) = parse_user_params(&msg).expect("Should parse valid USER");
        assert_eq!(user, "nick");
        assert_eq!(real, "Real Name With Spaces");
    }
}
