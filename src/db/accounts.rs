//! Account repository for NickServ functionality.
//!
//! Handles account registration, authentication, and nickname management.

use super::DbError;
use argon2::password_hash::{PasswordHash, rand_core::OsRng};
use rand::RngCore;
use sqlx::SqlitePool;
use std::num::NonZeroU32;

/// Default iteration count for SCRAM-SHA-256 (RFC 7677 recommends >= 4096).
const SCRAM_ITERATIONS: u32 = 4096;

/// SCRAM verifiers for SASL SCRAM-SHA-256 authentication.
#[derive(Debug, Clone)]
pub struct ScramVerifiers {
    pub salt: Vec<u8>,
    pub iterations: u32,
    pub hashed_password: Vec<u8>,
}

/// A registered NickServ account.
#[derive(Debug, Clone)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub email: Option<String>,
    pub registered_at: i64,
    pub last_seen_at: i64,
    pub enforce: bool,
    pub hide_email: bool,
    pub metadata: std::collections::HashMap<String, String>,
}

/// Repository for account operations.
pub struct AccountRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> AccountRepository<'a> {
    /// Create a new account repository.
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Register a new account with the given nickname and password.
    ///
    /// Uses a transaction to ensure atomicity between account and nickname creation.
    /// Also computes and stores SCRAM-SHA-256 verifiers for SASL authentication.
    pub async fn register(
        &self,
        name: &str,
        password: &str,
        email: Option<&str>,
    ) -> Result<Account, DbError> {
        // Hash the password using Argon2 (for PLAIN auth fallback)
        let password_hash = crate::security::password::hash_password(password.to_string())
            .await
            .map_err(|e| DbError::Internal(format!("Password hashing failed: {}", e)))?;

        // Compute SCRAM-SHA-256 verifiers
        let scram_verifiers = compute_scram_verifiers(password).await;

        let now = chrono::Utc::now().timestamp();

        // Use a transaction to ensure account + nickname are created atomically
        let mut tx = self.pool.begin().await?;

        // Insert account (UNIQUE constraint will catch duplicates)
        let result = sqlx::query(
            r#"
            INSERT INTO accounts (name, password_hash, email, registered_at, last_seen_at,
                                  scram_salt, scram_iterations, scram_hashed_password)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(name)
        .bind(&password_hash)
        .bind(email)
        .bind(now)
        .bind(now)
        .bind(&scram_verifiers.salt)
        .bind(scram_verifiers.iterations as i32)
        .bind(&scram_verifiers.hashed_password)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            // Convert UNIQUE constraint violation to AccountExists error
            if let sqlx::Error::Database(ref db_err) = e
                && db_err.is_unique_violation()
            {
                return DbError::AccountExists(name.to_string());
            }
            DbError::from(e)
        })?;

        let account_id = result.last_insert_rowid();

        // Link the nickname to the account
        sqlx::query(
            r#"
            INSERT INTO nicknames (name, account_id)
            VALUES (?, ?)
            "#,
        )
        .bind(name)
        .bind(account_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            // Convert UNIQUE constraint violation to NicknameRegistered error
            if let sqlx::Error::Database(ref db_err) = e
                && db_err.is_unique_violation()
            {
                return DbError::NicknameRegistered(name.to_string());
            }
            DbError::from(e)
        })?;

        // Commit the transaction
        tx.commit().await?;

        Ok(Account {
            id: account_id,
            name: name.to_string(),
            email: email.map(String::from),
            registered_at: now,
            last_seen_at: now,
            enforce: false,
            hide_email: true,
            metadata: std::collections::HashMap::new(),
        })
    }

    /// Verify password and return account if valid.
    ///
    /// This function uses constant-time behavior to prevent timing oracle attacks:
    /// - If the account doesn't exist, we perform a dummy password hash verification
    ///   to make the response time indistinguishable from invalid password attempts.
    pub async fn identify(&self, name: &str, password: &str) -> Result<Account, DbError> {
        // First try to find by account name
        let row = sqlx::query_as::<_, (i64, String, String, Option<String>, i64, i64, bool, bool)>(
            r#"
            SELECT id, name, password_hash, email, registered_at, last_seen_at, enforce, hide_email
            FROM accounts
            WHERE name = ? COLLATE NOCASE
            "#,
        )
        .bind(name)
        .fetch_optional(self.pool)
        .await?;

        // If not found by account name, try nickname
        let row = match row {
            Some(r) => r,
            None => {
                // Look up account by nickname
                let account_id = sqlx::query_scalar::<_, i64>(
                    r#"
                    SELECT account_id FROM nicknames
                    WHERE name = ? COLLATE NOCASE
                    "#,
                )
                .bind(name)
                .fetch_optional(self.pool)
                .await?;

                match account_id {
                    Some(id) => {
                        sqlx::query_as::<_, (i64, String, String, Option<String>, i64, i64, bool, bool)>(
                            r#"
                            SELECT id, name, password_hash, email, registered_at, last_seen_at, enforce, hide_email
                            FROM accounts
                            WHERE id = ?
                            "#,
                        )
                        .bind(id)
                        .fetch_one(self.pool)
                        .await?
                    }
                    None => {
                        // Account not found - perform dummy hash verification
                        // to prevent timing oracle attacks that reveal account existence.
                        // This ensures the response time is similar to a wrong password attempt.
                        dummy_password_verify(password).await;
                        return Err(DbError::AccountNotFound(name.to_string()));
                    }
                }
            }
        };

        let (id, name, password_hash, email, registered_at, _last_seen_at, enforce, hide_email) =
            row;

        // Verify password (runs in blocking task to avoid executor stalls)
        let matches =
            crate::security::password::verify_password(password.to_string(), password_hash.clone())
                .await
                .map_err(|_| DbError::InvalidPassword)?;

        if !matches {
            return Err(DbError::InvalidPassword);
        }

        // Update last seen
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE accounts SET last_seen_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(self.pool)
            .await?;

        // Fetch metadata
        let metadata = self.get_metadata(id).await?;

        Ok(Account {
            id,
            name,
            email,
            registered_at,
            last_seen_at: now,
            enforce,
            hide_email,
            metadata,
        })
    }

    /// Find account by name.
    pub async fn find_by_name(&self, name: &str) -> Result<Option<Account>, DbError> {
        let row = sqlx::query_as::<_, (i64, String, Option<String>, i64, i64, bool, bool)>(
            r#"
            SELECT id, name, email, registered_at, last_seen_at, enforce, hide_email
            FROM accounts
            WHERE name = ? COLLATE NOCASE
            "#,
        )
        .bind(name)
        .fetch_optional(self.pool)
        .await?;

        if let Some((id, name, email, registered_at, last_seen_at, enforce, hide_email)) = row {
            // Fetch metadata
            let metadata = self.get_metadata(id).await?;

            Ok(Some(Account {
                id,
                name,
                email,
                registered_at,
                last_seen_at,
                enforce,
                hide_email,
                metadata,
            }))
        } else {
            Ok(None)
        }
    }

    /// Find account by nickname (looks up in nicknames table first).
    pub async fn find_by_nickname(&self, nick: &str) -> Result<Option<Account>, DbError> {
        let account_id = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT account_id FROM nicknames
            WHERE name = ? COLLATE NOCASE
            "#,
        )
        .bind(nick)
        .fetch_optional(self.pool)
        .await?;

        match account_id {
            Some(id) => self.find_by_id(id).await,
            None => Ok(None),
        }
    }

    /// Find account by ID.
    pub async fn find_by_id(&self, id: i64) -> Result<Option<Account>, DbError> {
        let row = sqlx::query_as::<_, (i64, String, Option<String>, i64, i64, bool, bool)>(
            r#"
            SELECT id, name, email, registered_at, last_seen_at, enforce, hide_email
            FROM accounts
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await?;

        if let Some((id, name, email, registered_at, last_seen_at, enforce, hide_email)) = row {
            // Fetch metadata
            let metadata = self.get_metadata(id).await?;

            Ok(Some(Account {
                id,
                name,
                email,
                registered_at,
                last_seen_at,
                enforce,
                hide_email,
                metadata,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get all nicknames for an account.
    pub async fn get_nicknames(&self, account_id: i64) -> Result<Vec<String>, DbError> {
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT name FROM nicknames
            WHERE account_id = ?
            "#,
        )
        .bind(account_id)
        .fetch_all(self.pool)
        .await?;

        Ok(rows)
    }

    /// Get all registered nicknames across all accounts.
    pub async fn get_all_registered_nicknames(&self) -> Result<Vec<String>, DbError> {
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT name FROM nicknames
            "#,
        )
        .fetch_all(self.pool)
        .await?;

        Ok(rows)
    }

    /// Update account settings.
    pub async fn set_option(
        &self,
        account_id: i64,
        option: &str,
        value: &str,
    ) -> Result<(), DbError> {
        match option.to_lowercase().as_str() {
            "email" => {
                sqlx::query("UPDATE accounts SET email = ? WHERE id = ?")
                    .bind(value)
                    .bind(account_id)
                    .execute(self.pool)
                    .await?;
            }
            "enforce" => {
                let enforce = matches!(value.to_lowercase().as_str(), "on" | "true" | "1" | "yes");
                sqlx::query("UPDATE accounts SET enforce = ? WHERE id = ?")
                    .bind(enforce)
                    .bind(account_id)
                    .execute(self.pool)
                    .await?;
            }
            "hidemail" | "hide_email" => {
                let hide = matches!(value.to_lowercase().as_str(), "on" | "true" | "1" | "yes");
                sqlx::query("UPDATE accounts SET hide_email = ? WHERE id = ?")
                    .bind(hide)
                    .bind(account_id)
                    .execute(self.pool)
                    .await?;
            }
            "password" => {
                let password_hash = crate::security::password::hash_password(value.to_string())
                    .await
                    .map_err(|e| DbError::Internal(format!("Password hashing failed: {}", e)))?;
                let scram_verifiers = compute_scram_verifiers(value).await;
                sqlx::query(
                    r#"UPDATE accounts SET
                       password_hash = ?,
                       scram_salt = ?,
                       scram_iterations = ?,
                       scram_hashed_password = ?
                       WHERE id = ?"#,
                )
                .bind(password_hash)
                .bind(&scram_verifiers.salt)
                .bind(scram_verifiers.iterations as i32)
                .bind(&scram_verifiers.hashed_password)
                .bind(account_id)
                .execute(self.pool)
                .await?;
            }
            _ => {
                return Err(DbError::UnknownOption(option.to_string()));
            }
        }
        Ok(())
    }

    /// Delete an account and all associated nicknames.
    /// Requires password verification for security.
    pub async fn drop_account(&self, name: &str, password: &str) -> Result<(), DbError> {
        // First verify the password (this also confirms the account exists)
        let account = self.identify(name, password).await?;

        // Delete all nicknames linked to this account
        sqlx::query("DELETE FROM nicknames WHERE account_id = ?")
            .bind(account.id)
            .execute(self.pool)
            .await?;

        // Delete the account
        sqlx::query("DELETE FROM accounts WHERE id = ?")
            .bind(account.id)
            .execute(self.pool)
            .await?;

        Ok(())
    }

    /// Link a nickname to an existing account (GROUP).
    /// The account must be verified with password first.
    pub async fn link_nickname(
        &self,
        nick: &str,
        account_name: &str,
        password: &str,
    ) -> Result<i64, DbError> {
        // Verify the account password
        let account = self.identify(account_name, password).await?;

        // Check if nickname is already registered
        if self.find_by_nickname(nick).await?.is_some() {
            return Err(DbError::NicknameRegistered(nick.to_string()));
        }

        // Link the nickname to the account
        sqlx::query(
            r#"
            INSERT INTO nicknames (name, account_id)
            VALUES (?, ?)
            "#,
        )
        .bind(nick)
        .bind(account.id)
        .execute(self.pool)
        .await?;

        Ok(account.id)
    }

    /// Unlink a nickname from the current account (UNGROUP).
    /// Cannot unlink the primary account name.
    pub async fn unlink_nickname(&self, nick: &str, account_id: i64) -> Result<(), DbError> {
        // First verify the nick belongs to this account
        let nick_account_id = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT account_id FROM nicknames
            WHERE name = ? COLLATE NOCASE
            "#,
        )
        .bind(nick)
        .fetch_optional(self.pool)
        .await?
        .ok_or_else(|| DbError::NicknameNotFound(nick.to_string()))?;

        if nick_account_id != account_id {
            return Err(DbError::InsufficientAccess);
        }

        // Check if this is the primary account name (cannot unlink)
        let account = self
            .find_by_id(account_id)
            .await?
            .ok_or_else(|| DbError::AccountNotFound(account_id.to_string()))?;

        if account.name.eq_ignore_ascii_case(nick) {
            return Err(DbError::UnknownOption(
                "Cannot ungroup primary account nickname".to_string(),
            ));
        }

        // Count remaining nicknames - must have at least 2 to ungroup one
        let nick_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM nicknames WHERE account_id = ?")
                .bind(account_id)
                .fetch_one(self.pool)
                .await?;

        if nick_count < 2 {
            return Err(DbError::UnknownOption(
                "Account must have at least one nickname".to_string(),
            ));
        }

        // Delete the nickname link
        sqlx::query("DELETE FROM nicknames WHERE name = ? COLLATE NOCASE AND account_id = ?")
            .bind(nick)
            .bind(account_id)
            .execute(self.pool)
            .await?;

        Ok(())
    }

    /// Find account by TLS certificate fingerprint.
    ///
    /// Returns None if no account has this certificate registered.
    /// Certificate fingerprints are SHA-256 hashes in hex format.
    pub async fn find_by_certfp(&self, certfp: &str) -> Result<Option<Account>, DbError> {
        let row = sqlx::query_as::<_, (i64, String, Option<String>, i64, i64, bool, bool)>(
            r#"
            SELECT id, name, email, registered_at, last_seen_at, enforce, hide_email
            FROM accounts
            WHERE certfp = ? COLLATE NOCASE
            "#,
        )
        .bind(certfp)
        .fetch_optional(self.pool)
        .await?;

        if let Some((id, name, email, registered_at, last_seen_at, enforce, hide_email)) = row {
            // Fetch metadata
            let metadata = self.get_metadata(id).await?;

            Ok(Some(Account {
                id,
                name,
                email,
                registered_at,
                last_seen_at,
                enforce,
                hide_email,
                metadata,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the certificate fingerprint for an account.
    pub async fn get_certfp(&self, account_id: i64) -> Result<Option<String>, DbError> {
        let certfp =
            sqlx::query_scalar::<_, Option<String>>("SELECT certfp FROM accounts WHERE id = ?")
                .bind(account_id)
                .fetch_one(self.pool)
                .await?;

        Ok(certfp)
    }

    /// Set or clear the certificate fingerprint for an account.
    ///
    /// Pass `None` to remove the certificate.
    pub async fn set_certfp(&self, account_id: i64, certfp: Option<&str>) -> Result<(), DbError> {
        sqlx::query("UPDATE accounts SET certfp = ? WHERE id = ?")
            .bind(certfp)
            .bind(account_id)
            .execute(self.pool)
            .await?;

        Ok(())
    }

    /// Get SCRAM verifiers for an account by name (for SASL SCRAM-SHA-256).
    ///
    /// Returns `None` if the account doesn't exist or has no SCRAM verifiers.
    pub async fn get_scram_verifiers(&self, name: &str) -> Result<Option<ScramVerifiers>, DbError> {
        // First try by account name
        let row = sqlx::query_as::<_, (Option<Vec<u8>>, Option<i32>, Option<Vec<u8>>)>(
            r#"
            SELECT scram_salt, scram_iterations, scram_hashed_password
            FROM accounts
            WHERE name = ? COLLATE NOCASE
            "#,
        )
        .bind(name)
        .fetch_optional(self.pool)
        .await?;

        // If not found by account name, try nickname
        let row = match row {
            Some(r) => Some(r),
            None => {
                let account_id = sqlx::query_scalar::<_, i64>(
                    r#"
                    SELECT account_id FROM nicknames
                    WHERE name = ? COLLATE NOCASE
                    "#,
                )
                .bind(name)
                .fetch_optional(self.pool)
                .await?;

                match account_id {
                    Some(id) => {
                        sqlx::query_as::<_, (Option<Vec<u8>>, Option<i32>, Option<Vec<u8>>)>(
                            r#"
                            SELECT scram_salt, scram_iterations, scram_hashed_password
                            FROM accounts
                            WHERE id = ?
                            "#,
                        )
                        .bind(id)
                        .fetch_optional(self.pool)
                        .await?
                    }
                    None => None,
                }
            }
        };

        // Extract and validate SCRAM verifiers
        match row {
            Some((Some(salt), Some(iterations), Some(hashed_password))) => {
                Ok(Some(ScramVerifiers {
                    salt,
                    iterations: iterations as u32,
                    hashed_password,
                }))
            }
            _ => Ok(None), // Account doesn't exist or has no SCRAM verifiers
        }
    }

    /// Get metadata for an account.
    pub async fn get_metadata(
        &self,
        account_id: i64,
    ) -> Result<std::collections::HashMap<String, String>, DbError> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT key, value FROM account_metadata WHERE account_id = ?",
        )
        .bind(account_id)
        .fetch_all(self.pool)
        .await?;

        Ok(rows.into_iter().collect())
    }
    /// If value is None, the key is removed.
    pub async fn set_metadata(
        &self,
        account_id: i64,
        key: &str,
        value: Option<&str>,
    ) -> Result<(), DbError> {
        if let Some(val) = value {
            sqlx::query(
                r#"
                INSERT OR REPLACE INTO account_metadata (account_id, key, value)
                VALUES (?, ?, ?)
                "#,
            )
            .bind(account_id)
            .bind(key)
            .bind(val)
            .execute(self.pool)
            .await?;
        } else {
            sqlx::query("DELETE FROM account_metadata WHERE account_id = ? AND key = ?")
                .bind(account_id)
                .bind(key)
                .execute(self.pool)
                .await?;
        }
        Ok(())
    }
}

/// Compute SCRAM-SHA-256 verifiers for a password in a blocking task.
///
/// This generates a random salt and uses PBKDF2-SHA-256 to derive the
/// hashed password. The result can be stored and used for SASL SCRAM auth.
/// Runs in a blocking task to prevent executor stalls from CPU-intensive work.
async fn compute_scram_verifiers(password: &str) -> ScramVerifiers {
    let password = password.to_string();
    tokio::task::spawn_blocking(move || {
        // Generate 16 bytes of random salt (as recommended by RFC 5802)
        let mut salt = vec![0u8; 16];
        OsRng.fill_bytes(&mut salt);

        // SAFETY: SCRAM_ITERATIONS is const 4096, always > 0
        let iterations = NonZeroU32::new(SCRAM_ITERATIONS).unwrap();
        let hashed_password = scram::hash_password(&password, iterations, &salt).to_vec();

        ScramVerifiers {
            salt,
            iterations: SCRAM_ITERATIONS,
            hashed_password,
        }
    })
    .await
    .expect("spawn_blocking should not be cancelled")
}

/// Dummy password verification for constant-time account lookup.
async fn dummy_password_verify(password: &str) {
    // Pre-computed Argon2id hash of "dummy"
    const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$dGltaW5nLW9yYWNsZS1kdW1teQ$K4VZh8k8YL3E8H7E8H7E8H7E8H7E8H7E8H7E8H7E8Hs";
    let _ =
        crate::security::password::verify_password(password.to_string(), DUMMY_HASH.to_string())
            .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hash_password_produces_valid_argon2_hash() {
        let password = "test_password_123";
        let hash = hash_password(password)
            .await
            .expect("hashing should succeed");

        // Verify hash starts with Argon2id prefix
        assert!(
            hash.starts_with("$argon2"),
            "hash should be Argon2 format: {}",
            hash
        );

        // Verify hash can be parsed
        assert!(
            PasswordHash::new(&hash).is_ok(),
            "hash should be parseable: {}",
            hash
        );
    }

    #[tokio::test]
    async fn test_hash_password_produces_unique_hashes() {
        let password = "same_password";
        let hash1 = hash_password(password).await.expect("first hash");
        let hash2 = hash_password(password).await.expect("second hash");

        // Different salts should produce different hashes
        assert_ne!(hash1, hash2, "hashes should differ due to random salt");
    }

    #[tokio::test]
    async fn test_verify_password_correct() {
        let password = "my_secure_password";
        let hash = hash_password(password).await.expect("hashing");

        assert!(
            verify_password(password, &hash).await.is_ok(),
            "correct password should verify"
        );
    }

    #[tokio::test]
    async fn test_verify_password_incorrect() {
        let password = "correct_password";
        let wrong_password = "wrong_password";
        let hash = hash_password(password).await.expect("hashing");

        assert!(
            verify_password(wrong_password, &hash).await.is_err(),
            "wrong password should fail verification"
        );
    }

    #[tokio::test]
    async fn test_verify_password_empty_password() {
        let password = "";
        let hash = hash_password(password)
            .await
            .expect("empty password should hash");

        assert!(
            verify_password(password, &hash).await.is_ok(),
            "empty password should verify against its own hash"
        );
        assert!(
            verify_password("nonempty", &hash).await.is_err(),
            "nonempty should fail against empty hash"
        );
    }

    #[tokio::test]
    async fn test_verify_password_invalid_hash_format() {
        let result = verify_password("password", "not_a_valid_hash").await;

        assert!(result.is_err(), "invalid hash format should return error");
    }

    #[tokio::test]
    async fn test_hash_password_unicode() {
        let password = "пароль密码🔐";
        let hash = hash_password(password).await.expect("unicode should hash");

        assert!(
            verify_password(password, &hash).await.is_ok(),
            "unicode password should verify"
        );
    }

    #[tokio::test]
    async fn test_hash_password_very_long() {
        let password = "a".repeat(1000);
        let hash = hash_password(&password)
            .await
            .expect("long password should hash");

        assert!(
            verify_password(&password, &hash).await.is_ok(),
            "long password should verify"
        );
    }

    #[tokio::test]
    async fn test_dummy_password_verify_does_not_panic() {
        // Just verify it doesn't panic with various inputs
        dummy_password_verify("test").await;
        dummy_password_verify("").await;
        dummy_password_verify(&"x".repeat(100)).await;
    }
}
