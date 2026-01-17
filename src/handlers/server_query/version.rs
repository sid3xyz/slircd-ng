//! VERSION command handler.
//!
//! `VERSION [target]`
//!
//! Returns the version of the server.

use crate::handlers::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Server version string.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Handler for VERSION command.
pub struct VersionHandler;

#[async_trait]
impl PostRegHandler for VersionHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = ctx.server_name();
        let nick = ctx.nick();

        // RPL_VERSION (351): <version>.<debuglevel> <server> :<comments>
        #[cfg(debug_assertions)]
        let version_str = format!("{}-debug.1", VERSION);
        #[cfg(not(debug_assertions))]
        let version_str = format!("{}.0", VERSION);

        ctx.send_reply(
            Response::RPL_VERSION,
            vec![
                nick.to_string(),
                version_str,
                server_name.to_string(),
                "slircd-ng IRC daemon".to_string(),
            ],
        )
        .await?;

        Ok(())
    }
}
