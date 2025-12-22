//! MOTD and related handlers.

use super::super::core::traits::PostRegHandler;
use super::super::{Context, HandlerResult};
use crate::state::RegisteredState;
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
impl PostRegHandler for MotdHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = ctx.server_name();
        let nick = ctx.nick();

        // RPL_MOTDSTART (375): :- <server> Message of the day -
        ctx.send_reply(
            Response::RPL_MOTDSTART,
            vec![
                nick.to_string(),
                format!("- {} Message of the day -", server_name),
            ],
        )
        .await?;

        // RPL_MOTD (372): :- <text> - send each line from configured MOTD
        for line in &ctx.matrix.server_info.motd_lines {
            ctx.send_reply(
                Response::RPL_MOTD,
                vec![nick.to_string(), format!("- {}", line)],
            )
            .await?;
        }

        // RPL_ENDOFMOTD (376): :End of MOTD command
        ctx.send_reply(
            Response::RPL_ENDOFMOTD,
            vec![nick.to_string(), "End of MOTD command".to_string()],
        )
        .await?;

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
impl PostRegHandler for VersionHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = ctx.server_name();
        let nick = ctx.nick();

        // RPL_VERSION (351): <version>.<debuglevel> <server> :<comments>
        #[cfg(debug_assertions)]
        let version_str = format!("{}-debug.1", VERSION);
        #[cfg(not(debug_assertions))]
        let version_str = format!("{}.0", VERSION);

        ctx.send_reply(
            Response::RPL_VERSION,
            vec![
                nick.to_string(),
                version_str,
                server_name.to_string(),
                "slircd-ng IRC daemon".to_string(),
            ],
        )
        .await?;

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
impl PostRegHandler for TimeHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Compile-time guarantee: nick is always present for Registered connections
        let nick = ctx.nick(); // Returns &str, not Option!
        let server_name = ctx.server_name();

        // RPL_TIME (391): <server> :<string showing server's local time>
        let now = chrono::Local::now();
        let time_string = now.format("%A %B %d %Y -- %H:%M:%S %z").to_string();

        ctx.send_reply(
            Response::RPL_TIME,
            vec![nick.to_string(), server_name.to_string(), time_string],
        )
        .await?;

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
impl PostRegHandler for AdminHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Compile-time guarantee: nick is always present for Registered connections
        let nick = ctx.nick(); // Returns &str, not Option!
        let server_name = ctx.server_name();

        // RPL_ADMINME (256): <server> :Administrative info
        ctx.send_reply(
            Response::RPL_ADMINME,
            vec![
                nick.to_string(),
                server_name.to_string(),
                "Administrative info".to_string(),
            ],
        )
        .await?;

        // RPL_ADMINLOC1 (257): :<admin info> - organization/server description
        let admin_info1 = ctx
            .matrix
            .config
            .server
            .admin_info1
            .clone()
            .unwrap_or_else(|| ctx.matrix.server_info.description.clone());
        ctx.send_reply(Response::RPL_ADMINLOC1, vec![nick.to_string(), admin_info1])
            .await?;

        // RPL_ADMINLOC2 (258): :<admin info> - location/network
        let admin_info2 = ctx
            .matrix
            .config
            .server
            .admin_info2
            .clone()
            .unwrap_or_else(|| ctx.matrix.server_info.network.clone());
        ctx.send_reply(Response::RPL_ADMINLOC2, vec![nick.to_string(), admin_info2])
            .await?;

        // RPL_ADMINEMAIL (259): :<admin email>
        let admin_email = ctx
            .matrix
            .config
            .server
            .admin_email
            .clone()
            .unwrap_or_else(|| format!("admin@{}", server_name));
        ctx.send_reply(
            Response::RPL_ADMINEMAIL,
            vec![nick.to_string(), admin_email],
        )
        .await?;

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
impl PostRegHandler for InfoHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Compile-time guarantee: nick is always present for Registered connections
        let nick = ctx.nick(); // Returns &str, not Option!
        let server_name = ctx.server_name();

        // If a target is specified, check if it matches this server
        if let Some(target) = msg.arg(0) {
            // Accept if target matches our server name exactly, or as nick
            let target_lower = target.to_lowercase();
            let server_lower = server_name.to_lowercase();
            let nick_lower = nick.to_lowercase();

            // Check if target matches server name or nick
            // Also accept wildcards that would match our server (simple * check)
            let is_match = target_lower == server_lower
                || target_lower == nick_lower
                || target == "*"
                || (target.ends_with('*')
                    && server_lower.starts_with(&target_lower[..target_lower.len() - 1]));

            if !is_match {
                // ERR_NOSUCHSERVER (402)
                ctx.send_reply(
                    Response::ERR_NOSUCHSERVER,
                    vec![
                        nick.to_string(),
                        target.to_string(),
                        "No such server".to_string(),
                    ],
                )
                .await?;
                return Ok(());
            }
        }

        let info_lines = [
            format!("slircd-ng v{} - High-performance IRC daemon", VERSION),
            "https://github.com/sid3xyz/slircd-ng".to_string(),
            "".to_string(),
            "Built with Rust and Tokio async runtime".to_string(),
            "Zero-copy message parsing via slirc-proto".to_string(),
            "DashMap concurrent state management".to_string(),
            "".to_string(),
            format!("Server: {}", ctx.server_name()),
            format!("Network: {}", ctx.matrix.server_info.network),
        ];

        // RPL_INFO (371): :<string>
        for line in &info_lines {
            ctx.send_reply(Response::RPL_INFO, vec![nick.to_string(), line.clone()])
                .await?;
        }

        // RPL_ENDOFINFO (374): :End of INFO list
        ctx.send_reply(
            Response::RPL_ENDOFINFO,
            vec![nick.to_string(), "End of INFO list".to_string()],
        )
        .await?;

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
impl PostRegHandler for LusersHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let nick = ctx.nick();

        // Count users and channels
        // Collect user refs first to avoid holding DashMap shard lock across await points
        // Exclude service pseudoclients (NickServ, ChanServ) from counts
        let user_refs: Vec<_> = ctx
            .matrix
            .user_manager
            .users
            .iter()
            .map(|r| r.value().clone())
            .collect();

        let mut total_users: usize = 0;
        let mut invisible_count: usize = 0;
        let mut oper_count: usize = 0;

        for user_ref in user_refs {
            let user = user_ref.read().await;
            // Skip service pseudoclients - they are not real users
            if user.modes.service {
                continue;
            }
            total_users += 1;
            if user.modes.invisible {
                invisible_count += 1;
            }
            if user.modes.oper {
                oper_count += 1;
            }
        }

        // CRITICAL FIX: RFC 2812 LUSERS format: "There are X users and Y invisible on Z servers"
        // where X = visible users (non-+i) and Y = invisible users (+i).
        // Total users = X + Y (irctest lusers.py line 56: GlobalInvisible + GlobalVisible == total)
        let visible_users = total_users.saturating_sub(invisible_count);
        let channel_count = ctx.matrix.channel_manager.channels.len();

        // RPL_LUSERCLIENT (251): :There are <u> users and <i> invisible on <s> servers
        ctx.send_reply(
            Response::RPL_LUSERCLIENT,
            vec![
                nick.to_string(),
                format!(
                    "There are {} users and {} invisible on 1 servers",
                    visible_users, invisible_count
                ),
            ],
        )
        .await?;

        // RPL_LUSEROP (252): <ops> :operator(s) online
        ctx.send_reply(
            Response::RPL_LUSEROP,
            vec![
                nick.to_string(),
                oper_count.to_string(),
                "operator(s) online".to_string(),
            ],
        )
        .await?;

        // RPL_LUSERUNKNOWN (253): <u> :unknown connection(s)
        // Unregistered = connections with nick but not yet in users map
        // This includes connections that have sent NICK but not USER
        // Note: Services have nicks but are excluded from total_users, so we compare
        // total_nicks - service_nicks (which is the same as nicks.len() minus 2 for NickServ/ChanServ)
        // Actually, the correct way is to compare: nicks with non-service users
        // Since total_users already excludes services, we need total_nicks to also exclude them
        let service_nick_count = 2; // NickServ, ChanServ
        let real_nicks = ctx
            .matrix
            .user_manager
            .nicks
            .len()
            .saturating_sub(service_nick_count);
        let unregistered_count = real_nicks.saturating_sub(total_users);

        ctx.send_reply(
            Response::RPL_LUSERUNKNOWN,
            vec![
                nick.to_string(),
                unregistered_count.to_string(),
                "unknown connection(s)".to_string(),
            ],
        )
        .await?;

        // RPL_LUSERCHANNELS (254): <channels> :channels formed
        ctx.send_reply(
            Response::RPL_LUSERCHANNELS,
            vec![
                nick.to_string(),
                channel_count.to_string(),
                "channels formed".to_string(),
            ],
        )
        .await?;

        // RPL_LUSERME (255): :I have <c> clients and <s> servers
        ctx.send_reply(
            Response::RPL_LUSERME,
            vec![
                nick.to_string(),
                format!("I have {} clients and 0 servers", total_users),
            ],
        )
        .await?;

        // RPL_LOCALUSERS (265): <u> <m> :Current local users <u>, max <m>
        let max_local = ctx
            .matrix
            .user_manager
            .max_local_users
            .load(std::sync::atomic::Ordering::Relaxed);
        ctx.send_reply(
            Response::RPL_LOCALUSERS,
            vec![
                nick.to_string(),
                total_users.to_string(),
                max_local.to_string(),
                format!("Current local users {}, max {}", total_users, max_local),
            ],
        )
        .await?;

        // RPL_GLOBALUSERS (266): <u> <m> :Current global users <u>, max <m>
        let max_global = ctx
            .matrix
            .user_manager
            .max_global_users
            .load(std::sync::atomic::Ordering::Relaxed);
        ctx.send_reply(
            Response::RPL_GLOBALUSERS,
            vec![
                nick.to_string(),
                total_users.to_string(),
                max_global.to_string(),
                format!("Current global users {}, max {}", total_users, max_global),
            ],
        )
        .await?;

        Ok(())
    }
}
