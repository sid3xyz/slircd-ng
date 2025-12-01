//! NickServ - Nickname registration and identification service.
//!
//! Handles:
//! - REGISTER <password> [email] - Register current nick
//! - IDENTIFY <password> - Identify to account
//! - GHOST <nick> - Kill session using your nick
//! - INFO <nick> - Show account information
//! - SET <option> <value> - Configure account settings

mod commands;

use crate::db::Database;
use crate::services::apply_effects;
use crate::state::Matrix;
use slirc_proto::{Message, irc_to_lower};
use std::sync::Arc;
use tokio::sync::mpsc;

pub use commands::NickServ;

/// Handle service message routing.
/// Applies all effects returned by NickServ.
pub async fn route_service_message(
    matrix: &Arc<Matrix>,
    db: &Database,
    uid: &str,
    nick: &str,
    target: &str,
    text: &str,
    sender: &mpsc::Sender<Message>,
) -> bool {
    let target_lower = irc_to_lower(target);

    if target_lower == "nickserv" || target_lower == "ns" {
        let nickserv = NickServ::new(db.clone());
        let effects = nickserv.handle(matrix, uid, nick, text).await;

        // Apply all effects using centralized function
        apply_effects(matrix, nick, sender, effects).await;

        true
    } else {
        false
    }
}
