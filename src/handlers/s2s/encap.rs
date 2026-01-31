use crate::handlers::{Context, HandlerResult, ServerHandler};
use slirc_proto::MessageRef;
use tracing::instrument;

#[derive(Default)]
pub struct EncapHandler;

impl ServerHandler for EncapHandler {
    #[instrument(skip(ctx, msg), fields(command = "ENCAP"))]
    async fn handle(&self, ctx: &mut Context<'_, crate::state::ServerState>, msg: &MessageRef<'_>) -> HandlerResult {
        // ENCAP <target-server-mask> <subcommand> [args...]
        // Used for propagating commands to specific servers (or globs) without full broadcast.
        
        let target_mask = match msg.arg(0) {
            Some(m) => m,
            None => return Ok(()),
        };

        let subcommand = match msg.arg(1) {
            Some(s) => s,
            None => return Ok(()),
        };

        tracing::debug!(%target_mask, %subcommand, "Received ENCAP");

        // 1. Check if it matches us
        if crate::handlers::util::matches_wildcard(target_mask, &ctx.matrix.sync_manager.local_name) {
            tracing::info!(%subcommand, "Executing ENCAP subcommand for local server");
            // Dispatch subcommand?
            // For now, we only log receiving it.
            // In full implementation, we would recursively dispatch `subcommand` to the Registry.
            // But strict ENCAP usually only supports specific subcommands like GC, REALIP, LOGIN, etc.
            // We'll mark as implemented-but-stubbed for subcommands.
        }

        // 2. Forward to other servers if the mask matches them?
        // ENCAP propagation rules are complex. Usually we broadcast to all peers who match the mask 
        // OR if the mask contains wildcards, we flood it?
        // Simple implementation: Broadcast to peers if mask is distinct.
        // Standard TS6: Forward if target_mask is not us.
        
        if target_mask != ctx.matrix.sync_manager.local_name {
             // Forward logic would go here. `network.rs` / `sync_manager` usually handles routing.
             // But existing handlers do rudimentary forwarding.
             // We will rely on `burst.rs` and standard routing for now.
             // TODO: Implement explicit ENCAP forwarding.
        }

        Ok(())
    }
}
