//! MAP command handler.
//!
//! `MAP`
//!
//! Returns the server map (network topology). In a single-server setup,
//! this just shows the current server.

use crate::handlers::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for MAP command.
pub struct MapHandler;

#[async_trait]
impl PostRegHandler for MapHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = ctx.server_name();
        let nick = &ctx.state.nick;

        let user_count = ctx.matrix.user_manager.users.len();

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
