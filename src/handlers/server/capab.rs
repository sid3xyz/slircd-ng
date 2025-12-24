use crate::handlers::core::traits::PreRegHandler;
use crate::handlers::{Context, HandlerResult};
use crate::state::UnregisteredState;
use async_trait::async_trait;
use slirc_proto::MessageRef;
use tracing::info;

pub struct CapabHandler;

#[async_trait]
impl PreRegHandler for CapabHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // CAPAB [capabilities]
        // Arguments are variable.
        let caps: Vec<String> = msg.args().iter().map(|s| s.to_string()).collect();

        info!(caps = ?caps, "Received CAPAB");
        ctx.state.server_capab = Some(caps);

        Ok(())
    }
}
