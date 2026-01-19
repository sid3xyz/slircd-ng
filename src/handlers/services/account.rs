//! REGISTER command handler (draft/account-registration).

use crate::handlers::{
    Context, HandlerResult, UniversalHandler,
};
use crate::state::SessionState;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix};

/// Handler for REGISTER command (draft/account-registration).
///
/// Implements: <https://ircv3.net/specs/extensions/account-registration>
pub struct RegisterHandler;

/// Send a FAIL response for the REGISTER command.
fn fail_response(server_name: &str, code: &str, context: &str, description: &str) -> Message {
    Message {
        tags: None,
        prefix: Some(Prefix::new_from_str(server_name)),
        command: Command::FAIL(
            "REGISTER".to_string(),
            code.to_string(),
            vec![context.to_string(), description.to_string()],
        ),
    }
}

#[async_trait]
impl<S: SessionState> UniversalHandler<S> for RegisterHandler {
    async fn handle(&self, ctx: &mut Context<'_, S>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = ctx.server_name();
        let acct_cfg = &ctx.matrix.config.account_registration;

        // Get current nick or "*"
        let nick = ctx.state.nick_or_star().to_string();

        // Check if user is fully registered (has received 001)
        let is_registered = ctx.state.is_registered();

        // If before_connect is disabled, user must be fully registered
        if !acct_cfg.before_connect && !is_registered {
            let reply = fail_response(
                server_name,
                "COMPLETE_CONNECTION_REQUIRED",
                &nick,
                "You must complete connection registration before registering an account",
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // REGISTER <account> <email> <password>
        let account_arg = msg.arg(0);
        let email_arg = msg.arg(1);
        let password_arg = msg.arg(2);

        let (Some(account), Some(email), Some(password)) = (account_arg, email_arg, password_arg)
        else {
            let reply = fail_response(
                server_name,
                "NEED_MORE_PARAMS",
                &nick,
                "Not enough parameters",
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        // Validate email if required
        if acct_cfg.email_required && (email == "*" || email.is_empty() || !email.contains('@')) {
            let reply = fail_response(
                server_name,
                "INVALID_EMAIL",
                &nick,
                "A valid email address is required",
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Validate password (basic check)
        if password.is_empty() || password == "*" {
            let reply = fail_response(server_name, "INVALID_PARAMS", &nick, "Invalid password");
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Determine target account name
        let target_account = if account == "*" {
            // Use current nick - but only if we have a valid one
            if nick == "*" || nick.is_empty() {
                // No valid nick - this means they tried to use a nick but it was rejected
                // Return ACCOUNT_EXISTS to indicate the nick they wanted is taken
                let reply = fail_response(
                    server_name,
                    "ACCOUNT_EXISTS",
                    "*",
                    "The nickname you attempted to use is already in use",
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
            nick.clone()
        } else if !acct_cfg.custom_account_name {
            // Custom names not allowed, must use "*"
            let reply = fail_response(
                server_name,
                "ACCOUNT_NAME_MUST_BE_NICK",
                &nick,
                "You must use your current nickname as the account name",
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        } else {
            account.to_string()
        };

        // Check if nick is already in use by someone else (land grab protection)
        // Only applies if using "*" for account (current nick)
        // If someone else is using the nick, they effectively "own" it and we can't register it
        if account == "*" {
            // Check if someone else is using this nick
            let nick_lower = slirc_proto::irc_to_lower(&target_account);
            if let Some(existing_uid) = ctx.matrix.user_manager.get_first_uid(&nick_lower) {
                // If the existing user isn't us, fail with ACCOUNT_EXISTS
                // (they effectively have a claim on this nick)
                if existing_uid != ctx.uid {
                    let reply = fail_response(
                        server_name,
                        "ACCOUNT_EXISTS",
                        &nick,
                        "That nickname is already in use",
                    );
                    ctx.sender.send(reply).await?;
                    return Ok(());
                }
            }
        }

        // Check if account already exists (using NickServ)
        if ctx
            .matrix
            .service_manager
            .nickserv
            .account_exists(&target_account)
            .await
        {
            let reply = fail_response(
                server_name,
                "ACCOUNT_EXISTS",
                &nick,
                "Account already exists",
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Create the account
        match ctx
            .db
            .accounts()
            .register(&target_account, password, Some(email))
            .await
        {
            Ok(_) => {
                // Send success response
                let success_msg = Message {
                    tags: None,
                    prefix: Some(Prefix::new_from_str(server_name)),
                    command: Command::REGISTER {
                        account: target_account.to_string(),
                        message: Some("Account created".to_string()),
                    },
                };
                ctx.sender.send(success_msg).await?;
            }
            Err(crate::db::DbError::AccountExists(_)) => {
                let reply = fail_response(
                    server_name,
                    "ACCOUNT_EXISTS",
                    &nick,
                    "Account already exists",
                );
                ctx.sender.send(reply).await?;
            }
            Err(e) => {
                tracing::error!("Failed to register account: {}", e);
                let reply = fail_response(
                    server_name,
                    "REG_INVALID_CALLBACK",
                    &nick,
                    "Internal error during registration",
                );
                ctx.sender.send(reply).await?;
            }
        }

        Ok(())
    }
}
