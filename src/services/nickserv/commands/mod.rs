//! NickServ command handlers.

pub mod register;
pub mod identify;
pub mod drop;
pub mod group;
pub mod ungroup;
pub mod ghost;
pub mod info;
pub mod set;

use crate::db::Database;
use crate::services::ServiceEffect;
use crate::state::Matrix;
use slirc_proto::{Command, Message, Prefix};
use std::sync::Arc;

/// Result of a NickServ command - a list of effects to apply.
pub type NickServResult = Vec<ServiceEffect>;

/// NickServ service.
pub struct NickServ {
    db: Database,
}

impl NickServ {
    /// Create a new NickServ service.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Handle a PRIVMSG to NickServ.
    /// Returns a list of effects that the caller should apply.
    pub async fn handle(
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
            "HELP" => self.help_reply(uid),
            _ => self.unknown_command(uid, &command),
        }
    }

    /// Create a single reply effect.
    fn reply_effect(&self, target_uid: &str, text: &str) -> ServiceEffect {
        ServiceEffect::Reply {
            target_uid: target_uid.to_string(),
            msg: Message {
                tags: None,
                prefix: Some(Prefix::Nickname(
                    "NickServ".to_string(),
                    "NickServ".to_string(),
                    "services.".to_string(),
                )),
                command: Command::NOTICE(String::new(), text.to_string()),
            },
        }
    }

    /// Create multiple reply effects.
    fn reply_effects(&self, target_uid: &str, texts: Vec<&str>) -> NickServResult {
        texts
            .into_iter()
            .map(|t| self.reply_effect(target_uid, t))
            .collect()
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
                "  \x02HELP\x02                        - Show this help",
            ),
        ]
    }

    /// Create an unknown command reply.
    fn unknown_command(&self, uid: &str, cmd: &str) -> NickServResult {
        self.reply_effects(
            uid,
            vec![&format!(
                "Unknown command \x02{}\x02. Type \x02HELP\x02 for a list of commands.",
                cmd
            )],
        )
    }
}
