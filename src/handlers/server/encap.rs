use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::ServerState;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef};
use slirc_crdt::clock::ServerId;
use std::sync::Arc;
use tracing::{debug, warn};

/// Handler for the ENCAP command (Encapsulated Command).
///
/// ENCAP is used to send commands that may not be understood by all servers
/// in a network, allowing for protocol extensions without breaking compatibility.
///
/// Format: `:<source> ENCAP <target> <subcommand> [args...]`
///
/// - `<target>` can be `*` (broadcast to all) or a specific server SID/name
/// - `<subcommand>` is the encapsulated command (e.g., `CHGHOST`, `REALHOST`, etc.)
pub struct EncapHandler;

#[async_trait]
impl ServerHandler for EncapHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let target = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let subcommand = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        let source = msg
            .prefix
            .as_ref()
            .map(|p| p.raw.to_string())
            .unwrap_or_else(|| ctx.state.sid.clone());

        debug!(
            source = %source,
            target = %target,
            subcommand = %subcommand,
            "Received ENCAP"
        );

        // Check if this ENCAP is targeted at us
        let is_for_us = target == "*"
            || target == ctx.matrix.server_info.sid.as_str()
            || target == ctx.matrix.server_info.name;

        if is_for_us {
            // Process the encapsulated command
            match subcommand.to_uppercase().as_str() {
                "CHGHOST" => {
                    // ENCAP * CHGHOST <uid> <new_host>
                    if let (Some(uid), Some(new_host)) = (msg.arg(2), msg.arg(3))
                        && let Some(user_arc) = ctx.matrix.user_manager.users.get(uid)
                    {
                        let mut user = user_arc.write().await;
                        user.visible_host = new_host.to_string();
                        debug!(uid = %uid, new_host = %new_host, "Applied CHGHOST");
                    }
                }
                "REALHOST" => {
                    // ENCAP * REALHOST <uid> <real_host>
                    if let (Some(uid), Some(real_host)) = (msg.arg(2), msg.arg(3))
                        && let Some(user_arc) = ctx.matrix.user_manager.users.get(uid)
                    {
                        let mut user = user_arc.write().await;
                        user.host = real_host.to_string();
                        debug!(uid = %uid, real_host = %real_host, "Applied REALHOST");
                    }
                }
                "LOGIN" => {
                    // ENCAP * LOGIN <uid> <account>
                    if let (Some(uid), Some(account)) = (msg.arg(2), msg.arg(3))
                        && let Some(user_arc) = ctx.matrix.user_manager.users.get(uid)
                    {
                        let mut user = user_arc.write().await;
                        user.account = Some(account.to_string());
                        debug!(uid = %uid, account = %account, "Applied LOGIN");
                    }
                }
                "CERTFP" => {
                    // ENCAP * CERTFP <uid> <fingerprint>
                    if let (Some(uid), Some(fp)) = (msg.arg(2), msg.arg(3))
                        && let Some(user_arc) = ctx.matrix.user_manager.users.get(uid)
                    {
                        let mut user = user_arc.write().await;
                        user.certfp = Some(fp.to_string());
                        debug!(uid = %uid, certfp = %fp, "Applied CERTFP");
                    }
                }
                _ => {
                    // Unknown subcommand - log and continue
                    warn!(subcommand = %subcommand, "Unknown ENCAP subcommand");
                }
            }
        }

        // Propagate to other servers if target is broadcast or not us
        if target == "*" {
            let source_sid = ServerId::new(ctx.state.sid.clone());

            // Reconstruct the ENCAP message for propagation
            let mut args: Vec<String> = vec![target.to_string(), subcommand.to_string()];
            let mut idx = 2;
            while let Some(arg) = msg.arg(idx) {
                args.push(arg.to_string());
                idx += 1;
            }

            let encap_msg = Message {
                tags: None,
                prefix: Some(slirc_proto::Prefix::new_from_str(&source)),
                command: Command::Raw("ENCAP".to_string(), args),
            };

            ctx.matrix
                .sync_manager
                .broadcast(Arc::new(encap_msg), Some(&source_sid))
                .await;
        }

        Ok(())
    }
}
