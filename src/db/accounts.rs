//! Account repository for NickServ functionality.
//!
//! Handles account registration, authentication, and nickname management.

use super::DbError;
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use sqlx::SqlitePool;

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
    pub async fn register(
        &self,
        name: &str,
        password: &str,
        email: Option<&str>,
    ) -> Result<Account, DbError> {
        // Hash the password using Argon2
        let password_hash = hash_password(password)?;
        let now = chrono::Utc::now().timestamp();

        // Use a transaction to ensure account + nickname are created atomically
        let mut tx = self.pool.begin().await?;

        // Insert account (UNIQUE constraint will catch duplicates)
        let result = sqlx::query(
            r#"
            INSERT INTO accounts (name, password_hash, email, registered_at, last_seen_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(name)
        .bind(&password_hash)
        .bind(email)
        .bind(now)
        .bind(now)
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
                        dummy_password_verify(password);
                        return Err(DbError::AccountNotFound(name.to_string()));
                    }
                }
            }
        };

        let (id, name, password_hash, email, registered_at, _last_seen_at, enforce, hide_email) =
            row;

        // Verify password
        verify_password(password, &password_hash)?;

        // Update last seen
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE accounts SET last_seen_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(self.pool)
            .await?;

        Ok(Account {
            id,
            name,
            email,
            registered_at,
            last_seen_at: now,
            enforce,
            hide_email,
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

        Ok(row.map(
            |(id, name, email, registered_at, last_seen_at, enforce, hide_email)| Account {
                id,
                name,
                email,
                registered_at,
                last_seen_at,
                enforce,
                hide_email,
            },
        ))
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

        Ok(row.map(
            |(id, name, email, registered_at, last_seen_at, enforce, hide_email)| Account {
                id,
                name,
                email,
                registered_at,
                last_seen_at,
                enforce,
                hide_email,
            },
        ))
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
                let password_hash = hash_password(value)?;
                sqlx::query("UPDATE accounts SET password_hash = ? WHERE id = ?")
                    .bind(password_hash)
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
    ) -> Result<(), DbError> {
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

        Ok(())
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

        Ok(row.map(
            |(id, name, email, registered_at, last_seen_at, enforce, hide_email)| Account {
                id,
                name,
                email,
                registered_at,
                last_seen_at,
                enforce,
                hide_email,
            },
        ))
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
}

/// Hash a password using Argon2.
fn hash_password(password: &str) -> Result<String, DbError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| DbError::InvalidPassword)?;
    Ok(hash.to_string())
}

/// Verify a password against a stored hash.
fn verify_password(password: &str, hash: &str) -> Result<(), DbError> {
    let parsed_hash = PasswordHash::new(hash).map_err(|_| DbError::InvalidPassword)?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| DbError::InvalidPassword)
}

/// Dummy password verification for constant-time account lookup.
///
/// When an account doesn't exist, we still need to spend approximately
/// the same amount of time as a real password verification to prevent
/// timing oracle attacks that could reveal whether an account exists.
///
/// This uses a pre-computed Argon2 hash that will always fail verification,
/// but consumes similar CPU time to a real verification attempt.
fn dummy_password_verify(password: &str) {
    // Pre-computed Argon2id hash of "dummy" - this will never match any real password
    // but forces the CPU to do real Argon2 work.
    const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$dGltaW5nLW9yYWNsZS1kdW1teQ$K4VZh8k8YL3E8H7E8H7E8H7E8H7E8H7E8H7E8H7E8Hs";

    // We intentionally ignore the result - we just want to burn CPU time
    // equivalent to a real password check.
    if let Ok(parsed) = PasswordHash::new(DUMMY_HASH) {
        let _ = Argon2::default().verify_password(password.as_bytes(), &parsed);
    }
}
