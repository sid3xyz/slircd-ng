//! Password hashing and verification utilities.
//!
//! Centralizes Argon2 password handling for User Accounts and Operator Blocks.

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};

/// Verify a password against a stored Argon2 hash.
/// Verify a password against a stored Argon2 hash (non-blocking).
#[must_use = "password verification result must be checked"]
pub async fn verify_password(
    password: String,
    hash: String,
) -> Result<bool, argon2::password_hash::Error> {
    tokio::task::spawn_blocking(move || {
        let parsed_hash = PasswordHash::new(&hash)?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    })
    .await
    .expect("spawn_blocking failed")
}

/// Hash a password using default Argon2 settings.
/// Hash a password using default Argon2 settings (non-blocking).
#[must_use = "password hash must be used"]
pub async fn hash_password(password: String) -> Result<String, argon2::password_hash::Error> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        Ok(argon2
            .hash_password(password.as_bytes(), &salt)?
            .to_string())
    })
    .await
    .expect("spawn_blocking failed")
}
