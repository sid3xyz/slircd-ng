//! NickServ CERT command - Manage TLS certificate fingerprints for SASL EXTERNAL.
//!
//! Commands:
//! - CERT ADD       - Add your current certificate to your account
//! - CERT DEL       - Remove your certificate from your account
//! - CERT LIST      - Show your registered certificate (if any)

use crate::db::Database;
use crate::services::ServiceEffect;
use crate::state::Matrix;
use std::sync::Arc;

/// Handle the CERT command.
///
/// # Arguments
/// * `db` - Database connection
/// * `matrix` - Server state for looking up user info
/// * `uid` - User ID of the command sender
/// * `args` - Command arguments (subcommand)
/// * `reply` - Function to create a single reply effect
/// * `replies` - Function to create multiple reply effects
pub async fn handle_cert<F, M>(
    db: &Database,
    matrix: &Arc<Matrix>,
    uid: &str,
    args: &[&str],
    reply: F,
    replies: M,
) -> Vec<ServiceEffect>
where
    F: Fn(&str, &str) -> ServiceEffect,
    M: Fn(&str, Vec<&str>) -> Vec<ServiceEffect>,
{
    // User must be logged in
    let user_arc = matrix.users.get(uid).map(|u| u.clone());
    let Some(user_arc) = user_arc else {
        return vec![reply(uid, "You are not connected.")];
    };

    let user = user_arc.read().await;
    let Some(ref account_name) = user.account else {
        return vec![reply(
            uid,
            "You must be identified to an account to use this command.",
        )];
    };
    let account_name = account_name.clone();
    drop(user); // Release lock before async database call

    // Get account info
    let account = match db.accounts().find_by_name(&account_name).await {
        Ok(Some(acc)) => acc,
        Ok(None) => {
            return vec![reply(uid, "Your account was not found (internal error).")];
        }
        Err(e) => {
            return vec![reply(uid, &format!("Database error: {}", e))];
        }
    };

    if args.is_empty() {
        return replies(
            uid,
            vec![
                "Usage: \x02CERT\x02 <ADD|DEL|LIST>",
                "  \x02ADD\x02  - Register your current TLS certificate",
                "  \x02DEL\x02  - Remove your registered certificate",
                "  \x02LIST\x02 - Show your registered certificate",
            ],
        );
    }

    let subcommand = args[0].to_uppercase();

    match subcommand.as_str() {
        "ADD" => handle_cert_add(db, matrix, uid, &account.id, &reply).await,
        "DEL" | "DELETE" | "REMOVE" => {
            handle_cert_del(db, uid, &account.id, &account.name, &reply).await
        }
        "LIST" | "SHOW" => handle_cert_list(db, uid, &account.id, &reply).await,
        _ => replies(
            uid,
            vec![
                &format!("Unknown CERT subcommand: \x02{}\x02", subcommand),
                "Usage: \x02CERT\x02 <ADD|DEL|LIST>",
            ],
        ),
    }
}

/// Handle CERT ADD - register current TLS certificate.
async fn handle_cert_add<F>(
    db: &Database,
    matrix: &Arc<Matrix>,
    uid: &str,
    account_id: &i64,
    reply: &F,
) -> Vec<ServiceEffect>
where
    F: Fn(&str, &str) -> ServiceEffect,
{
    // Get user's current certfp from their connection
    let user_arc = matrix.users.get(uid).map(|u| u.clone());
    let Some(user_arc) = user_arc else {
        return vec![reply(uid, "You are not connected.")];
    };

    let user = user_arc.read().await;
    let Some(ref certfp) = user.certfp else {
        return vec![reply(
            uid,
            "You are not connected with a TLS client certificate.",
        )];
    };
    let certfp = certfp.clone();
    drop(user); // Release lock before async database call

    // Check if this certfp is already registered to another account
    match db.accounts().find_by_certfp(&certfp).await {
        Ok(Some(existing)) => {
            if existing.id == *account_id {
                return vec![reply(
                    uid,
                    "This certificate is already registered to your account.",
                )];
            } else {
                return vec![reply(
                    uid,
                    &format!(
                        "This certificate is already registered to account \x02{}\x02.",
                        existing.name
                    ),
                )];
            }
        }
        Ok(None) => {}
        Err(e) => {
            return vec![reply(uid, &format!("Database error: {}", e))];
        }
    }

    // Register the certificate
    if let Err(e) = db.accounts().set_certfp(*account_id, Some(&certfp)).await {
        return vec![reply(
            uid,
            &format!("Failed to register certificate: {}", e),
        )];
    }

    vec![
        reply(
            uid,
            &format!("Certificate fingerprint added: \x02{}\x02", certfp),
        ),
        reply(
            uid,
            "You can now use SASL EXTERNAL to authenticate automatically.",
        ),
    ]
}

/// Handle CERT DEL - remove registered certificate.
async fn handle_cert_del<F>(
    db: &Database,
    uid: &str,
    account_id: &i64,
    account_name: &str,
    reply: &F,
) -> Vec<ServiceEffect>
where
    F: Fn(&str, &str) -> ServiceEffect,
{
    // Check if account has a certificate
    match db.accounts().get_certfp(*account_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return vec![reply(
                uid,
                "You don't have a certificate registered to your account.",
            )];
        }
        Err(e) => {
            return vec![reply(uid, &format!("Database error: {}", e))];
        }
    }

    // Remove the certificate
    if let Err(e) = db.accounts().set_certfp(*account_id, None).await {
        return vec![reply(uid, &format!("Failed to remove certificate: {}", e))];
    }

    vec![reply(
        uid,
        &format!("Certificate removed from account \x02{}\x02.", account_name),
    )]
}

/// Handle CERT LIST - show registered certificate.
async fn handle_cert_list<F>(
    db: &Database,
    uid: &str,
    account_id: &i64,
    reply: &F,
) -> Vec<ServiceEffect>
where
    F: Fn(&str, &str) -> ServiceEffect,
{
    match db.accounts().get_certfp(*account_id).await {
        Ok(Some(certfp)) => vec![
            reply(uid, "Your registered certificate fingerprint:"),
            reply(uid, &format!("  \x02{}\x02", certfp)),
        ],
        Ok(None) => vec![reply(
            uid,
            "You don't have a certificate registered to your account.",
        )],
        Err(e) => vec![reply(uid, &format!("Database error: {}", e))],
    }
}
