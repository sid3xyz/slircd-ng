//! REGISTER command handler for NickServ.

use super::NickServResult;
use crate::db::Database;
use crate::services::ServiceEffect;
use tracing::info;

/// Handle REGISTER command.
pub async fn handle_register(
    db: &Database,
    uid: &str,
    nick: &str,
    args: &[&str],
    reply_effect: impl Fn(&str, &str) -> ServiceEffect,
    reply_effects: impl Fn(&str, Vec<&str>) -> NickServResult,
) -> NickServResult {
    if args.is_empty() {
        return reply_effects(uid, vec!["Syntax: REGISTER <password> [email]"]);
    }

    let password = args[0];
    let email = args.get(1).copied();

    match db.accounts().register(nick, password, email).await {
        Ok(account) => {
            info!(nick = %nick, account = %account.name, "Account registered");
            vec![
                reply_effect(
                    uid,
                    &format!("Your nickname \x02{}\x02 has been registered.", nick),
                ),
                reply_effect(uid, "You are now identified to your account."),
                ServiceEffect::AccountIdentify {
                    target_uid: uid.to_string(),
                    account: account.name.clone(),
                    account_id: Some(account.id),
                    metadata: std::collections::HashMap::new(),
                },
                ServiceEffect::BroadcastAccount {
                    target_uid: uid.to_string(),
                    new_account: account.name,
                },
            ]
        }
        Err(crate::db::DbError::AccountExists(name)) => reply_effects(
            uid,
            vec![&format!(
                "An account named \x02{}\x02 already exists.",
                name
            )],
        ),
        Err(crate::db::DbError::NicknameRegistered(name)) => reply_effects(
            uid,
            vec![&format!(
                "The nickname \x02{}\x02 is already registered.",
                name
            )],
        ),
        Err(_e) => reply_effects(uid, vec!["Registration failed. Please try again later."]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::{Command, Message};
    use std::sync::Arc;
    use std::sync::Mutex;

    fn dummy_effect() -> ServiceEffect {
        ServiceEffect::Reply {
            target_uid: "dummy".to_string(),
            msg: Message {
                tags: None,
                prefix: None,
                command: Command::NOTICE("dummy".to_string(), "dummy".to_string()),
            },
        }
    }

    #[tokio::test]
    async fn test_register_success() {
        let db = Database::new(":memory:").await.unwrap();

        let replies = Arc::new(Mutex::new(Vec::new()));
        let replies_clone = replies.clone();

        let uid = "uid1";
        let nick = "TestUser";
        let args = vec!["password123", "test@example.com"];

        handle_register(
            &db,
            uid,
            nick,
            &args,
            |_, text| {
                replies_clone.lock().unwrap().push(text.to_string());
                dummy_effect()
            },
            |_, texts| {
                let mut guard = replies_clone.lock().unwrap();
                for t in texts {
                    guard.push(t.to_string());
                }
                vec![]
            },
        )
        .await;

        let r = replies.lock().unwrap();
        assert!(r.iter().any(|s| s.contains("has been registered")));

        // Verify in DB
        let account = db.accounts().find_by_name("TestUser").await.unwrap();
        assert!(account.is_some());
    }

    #[tokio::test]
    async fn test_register_duplicate_account() {
        let db = Database::new(":memory:").await.unwrap();
        db.accounts()
            .register("ExistingUser", "pass", None)
            .await
            .unwrap();

        let replies = Arc::new(Mutex::new(Vec::new()));
        let replies_clone = replies.clone();

        let uid = "uid2";
        let nick = "NewNick";
        let args = vec!["password", "email"];

        // Try to register with 'ExistingUser' nick
        handle_register(
            &db,
            uid,
            "ExistingUser",
            &args,
            |_, _| dummy_effect(),
            |_, texts| {
                let mut guard = replies_clone.lock().unwrap();
                for t in texts {
                    guard.push(t.to_string());
                }
                vec![]
            },
        )
        .await;

        let r = replies.lock().unwrap();
        assert!(r.iter().any(|s| s.contains("already exists")));
    }

    #[test]
    fn test_register_validation_syntax() {
        // Validation logic for empty args is handled in handle_register
    }
}
