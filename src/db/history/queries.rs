//! Query functions for message history retrieval.

use crate::db::DbError;
use super::types::{HistoryRow, StoredMessage, rows_to_messages};
use slirc_proto::irc_to_lower;
use sqlx::SqlitePool;

/// Query most recent N messages (CHATHISTORY LATEST).
pub(super) async fn query_latest(
    pool: &SqlitePool,
    target: &str,
    limit: u32,
) -> Result<Vec<StoredMessage>, DbError> {
    let normalized_target = irc_to_lower(target);

    let rows: Vec<HistoryRow> = sqlx::query_as(
        r#"
        SELECT msgid, target, sender, message_data, nanotime, account
        FROM message_history
        WHERE target = ?
        ORDER BY nanotime DESC, rowid DESC
        LIMIT ?
        "#,
    )
    .bind(&normalized_target)
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    Ok(rows_to_messages(rows, true))
}

/// Query most recent N messages after a timestamp (CHATHISTORY LATEST with lower bound).
pub(super) async fn query_latest_after(
    pool: &SqlitePool,
    target: &str,
    after_nanos: i64,
    limit: u32,
) -> Result<Vec<StoredMessage>, DbError> {
    let normalized_target = irc_to_lower(target);

    let rows: Vec<HistoryRow> = sqlx::query_as(
        r#"
        SELECT msgid, target, sender, message_data, nanotime, account
        FROM message_history
        WHERE target = ? AND nanotime > ?
        ORDER BY nanotime DESC, rowid DESC
        LIMIT ?
        "#,
    )
    .bind(&normalized_target)
    .bind(after_nanos)
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    Ok(rows_to_messages(rows, true))
}

/// Query messages before a timestamp (CHATHISTORY BEFORE).
pub(super) async fn query_before(
    pool: &SqlitePool,
    target: &str,
    before_nanos: i64,
    limit: u32,
) -> Result<Vec<StoredMessage>, DbError> {
    let normalized_target = irc_to_lower(target);

    let rows: Vec<HistoryRow> = sqlx::query_as(
        r#"
        SELECT msgid, target, sender, message_data, nanotime, account
        FROM message_history
        WHERE target = ? AND nanotime < ?
        ORDER BY nanotime DESC, rowid DESC
        LIMIT ?
        "#,
    )
    .bind(&normalized_target)
    .bind(before_nanos)
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    Ok(rows_to_messages(rows, true))
}

/// Query messages after a timestamp (CHATHISTORY AFTER).
pub(super) async fn query_after(
    pool: &SqlitePool,
    target: &str,
    after_nanos: i64,
    limit: u32,
) -> Result<Vec<StoredMessage>, DbError> {
    let normalized_target = irc_to_lower(target);

    let rows: Vec<HistoryRow> = sqlx::query_as(
        r#"
        SELECT msgid, target, sender, message_data, nanotime, account
        FROM message_history
        WHERE target = ? AND nanotime > ?
        ORDER BY nanotime ASC, rowid ASC
        LIMIT ?
        "#,
    )
    .bind(&normalized_target)
    .bind(after_nanos)
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    Ok(rows_to_messages(rows, false))
}

/// Query messages between two timestamps (CHATHISTORY BETWEEN).
pub(super) async fn query_between(
    pool: &SqlitePool,
    target: &str,
    start_nanos: i64,
    end_nanos: i64,
    limit: u32,
) -> Result<Vec<StoredMessage>, DbError> {
    let normalized_target = irc_to_lower(target);

    let rows: Vec<HistoryRow> = sqlx::query_as(
        r#"
        SELECT msgid, target, sender, message_data, nanotime, account
        FROM message_history
        WHERE target = ? AND nanotime > ? AND nanotime < ?
        ORDER BY nanotime ASC, rowid ASC
        LIMIT ?
        "#,
    )
    .bind(&normalized_target)
    .bind(start_nanos)
    .bind(end_nanos)
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    Ok(rows_to_messages(rows, false))
}

/// Query messages between two timestamps (CHATHISTORY BETWEEN) in reverse order.
pub(super) async fn query_between_desc(
    pool: &SqlitePool,
    target: &str,
    start_nanos: i64,
    end_nanos: i64,
    limit: u32,
) -> Result<Vec<StoredMessage>, DbError> {
    let normalized_target = irc_to_lower(target);

    let rows: Vec<HistoryRow> = sqlx::query_as(
        r#"
        SELECT msgid, target, sender, message_data, nanotime, account
        FROM message_history
        WHERE target = ? AND nanotime > ? AND nanotime < ?
        ORDER BY nanotime DESC, rowid DESC
        LIMIT ?
        "#,
    )
    .bind(&normalized_target)
    .bind(start_nanos)
    .bind(end_nanos)
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;

    Ok(rows_to_messages(rows, true))
}

/// Query DM history between two users (LATEST).
pub(super) async fn query_dm_latest(
    pool: &SqlitePool,
    user1: &str,
    user1_account: Option<&str>,
    user2: &str,
    limit: u32,
) -> Result<Vec<StoredMessage>, DbError> {
    let u1_lower = irc_to_lower(user1);
    let u2_lower = irc_to_lower(user2);

    let rows: Vec<HistoryRow> = if let Some(acct) = user1_account {
        sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE ((target = ? AND lower(sender) = ? AND account = ?) OR (target = ? AND lower(sender) = ? AND target_account = ?))
            ORDER BY nanotime DESC, rowid DESC
            LIMIT ?
            "#,
        )
        .bind(&u2_lower)
        .bind(&u1_lower)
        .bind(acct)
        .bind(&u1_lower)
        .bind(&u2_lower)
        .bind(acct)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE (target = ? AND lower(sender) = ?) OR (target = ? AND lower(sender) = ?)
            ORDER BY nanotime DESC, rowid DESC
            LIMIT ?
            "#,
        )
        .bind(&u1_lower)
        .bind(&u2_lower)
        .bind(&u2_lower)
        .bind(&u1_lower)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    };

    Ok(rows_to_messages(rows, true))
}

/// Query DM history between two users (LATEST with lower bound).
pub(super) async fn query_dm_latest_after(
    pool: &SqlitePool,
    user1: &str,
    user1_account: Option<&str>,
    user2: &str,
    after_nanos: i64,
    limit: u32,
) -> Result<Vec<StoredMessage>, DbError> {
    let u1_lower = irc_to_lower(user1);
    let u2_lower = irc_to_lower(user2);

    let rows: Vec<HistoryRow> = if let Some(acct) = user1_account {
        sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE ((target = ? AND lower(sender) = ? AND account = ?) OR (target = ? AND lower(sender) = ? AND target_account = ?))
              AND nanotime > ?
                            ORDER BY nanotime DESC, rowid DESC
            LIMIT ?
            "#,
        )
        .bind(&u2_lower)
        .bind(&u1_lower)
        .bind(acct)
        .bind(&u1_lower)
        .bind(&u2_lower)
        .bind(acct)
        .bind(after_nanos)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE ((target = ? AND lower(sender) = ?) OR (target = ? AND lower(sender) = ?))
              AND nanotime > ?
                            ORDER BY nanotime DESC, rowid DESC
            LIMIT ?
            "#,
        )
        .bind(&u1_lower)
        .bind(&u2_lower)
        .bind(&u2_lower)
        .bind(&u1_lower)
        .bind(after_nanos)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    };

    Ok(rows_to_messages(rows, true))
}

/// Query DM history between two users (BEFORE).
pub(super) async fn query_dm_before(
    pool: &SqlitePool,
    user1: &str,
    user1_account: Option<&str>,
    user2: &str,
    before_nanos: i64,
    limit: u32,
) -> Result<Vec<StoredMessage>, DbError> {
    let u1_lower = irc_to_lower(user1);
    let u2_lower = irc_to_lower(user2);

    tracing::debug!("query_dm_before u1={} u2={} before={} limit={}", u1_lower, u2_lower, before_nanos, limit);

    let rows: Vec<HistoryRow> = if let Some(acct) = user1_account {
        sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE ((target = ? AND lower(sender) = ? AND account = ?) OR (target = ? AND lower(sender) = ? AND target_account = ?))
              AND nanotime < ?
                            ORDER BY nanotime DESC, rowid DESC
            LIMIT ?
            "#,
        )
        .bind(&u2_lower)
        .bind(&u1_lower)
        .bind(acct)
        .bind(&u1_lower)
        .bind(&u2_lower)
        .bind(acct)
        .bind(before_nanos)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE ((target = ? AND lower(sender) = ?) OR (target = ? AND lower(sender) = ?))
              AND nanotime < ?
                            ORDER BY nanotime DESC, rowid DESC
            LIMIT ?
            "#,
        )
        .bind(&u1_lower)
        .bind(&u2_lower)
        .bind(&u2_lower)
        .bind(&u1_lower)
        .bind(before_nanos)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    };

    Ok(rows_to_messages(rows, true))
}

/// Query DM history between two users (AFTER).
pub(super) async fn query_dm_after(
    pool: &SqlitePool,
    user1: &str,
    user1_account: Option<&str>,
    user2: &str,
    after_nanos: i64,
    limit: u32,
) -> Result<Vec<StoredMessage>, DbError> {
    let u1_lower = irc_to_lower(user1);
    let u2_lower = irc_to_lower(user2);

    let rows: Vec<HistoryRow> = if let Some(acct) = user1_account {
        sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE ((target = ? AND lower(sender) = ? AND account = ?) OR (target = ? AND lower(sender) = ? AND target_account = ?))
              AND nanotime > ?
                            ORDER BY nanotime ASC, rowid ASC
            LIMIT ?
            "#,
        )
        .bind(&u2_lower)
        .bind(&u1_lower)
        .bind(acct)
        .bind(&u1_lower)
        .bind(&u2_lower)
        .bind(acct)
        .bind(after_nanos)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE ((target = ? AND lower(sender) = ?) OR (target = ? AND lower(sender) = ?))
              AND nanotime > ?
                            ORDER BY nanotime ASC, rowid ASC
            LIMIT ?
            "#,
        )
        .bind(&u1_lower)
        .bind(&u2_lower)
        .bind(&u2_lower)
        .bind(&u1_lower)
        .bind(after_nanos)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    };

    Ok(rows_to_messages(rows, false))
}

/// Query DM history between two users (BETWEEN).
pub(super) async fn query_dm_between(
    pool: &SqlitePool,
    user1: &str,
    user1_account: Option<&str>,
    user2: &str,
    start_nanos: i64,
    end_nanos: i64,
    limit: u32,
) -> Result<Vec<StoredMessage>, DbError> {
    let u1_lower = irc_to_lower(user1);
    let u2_lower = irc_to_lower(user2);

    let rows: Vec<HistoryRow> = if let Some(acct) = user1_account {
        sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE ((target = ? AND lower(sender) = ? AND account = ?) OR (target = ? AND lower(sender) = ? AND target_account = ?))
              AND nanotime > ? AND nanotime < ?
                            ORDER BY nanotime ASC, rowid ASC
            LIMIT ?
            "#,
        )
        .bind(&u2_lower)
        .bind(&u1_lower)
        .bind(acct)
        .bind(&u1_lower)
        .bind(&u2_lower)
        .bind(acct)
        .bind(start_nanos)
        .bind(end_nanos)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE ((target = ? AND lower(sender) = ?) OR (target = ? AND lower(sender) = ?))
              AND nanotime > ? AND nanotime < ?
                            ORDER BY nanotime ASC, rowid ASC
            LIMIT ?
            "#,
        )
        .bind(&u1_lower)
        .bind(&u2_lower)
        .bind(&u2_lower)
        .bind(&u1_lower)
        .bind(start_nanos)
        .bind(end_nanos)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    };

    Ok(rows_to_messages(rows, false))
}

/// Query DM history between two users (BETWEEN) in reverse order.
pub(super) async fn query_dm_between_desc(
    pool: &SqlitePool,
    user1: &str,
    user1_account: Option<&str>,
    user2: &str,
    start_nanos: i64,
    end_nanos: i64,
    limit: u32,
) -> Result<Vec<StoredMessage>, DbError> {
    let u1_lower = irc_to_lower(user1);
    let u2_lower = irc_to_lower(user2);

    let rows: Vec<HistoryRow> = if let Some(acct) = user1_account {
        sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE ((target = ? AND lower(sender) = ? AND account = ?) OR (target = ? AND lower(sender) = ? AND target_account = ?))
              AND nanotime > ? AND nanotime < ?
                            ORDER BY nanotime DESC, rowid DESC
            LIMIT ?
            "#,
        )
        .bind(&u2_lower)
        .bind(&u1_lower)
        .bind(acct)
        .bind(&u1_lower)
        .bind(&u2_lower)
        .bind(acct)
        .bind(start_nanos)
        .bind(end_nanos)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE ((target = ? AND lower(sender) = ?) OR (target = ? AND lower(sender) = ?))
              AND nanotime > ? AND nanotime < ?
                            ORDER BY nanotime DESC, rowid DESC
            LIMIT ?
            "#,
        )
        .bind(&u1_lower)
        .bind(&u2_lower)
        .bind(&u2_lower)
        .bind(&u1_lower)
        .bind(start_nanos)
        .bind(end_nanos)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
    };

    Ok(rows_to_messages(rows, true))
}

/// Lookup msgid and return its nanotime.
pub(super) async fn lookup_msgid_nanotime(
    pool: &SqlitePool,
    target: &str,
    msgid: &str,
) -> Result<Option<i64>, DbError> {
    let normalized_target = irc_to_lower(target);

    let result: Option<(i64,)> =
        sqlx::query_as("SELECT nanotime FROM message_history WHERE target = ? AND msgid = ?")
            .bind(&normalized_target)
            .bind(msgid)
            .fetch_optional(pool)
            .await?;

    Ok(result.map(|(n,)| n))
}

/// Lookup msgid for DM and return its nanotime.
pub(super) async fn lookup_dm_msgid_nanotime(
    pool: &SqlitePool,
    _user1: &str,
    _user2: &str,
    msgid: &str,
) -> Result<Option<i64>, DbError> {
    // Relaxed lookup to fix AROUND failure
    let result: Option<(i64,)> = sqlx::query_as(
        "SELECT nanotime FROM message_history WHERE msgid = ?"
    )
    .bind(msgid)
    .fetch_optional(pool)
    .await?;

    Ok(result.map(|(n,)| n))
}
