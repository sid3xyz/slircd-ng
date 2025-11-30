//! ChanServ - Channel Registration and Management Services
//!
//! Competitive Analysis: Anope (cs_*.cpp), Atheme (chanserv/*.c), Ergo (chanserv.go).
//!
//! Phase 3: REGISTER, INFO, OP/DEOP, DROP, auto-op founder.
//! Future: ACCESS, AKICK, SET options.

use crate::infrastructure::persistence::database::Database;
use crate::extensions::services::routing::send_service_notice;
use crate::core::state::{normalize_channel, ClientId, ServerState};
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// ChanServ command dispatcher
///
/// Parse message, route to appropriate handler. Pattern: Same as NickServ (nickserv.rs:65-110)
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
                "ChanServ",
                "No command specified. Use /msg ChanServ HELP for help.",
            )
            .await?;
            return Ok(());
        }
    };

    let params: Vec<&str> = parts.collect();

    match command.as_str() {
        "HELP" => handle_help(state, client_id).await,
        "REGISTER" => handle_register(state, client_id, &params).await,
        "IDENTIFY" => handle_identify(state, client_id, &params).await,
        "INFO" => handle_info(state, client_id, &params).await,
        "OP" => handle_op(state, client_id, &params).await,
        "DEOP" => handle_deop(state, client_id, &params).await,
        "DROP" => handle_drop(state, client_id, &params).await,
        _ => {
            send_service_notice(
                state,
                client_id,
                "ChanServ",
                &format!(
                    "Unknown command: {}. Use /msg ChanServ HELP for help.",
                    command
                ),
            )
            .await?;
            Ok(())
        }
    }
}

/// HELP command - Display available commands
///
/// Pattern: NickServ HELP (nickserv.rs:117-160)
async fn handle_help(state: &Arc<ServerState>, client_id: ClientId) -> Result<()> {
    let help_lines = vec![
        "*** ChanServ Help ***",
        "",
        "ChanServ allows you to register and manage channels.",
        "",
        "Available commands:",
        "  HELP                      - This help text",
        "  REGISTER <#channel>       - Register a channel you own",
        "  IDENTIFY <#channel> <pw>  - Authenticate as founder (auto-op)",
        "  INFO <#channel>           - View channel registration info",
        "  OP <#channel> <nickname>  - Grant operator status",
        "  DEOP <#channel> <nickname> - Remove operator status",
        "  DROP <#channel>           - Delete channel registration",
        "",
        "Coming soon (Phase 4):",
        "  ACCESS <#channel> ADD <nick> <level> - Delegate ops",
        "  AKICK <#channel> ADD <mask>          - Auto-kick ban list",
        "  SET <#channel> <option> <value>      - Channel settings",
        "",
        "For more information: /msg ChanServ HELP <command>",
    ];

    for line in help_lines {
        send_service_notice(state, client_id, "ChanServ", line).await?;
    }

    Ok(())
}

/// REGISTER command - Register a channel
///
/// Usage: `/msg ChanServ REGISTER <#channel>`
///
/// Competitive Patterns: Anope (cs_register.cpp), Atheme (register.c), Ergo (chanserv.go:registerChannel)
async fn handle_register(
    state: &Arc<ServerState>,
    client_id: ClientId,
    params: &[&str],
) -> Result<()> {
    // Validate parameters
    if params.is_empty() {
        send_service_notice(state, client_id, "ChanServ", "Usage: REGISTER <#channel>").await?;
        return Ok(());
    }

    let channel_name = params[0];

    // Validate channel name format
    if !channel_name.starts_with('#') {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            "Invalid channel name. Must start with #.",
        )
        .await?;
        return Ok(());
    }

    // Get user's account (must be identified)
    let account = match state.get_client_account(client_id) {
        Some(acc) => acc,
        None => {
            send_service_notice(
                state,
                client_id,
                "ChanServ",
                "You must be identified to register a channel. Use /msg NickServ IDENTIFY <password>",
            )
            .await?;
            return Ok(());
        }
    };

    // Normalize channel name
    let normalized = normalize_channel(channel_name);

    // Check if channel exists
    if state.channel_members(&normalized).await.is_none() {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            &format!(
                "Channel {} does not exist. Join it first, then register.",
                channel_name
            ),
        )
        .await?;
        return Ok(());
    }

    // Check if user is in channel
    let in_channel = state
        .channel_members(&normalized)
        .await
        .map(|members| members.iter().any(|(id, _record)| *id == client_id))
        .unwrap_or(false);

    if !in_channel {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            &format!("You must be in {} to register it.", channel_name),
        )
        .await?;
        return Ok(());
    }

    // Check if already registered
    let db = state.database();
    if is_channel_registered(db, &normalized).await? {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            &format!("Channel {} is already registered.", channel_name),
        )
        .await?;
        return Ok(());
    }

    // Register channel
    register_channel(db, &normalized, &account).await?;

    tracing::info!(
        channel = %channel_name,
        founder = %account,
        client_id = ?client_id,
        "channel registered"
    );

    send_service_notice(
        state,
        client_id,
        "ChanServ",
        &format!(
            "Channel {} has been registered with you as founder.",
            channel_name
        ),
    )
    .await?;

    send_service_notice(
        state,
        client_id,
        "ChanServ",
        "You will be automatically opped when you join.",
    )
    .await?;

    Ok(())
}

/// IDENTIFY command - Founder authentication and auto-op
///
/// Usage: `/msg ChanServ IDENTIFY <#channel> <password>`
///
/// Verifies user is founder via NickServ account, grants operator status.
/// Pattern: Atheme cs_identify.c (founder authentication)
///
/// NOTE: Phase 3 uses NickServ accounts for auth (no separate channel passwords).
async fn handle_identify(
    state: &Arc<ServerState>,
    client_id: ClientId,
    params: &[&str],
) -> Result<()> {
    // Validate parameters
    if params.len() < 2 {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            "Usage: IDENTIFY <#channel> <password>",
        )
        .await?;
        return Ok(());
    }

    let channel_name = params[0];
    let _password = params[1]; // Future: check channel password

    // Validate channel name format
    if !channel_name.starts_with('#') {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            "Invalid channel name. Must start with #.",
        )
        .await?;
        return Ok(());
    }

    // Get user's account (must be identified to NickServ)
    let account = match state.get_client_account(client_id) {
        Some(acc) => acc,
        None => {
            send_service_notice(
                state,
                client_id,
                "ChanServ",
                "You must be identified to NickServ first. Use /msg NickServ IDENTIFY <password>",
            )
            .await?;
            return Ok(());
        }
    };

    // Normalize channel name
    let normalized = normalize_channel(channel_name);

    // Check if channel is registered
    let db = state.database();
    let founder = match get_channel_founder(db, &normalized).await? {
        Some(f) => f,
        None => {
            send_service_notice(
                state,
                client_id,
                "ChanServ",
                &format!("Channel {} is not registered.", channel_name),
            )
            .await?;
            return Ok(());
        }
    };

    // Verify user is the founder
    if account != founder {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            &format!(
                "You are not the founder of {}. Registered to: {}",
                channel_name, founder
            ),
        )
        .await?;
        return Ok(());
    }

    // Check if user is in channel
    let in_channel = state
        .channel_members(&normalized)
        .await
        .map(|members| members.iter().any(|(id, _record)| *id == client_id))
        .unwrap_or(false);

    if !in_channel {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            &format!(
                "You must be in {} to identify. Join the channel first.",
                channel_name
            ),
        )
        .await?;
        return Ok(());
    }

    // Grant operator status (+o)
    // Pattern: Anope cs_identify.cpp - auto-op founder
    let client = state
        .get_client(client_id)
        .await
        .context("client not found")?;

    let nickname = client
        .nickname
        .as_ref()
        .context("client has no nickname")?
        .clone();

    // Send MODE command to channel (grant +o)
    if let Err(e) = grant_channel_op(state, &normalized, client_id, &nickname).await {
        tracing::error!(
            error = ?e,
            channel = %channel_name,
            nick = %nickname,
            "failed to grant operator status"
        );
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            &format!(
                "You are identified for {}, but operator grant failed.",
                channel_name
            ),
        )
        .await?;
        return Ok(());
    }

    tracing::info!(
        channel = %channel_name,
        founder = %account,
        nick = %nickname,
        client_id = ?client_id,
        "founder identified and opped"
    );

    send_service_notice(
        state,
        client_id,
        "ChanServ",
        &format!("You are now identified for {}.", channel_name),
    )
    .await?;

    send_service_notice(
        state,
        client_id,
        "ChanServ",
        "You have been granted operator status.",
    )
    .await?;

    Ok(())
}

/// INFO command - Display channel registration information
///
/// Usage: `/msg ChanServ INFO <#channel>`
///
/// Displays: Founder, registration date, flags
async fn handle_info(state: &Arc<ServerState>, client_id: ClientId, params: &[&str]) -> Result<()> {
    if params.is_empty() {
        send_service_notice(state, client_id, "ChanServ", "Usage: INFO <#channel>").await?;
        return Ok(());
    }

    let channel_name = params[0];
    let normalized = normalize_channel(channel_name);

    let db = state.database();
    let info = match get_channel_info(db, &normalized).await? {
        Some(info) => info,
        None => {
            send_service_notice(
                state,
                client_id,
                "ChanServ",
                &format!("Channel {} is not registered.", channel_name),
            )
            .await?;
            return Ok(());
        }
    };

    send_service_notice(
        state,
        client_id,
        "ChanServ",
        &format!("Information on {}:", channel_name),
    )
    .await?;

    send_service_notice(
        state,
        client_id,
        "ChanServ",
        &format!("  Founder: {}", info.founder_nick),
    )
    .await?;

    // Format timestamp (Unix timestamp → human readable)
    let datetime = chrono::DateTime::from_timestamp(info.registered_at, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    send_service_notice(
        state,
        client_id,
        "ChanServ",
        &format!("  Registered: {}", datetime),
    )
    .await?;

    if let Some(topic) = info.topic {
        send_service_notice(state, client_id, "ChanServ", &format!("  Topic: {}", topic)).await?;
    }

    Ok(())
}

/// OP command - Grant operator status
///
/// Usage: `/msg ChanServ OP <#channel> <nickname>`
///
/// Competitive Patterns: Anope (cs_op.cpp), Our impl: Founder-only (Phase 3)
async fn handle_op(state: &Arc<ServerState>, client_id: ClientId, params: &[&str]) -> Result<()> {
    if params.len() < 2 {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            "Usage: OP <#channel> <nickname>",
        )
        .await?;
        return Ok(());
    }

    let channel_name = params[0];
    let target_nick = params[1];
    let normalized = normalize_channel(channel_name);

    // Verify requester is founder
    if !is_founder(state, client_id, &normalized).await? {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            "Access denied. You must be the channel founder.",
        )
        .await?;
        return Ok(());
    }

    // Find target client
    let target_id = match state.find_client_id_by_nick(target_nick).await {
        Some(id) => id,
        None => {
            send_service_notice(
                state,
                client_id,
                "ChanServ",
                &format!("User {} is not online.", target_nick),
            )
            .await?;
            return Ok(());
        }
    };

    // Apply MODE +o
    apply_channel_mode(state, &normalized, target_id, true).await?;

    send_service_notice(
        state,
        client_id,
        "ChanServ",
        &format!("{} is now an operator on {}.", target_nick, channel_name),
    )
    .await?;

    Ok(())
}

/// DEOP command - Remove operator status
///
/// Usage: `/msg ChanServ DEOP <#channel> <nickname>`
async fn handle_deop(state: &Arc<ServerState>, client_id: ClientId, params: &[&str]) -> Result<()> {
    if params.len() < 2 {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            "Usage: DEOP <#channel> <nickname>",
        )
        .await?;
        return Ok(());
    }

    let channel_name = params[0];
    let target_nick = params[1];
    let normalized = normalize_channel(channel_name);

    // Verify requester is founder
    if !is_founder(state, client_id, &normalized).await? {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            "Access denied. You must be the channel founder.",
        )
        .await?;
        return Ok(());
    }

    // Find target client
    let target_id = match state.find_client_id_by_nick(target_nick).await {
        Some(id) => id,
        None => {
            send_service_notice(
                state,
                client_id,
                "ChanServ",
                &format!("User {} is not online.", target_nick),
            )
            .await?;
            return Ok(());
        }
    };

    // Apply MODE -o
    apply_channel_mode(state, &normalized, target_id, false).await?;

    send_service_notice(
        state,
        client_id,
        "ChanServ",
        &format!(
            "{} is no longer an operator on {}.",
            target_nick, channel_name
        ),
    )
    .await?;

    Ok(())
}

/// DROP command - Delete channel registration
///
/// Usage: `/msg ChanServ DROP <#channel>`
///
/// Requires founder authentication
async fn handle_drop(state: &Arc<ServerState>, client_id: ClientId, params: &[&str]) -> Result<()> {
    if params.is_empty() {
        send_service_notice(state, client_id, "ChanServ", "Usage: DROP <#channel>").await?;
        return Ok(());
    }

    let channel_name = params[0];
    let normalized = normalize_channel(channel_name);

    // Verify requester is founder
    if !is_founder(state, client_id, &normalized).await? {
        send_service_notice(
            state,
            client_id,
            "ChanServ",
            "Access denied. You must be the channel founder.",
        )
        .await?;
        return Ok(());
    }

    // Delete registration
    let db = state.database();
    delete_channel_registration(db, &normalized).await?;

    tracing::info!(
        channel = %channel_name,
        client_id = ?client_id,
        "channel registration dropped"
    );

    send_service_notice(
        state,
        client_id,
        "ChanServ",
        &format!("Channel {} has been dropped successfully.", channel_name),
    )
    .await?;

    Ok(())
}

// ============================================================================
// Database Helpers
// ============================================================================

/// Check if channel is registered
async fn is_channel_registered(db: &Database, channel_name: &str) -> Result<bool> {
    let channel_name = channel_name.to_string();

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let exists = conn
        .interact(move |conn| {
            let mut stmt =
                conn.prepare("SELECT COUNT(*) FROM registered_channels WHERE channel_name = ?1")?;
            let count: i64 = stmt.query_row([&channel_name], |row| row.get(0))?;
            Ok::<bool, anyhow::Error>(count > 0)
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(exists)
}

/// Register channel in database
async fn register_channel(db: &Database, channel_name: &str, founder_nick: &str) -> Result<()> {
    let channel_name = channel_name.to_string();
    let founder_nick = founder_nick.to_string();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    // Note: founder_account_id should be FK to registered_nicks.id
    // For Phase 3 simplicity, we store founder_nick directly (TODO: Phase 4 FK constraint)
    conn.interact(move |conn| {
        conn.execute(
            "INSERT INTO registered_channels (channel_name, founder_account_id, founder_nick, registered_at, flags)
             VALUES (?1, 1, ?2, ?3, '')",
            rusqlite::params![&channel_name, &founder_nick, now],
        )?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(())
}

/// Get channel registration info
struct ChannelInfo {
    founder_nick: String,
    registered_at: i64,
    topic: Option<String>,
}

async fn get_channel_info(db: &Database, channel_name: &str) -> Result<Option<ChannelInfo>> {
    let channel_name = channel_name.to_string();

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let info = conn
        .interact(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT founder_nick, registered_at, topic FROM registered_channels WHERE channel_name = ?1",
            )?;
            let mut rows = stmt.query([&channel_name])?;

            if let Some(row) = rows.next()? {
                let founder_nick: String = row.get(0)?;
                let registered_at: i64 = row.get(1)?;
                let topic: Option<String> = row.get(2)?;

                Ok::<Option<ChannelInfo>, anyhow::Error>(Some(ChannelInfo {
                    founder_nick,
                    registered_at,
                    topic,
                }))
            } else {
                Ok(None)
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(info)
}

/// Get channel founder account name
///
/// Returns the founder_nick for the channel (used by IDENTIFY command)
async fn get_channel_founder(db: &Database, channel_name: &str) -> Result<Option<String>> {
    let channel_name = channel_name.to_string();

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let founder = conn
        .interact(move |conn| {
            let mut stmt = conn
                .prepare("SELECT founder_nick FROM registered_channels WHERE channel_name = ?1")?;
            let mut rows = stmt.query([&channel_name])?;

            if let Some(row) = rows.next()? {
                let founder_nick: String = row.get(0)?;
                Ok::<Option<String>, anyhow::Error>(Some(founder_nick))
            } else {
                Ok(None)
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(founder)
}

/// Delete channel registration
async fn delete_channel_registration(db: &Database, channel_name: &str) -> Result<()> {
    let channel_name = channel_name.to_string();

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    conn.interact(move |conn| {
        conn.execute(
            "DELETE FROM registered_channels WHERE channel_name = ?1",
            rusqlite::params![&channel_name],
        )?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(())
}

// ============================================================================
// Authorization Helpers
// ============================================================================

/// Check if user is channel founder
async fn is_founder(
    state: &Arc<ServerState>,
    client_id: ClientId,
    channel_name: &str,
) -> Result<bool> {
    // Get user's account
    let account = match state.get_client_account(client_id) {
        Some(acc) => acc,
        None => return Ok(false), // Not identified = not founder
    };

    // Query database for founder
    let db = state.database();
    let channel_name = channel_name.to_string();
    let account_clone = account.clone();

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let is_founder = conn
        .interact(move |conn| {
            let mut stmt = conn
                .prepare("SELECT founder_nick FROM registered_channels WHERE channel_name = ?1")?;
            let mut rows = stmt.query([&channel_name])?;

            if let Some(row) = rows.next()? {
                let founder: String = row.get(0)?;
                Ok::<bool, anyhow::Error>(founder == account_clone)
            } else {
                Ok(false) // Channel not registered
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(is_founder)
}

// ============================================================================
// Mode Application Helpers
// ============================================================================

/// Apply MODE +o/-o to user in channel
///
/// Uses ServerState channel mode system (state::channel::ChannelModes)
/// Sends MODE notification to channel
async fn apply_channel_mode(
    state: &Arc<ServerState>,
    channel_name: &str,
    target_id: ClientId,
    is_op: bool,
) -> Result<()> {
    // Get ChanServ client_id for the requester_id parameter (services have full permissions)
    let chanserv_id = state.find_client_id_by_nick("ChanServ").await.unwrap_or(2); // Fallback to assumed ChanServ ID

    // Set operator status using state API
    state
        .set_channel_operator(chanserv_id, channel_name, target_id, is_op)
        .await
        .context("setting channel operator status")?;

    Ok(())
}

/// Grant operator status to user (used by IDENTIFY command)
///
/// Wrapper around apply_channel_mode for +o operations
async fn grant_channel_op(
    state: &Arc<ServerState>,
    channel_name: &str,
    target_id: ClientId,
    nickname: &str,
) -> Result<()> {
    tracing::debug!(
        channel = %channel_name,
        target_id = ?target_id,
        nick = %nickname,
        "granting operator status"
    );

    apply_channel_mode(state, channel_name, target_id, true).await
}

/// Auto-op founder on JOIN (called from commands/channel/join.rs)
///
/// Competitive Pattern: Anope (cs_join.cpp) - auto-op founder after JOIN if account matches.
pub async fn check_auto_op(
    state: &Arc<ServerState>,
    client_id: ClientId,
    channel_name: &str,
) -> Result<()> {
    // Check if user is founder
    if is_founder(state, client_id, channel_name).await? {
        tracing::debug!(
            client_id = ?client_id,
            channel = %channel_name,
            "auto-opping channel founder on JOIN"
        );

        apply_channel_mode(state, channel_name, client_id, true).await?;

        send_service_notice(
            state,
            client_id,
            "ChanServ",
            &format!("You have been opped in {} (channel founder).", channel_name),
        )
        .await?;
    }

    Ok(())
}
