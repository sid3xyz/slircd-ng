//! NickServ command handlers.

pub mod cert;
pub mod drop;
pub mod ghost;
pub mod group;
pub mod identify;
pub mod info;
pub mod register;
pub mod set;
pub mod ungroup;

use crate::db::Database;
use crate::services::base::ServiceBase;
use crate::services::{Service, ServiceEffect};
use crate::state::Matrix;
use async_trait::async_trait;
use std::sync::Arc;

/// Result of a NickServ command - a list of effects to apply.
pub type NickServResult = Vec<ServiceEffect>;

/// NickServ service.
pub struct NickServ {
    db: Database,
}

impl ServiceBase for NickServ {
    fn service_name(&self) -> &'static str {
        "NickServ"
    }

    fn db(&self) -> &Database {
        &self.db
    }
}

#[async_trait]
impl Service for NickServ {
    fn name(&self) -> &'static str {
        "NickServ"
    }

    fn aliases(&self) -> Vec<&'static str> {
        vec!["NS"]
    }

    async fn handle(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        text: &str,
    ) -> Vec<ServiceEffect> {
        self.handle_command(matrix, uid, nick, text).await
    }
}

impl NickServ {
    /// Create a new NickServ service.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Check if an account with the given name exists.
    pub async fn account_exists(&self, name: &str) -> bool {
        matches!(self.db.accounts().find_by_name(name).await, Ok(Some(_)))
    }

    /// Handle a PRIVMSG to NickServ.
    /// Returns a list of effects that the caller should apply.
    pub async fn handle_command(
        &self,
        matrix: &Arc<Matrix>,
        uid: &str,
        nick: &str,
        text: &str,
    ) -> NickServResult {
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.is_empty() {
            return self.help_reply(uid);
        }

        let command = parts[0].to_uppercase();
        let args = &parts[1..];

        match command.as_str() {
            "REGISTER" => {
                register::handle_register(
                    &self.db,
                    uid,
                    nick,
                    args,
                    |u, t| self.reply_effect(u, t),
                    |u, ts| self.reply_effects(u, ts),
                )
                .await
            }
            "IDENTIFY" => {
                identify::handle_identify(
                    &self.db,
                    uid,
                    nick,
                    args,
                    |u, t| self.reply_effect(u, t),
                    |u, ts| self.reply_effects(u, ts),
                )
                .await
            }
            "DROP" => {
                drop::handle_drop(
                    &self.db,
                    uid,
                    nick,
                    args,
                    |u, t| self.reply_effect(u, t),
                    |u, ts| self.reply_effects(u, ts),
                )
                .await
            }
            "GROUP" => {
                group::handle_group(
                    &self.db,
                    uid,
                    nick,
                    args,
                    |u, t| self.reply_effect(u, t),
                    |u, ts| self.reply_effects(u, ts),
                )
                .await
            }
            "UNGROUP" => {
                ungroup::handle_ungroup(&self.db, matrix, uid, args, |u, ts| {
                    self.reply_effects(u, ts)
                })
                .await
            }
            "GHOST" => {
                ghost::handle_ghost(
                    &self.db,
                    matrix,
                    uid,
                    nick,
                    args,
                    |u, t| self.reply_effect(u, t),
                    |u, ts| self.reply_effects(u, ts),
                )
                .await
            }
            "INFO" => {
                info::handle_info(
                    &self.db,
                    uid,
                    args,
                    |u, t| self.reply_effect(u, t),
                    |u, ts| self.reply_effects(u, ts),
                )
                .await
            }
            "SET" => {
                set::handle_set(
                    &self.db,
                    matrix,
                    uid,
                    args,
                    |u, t| self.reply_effect(u, t),
                    |u, ts| self.reply_effects(u, ts),
                )
                .await
            }
            "CERT" => {
                cert::handle_cert(
                    &self.db,
                    matrix,
                    uid,
                    args,
                    |u, t| self.reply_effect(u, t),
                    |u, ts| self.reply_effects(u, ts),
                )
                .await
            }
            "HELP" => self.help_reply(uid),
            _ => self.unknown_command(uid, &command),
        }
    }

    // ========== Reply helper methods (delegate to trait) ==========

    /// Create a single reply effect.
    fn reply_effect(&self, target_uid: &str, text: &str) -> ServiceEffect {
        <Self as ServiceBase>::reply_effect(self, target_uid, text)
    }

    /// Create multiple reply effects.
    fn reply_effects(&self, target_uid: &str, texts: Vec<&str>) -> NickServResult {
        <Self as ServiceBase>::reply_effects(self, target_uid, texts)
    }

    /// Create a help reply.
    fn help_reply(&self, uid: &str) -> NickServResult {
        vec![
            self.reply_effect(
                uid,
                "NickServ allows you to register and protect your nickname.",
            ),
            self.reply_effect(uid, "Commands:"),
            self.reply_effect(
                uid,
                "  \x02REGISTER\x02 <password> [email] - Register your nickname",
            ),
            self.reply_effect(
                uid,
                "  \x02IDENTIFY\x02 <password>         - Identify to your account",
            ),
            self.reply_effect(
                uid,
                "  \x02DROP\x02 <password>             - Delete your account",
            ),
            self.reply_effect(
                uid,
                "  \x02GROUP\x02 <account> <password>  - Link nick to account",
            ),
            self.reply_effect(
                uid,
                "  \x02UNGROUP\x02 <nick>              - Remove nick from account",
            ),
            self.reply_effect(
                uid,
                "  \x02GHOST\x02 <nick> [password]     - Kill session using your nick",
            ),
            self.reply_effect(
                uid,
                "  \x02INFO\x02 <nick>                 - Show account information",
            ),
            self.reply_effect(
                uid,
                "  \x02SET\x02 <option> <value>        - Configure account settings",
            ),
            self.reply_effect(
                uid,
                "  \x02CERT\x02 <ADD|DEL|LIST>         - Manage TLS certificate",
            ),
            self.reply_effect(
                uid,
                "  \x02HELP\x02                        - Show this help",
            ),
        ]
    }
}
