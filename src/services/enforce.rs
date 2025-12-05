//! Nick enforcement background task.
//!
//! Monitors enforce_timers in the Matrix and force-renames users who
//! don't identify within the timeout period.

use crate::handlers::ResponseMiddleware;
use crate::services::{ServiceEffect, apply_effect};
use crate::state::Matrix;
use rand::Rng;
use slirc_proto::{Command, Message, Prefix, irc_to_lower};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Spawn the nick enforcement background task.
///
/// This task runs every 5 seconds and checks for expired enforcement timers.
/// Users who haven't identified in time are renamed to Guest<random>.
pub fn spawn_enforcement_task(matrix: Arc<Matrix>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));

        loop {
            interval.tick().await;
            check_expired_timers(&matrix).await;
        }
    });
}

/// Check for expired enforcement timers and force-rename affected users.
async fn check_expired_timers(matrix: &Arc<Matrix>) {
    let now = Instant::now();
    let mut expired: Vec<String> = Vec::new();

    // Collect expired UIDs (keeping lock short)
    for entry in matrix.enforce_timers.iter() {
        let uid = entry.key();
        let deadline = entry.value();
        if now >= *deadline {
            expired.push(uid.clone());
        }
    }

    // Process each expired user
    for uid in expired {
        // Remove the timer first
        matrix.enforce_timers.remove(&uid);

        // Get user info
        let old_nick = {
            if let Some(user_ref) = matrix.users.get(&uid) {
                let user = user_ref.read().await;
                user.nick.clone()
            } else {
                debug!(uid = %uid, "User not found for enforcement (already disconnected?)");
                continue;
            }
        };

        // Generate a unique guest nick
        let new_nick = generate_guest_nick(matrix).await;

        info!(
            uid = %uid,
            old_nick = %old_nick,
            new_nick = %new_nick,
            "Nick enforcement: forcing nick change"
        );

        // Get sender for the user
        let sender = if let Some(s) = matrix.senders.get(&uid) {
            s.clone()
        } else {
            debug!(uid = %uid, "No sender found for user, cannot send enforcement messages");
            continue;
        };

        let sender_middleware = ResponseMiddleware::Direct(&sender);

        // Apply the forced nick change using centralized effect
        let effect = ServiceEffect::ForceNick {
            target_uid: uid.clone(),
            old_nick: old_nick.clone(),
            new_nick: new_nick.clone(),
        };
        apply_effect(matrix, &old_nick, &sender_middleware, effect).await;

        // Send notice to user explaining what happened
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                "NickServ".to_string(),
                "NickServ".to_string(),
                "services.".to_string(),
            )),
            command: Command::NOTICE(
                new_nick.clone(),
                format!(
                    "Your nickname has been changed to \x02{}\x02 because you did not identify in time.",
                    new_nick
                ),
            ),
        };
        let _ = sender.send(notice).await;
    }
}

/// Generate a unique guest nickname (Guest + 5 random digits).
async fn generate_guest_nick(matrix: &Arc<Matrix>) -> String {
    let mut rng = rand::thread_rng();

    loop {
        let num: u32 = rng.gen_range(10000..100000);
        let nick = format!("Guest{}", num);
        let nick_lower = irc_to_lower(&nick);

        // Check if this nick is already in use
        if !matrix.nicks.contains_key(&nick_lower) {
            return nick;
        }
        // If taken, loop and try again
    }
}
