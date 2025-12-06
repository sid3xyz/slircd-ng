//! Base trait for IRC services (NickServ, ChanServ, etc.).
//!
//! Provides common functionality for service reply handling, account verification,
//! and error handling to eliminate code duplication across services.

use crate::db::{Database, DbError};
use crate::state::Matrix;
use slirc_proto::{Command, Message, Prefix};
use std::sync::Arc;
use tracing::warn;

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
    fn get_user_account_id(&self, matrix: &Arc<Matrix>, uid: &str) -> impl std::future::Future<Output = Option<i64>> + Send
    where
        Self: Sync,
    {
        async move {
            let user = matrix.users.get(uid)?;
            let user = user.read().await;

            if !user.modes.registered {
                return None;
            }

            let account_name = user.account.as_ref()?;

            // Look up account ID
            match self.db().accounts().find_by_name(account_name).await {
                Ok(Some(account)) => Some(account.id),
                _ => None,
            }
        }
    }

    /// Require user to be identified, returning account ID or error.
    #[allow(dead_code)]
    fn require_identified(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
    ) -> impl std::future::Future<Output = Result<i64, ServiceResult>> + Send
    where
        Self: Sync,
    {
        async move {
            match self.get_user_account_id(matrix, uid).await {
                Some(id) => Ok(id),
                None => Err(self.error_reply(
                    uid,
                    "You must be identified to an account to use this command.",
                )),
            }
        }
    }

    /// Handle database errors with consistent user-friendly messages.
    ///
    /// Logs the error and returns appropriate error reply to user.
    #[allow(dead_code)]
    fn handle_db_error(&self, uid: &str, error: DbError, operation: &str) -> ServiceResult {
        match error {
            DbError::AccountNotFound(name) => {
                self.error_reply(uid, &format!("Account \x02{}\x02 not found.", name))
            }
            DbError::AccountExists(name) => self.error_reply(
                uid,
                &format!("An account named \x02{}\x02 already exists.", name),
            ),
            DbError::NicknameRegistered(name) => self.error_reply(
                uid,
                &format!("The nickname \x02{}\x02 is already registered.", name),
            ),
            DbError::ChannelExists(name) => self.error_reply(
                uid,
                &format!("Channel \x02{}\x02 is already registered.", name),
            ),
            DbError::ChannelNotFound(name) => {
                self.error_reply(uid, &format!("Channel \x02{}\x02 not found.", name))
            }
            DbError::InvalidPassword => self.error_reply(uid, "Invalid password."),
            _ => {
                warn!(
                    service = %self.service_name(),
                    error = ?error,
                    operation = %operation,
                    "{} operation failed",
                    self.service_name()
                );
                self.error_reply(uid, &format!("{} failed. Please try again later.", operation))
            }
        }
    }
}
