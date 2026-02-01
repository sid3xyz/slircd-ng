//! Password hashing and verification utilities.
//!
//! Centralizes Argon2 password handling for User Accounts and Operator Blocks.

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};

/// Verify a password against a stored Argon2 hash.
pub fn verify_password(
    password: &str,
    hash: &PasswordHash<'_>,
) -> Result<bool, argon2::password_hash::Error> {
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), hash)
        .is_ok())
}

/// Hash a password using default Argon2 settings.
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    Ok(argon2
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}
