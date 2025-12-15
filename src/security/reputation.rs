//! Reputation System
//!
//! Tracks user behavior over time to assign a Trust Score (0-100).
//! High trust users bypass aggressive spam checks.
//!
//! Entities are tracked by:
//! - NickServ Account (primary)
//! - IP Hash (fallback for unauthenticated users)

use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct ReputationManager {
    pool: SqlitePool,
}

impl ReputationManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Get the trust score for an entity (0-100).
    /// Returns 0 if entity is unknown.
    pub async fn get_trust_score(&self, entity: &str) -> i32 {
        let result = sqlx::query_scalar::<_, i32>(
            "SELECT trust_score FROM reputation WHERE entity = ?"
        )
        .bind(entity)
        .fetch_optional(&self.pool)
        .await;

        match result {
            Ok(Some(score)) => score,
            Ok(None) => 0,
            Err(e) => {
                warn!("Failed to fetch reputation for {}: {}", entity, e);
                0
            }
        }
    }

    /// Record a successful connection or positive interaction.
    /// Increases trust score slightly (up to max 100).
    pub async fn record_connection(&self, entity: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Insert or update
        // Bonus: +1 trust per connection, max cap handled by application logic or clamp
        // Here we use a simple logic: New users start at 10. Existing users gain +1, capped at 100.
        let sql = r#"
            INSERT INTO reputation (entity, trust_score, first_seen, last_seen, connections, violations)
            VALUES (?, 10, ?, ?, 1, 0)
            ON CONFLICT(entity) DO UPDATE SET
                trust_score = MIN(100, trust_score + 1),
                last_seen = excluded.last_seen,
                connections = connections + 1
        "#;

        if let Err(e) = sqlx::query(sql)
            .bind(entity)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await
        {
            warn!("Failed to record connection for {}: {}", entity, e);
        }
    }

    /// Record a violation (spam detection).
    /// Decreases trust score significantly.
    pub async fn record_violation(&self, entity: &str, penalty: i32) -> i32 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Ensure record exists (start at 0 trust if new offender), then deduct
        let sql = r#"
            INSERT INTO reputation (entity, trust_score, first_seen, last_seen, connections, violations)
            VALUES (?, MAX(0, 0 - ?), ?, ?, 1, 1)
            ON CONFLICT(entity) DO UPDATE SET
                trust_score = MAX(0, trust_score - ?),
                last_seen = excluded.last_seen,
                violations = violations + 1
            RETURNING trust_score
        "#;

        match sqlx::query_scalar::<_, i32>(sql)
            .bind(entity)
            .bind(penalty)
            .bind(now)
            .bind(now)
            .bind(penalty)
            .fetch_one(&self.pool)
            .await
        {
            Ok(new_score) => {
                debug!("Reputation penalty for {}: -{} (New Score: {})", entity, penalty, new_score);
                new_score
            }
            Err(e) => {
                warn!("Failed to record violation for {}: {}", entity, e);
                0
            }
        }
    }
}
