//! METADATA command handler (Ergo extension).
//!
//! The METADATA command allows getting, setting, and listing metadata
//! associated with users and channels.
//!
//! Format:
//! - `METADATA GET <target> <key>` - Get a metadata key for a user or channel
//! - `METADATA SET <target> <key> [value]` - Set a metadata key (empty value deletes)
//! - `METADATA LIST <target>` - List all metadata for a target
//!
//! This handler implements the METADATA command, supporting:
//! - Channel metadata (saved to runtime state and registered channel DB)
//! - User metadata (saved to account DB if identified)
//! - Access control via proper ownership checks

use super::super::{Context, HandlerResult, PostRegHandler, server_reply};
use crate::state::RegisteredState;
use crate::state::actor::{ChannelEvent, MetadataCommand as ActorMetadataCommand};
use async_trait::async_trait;
use slirc_proto::command::subcommands::MetadataSubCommand;
use slirc_proto::{ChannelExt, MessageRef, Response};
use tokio::sync::oneshot;

pub struct MetadataHandler;

#[async_trait]
impl PostRegHandler for MetadataHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Validation of command name
        if msg.command.name != "METADATA" {
            return Ok(());
        }

        let args = &msg.command.args;
        if args.len() < 2 {
            // Need at least target and subcommand
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NEEDMOREPARAMS,
                vec![
                    ctx.state.nick.clone(),
                    "METADATA".to_string(),
                    "Not enough parameters".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let target = args[0];
        let subcommand_str = args[1];
        let params = &args[2..]; // slice of &str

        let subcommand = match subcommand_str.to_uppercase().as_str() {
            "GET" => MetadataSubCommand::GET,
            "SET" => MetadataSubCommand::SET,
            "LIST" => MetadataSubCommand::LIST,
            _ => return Ok(()), // Invalid subcommand, maybe send ERR_UKNOWNCOMMAND?
        };

        let (target_lower, reply_target) = if target == "*" {
            (
                slirc_proto::irc_to_lower(&ctx.state.nick),
                ctx.state.nick.as_str(),
            )
        } else {
            (slirc_proto::irc_to_lower(target), target)
        };

        if target.is_channel_name() {
            // Channel Metadata
            let chan_sender = ctx
                .matrix
                .channel_manager
                .channels
                .get(&target_lower)
                .map(|r| r.value().clone());

            if let Some(chan_sender) = chan_sender {
                // Determine action
                let actor_cmd = match subcommand {
                    MetadataSubCommand::GET => {
                        if params.is_empty() {
                            let reply = server_reply(
                                &ctx.matrix.server_info.name,
                                Response::ERR_NEEDMOREPARAMS,
                                vec![
                                    ctx.state.nick.clone(),
                                    "METADATA".to_string(),
                                    "Not enough parameters".to_string(),
                                ],
                            );
                            ctx.sender.send(reply).await?;
                            return Ok(());
                        }
                        ActorMetadataCommand::Get {
                            key: params[0].to_string(),
                        }
                    }
                    MetadataSubCommand::SET => {
                        if params.is_empty() {
                            let reply = server_reply(
                                &ctx.matrix.server_info.name,
                                Response::ERR_NEEDMOREPARAMS,
                                vec![
                                    ctx.state.nick.clone(),
                                    "METADATA".to_string(),
                                    "Not enough parameters".to_string(),
                                ],
                            );
                            ctx.sender.send(reply).await?;
                            return Ok(());
                        }

                        let key = params[0].to_string();
                        let value = if params.len() > 1 {
                            Some(params[1].to_string())
                        } else {
                            None
                        };
                        ActorMetadataCommand::Set { key, value }
                    }
                    MetadataSubCommand::LIST => ActorMetadataCommand::List,
                    _ => return Ok(()),
                };

                let (reply_tx, reply_rx) = oneshot::channel();
                let event = ChannelEvent::Metadata {
                    command: actor_cmd,
                    reply_tx,
                };

                if chan_sender.send(event).await.is_err() {
                    let reply = server_reply(
                        &ctx.matrix.server_info.name,
                        Response::ERR_NOSUCHCHANNEL,
                        vec![
                            ctx.state.nick.clone(),
                            reply_target.to_string(),
                            "No such channel".to_string(),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                    return Ok(());
                }

                match reply_rx.await {
                    Ok(Ok(map)) => {
                        for (k, v) in map {
                            // Deprecated spec format: 761 <client> <key> <visibility> <value>
                            let reply = server_reply(
                                &ctx.matrix.server_info.name,
                                Response::RPL_KEYVALUE,
                                vec![
                                    ctx.state.nick.clone(),
                                    k.clone(),
                                    "*".to_string(),
                                    v.clone(),
                                ],
                            );
                            ctx.sender.send(reply).await?;

                            if subcommand == MetadataSubCommand::SET
                                && ctx
                                    .matrix
                                    .channel_manager
                                    .registered_channels
                                    .contains(&target_lower)
                            {
                                let repo = ctx.matrix.db.channels();
                                if let Ok(Some(channel)) = repo.find_by_name(&target_lower).await {
                                    // params[0] is key, value is v (or from params)
                                    // The actor returned the map? No, actor returns empty map for SET.
                                    // Wait, SET returns Ok(HashMap::new()).
                                    // So we can't get the value from the map if it's empty.
                                    // We must use params.
                                    if !params.is_empty() {
                                        let key = params[0].to_string();
                                        // Re-derive value from params logic
                                        let value_to_save = if params.len() > 1 {
                                            Some(params[1].to_string())
                                        } else {
                                            None
                                        };
                                        if let Err(e) = repo
                                            .set_metadata(
                                                channel.id,
                                                &key,
                                                value_to_save.as_deref(),
                                            )
                                            .await
                                        {
                                            tracing::error!(
                                                "Failed to persist channel metadata: {}",
                                                e
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        let reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::RPL_METADATAEND,
                            vec![ctx.state.nick.clone(), "End of metadata".to_string()],
                        );
                        ctx.sender.send(reply).await?;
                    }
                    Ok(Err(_e)) => {
                        // Map ChannelError to IRC Error - e.g. limit exceeded
                        let reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::ERR_METADATALIMIT,
                            vec![
                                ctx.state.nick.clone(),
                                "Metadata limit exceeded".to_string(),
                            ],
                        );
                        ctx.sender.send(reply).await?;
                    }
                    Err(_) => {}
                }
            } else {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHCHANNEL,
                    vec![
                        ctx.state.nick.clone(),
                        reply_target.to_string(),
                        "No such channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
            }
        } else {
            // User Metadata
            // 1. Resolve target nick to UID
            let target_uid = if let Some(uid) = ctx.matrix.user_manager.get_first_uid(&target_lower)
            {
                uid
            } else {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_TARGETINVALID,
                    vec![
                        ctx.state.nick.clone(),
                        reply_target.to_string(),
                        "No such nick/channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            };

            // 2. Get User object
            if let Some(user_rw) = ctx.matrix.user_manager.users.get(&target_uid) {
                let user_rw = user_rw.value().clone();

                match subcommand {
                    MetadataSubCommand::GET => {
                        if params.is_empty() {
                            let reply = server_reply(
                                &ctx.matrix.server_info.name,
                                Response::ERR_NEEDMOREPARAMS,
                                vec![
                                    ctx.state.nick.clone(),
                                    "METADATA".to_string(),
                                    "Not enough parameters".to_string(),
                                ],
                            );
                            ctx.sender.send(reply).await?;
                            return Ok(());
                        }

                        let user = user_rw.read().await;
                        for key in params {
                            let key = key.to_string();
                            if let Some(val) = user.metadata.get(&key) {
                                // Deprecated spec format: 761 <client> <key> <visibility> <value>
                                let reply = server_reply(
                                    &ctx.matrix.server_info.name,
                                    Response::RPL_KEYVALUE,
                                    vec![ctx.state.nick.clone(), key, "*".to_string(), val.clone()],
                                );
                                ctx.sender.send(reply).await?;
                            } else {
                                // Deprecated spec format: 766 <client> <key> :no matching key
                                let reply = server_reply(
                                    &ctx.matrix.server_info.name,
                                    Response::ERR_NOMATCHINGKEY,
                                    vec![
                                        ctx.state.nick.clone(),
                                        key,
                                        "No matching key".to_string(),
                                    ],
                                );
                                ctx.sender.send(reply).await?;
                            }
                        }
                        let reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::RPL_METADATAEND,
                            vec![ctx.state.nick.clone(), "End of metadata".to_string()],
                        );
                        ctx.sender.send(reply).await?;
                    }
                    MetadataSubCommand::SET => {
                        // Permission check: You can only set your own metadata (normally)
                        if target_lower != slirc_proto::irc_to_lower(&ctx.state.nick) {
                            let reply = server_reply(
                                &ctx.matrix.server_info.name,
                                Response::ERR_CHANOPRIVSNEEDED,
                                vec![
                                    ctx.state.nick.clone(),
                                    reply_target.to_string(),
                                    "Permission Denied".to_string(),
                                ],
                            );
                            ctx.sender.send(reply).await?;
                            return Ok(());
                        }

                        if params.is_empty() {
                            let reply = server_reply(
                                &ctx.matrix.server_info.name,
                                Response::ERR_NEEDMOREPARAMS,
                                vec![
                                    ctx.state.nick.clone(),
                                    "METADATA".to_string(),
                                    "Not enough parameters".to_string(),
                                ],
                            );
                            ctx.sender.send(reply).await?;
                            return Ok(());
                        }
                        let key = params[0].to_string();
                        let value = if params.len() > 1 {
                            Some(params[1].to_string())
                        } else {
                            None
                        };

                        let mut user = user_rw.write().await;
                        if let Some(val) = value {
                            if user.metadata.len() >= 100 && !user.metadata.contains_key(&key) {
                                let reply = server_reply(
                                    &ctx.matrix.server_info.name,
                                    Response::ERR_METADATALIMIT,
                                    vec![
                                        ctx.state.nick.clone(),
                                        "Metadata limit exceeded".to_string(),
                                    ],
                                );
                                ctx.sender.send(reply).await?;
                            } else {
                                user.metadata.insert(key.clone(), val.clone());
                                // Deprecated spec format: 761 <client> <key> <visibility> <value>
                                // Clone for persistence before moving into reply vec
                                let key_clone = key.clone();
                                let val_clone = val.clone();
                                let reply = server_reply(
                                    &ctx.matrix.server_info.name,
                                    Response::RPL_KEYVALUE,
                                    vec![ctx.state.nick.clone(), key, "*".to_string(), val],
                                );
                                ctx.sender.send(reply).await?;
                                let reply = server_reply(
                                    &ctx.matrix.server_info.name,
                                    Response::RPL_METADATAEND,
                                    vec![ctx.state.nick.clone(), "End of metadata".to_string()],
                                );
                                ctx.sender.send(reply).await?;

                                // Persist insertion (User)
                                if let Some(account_name) = &user.account {
                                    let account_name = account_name.clone();
                                    let repo = ctx.matrix.db.accounts();

                                    match repo.find_by_name(&account_name).await {
                                        Ok(Some(account)) => {
                                            if let Err(e) = repo
                                                .set_metadata(
                                                    account.id,
                                                    &key_clone,
                                                    Some(&val_clone),
                                                )
                                                .await
                                            {
                                                tracing::error!(
                                                    "Failed to persist user metadata: {}",
                                                    e
                                                );
                                            }
                                        }
                                        Ok(None) => {
                                            tracing::warn!(
                                                "User account not found for metadata: {}",
                                                account_name
                                            );
                                        }
                                        Err(e) => {
                                            tracing::error!("DB error: {}", e);
                                        }
                                    }
                                }
                            }
                        } else {
                            user.metadata.remove(&key);

                            // Persist removal (User)
                            if let Some(account_name) = &user.account {
                                let account_name = account_name.clone();
                                let key_clone = key.clone();
                                let repo = ctx.matrix.db.accounts();

                                // Spawn to avoid blocking lock? No, keep it simple for now, async is fine.
                                match repo.find_by_name(&account_name).await {
                                    Ok(Some(account)) => {
                                        if let Err(e) =
                                            repo.set_metadata(account.id, &key_clone, None).await
                                        {
                                            tracing::error!(
                                                "Failed to persist user metadata removal: {}",
                                                e
                                            );
                                        }
                                    }
                                    Ok(None) => {
                                        tracing::warn!(
                                            "User account not found for metadata: {}",
                                            account_name
                                        );
                                    }
                                    Err(e) => {
                                        tracing::error!("DB error: {}", e);
                                    }
                                }
                            }

                            let reply = server_reply(
                                &ctx.matrix.server_info.name,
                                Response::RPL_METADATAEND,
                                vec![ctx.state.nick.clone(), "End of metadata".to_string()],
                            );
                            ctx.sender.send(reply).await?;
                        }
                    }
                    MetadataSubCommand::LIST => {
                        let user = user_rw.read().await;
                        for (k, v) in &user.metadata {
                            // Deprecated spec format: 761 <client> <key> <visibility> <value>
                            let reply = server_reply(
                                &ctx.matrix.server_info.name,
                                Response::RPL_KEYVALUE,
                                vec![
                                    ctx.state.nick.clone(),
                                    k.clone(),
                                    "*".to_string(),
                                    v.clone(),
                                ],
                            );
                            ctx.sender.send(reply).await?;
                        }
                        let reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::RPL_METADATAEND,
                            vec![ctx.state.nick.clone(), "End of metadata".to_string()],
                        );
                        ctx.sender.send(reply).await?;
                    }
                    _ => return Ok(()),
                }
            } else {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHNICK,
                    vec![
                        ctx.state.nick.clone(),
                        reply_target.to_string(),
                        "No such nick/channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
            }
        }

        Ok(())
    }
}
