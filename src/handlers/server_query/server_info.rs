//! MOTD and related handlers.

use super::super::{
    Context, Handler, HandlerError, HandlerResult, err_notregistered, server_reply,
};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Server version string.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Handler for MOTD command.
///
/// `MOTD [target]`
///
/// Returns the "Message of the Day" for the server.
pub struct MotdHandler;

#[async_trait]
impl Handler for MotdHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // RPL_MOTDSTART (375): :- <server> Message of the day -
        let reply = server_reply(
            server_name,
            Response::RPL_MOTDSTART,
            vec![
                nick.clone(),
                format!("- {} Message of the day -", server_name),
            ],
        );
        ctx.sender.send(reply).await?;

        // RPL_MOTD (372): :- <text> - send each line from configured MOTD
        for line in &ctx.matrix.server_info.motd_lines {
            let reply = server_reply(
                server_name,
                Response::RPL_MOTD,
                vec![nick.clone(), format!("- {}", line)],
            );
            ctx.sender.send(reply).await?;
        }

        // RPL_ENDOFMOTD (376): :End of MOTD command
        let reply = server_reply(
            server_name,
            Response::RPL_ENDOFMOTD,
            vec![nick.clone(), "End of MOTD command".to_string()],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for VERSION command.
///
/// `VERSION [target]`
///
/// Returns the version of the server.
pub struct VersionHandler;

#[async_trait]
impl Handler for VersionHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // RPL_VERSION (351): <version>.<debuglevel> <server> :<comments>
        #[cfg(debug_assertions)]
        let version_str = format!("{}-debug.1", VERSION);
        #[cfg(not(debug_assertions))]
        let version_str = format!("{}.0", VERSION);

        let reply = server_reply(
            server_name,
            Response::RPL_VERSION,
            vec![
                nick.clone(),
                version_str,
                server_name.clone(),
                "slircd-ng IRC daemon".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for TIME command.
///
/// `TIME [target]`
///
/// Returns the local time on the server.
pub struct TimeHandler;

#[async_trait]
impl Handler for TimeHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // RPL_TIME (391): <server> :<string showing server's local time>
        let now = chrono::Local::now();
        let time_string = now.format("%A %B %d %Y -- %H:%M:%S %z").to_string();

        let reply = server_reply(
            server_name,
            Response::RPL_TIME,
            vec![nick.clone(), server_name.clone(), time_string],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for ADMIN command.
///
/// `ADMIN [target]`
///
/// Returns administrative information about the server.
pub struct AdminHandler;

#[async_trait]
impl Handler for AdminHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // RPL_ADMINME (256): <server> :Administrative info
        let reply = server_reply(
            server_name,
            Response::RPL_ADMINME,
            vec![
                nick.clone(),
                server_name.clone(),
                "Administrative info".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        // RPL_ADMINLOC1 (257): :<admin info>
        let reply = server_reply(
            server_name,
            Response::RPL_ADMINLOC1,
            vec![nick.clone(), "slircd-ng IRC Server".to_string()],
        );
        ctx.sender.send(reply).await?;

        // RPL_ADMINLOC2 (258): :<admin info>
        let reply = server_reply(
            server_name,
            Response::RPL_ADMINLOC2,
            vec![nick.clone(), ctx.matrix.server_info.network.clone()],
        );
        ctx.sender.send(reply).await?;

        // RPL_ADMINEMAIL (259): :<admin email>
        let reply = server_reply(
            server_name,
            Response::RPL_ADMINEMAIL,
            vec![nick.clone(), format!("admin@{}", server_name)],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for INFO command.
///
/// `INFO [target]`
///
/// Returns information describing the server.
pub struct InfoHandler;

#[async_trait]
impl Handler for InfoHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        let info_lines = [
            format!("slircd-ng v{} - High-performance IRC daemon", VERSION),
            "https://github.com/sid3xyz/slircd-ng".to_string(),
            "".to_string(),
            "Built with Rust and Tokio async runtime".to_string(),
            "Zero-copy message parsing via slirc-proto".to_string(),
            "DashMap concurrent state management".to_string(),
            "".to_string(),
            format!("Server: {}", ctx.matrix.server_info.name),
            format!("Network: {}", ctx.matrix.server_info.network),
        ];

        // RPL_INFO (371): :<string>
        for line in &info_lines {
            let reply = server_reply(
                server_name,
                Response::RPL_INFO,
                vec![nick.clone(), line.clone()],
            );
            ctx.sender.send(reply).await?;
        }

        // RPL_ENDOFINFO (374): :End of INFO list
        let reply = server_reply(
            server_name,
            Response::RPL_ENDOFINFO,
            vec![nick.clone(), "End of INFO list".to_string()],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for LUSERS command.
///
/// `LUSERS [mask [target]]`
///
/// Returns statistics about the size of the IRC network.
pub struct LusersHandler;

#[async_trait]
impl Handler for LusersHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Count users and channels
        let total_users = ctx.matrix.users.len();
        let mut invisible_count = 0;
        let mut oper_count = 0;

        for user_ref in ctx.matrix.users.iter() {
            let user = user_ref.read().await;
            if user.modes.invisible {
                invisible_count += 1;
            }
            if user.modes.oper {
                oper_count += 1;
            }
        }

        let visible_users = total_users.saturating_sub(invisible_count);
        let channel_count = ctx.matrix.channels.len();

        // RPL_LUSERCLIENT (251): :There are <u> users and <i> invisible on <s> servers
        let reply = server_reply(
            server_name,
            Response::RPL_LUSERCLIENT,
            vec![
                nick.clone(),
                format!(
                    "There are {} users and {} invisible on 1 servers",
                    visible_users, invisible_count
                ),
            ],
        );
        ctx.sender.send(reply).await?;

        // RPL_LUSEROP (252): <ops> :operator(s) online
        let reply = server_reply(
            server_name,
            Response::RPL_LUSEROP,
            vec![
                nick.clone(),
                oper_count.to_string(),
                "operator(s) online".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        // RPL_LUSERUNKNOWN (253): <u> :unknown connection(s)
        // Unregistered = connections with nick but not yet in users map
        // This includes connections that have sent NICK but not USER
        let total_nicks = ctx.matrix.nicks.len();
        let unregistered_count = total_nicks.saturating_sub(total_users);

        let reply = server_reply(
            server_name,
            Response::RPL_LUSERUNKNOWN,
            vec![
                nick.clone(),
                unregistered_count.to_string(),
                "unknown connection(s)".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        // RPL_LUSERCHANNELS (254): <channels> :channels formed
        let reply = server_reply(
            server_name,
            Response::RPL_LUSERCHANNELS,
            vec![
                nick.clone(),
                channel_count.to_string(),
                "channels formed".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        // RPL_LUSERME (255): :I have <c> clients and <s> servers
        let reply = server_reply(
            server_name,
            Response::RPL_LUSERME,
            vec![
                nick.clone(),
                format!("I have {} clients and 0 servers", total_users),
            ],
        );
        ctx.sender.send(reply).await?;

        // RPL_LOCALUSERS (265): <u> <m> :Current local users <u>, max <m>
        let reply = server_reply(
            server_name,
            Response::RPL_LOCALUSERS,
            vec![
                nick.clone(),
                total_users.to_string(),
                total_users.to_string(), // max = current for now
                format!("Current local users {}, max {}", total_users, total_users),
            ],
        );
        ctx.sender.send(reply).await?;

        // RPL_GLOBALUSERS (266): <u> <m> :Current global users <u>, max <m>
        let reply = server_reply(
            server_name,
            Response::RPL_GLOBALUSERS,
            vec![
                nick.clone(),
                total_users.to_string(),
                total_users.to_string(),
                format!("Current global users {}, max {}", total_users, total_users),
            ],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}
