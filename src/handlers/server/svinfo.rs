use crate::handlers::core::traits::PreRegHandler;
use crate::handlers::{Context, HandlerError, HandlerResult};
use crate::state::UnregisteredState;
use async_trait::async_trait;
use slirc_proto::MessageRef;
use tracing::info;

pub struct SvinfoHandler;

#[async_trait]
impl PreRegHandler for SvinfoHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // SVINFO <ts6_ver> <min_ver> 0 :<current_time>
        let v = msg
            .arg(0)
            .and_then(|s| s.parse().ok())
            .ok_or(HandlerError::NeedMoreParams)?;
        let m = msg
            .arg(1)
            .and_then(|s| s.parse().ok())
            .ok_or(HandlerError::NeedMoreParams)?;
        let z = msg
            .arg(2)
            .and_then(|s| s.parse().ok())
            .ok_or(HandlerError::NeedMoreParams)?;
        let t = msg
            .arg(3)
            .and_then(|s| s.parse().ok())
            .ok_or(HandlerError::NeedMoreParams)?;

        info!(v, m, z, t, "Received SVINFO");
        ctx.state.server_svinfo = Some((v, m, z, t));

        Ok(())
    }
}
