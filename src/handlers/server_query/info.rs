//! MAP, RULES, USERIP, and LINKS handlers.
//!
//! Additional server query commands for network information.

use super::super::{
    Context, Handler, HandlerError, HandlerResult, err_needmoreparams, err_noprivileges,
    get_oper_info,
};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for MAP command.
///
/// `MAP`
///
/// Returns the server map (network topology). In a single-server setup,
/// this just shows the current server.
pub struct MapHandler;

#[async_trait]
impl Handler for MapHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        let user_count = ctx.matrix.users.len();

        // RPL_MAP (006): <server> [<users>]
        ctx.send_reply(
            Response::RPL_MAP,
            vec![
                nick.clone(),
                format!("`- {} [{} users]", server_name, user_count),
            ],
        )
        .await?;

        // RPL_MAPEND (007): :End of MAP
        ctx.send_reply(
            Response::RPL_MAPEND,
            vec![nick.clone(), "End of MAP".to_string()],
        )
        .await?;

        Ok(())
    }
}

/// Handler for RULES command.
///
/// `RULES`
///
/// Returns the server rules.
pub struct RulesHandler;

#[async_trait]
impl Handler for RulesHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // RPL_RULESTART (232): :- <server> Server Rules -
        ctx.send_reply(
            Response::RPL_RULESTART,
            vec![nick.clone(), format!("- {} Server Rules -", server_name)],
        )
        .await?;

        // Server rules (could be loaded from config in the future)
        let rules = [
            "1. Be respectful to other users.",
            "2. No flooding or spamming.",
            "3. No unauthorized bots.",
            "4. Follow the network guidelines.",
            "5. Have fun!",
        ];

        // RPL_RULES (633): :- <rule>
        for rule in &rules {
            ctx.send_reply(
                Response::RPL_RULES,
                vec![nick.clone(), format!("- {}", rule)],
            )
            .await?;
        }

        // RPL_ENDOFRULES (634): :End of RULES command
        ctx.send_reply(
            Response::RPL_ENDOFRULES,
            vec![nick.clone(), "End of RULES command".to_string()],
        )
        .await?;

        Ok(())
    }
}

/// Handler for USERIP command.
///
/// `USERIP nickname [nickname...]`
///
/// Returns the IP addresses of the specified nicknames.
/// This is an oper-only command.
pub struct UseripHandler;

#[async_trait]
impl Handler for UseripHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Check for oper privileges
        let Some((_, is_oper)) = get_oper_info(ctx).await else {
            return Ok(());
        };

        if !is_oper {
            ctx.sender.send(err_noprivileges(server_name, nick)).await?;
            return Ok(());
        }

        // Need at least one nickname
        if msg.arg(0).is_none() {
            ctx.sender
                .send(err_needmoreparams(server_name, nick, "USERIP"))
                .await?;
            return Ok(());
        }

        // Collect all target nicknames from arguments
        let mut results = Vec::new();

        for i in 0..16 {
            // Limit to 16 nicknames
            let Some(target_nick) = msg.arg(i) else {
                break;
            };

            // Look up the user by nick
            let lower_nick = slirc_proto::irc_to_lower(target_nick);
            if let Some(uid_ref) = ctx.matrix.nicks.get(&lower_nick) {
                let uid = uid_ref.value();
                if let Some(user_ref) = ctx.matrix.users.get(uid) {
                    let user = user_ref.read().await;
                    // Format: nick[*]=+user@host
                    // * indicates oper, + indicates away (or - if away)
                    let oper_flag = if user.modes.oper { "*" } else { "" };
                    let away_flag = if user.away.is_some() { "-" } else { "+" };
                    results.push(format!(
                        "{}{}={}{}@{}",
                        user.nick, oper_flag, away_flag, user.user, user.host
                    ));
                }
            }
        }

        // RPL_USERIP (340): <reply> [<reply> ...]
        ctx.send_reply(Response::RPL_USERIP, vec![nick.clone(), results.join(" ")])
            .await?;

        Ok(())
    }
}

/// Handler for LINKS command.
///
/// `LINKS [[remote] mask]`
///
/// Returns a list of servers linked to the network.
/// In a single-server setup, this just shows the current server.
pub struct LinksHandler;

#[async_trait]
impl Handler for LinksHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // RPL_LINKS (364): <mask> <server> :<hopcount> <server info>
        ctx.send_reply(
            Response::RPL_LINKS,
            vec![
                nick.clone(),
                server_name.clone(),
                server_name.clone(),
                format!("0 {}", ctx.matrix.server_info.description),
            ],
        )
        .await?;

        // RPL_ENDOFLINKS (365): <mask> :End of LINKS list
        ctx.send_reply(
            Response::RPL_ENDOFLINKS,
            vec![
                nick.clone(),
                "*".to_string(),
                "End of LINKS list".to_string(),
            ],
        )
        .await?;

        Ok(())
    }
}
