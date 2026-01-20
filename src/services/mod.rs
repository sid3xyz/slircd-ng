//! IRC services module.
//!
//! Provides virtual services like NickServ and ChanServ.

pub mod base;
pub mod chanserv;
pub mod effect;
pub mod enforce;
pub mod nickserv;
pub mod playback;
pub mod traits;

pub use effect::{ServiceEffect, apply_effect, apply_effects, apply_effects_no_sender};
pub use traits::Service;

use crate::{handlers::ResponseMiddleware, state::Matrix};
use slirc_proto::irc_to_lower;
use std::sync::Arc;

/// Unified service message router.
///
/// Routes PRIVMSG/SQUERY to NickServ or ChanServ based on target.
/// Returns true if the message was handled by a service.
///
/// Services are singletons stored in Matrix, created once at server startup.
pub async fn route_service_message(
    matrix: &Arc<Matrix>,
    uid: &str,
    nick: &str,
    target: &str,
    text: &str,
    sender: &ResponseMiddleware<'_>,
) -> bool {
    let target_lower = irc_to_lower(target);

    // Check core services first
    if target_lower == "nickserv" || target_lower == "ns" {
        let effects = matrix
            .service_manager
            .nickserv
            .handle_command(matrix, uid, nick, text)
            .await;
        apply_effects(matrix, nick, sender, effects).await;
        return true;
    }

    if target_lower == "chanserv" || target_lower == "cs" {
        let effects = matrix
            .service_manager
            .chanserv
            .handle_command(matrix, uid, nick, text)
            .await;
        apply_effects(matrix, nick, sender, effects).await;
        return true;
    }

    // Check extra services
    // We iterate because we need to check aliases too.
    for service in matrix.service_manager.extra_services.values() {
        if irc_to_lower(service.name()) == target_lower
            || service
                .aliases()
                .iter()
                .any(|a| irc_to_lower(a) == target_lower)
        {
            let effects = service.handle(matrix, uid, nick, text).await;
            apply_effects(matrix, nick, sender, effects).await;
            return true;
        }
    }

    false
}
