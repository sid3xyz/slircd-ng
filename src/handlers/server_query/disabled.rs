//! Handlers for disabled commands (SUMMON, USERS).

use super::super::{Context, HandlerResult, UniversalHandler};
use crate::state::SessionState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for SUMMON command (disabled).
pub struct SummonHandler;

#[async_trait]
impl<S: SessionState> UniversalHandler<S> for SummonHandler {
    async fn handle(&self, ctx: &mut Context<'_, S>, _msg: &MessageRef<'_>) -> HandlerResult {
        let nick = ctx.state.nick_or_star();
        let reply = Response::err_summondisabled(nick).with_prefix(ctx.server_prefix());
        ctx.sender.send(reply).await?;
        Ok(())
    }
}

/// Handler for USERS command (disabled).
pub struct UsersHandler;

#[async_trait]
impl<S: SessionState> UniversalHandler<S> for UsersHandler {
    async fn handle(&self, ctx: &mut Context<'_, S>, _msg: &MessageRef<'_>) -> HandlerResult {
        let nick = ctx.state.nick_or_star();
        let reply = Response::err_usersdisabled(nick).with_prefix(ctx.server_prefix());
        ctx.sender.send(reply).await?;
        Ok(())
    }
}
