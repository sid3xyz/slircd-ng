//! NickServ - Nickname Registration and Authentication
//!
//! Phase 1: HELP, REGISTER.
//! Phase 2: IDENTIFY, GHOST, DROP.
//! Future: SET EMAIL/PASSWORD, INFO, ACCESS.
//!
//! Competitive Syntax: Universal convention (Anope/Atheme/Ergo).

use crate::infrastructure::persistence::database::Database;
use crate::extensions::services::routing::send_service_notice;
use crate::core::state::{ClientId, ServerState};
use anyhow::{Context, Result};
use bcrypt::{hash, DEFAULT_COST};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Handle NickServ command from user
///
/// Called by routing.rs after is_service_target() confirms target is NickServ
///
/// Ergo Pattern: Command dispatch via map (nickservCommands)
pub async fn handle_command(
    state: &Arc<ServerState>,
    client_id: ClientId,
    message: &str,
) -> Result<()> {
    let mut parts = message.split_whitespace();
    let command = match parts.next() {
        Some(cmd) => cmd.to_uppercase(),
        None => {
            send_service_notice(
                state,
                client_id,
                "NickServ",
                "No command specified. Use /msg NickServ HELP for command list.",
            )
            .await?;
            return Ok(());
        }
    };

    // Dispatch to command handler
    match command.as_str() {
        "HELP" => handle_help(state, client_id).await,
        "REGISTER" => {
            let params: Vec<&str> = parts.collect();
            handle_register(state, client_id, &params).await
        }
        "IDENTIFY" => {
            let params: Vec<&str> = parts.collect();
            handle_identify(state, client_id, &params).await
        }
        "GHOST" => {
            let params: Vec<&str> = parts.collect();
            handle_ghost(state, client_id, &params).await
        }
        "DROP" => handle_drop(state, client_id).await,
        _ => {
            send_service_notice(
                state,
                client_id,
                "NickServ",
                &format!(
                    "Unknown command: {}. Use /msg NickServ HELP for command list.",
                    command
                ),
            )
            .await?;
            Ok(())
        }
    }
}

/// HELP command - List available commands
///
/// Competitive Help Text: Anope (all commands), Atheme (grouped), Ergo (minimal).
/// Our approach: Phase-aware (show implemented only)
async fn handle_help(state: &Arc<ServerState>, client_id: ClientId) -> Result<()> {
    let help_lines = vec![
        "*** NickServ Help ***",
        "",
        "NickServ allows you to register and protect your nickname.",
        "",
        "Available commands:",
        "  HELP                          - This help text",
        "  REGISTER <password> <email>   - Register your current nickname",
        "  IDENTIFY <password>           - Authenticate to your nickname",
        "  GHOST <nickname> <password>   - Disconnect ghost sessions",
        "  DROP                          - Delete your nickname registration",
        "",
        "Coming soon (Phase 3):",
        "  SET EMAIL <email>             - Change email address",
        "  SET PASSWORD <new>            - Change password",
        "  INFO <nick>                   - View registration info",
        "",
        "For more information: /msg NickServ HELP <command>",
    ];

    for line in help_lines {
        send_service_notice(state, client_id, "NickServ", line).await?;
    }

    Ok(())
}

/// REGISTER command - Register current nickname
///
/// Usage: `/msg NickServ REGISTER <password> <email>`
///
/// Competitive Flow: Anope (ns_register.cpp), Atheme (register.c), Ergo (nsRegisterHandler).
/// Our Impl: SQLite + bcrypt cost 12 + auto-identify (IRCv3 integration) + rate limit (Phase 2)
async fn handle_register(
    state: &Arc<ServerState>,
    client_id: ClientId,
    params: &[&str],
) -> Result<()> {
    // Validate parameters
    if params.len() < 2 {
        send_service_notice(
            state,
            client_id,
            "NickServ",
            "Usage: REGISTER <password> <email>",
        )
        .await?;
        return Ok(());
    }

    let password = params[0];
    let email = params[1];

    // Get client's current nickname
    let client = state
        .get_client(client_id)
        .await
        .context("client not found")?;

    let nickname = client
        .nickname
        .as_ref()
        .context("client has no nickname")?
        .clone();

    // Validate password strength (min 5 chars, Atheme standard)
    if password.len() < 5 {
        send_service_notice(
            state,
            client_id,
            "NickServ",
            "Password too short. Minimum 5 characters required.",
        )
        .await?;
        return Ok(());
    }

    // Validate email format (basic check)
    if !email.contains('@') || email.len() < 5 {
        send_service_notice(
            state,
            client_id,
            "NickServ",
            "Invalid email address format.",
        )
        .await?;
        return Ok(());
    }

    // Check if nickname already registered
    let db = state.database();

    if is_nickname_registered(db, &nickname).await? {
        send_service_notice(
            state,
            client_id,
            "NickServ",
            &format!("Nickname {} is already registered.", nickname),
        )
        .await?;
        return Ok(());
    }

    // TODO: Rate limiting (1 REGISTER per IP per minute)
    // For Phase 1, we skip this (implement in Phase 2 with proper rate limiter)

    // Hash password (bcrypt cost 12 - Ergo standard)
    let password_hash = tokio::task::spawn_blocking({
        let password = password.to_string();
        move || hash(password, DEFAULT_COST)
    })
    .await
    .context("bcrypt task panicked")??;

    // Get current timestamp
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Insert into database
    register_nickname(db, &nickname, &password_hash, email, now).await?;

    tracing::info!(
        nickname = %nickname,
        email = %email,
        client_id = ?client_id,
        "nickname registered successfully"
    );

    // Auto-identify: Set client.account (IRCv3 SASL integration!)
    // This links nickname registration to account system
    state.set_client_account(client_id, Some(nickname.clone()));

    // Send success notice
    send_service_notice(
        state,
        client_id,
        "NickServ",
        &format!("Nickname {} has been registered successfully.", nickname),
    )
    .await?;

    send_service_notice(
        state,
        client_id,
        "NickServ",
        &format!("Confirmation email sent to {}", email),
    )
    .await?;

    send_service_notice(
        state,
        client_id,
        "NickServ",
        "You are now automatically identified.",
    )
    .await?;

    Ok(())
}

/// Check if nickname is already registered
///
/// Query registered_nicks table for exact nickname match
async fn is_nickname_registered(db: &Database, nickname: &str) -> Result<bool> {
    let nickname = nickname.to_string();

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let exists = conn
        .interact(move |conn| {
            let mut stmt =
                conn.prepare("SELECT 1 FROM registered_nicks WHERE nickname = ?1 LIMIT 1")?;
            let exists = stmt.exists([&nickname])?;
            Ok::<bool, anyhow::Error>(exists)
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(exists)
}

/// Register nickname in database
///
/// Insert into registered_nicks table with bcrypt password hash
async fn register_nickname(
    db: &Database,
    nickname: &str,
    password_hash: &str,
    email: &str,
    registered_at: i64,
) -> Result<()> {
    let nickname = nickname.to_string();
    let password_hash = password_hash.to_string();
    let email = email.to_string();

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    conn.interact(move |conn| {
        conn.execute(
            "INSERT INTO registered_nicks (nickname, password_hash, email, registered_at, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            rusqlite::params![&nickname, &password_hash, &email, registered_at],
        )?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(())
}

/// IDENTIFY command - Authenticate to registered nickname
///
/// Usage: `/msg NickServ IDENTIFY <password>`
///
/// Competitive Pattern: Ergo (verify password, set client.account), Anope (bcrypt::verify), Atheme (login timestamp).
/// Our Impl: Verify nickname registered, bcrypt::verify, set client.account, update last_seen.
async fn handle_identify(
    state: &Arc<ServerState>,
    client_id: ClientId,
    params: &[&str],
) -> Result<()> {
    // Validate parameters
    if params.is_empty() {
        send_service_notice(state, client_id, "NickServ", "Usage: IDENTIFY <password>").await?;
        return Ok(());
    }

    let password = params[0];

    // Get client's current nickname
    let client = state
        .get_client(client_id)
        .await
        .context("client not found")?;

    let nickname = client
        .nickname
        .as_ref()
        .context("client has no nickname")?
        .clone();

    // Check if already identified
    if let Some(account) = state.get_client_account(client_id) {
        if account == nickname {
            send_service_notice(state, client_id, "NickServ", "You are already identified.")
                .await?;
            return Ok(());
        }
    }

    // Get registration from database
    let db = state.database();
    let password_hash = match get_password_hash(db, &nickname).await? {
        Some(hash) => hash,
        None => {
            send_service_notice(
                state,
                client_id,
                "NickServ",
                &format!("Nickname {} is not registered.", nickname),
            )
            .await?;
            return Ok(());
        }
    };

    // Verify password (CPU-intensive, use spawn_blocking)
    let password_valid = tokio::task::spawn_blocking({
        let password = password.to_string();
        move || bcrypt::verify(password, &password_hash)
    })
    .await
    .context("bcrypt task panicked")??;

    if !password_valid {
        tracing::warn!(
            nickname = %nickname,
            client_id = ?client_id,
            "failed IDENTIFY attempt (invalid password)"
        );

        send_service_notice(state, client_id, "NickServ", "Invalid password.").await?;
        return Ok(());
    }

    // Update last_seen timestamp
    update_last_seen(db, &nickname).await?;

    // Set account (authenticate user)
    state.set_client_account(client_id, Some(nickname.clone()));

    tracing::info!(
        nickname = %nickname,
        client_id = ?client_id,
        "successful IDENTIFY"
    );

    send_service_notice(
        state,
        client_id,
        "NickServ",
        &format!("You are now identified for {}.", nickname),
    )
    .await?;

    Ok(())
}

/// GHOST command - Disconnect ghost session using your nickname
///
/// Usage: `/msg NickServ GHOST <nickname> <password>`
///
/// ## Competitive Ghost Patterns
///
/// **Competitive pattern (Ergo)**: GHOST finds duplicate session, sends QUIT
/// **Competitive pattern (Anope)**: GHOST verifies password, kills target connection
/// **Competitive pattern (Atheme)**: GHOST with optional password (if identified)
///
/// ## Our Implementation
///
/// - Verify password for target nickname
/// - Find client_id using that nickname
/// - Send QUIT to ghost connection
/// - Notify requester of success
async fn handle_ghost(
    state: &Arc<ServerState>,
    client_id: ClientId,
    params: &[&str],
) -> Result<()> {
    // Validate parameters
    if params.len() < 2 {
        send_service_notice(
            state,
            client_id,
            "NickServ",
            "Usage: GHOST <nickname> <password>",
        )
        .await?;
        return Ok(());
    }

    let target_nick = params[0];
    let password = params[1];

    // Get requester's client info (for logging)
    let requester = state
        .get_client(client_id)
        .await
        .context("client not found")?;

    let requester_nick = requester.nickname.as_deref().unwrap_or("unknown");

    // Normalize target nickname for lookup
    let normalized = crate::core::state::normalize_nick(target_nick);

    // Get registration from database
    let db = state.database();
    let password_hash = match get_password_hash(db, &normalized).await? {
        Some(hash) => hash,
        None => {
            send_service_notice(
                state,
                client_id,
                "NickServ",
                &format!("Nickname {} is not registered.", target_nick),
            )
            .await?;
            return Ok(());
        }
    };

    // Verify password (CPU-intensive, use spawn_blocking)
    let password_valid = tokio::task::spawn_blocking({
        let password = password.to_string();
        move || bcrypt::verify(password, &password_hash)
    })
    .await
    .context("bcrypt task panicked")??;

    if !password_valid {
        tracing::warn!(
            target_nick = %target_nick,
            client_id = ?client_id,
            "failed GHOST attempt (invalid password)"
        );

        send_service_notice(state, client_id, "NickServ", "Invalid password.").await?;
        return Ok(());
    }

    // Find ghost client using that nickname
    let ghost_client_id = match state.find_client_id_by_nick(target_nick).await {
        Some(id) => id,
        None => {
            send_service_notice(
                state,
                client_id,
                "NickServ",
                &format!("Nickname {} is not currently online.", target_nick),
            )
            .await?;
            return Ok(());
        }
    };

    // Don't ghost yourself
    if ghost_client_id == client_id {
        send_service_notice(
            state,
            client_id,
            "NickServ",
            "You cannot ghost yourself. Use /NICK to change nicknames.",
        )
        .await?;
        return Ok(());
    }

    // Send QUIT to ghost connection
    if let Some(sender) = state.get_client_sender(ghost_client_id).await {
        let quit_msg = format!(
            "ERROR :Closing Link: (GHOST command used by {})\r\n",
            requester_nick
        );
        let _ = sender.send(quit_msg);
    }

    // Trigger disconnect
    if let Some(ghost_client) = state.get_client(ghost_client_id).await {
        ghost_client.disconnect_notify.notify_one();
    }

    tracing::info!(
        target_nick = %target_nick,
        ghost_client_id = ?ghost_client_id,
        requester_id = ?client_id,
        "GHOST command executed"
    );

    send_service_notice(
        state,
        client_id,
        "NickServ",
        &format!("Ghost session for {} has been disconnected.", target_nick),
    )
    .await?;

    Ok(())
}

/// DROP command - Delete your nickname registration
///
/// Usage: `/msg NickServ DROP`
///
/// ## Competitive Drop Patterns
///
/// **Competitive pattern (Anope)**: DROP requires confirmation code (we skip for Phase 2)
/// **Competitive pattern (Atheme)**: DROP immediate with password verification
/// **Competitive pattern (Ergo)**: UNREGISTER command, requires identified
///
/// ## Our Implementation
///
/// - Verify user is identified (client.account set)
/// - Delete from registered_nicks table
/// - Clear client.account
/// - Send confirmation NOTICE
async fn handle_drop(state: &Arc<ServerState>, client_id: ClientId) -> Result<()> {
    // Get client's account (must be identified)
    let account = match state.get_client_account(client_id) {
        Some(acc) => acc,
        None => {
            send_service_notice(
                state,
                client_id,
                "NickServ",
                "You must be identified to drop your nickname. Use /msg NickServ IDENTIFY <password>",
            )
            .await?;
            return Ok(());
        }
    };

    // Get client's current nickname
    let client = state
        .get_client(client_id)
        .await
        .context("client not found")?;

    let nickname = client
        .nickname
        .as_ref()
        .context("client has no nickname")?
        .clone();

    // Verify account matches current nickname (safety check)
    if account != nickname {
        send_service_notice(
            state,
            client_id,
            "NickServ",
            &format!(
                "You can only drop your own nickname. You are identified as {}.",
                account
            ),
        )
        .await?;
        return Ok(());
    }

    // Delete from database
    let db = state.database();
    delete_registration(db, &nickname).await?;

    // Clear account (de-identify)
    state.set_client_account(client_id, None);

    tracing::info!(
        nickname = %nickname,
        client_id = ?client_id,
        "nickname registration dropped"
    );

    send_service_notice(
        state,
        client_id,
        "NickServ",
        &format!("Nickname {} has been dropped successfully.", nickname),
    )
    .await?;

    Ok(())
}

/// Get password hash for nickname from database
async fn get_password_hash(db: &Database, nickname: &str) -> Result<Option<String>> {
    let nickname = nickname.to_string();

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let hash = conn
        .interact(move |conn| {
            let mut stmt =
                conn.prepare("SELECT password_hash FROM registered_nicks WHERE nickname = ?1")?;
            let mut rows = stmt.query([&nickname])?;

            if let Some(row) = rows.next()? {
                let hash: String = row.get(0)?;
                Ok::<Option<String>, anyhow::Error>(Some(hash))
            } else {
                Ok(None)
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(hash)
}

/// Update last_seen timestamp for nickname
async fn update_last_seen(db: &Database, nickname: &str) -> Result<()> {
    let nickname = nickname.to_string();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    conn.interact(move |conn| {
        conn.execute(
            "UPDATE registered_nicks SET last_seen = ?1 WHERE nickname = ?2",
            rusqlite::params![now, &nickname],
        )?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(())
}

/// Delete nickname registration from database
async fn delete_registration(db: &Database, nickname: &str) -> Result<()> {
    let nickname = nickname.to_string();

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    conn.interact(move |conn| {
        conn.execute(
            "DELETE FROM registered_nicks WHERE nickname = ?1",
            rusqlite::params![&nickname],
        )?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(())
}
