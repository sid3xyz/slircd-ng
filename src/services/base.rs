//! Base trait for IRC services (NickServ, ChanServ, etc.).
//!
//! Provides common functionality for service reply handling, account verification,
//! and error handling to eliminate code duplication across services.

use crate::db::Database;
use crate::state::Matrix;
use slirc_proto::{Command, Message, Prefix};
use std::sync::Arc;

use super::ServiceEffect;

/// Result type for service commands - a list of effects to apply.
pub type ServiceResult = Vec<ServiceEffect>;

/// Base trait for IRC services providing common reply and validation functionality.
pub trait ServiceBase {
    /// Get the service name (e.g., "NickServ", "ChanServ").
    fn service_name(&self) -> &'static str;

    /// Get the database handle.
    fn db(&self) -> &Database;

    /// Create a single reply effect (NOTICE to user).
    fn reply_effect(&self, target_uid: &str, text: &str) -> ServiceEffect {
        ServiceEffect::Reply {
            target_uid: target_uid.to_string(),
            msg: Message {
                tags: None,
                prefix: Some(Prefix::ServerName(self.service_name().to_string())),
                command: Command::NOTICE("*".to_string(), text.to_string()),
            },
        }
    }

    /// Create multiple reply effects.
    fn reply_effects(&self, target_uid: &str, texts: Vec<&str>) -> ServiceResult {
        texts
            .into_iter()
            .map(|t| self.reply_effect(target_uid, t))
            .collect()
    }

    /// Create an error reply (single message).
    fn error_reply(&self, uid: &str, text: &str) -> ServiceResult {
        vec![self.reply_effect(uid, text)]
    }

    /// Create an unknown command reply.
    fn unknown_command(&self, uid: &str, cmd: &str) -> ServiceResult {
        self.error_reply(
            uid,
            &format!(
                "Unknown command: \x02{}\x02. Use \x02HELP\x02 for a list of commands.",
                cmd
            ),
        )
    }

    /// Get user's account ID if identified.
    ///
    /// Returns None if user is not found, not registered, or not identified.
    fn get_user_account_id(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
    ) -> impl std::future::Future<Output = Option<i64>> + Send
    where
        Self: Sync,
    {
        async move {
            let user_arc = matrix
                .user_manager
                .users
                .get(uid)
                .map(|u| u.value().clone())?;

            // Fast path: Check cache with read lock
            {
                let user = user_arc.read().await;
                if let Some(id) = user.account_id {
                    return Some(id);
                }
                if !user.modes.registered || user.account.is_none() {
                    return None;
                }
            }

            // Slow path: Need lookup
            // We need to clone account name to release lock before await
            let account_name = {
                let user = user_arc.read().await;
                user.account.clone()?
            };

            // Look up account ID
            let account_id = match self.db().accounts().find_by_name(&account_name).await {
                Ok(Some(account)) => account.id,
                _ => return None,
            };

            // Cache the result
            // Re-acquire lock (write) to update cache
            {
                let mut user = user_arc.write().await;
                // Verify account name hasn't changed while we were looking up
                if let Some(current_account) = &user.account {
                    if current_account == &account_name {
                        user.account_id = Some(account_id);
                    }
                }
            }

            Some(account_id)
        }
    }
}
