//! Nick enforcement background task.
//!
//! Monitors enforce_timers in the Matrix and force-renames users who
//! don't identify within the timeout period.

use crate::db::Database;
use crate::state::Matrix;
use rand::Rng;
use slirc_proto::{irc_to_lower, Command, Message, Prefix};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Spawn the nick enforcement background task.
///
/// This task runs every 5 seconds and checks for expired enforcement timers.
/// Users who haven't identified in time are renamed to Guest<random>.
pub fn spawn_enforcement_task(matrix: Arc<Matrix>, db: Database) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        
        loop {
            interval.tick().await;
            check_expired_timers(&matrix, &db).await;
        }
    });
}

/// Check for expired enforcement timers and force-rename affected users.
async fn check_expired_timers(matrix: &Arc<Matrix>, _db: &Database) {
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
        let (old_nick, username, hostname, channels) = {
            if let Some(user_ref) = matrix.users.get(&uid) {
                let user = user_ref.read().await;
                (
                    user.nick.clone(),
                    user.user.clone(),
                    user.host.clone(),
                    user.channels.iter().cloned().collect::<Vec<_>>(),
                )
            } else {
                debug!(uid = %uid, "User not found for enforcement (already disconnected?)");
                continue;
            }
        };

        // Generate a unique guest nick
        let new_nick = generate_guest_nick(matrix).await;
        let old_lower = irc_to_lower(&old_nick);
        let new_lower = irc_to_lower(&new_nick);

        info!(
            uid = %uid,
            old_nick = %old_nick,
            new_nick = %new_nick,
            "Nick enforcement: forcing nick change"
        );

        // Build NICK message
        let nick_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(old_nick.clone(), username, hostname)),
            command: Command::NICK(new_nick.clone()),
        };

        // Update nick mapping
        matrix.nicks.remove(&old_lower);
        matrix.nicks.insert(new_lower, uid.clone());

        // Update user's nick
        if let Some(user_ref) = matrix.users.get(&uid) {
            let mut user = user_ref.write().await;
            user.nick = new_nick.clone();
        }

        // Broadcast NICK change to all channels the user is in
        for channel_name in &channels {
            matrix.broadcast_to_channel(channel_name, nick_msg.clone(), None).await;
        }

        // Also send to the user themselves
        if let Some(sender) = matrix.senders.get(&uid) {
            let _ = sender.send(nick_msg).await;
        }

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
        if let Some(sender) = matrix.senders.get(&uid) {
            let _ = sender.send(notice).await;
        }
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
