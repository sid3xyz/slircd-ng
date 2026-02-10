use crate::handlers::core::traits::ServerHandler;
use crate::handlers::{Context, HandlerResult};
use async_trait::async_trait;
use slirc_proto::MessageRef;
use tracing::instrument;

#[derive(Default, Debug)]
pub struct KlineHandler;

#[derive(Default, Debug)]
pub struct UnklineHandler;

#[async_trait]
impl ServerHandler for KlineHandler {
    #[instrument(skip(ctx, msg), fields(command = "KLINE"))]
    async fn handle(
        &self,
        ctx: &mut Context<'_, crate::state::ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // KLINE <timestamp> <duration> <mask-user> <mask-host> :<reason>
        // OR TS6 variants.
        // For simplicity, we assume TS6 style: KLINE <mask-user> <mask-host> <duration> :<reason>
        // But the common s2s protocol (Charybdis/solanum) usually sends TKL (Token K-Line) or KLINE-with-TS.
        // Let's implement the standard defined in slirc-proto Command::KLINE if possible, or RAW.
        //
        // Check arguments.
        // KLINE arguments are variable based on specific IRCd flavor, but typically:
        // Arg 0: Mask
        // Arg 1: Duration (seconds)
        // Arg 2: Reason
        // Sometimes Timestamp is implicit or prepended.

        let mask = match msg.arg(0) {
            Some(m) => m,
            None => return Ok(()),
        };

        let duration_str = msg.arg(1).unwrap_or("0");
        let reason = msg.arg(2).unwrap_or("No reason");

        let duration: u64 = duration_str.parse().unwrap_or(0);

        tracing::info!(%mask, %duration, %reason, "Received propagated K-Line");

        // Apply to local ban cache
        // Note: For a real KLINE, we combine user/host. The mask might be "user@host".
        // If it's just "host", we treat it as "*@host".

        // Parse mask properly. If no '@', assume it is a host mask and prepend "*@"
        let normalized_mask = if mask.contains('@') {
            mask.to_string()
        } else {
            format!("*@{}", mask)
        };

        ctx.matrix.security_manager.ban_cache.add_gline(
            normalized_mask,
            reason.to_string(),
            Some(duration as i64),
        );

        Ok(())
    }
}

#[async_trait]
impl ServerHandler for UnklineHandler {
    #[instrument(skip(ctx, msg), fields(command = "UNKLINE"))]
    async fn handle(
        &self,
        ctx: &mut Context<'_, crate::state::ServerState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // UNKLINE <mask-user> <mask-host>
        // or UNKLINE <mask>

        let mask = match msg.arg(0) {
            Some(m) => m,
            None => return Ok(()),
        };

        tracing::info!(%mask, "Received propagated UnK-Line");
        ctx.matrix.security_manager.ban_cache.remove_gline(mask);

        Ok(())
    }
}
