//! RULES command handler.
//!
//! `RULES`
//!
//! Returns the server rules.

use crate::handlers::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for RULES command.
pub struct RulesHandler;

#[async_trait]
impl PostRegHandler for RulesHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = ctx.server_name();
        let nick = &ctx.state.nick;

        // RPL_RULESTART (232): :- <server> Server Rules -
        ctx.send_reply(
            Response::RPL_RULESTART,
            vec![nick.clone(), format!("- {} Server Rules -", server_name)],
        )
        .await?;

        // Server rules (could be loaded from config in the future)
        let rules = [
            "1. Be respectful to other users.",
            "2. No flooding or spamming.",
            "3. No unauthorized bots.",
            "4. Follow the network guidelines.",
            "5. Have fun!",
        ];

        // RPL_RULES (633): :- <rule>
        for rule in &rules {
            ctx.send_reply(
                Response::RPL_RULES,
                vec![nick.clone(), format!("- {}", rule)],
            )
            .await?;
        }

        // RPL_ENDOFRULES (634): :End of RULES command
        ctx.send_reply(
            Response::RPL_ENDOFRULES,
            vec![nick.clone(), "End of RULES command".to_string()],
        )
        .await?;

        Ok(())
    }
}
