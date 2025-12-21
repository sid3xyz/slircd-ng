//! CAP command handler for IRCv3 capability negotiation.
//!
//! Implements CAP LS, LIST, REQ, ACK, NAK, END subcommands.
//! Reference: <https://ircv3.net/specs/extensions/capability-negotiation>
//!
//! # Security: Credential Handling
//!
//! SASL credentials are handled with care:
//! - Password data uses `SecureString` which is zeroized on drop
//! - SASL buffers are cleared after processing

mod helpers;
mod sasl;
mod subcommands;
mod types;

pub use sasl::AuthenticateHandler;
pub use types::SaslState;

use crate::handlers::{Context, HandlerResult, UniversalHandler};
use crate::state::SessionState;
use async_trait::async_trait;
use slirc_proto::{CapSubCommand, MessageRef, Response};
use subcommands::{handle_end, handle_list, handle_ls, handle_req};
use tracing::debug;

/// Handler for CAP command.
pub struct CapHandler;

#[async_trait]
impl<S: SessionState> UniversalHandler<S> for CapHandler {
    async fn handle(&self, ctx: &mut Context<'_, S>, msg: &MessageRef<'_>) -> HandlerResult {
        // CAP can be used before and after registration
        // CAP <subcommand> [arg]
        let subcommand_str = msg.arg(0).unwrap_or("");
        let arg = msg.arg(1);

        // Get nick using SessionState trait
        let nick = ctx.state.nick_or_star().to_string();

        // Parse subcommand using slirc-proto's FromStr implementation
        let subcommand: CapSubCommand = match subcommand_str.parse() {
            Ok(cmd) => cmd,
            Err(_) => {
                // Send ERR_INVALIDCAPCMD (410) for unknown subcommand
                let reply = Response::err_invalidcapcmd(&nick, subcommand_str)
                    .with_prefix(ctx.server_prefix());
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        match subcommand {
            CapSubCommand::LS => handle_ls(ctx, &nick, arg).await,
            CapSubCommand::LIST => handle_list(ctx, &nick).await,
            CapSubCommand::REQ => handle_req(ctx, &nick, arg).await,
            CapSubCommand::END => handle_end(ctx, &nick).await,
            _ => {
                // ACK, NAK, NEW, DEL are server-to-client only
                debug!(subcommand = ?subcommand, "Ignoring client->server CAP subcommand");
                Ok(())
            }
        }
    }
}
