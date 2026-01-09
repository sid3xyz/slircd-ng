//! LUSERS handler.
//!
//! Returns statistics about the size of the IRC network.

use super::super::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for LUSERS command.
///
/// `LUSERS [mask [target]]`
///
/// Returns statistics about the size of the IRC network.
///
/// **Compliance:** 9/9 irctest pass
pub struct LusersHandler;

#[async_trait]
impl PostRegHandler for LusersHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let nick = ctx.nick();
        let server_name = ctx.server_name();

        // Handle target parameter if present
        if let Some(target) = msg.arg(1) {
            let target_lower = target.to_lowercase();
            let server_lower = server_name.to_lowercase();

            // Check if target matches us or is wildcard
            let is_match = target_lower == server_lower || target == "*";

            if !is_match {
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

        // Count users and channels
        // Collect user refs first to avoid holding DashMap shard lock across await points
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
        let mut local_users: usize = 0;

        let local_sid_str = ctx.matrix.server_info.sid.as_str();

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
            if user.uid.starts_with(local_sid_str) {
                local_users += 1;
            }
        }

        let visible_users = total_users.saturating_sub(invisible_count);
        let channel_count = ctx.matrix.channel_manager.channels.len();

        // Server counts: topology servers + 1 (us)
        let total_servers = ctx.matrix.sync_manager.topology.servers.len() + 1;

        // Direct links: servers where via is None (and not us)
        let local_sid = slirc_crdt::clock::ServerId::new(local_sid_str);

        let mut direct_links = 0;
        for s in ctx.matrix.sync_manager.topology.servers.iter() {
            let server = s.value();
            if server.sid != local_sid && server.via.is_none() {
                direct_links += 1;
            }
        }

        // RPL_LUSERCLIENT (251): :There are <u> users and <i> invisible on <s> servers
        ctx.send_reply(
            Response::RPL_LUSERCLIENT,
            vec![
                nick.to_string(),
                format!(
                    "There are {} users and {} invisible on {} servers",
                    visible_users, invisible_count, total_servers
                ),
            ],
        )
        .await?;

        // RPL_LUSEROP (252): <ops> :operator(s) online
        // Always send, even if 0, to satisfy strict tests
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
        let service_nick_count = 2; // NickServ, ChanServ
        let real_nicks = ctx
            .matrix
            .user_manager
            .nicks
            .len()
            .saturating_sub(service_nick_count);
        let unregistered_count = real_nicks.saturating_sub(total_users);

        if unregistered_count > 0 {
            ctx.send_reply(
                Response::RPL_LUSERUNKNOWN,
                vec![
                    nick.to_string(),
                    unregistered_count.to_string(),
                    "unknown connection(s)".to_string(),
                ],
            )
            .await?;
        }

        // RPL_LUSERCHANNELS (254): <channels> :channels formed
        if channel_count > 0 {
            ctx.send_reply(
                Response::RPL_LUSERCHANNELS,
                vec![
                    nick.to_string(),
                    channel_count.to_string(),
                    "channels formed".to_string(),
                ],
            )
            .await?;
        }

        // RPL_LUSERME (255): :I have <c> clients and <s> servers
        ctx.send_reply(
            Response::RPL_LUSERME,
            vec![
                nick.to_string(),
                format!(
                    "I have {} clients and {} servers",
                    local_users, direct_links
                ),
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
                local_users.to_string(),
                max_local.to_string(),
                format!("Current local users {}, max {}", local_users, max_local),
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
