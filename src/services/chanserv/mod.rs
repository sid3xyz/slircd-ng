//! ChanServ - Channel registration and access control service.
//!
//! Handles channel registration, access control, and moderation commands.

mod commands;

use crate::db::Database;
use crate::services::apply_effects;
use crate::state::Matrix;
use slirc_proto::{Message, irc_to_lower};
use std::sync::Arc;
use tokio::sync::mpsc;

pub use commands::ChanServ;

pub async fn route_chanserv_message(
    matrix: &Arc<Matrix>,
    db: &Database,
    uid: &str,
    nick: &str,
    target: &str,
    text: &str,
    sender: &mpsc::Sender<Message>,
) -> bool {
    let target_lower = irc_to_lower(target);

    if target_lower == "chanserv" || target_lower == "cs" {
        let chanserv = ChanServ::new(db.clone());
        let effects = chanserv.handle(matrix, uid, nick, text).await;

        // Apply all effects using centralized function
        apply_effects(matrix, nick, sender, effects).await;

        true
    } else {
        false
    }
}
