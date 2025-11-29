//! Server query handlers: VERSION, TIME, ADMIN, INFO, LUSERS, STATS, MOTD
//!
//! RFC 2812 §3.4 - Server queries and commands

use super::{server_reply, Context, Handler, HandlerError, HandlerResult};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};
use std::time::{SystemTime, UNIX_EPOCH};

/// Server version string.
const VERSION: &str = env!("CARGO_PKG_VERSION");

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
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

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
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

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
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

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
            vec![nick.clone(), "admin@localhost".to_string()],
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
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

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
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

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
        let reply = server_reply(
            server_name,
            Response::RPL_LUSERUNKNOWN,
            vec![
                nick.clone(),
                "0".to_string(),
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

/// Handler for STATS command.
///
/// `STATS [query [target]]`
///
/// Returns statistics about the server.
pub struct StatsHandler;

#[async_trait]
impl Handler for StatsHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

        // STATS [query]
        let query = msg.arg(0).and_then(|s| s.chars().next());

        let query_char = query.unwrap_or('?');

        match query_char {
            'u' => {
                // RPL_STATSUPTIME (242): Server uptime
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let created = ctx.matrix.server_info.created as u64;
                let uptime = now.saturating_sub(created);

                let days = uptime / 86400;
                let hours = (uptime % 86400) / 3600;
                let minutes = (uptime % 3600) / 60;
                let seconds = uptime % 60;

                let reply = server_reply(
                    server_name,
                    Response::RPL_STATSUPTIME,
                    vec![
                        nick.clone(),
                        format!(
                            "Server Up {} days {}:{:02}:{:02}",
                            days, hours, minutes, seconds
                        ),
                    ],
                );
                ctx.sender.send(reply).await?;
            }
            '?' => {
                // Help - list available queries
                let help_lines = [
                    "*** Available STATS queries:",
                    "*** u - Server uptime",
                    "*** ? - This help message",
                ];
                for line in &help_lines {
                    let reply = server_reply(
                        server_name,
                        Response::RPL_STATSDLINE, // Using generic stats reply
                        vec![nick.clone(), (*line).to_string()],
                    );
                    ctx.sender.send(reply).await?;
                }
            }
            _ => {
                // Unknown query - just return end of stats
            }
        }

        // RPL_ENDOFSTATS (219): <query> :End of STATS report
        let reply = server_reply(
            server_name,
            Response::RPL_ENDOFSTATS,
            vec![
                nick.clone(),
                query_char.to_string(),
                "End of STATS report".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

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
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

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

        // RPL_MOTD (372): :- <text>
        let motd_lines = [
            format!("- Welcome to {}", ctx.matrix.server_info.network),
            format!("- Running slircd-ng v{}", VERSION),
            "- ".to_string(),
            "- This server is powered by Rust and Tokio.".to_string(),
            "- Enjoy your stay!".to_string(),
        ];

        for line in &motd_lines {
            let reply = server_reply(
                server_name,
                Response::RPL_MOTD,
                vec![nick.clone(), line.clone()],
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

/// Handler for LIST command.
///
/// `LIST [channels [target]]`
///
/// Lists channels and their topics.
pub struct ListHandler;

#[async_trait]
impl Handler for ListHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

        // LIST [channels]
        let filter = msg.arg(0);

        // RPL_LISTSTART (321): Channel :Users Name (optional, some clients don't expect it)

        // Iterate channels
        for channel_ref in ctx.matrix.channels.iter() {
            let channel = channel_ref.read().await;

            // Skip secret channels unless user is a member
            if channel.modes.secret && !channel.is_member(ctx.uid) {
                continue;
            }

            // Apply filter if provided
            if let Some(f) = filter
                && !channel.name.eq_ignore_ascii_case(f)
            {
                continue;
            }

            let topic_text = channel
                .topic
                .as_ref()
                .map(|t| t.text.clone())
                .unwrap_or_default();

            // RPL_LIST (322): <channel> <# visible> :<topic>
            let reply = server_reply(
                server_name,
                Response::RPL_LIST,
                vec![
                    nick.clone(),
                    channel.name.clone(),
                    channel.members.len().to_string(),
                    topic_text,
                ],
            );
            ctx.sender.send(reply).await?;
        }

        // RPL_LISTEND (323): :End of LIST
        let reply = server_reply(
            server_name,
            Response::RPL_LISTEND,
            vec![nick.clone(), "End of LIST".to_string()],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}
