//! SCRAM-SHA-256 SASL mechanism (RFC 5802, RFC 7677).
//!
//! Challenge-response authentication mechanism.
//!
//! # Feature Flag
//!
//! Full SCRAM-SHA-256 support requires the `scram` feature flag, which enables
//! the cryptographic dependencies (`sha2`, `hmac`, `pbkdf2`, `getrandom`).
//!
//! Without the `scram` feature, the state machine is available but
//! `process_server_first()` returns `ScramError::CryptoNotAvailable`.
//!
//! # SCRAM Protocol Flow
//!
//! 1. Client sends `client-first-message`: `n,,n=user,r=nonce`
//! 2. Server sends `server-first-message`: `r=nonce+server,s=salt,i=iterations`
//! 3. Client sends `client-final-message`: `c=biws,r=nonce+server,p=proof`
//! 4. Server sends `server-final-message`: `v=verifier`
//!
//! # Example
//!
//! ```ignore
//! use slirc_proto::sasl::ScramClient;
//!
//! let mut client = ScramClient::new("username", "password");
//! let first = client.client_first_message();
//! // Send first to server, receive server_first back
//! # #[cfg(feature = "scram")]
//! let final_msg = client.process_server_first(&server_first)?;
//! // Send final_msg to server, receive server_final back
//! # #[cfg(feature = "scram")]
//! client.verify_server_final(&server_final)?;
//! ```
//!
//! # Reference
//! - RFC 5802: <https://tools.ietf.org/html/rfc5802>
//! - RFC 7677: <https://tools.ietf.org/html/rfc7677>

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

use super::decode_base64;

#[cfg(feature = "scram")]
use hmac::{Hmac, Mac};
#[cfg(feature = "scram")]
use sha2::{Digest, Sha256};

/// SCRAM-SHA-256 client state machine.
///
/// Provides the full SCRAM-SHA-256 authentication flow when the `scram`
/// feature is enabled.
///
/// # Example
///
/// ```
/// use slirc_proto::sasl::ScramClient;
///
/// let mut client = ScramClient::new("username", "password").unwrap();
/// let first_message = client.client_first_message();
/// // Send first_message to server via AUTHENTICATE
/// ```
#[derive(Clone, Debug)]
pub struct ScramClient {
    username: String,
    #[cfg(feature = "scram")]
    password: String,
    client_nonce: String,
    /// Stored for AuthMessage computation
    client_first_message_bare: String,
    /// Stored server-first-message for AuthMessage
    server_first_message: String,
    state: ScramState,
    /// Stored for server verification (only with scram feature)
    #[cfg(feature = "scram")]
    server_signature: Option<[u8; 32]>,
}

/// Internal state of SCRAM authentication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScramState {
    /// Initial state.
    Initial,
    /// Sent client-first, awaiting server-first.
    ClientFirstSent,
    /// Received server-first, ready to send client-final.
    ServerFirstReceived {
        /// Combined nonce (client + server).
        nonce: String,
        /// Salt from server (base64 decoded).
        salt: Vec<u8>,
        /// Iteration count.
        iterations: u32,
    },
    /// Sent client-final, awaiting server-final.
    ClientFinalSent,
    /// Authentication complete.
    Complete,
    /// Authentication failed.
    Failed(String),
}

impl ScramClient {
    /// Create a new SCRAM client with the given credentials.
    #[allow(unused_variables)] // password used only with scram feature
    pub fn new(username: &str, password: &str) -> Result<Self, ScramError> {
        let nonce = generate_nonce()?;

        Ok(Self {
            username: username.to_string(),
            #[cfg(feature = "scram")]
            password: password.to_string(),
            client_nonce: nonce,
            client_first_message_bare: String::new(),
            server_first_message: String::new(),
            state: ScramState::Initial,
            #[cfg(feature = "scram")]
            server_signature: None,
        })
    }

    /// Get the current SCRAM state.
    #[must_use]
    pub fn state(&self) -> &ScramState {
        &self.state
    }

    /// Generate the client-first-message.
    ///
    /// This is the first message sent to the server after AUTHENTICATE SCRAM-SHA-256.
    /// Returns a base64-encoded message ready for transmission.
    #[must_use]
    pub fn client_first_message(&mut self) -> String {
        self.state = ScramState::ClientFirstSent;

        // gs2-header: n,, (no channel binding, no authzid)
        // client-first-message-bare: n=username,r=nonce
        let bare = format!("n={},r={}", saslprep(&self.username), self.client_nonce);
        self.client_first_message_bare = bare.clone();
        let full = format!("n,,{bare}");

        BASE64.encode(full.as_bytes())
    }

    /// Process the server-first-message and generate client-final-message.
    ///
    /// # Arguments
    ///
    /// * `server_first` - The base64-encoded server-first-message.
    ///
    /// # Returns
    ///
    /// The base64-encoded client-final-message, or an error.
    ///
    /// # Errors
    ///
    /// Returns `ScramError::CryptoNotAvailable` if the `scram` feature is not enabled.
    pub fn process_server_first(&mut self, server_first: &str) -> Result<String, ScramError> {
        let decoded = decode_base64(server_first).map_err(|_| ScramError::InvalidEncoding)?;
        let message = String::from_utf8(decoded).map_err(|_| ScramError::InvalidEncoding)?;

        // Store for AuthMessage computation
        self.server_first_message = message.clone();

        // Parse server-first-message: r=nonce,s=salt,i=iterations
        let mut nonce = None;
        let mut salt = None;
        let mut iterations = None;

        for part in message.split(',') {
            if let Some(value) = part.strip_prefix("r=") {
                nonce = Some(value.to_string());
            } else if let Some(value) = part.strip_prefix("s=") {
                salt = Some(decode_base64(value).map_err(|_| ScramError::InvalidEncoding)?);
            } else if let Some(value) = part.strip_prefix("i=") {
                iterations = Some(value.parse().map_err(|_| ScramError::InvalidIterations)?);
            }
        }

        let nonce = nonce.ok_or(ScramError::MissingNonce)?;
        let salt = salt.ok_or(ScramError::MissingSalt)?;
        let iterations = iterations.ok_or(ScramError::MissingIterations)?;

        // Verify that server nonce starts with our client nonce
        if !nonce.starts_with(&self.client_nonce) {
            return Err(ScramError::NonceMismatch);
        }

        self.state = ScramState::ServerFirstReceived {
            nonce: nonce.clone(),
            salt: salt.clone(),
            iterations,
        };

        #[cfg(feature = "scram")]
        {
            self.compute_client_final(&nonce, &salt, iterations)
        }

        #[cfg(not(feature = "scram"))]
        {
            let _ = (nonce, salt, iterations);
            Err(ScramError::CryptoNotAvailable)
        }
    }

    /// Compute and return the client-final-message (requires `scram` feature).
    #[cfg(feature = "scram")]
    fn compute_client_final(
        &mut self,
        nonce: &str,
        salt: &[u8],
        iterations: u32,
    ) -> Result<String, ScramError> {
        // Compute SaltedPassword = Hi(Normalize(password), salt, i)
        let salted_password = hi(&self.password, salt, iterations)?;

        // ClientKey = HMAC(SaltedPassword, "Client Key")
        let client_key = hmac_sha256(&salted_password, b"Client Key")?;

        // StoredKey = H(ClientKey)
        let stored_key = sha256(&client_key);

        // client-final-message-without-proof = c=biws,r=nonce
        // biws = base64("n,,") for no channel binding
        let client_final_without_proof = format!("c=biws,r={nonce}");

        // AuthMessage = client-first-message-bare + "," +
        //               server-first-message + "," +
        //               client-final-message-without-proof
        let auth_message = format!(
            "{},{},{}",
            self.client_first_message_bare, self.server_first_message, client_final_without_proof
        );

        // ClientSignature = HMAC(StoredKey, AuthMessage)
        let client_signature = hmac_sha256(&stored_key, auth_message.as_bytes())?;

        // ClientProof = ClientKey XOR ClientSignature
        let client_proof: Vec<u8> = client_key
            .iter()
            .zip(client_signature.iter())
            .map(|(a, b)| a ^ b)
            .collect();

        // ServerKey = HMAC(SaltedPassword, "Server Key")
        let server_key = hmac_sha256(&salted_password, b"Server Key")?;

        // ServerSignature = HMAC(ServerKey, AuthMessage)
        let server_signature = hmac_sha256(&server_key, auth_message.as_bytes())?;
        self.server_signature = Some(server_signature);

        // Build client-final-message
        let client_final = format!(
            "{},p={}",
            client_final_without_proof,
            BASE64.encode(&client_proof)
        );

        self.state = ScramState::ClientFinalSent;

        Ok(BASE64.encode(client_final.as_bytes()))
    }

    /// Verify the server-final-message.
    ///
    /// # Arguments
    ///
    /// * `server_final` - The base64-encoded server-final-message.
    ///
    /// # Returns
    ///
    /// `Ok(())` if verification succeeds, or an error.
    ///
    /// # Errors
    ///
    /// - `ScramError::CryptoNotAvailable` if `scram` feature not enabled
    /// - `ScramError::ServerVerificationFailed` if signature doesn't match
    pub fn verify_server_final(&mut self, server_final: &str) -> Result<(), ScramError> {
        #[cfg(feature = "scram")]
        {
            let decoded = decode_base64(server_final).map_err(|_| ScramError::InvalidEncoding)?;
            let message = String::from_utf8(decoded).map_err(|_| ScramError::InvalidEncoding)?;

            // Parse v=verifier
            let verifier = message
                .strip_prefix("v=")
                .ok_or(ScramError::ServerVerificationFailed)?;

            let server_sig = decode_base64(verifier).map_err(|_| ScramError::InvalidEncoding)?;

            let expected = self
                .server_signature
                .as_ref()
                .ok_or(ScramError::ServerVerificationFailed)?;

            if server_sig == *expected {
                self.state = ScramState::Complete;
                Ok(())
            } else {
                self.state = ScramState::Failed("server verification failed".to_string());
                Err(ScramError::ServerVerificationFailed)
            }
        }

        #[cfg(not(feature = "scram"))]
        {
            let _ = server_final;
            Err(ScramError::CryptoNotAvailable)
        }
    }
}

/// Errors that can occur during SCRAM authentication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScramError {
    /// Base64 decoding failed.
    InvalidEncoding,
    /// Server nonce doesn't match client nonce prefix.
    NonceMismatch,
    /// Missing nonce in server message.
    MissingNonce,
    /// Missing salt in server message.
    MissingSalt,
    /// Missing iteration count in server message.
    MissingIterations,
    /// Invalid iteration count.
    InvalidIterations,
    /// Server verification failed.
    ServerVerificationFailed,
    /// Cryptographic error.
    CryptoError(String),
    /// Cryptographic operations not available (requires `scram` feature).
    CryptoNotAvailable,
}

impl std::fmt::Display for ScramError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEncoding => write!(f, "invalid base64 encoding"),
            Self::NonceMismatch => write!(f, "server nonce doesn't match client nonce"),
            Self::MissingNonce => write!(f, "missing nonce in server message"),
            Self::MissingSalt => write!(f, "missing salt in server message"),
            Self::MissingIterations => write!(f, "missing iteration count"),
            Self::InvalidIterations => write!(f, "invalid iteration count"),
            Self::ServerVerificationFailed => write!(f, "server verification failed"),
            Self::CryptoError(msg) => write!(f, "crypto error: {}", msg),
            Self::CryptoNotAvailable => {
                write!(f, "SCRAM crypto not available (requires scram feature)")
            }
        }
    }
}

impl std::error::Error for ScramError {}

// ============================================================================
// Cryptographic primitives (only with scram feature)
// ============================================================================

/// Hi() function from RFC 5802 - essentially PBKDF2-HMAC-SHA256.
#[cfg(feature = "scram")]
fn hi(password: &str, salt: &[u8], iterations: u32) -> Result<[u8; 32], ScramError> {
    let mut output = [0u8; 32];
    pbkdf2::pbkdf2::<Hmac<Sha256>>(password.as_bytes(), salt, iterations, &mut output)
        .map_err(|_| ScramError::CryptoError("PBKDF2 failed".to_string()))?;
    Ok(output)
}

/// HMAC-SHA-256.
#[cfg(feature = "scram")]
fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<[u8; 32], ScramError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .map_err(|_| ScramError::CryptoError("HMAC initialization failed".to_string()))?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().into())
}

/// SHA-256 hash.
#[cfg(feature = "scram")]
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

// ============================================================================
// Nonce generation
// ============================================================================

/// Generate a random nonce for SCRAM.
///
/// With the `scram` feature, uses a cryptographically secure random generator.
/// Without it, falls back to a timestamp-based nonce (not secure, but functional
/// for the state machine skeleton).
#[cfg(feature = "scram")]
fn generate_nonce() -> Result<String, ScramError> {
    let mut bytes = [0u8; 24];
    getrandom::getrandom(&mut bytes).map_err(|e| ScramError::CryptoError(e.to_string()))?;
    Ok(BASE64.encode(bytes))
}

#[cfg(not(feature = "scram"))]
fn generate_nonce() -> Result<String, ScramError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    Ok(format!("{}_{}", now.as_nanos(), std::process::id()))
}

// ============================================================================
// SASLprep (simplified)
// ============================================================================

/// Perform SASLprep normalization on a string (RFC 4013).
///
/// This is a simplified version that handles ASCII usernames/passwords.
/// Full RFC 4013 compliance requires Unicode NFKC normalization.
fn saslprep(s: &str) -> String {
    // For ASCII-only inputs, SASLprep is essentially a pass-through.
    // A full implementation would:
    // 1. Map certain characters (e.g., NFKC normalization)
    // 2. Check for prohibited characters
    // 3. Check bidirectional text rules
    s.to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_first_message_format() {
        let mut client = ScramClient::new("user", "pencil").unwrap();
        let first = client.client_first_message();
        let decoded = String::from_utf8(BASE64.decode(&first).unwrap()).unwrap();

        assert!(decoded.starts_with("n,,n=user,r="));
        assert!(matches!(client.state(), ScramState::ClientFirstSent));
    }

    #[test]
    fn test_parse_server_first_validates_nonce() {
        let mut client = ScramClient::new("user", "pencil").unwrap();
        let _ = client.client_first_message();

        // Server nonce that doesn't start with client nonce
        let bad_server_first = BASE64.encode(b"r=wrong_nonce,s=QSXCR+Q6sek8bf92,i=4096");
        let result = client.process_server_first(&bad_server_first);

        assert_eq!(result.unwrap_err(), ScramError::NonceMismatch);
    }

    #[test]
    fn test_missing_fields_error() {
        let mut client = ScramClient::new("user", "pencil").unwrap();
        let _ = client.client_first_message();

        // Missing salt
        let nonce = &client.client_nonce;
        let bad = BASE64.encode(format!("r={nonce}server,i=4096").as_bytes());
        assert_eq!(
            client.process_server_first(&bad).unwrap_err(),
            ScramError::MissingSalt
        );
    }

    /// RFC 7677 test vector.
    /// Username: user, Password: pencil
    #[cfg(feature = "scram")]
    #[test]
    fn test_scram_sha256_rfc7677_vector() {
        // Create client with the exact nonce from RFC 7677
        let mut client = ScramClient {
            username: "user".to_string(),
            password: "pencil".to_string(),
            client_nonce: "rOprNGfwEbeRWgbNEkqO".to_string(),
            client_first_message_bare: String::new(),
            server_first_message: String::new(),
            state: ScramState::Initial,
            server_signature: None,
        };

        // Step 1: Client first message
        let client_first = client.client_first_message();
        let decoded_first = String::from_utf8(BASE64.decode(&client_first).unwrap()).unwrap();
        assert_eq!(decoded_first, "n,,n=user,r=rOprNGfwEbeRWgbNEkqO");

        // Step 2: Server first message (from RFC 7677 example)
        let server_first = BASE64.encode(
            b"r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096",
        );
        let client_final = client.process_server_first(&server_first).unwrap();

        // Decode and verify client-final structure
        let decoded_final = String::from_utf8(BASE64.decode(&client_final).unwrap()).unwrap();
        assert!(decoded_final
            .starts_with("c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,p="));

        // Extract proof and verify it matches RFC 7677 expected value
        let proof_part = decoded_final.split(",p=").nth(1).unwrap();
        assert_eq!(proof_part, "dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=");

        // Step 3: Verify server final (RFC 7677 expected server signature)
        let server_final = BASE64.encode(b"v=6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=");
        client.verify_server_final(&server_final).unwrap();

        assert!(matches!(client.state(), ScramState::Complete));
    }

    #[cfg(feature = "scram")]
    #[test]
    fn test_hi_pbkdf2() {
        // Test vector: simple verification that Hi produces 32 bytes
        let result = hi("password", b"salt", 4096).unwrap();
        assert_eq!(result.len(), 32);
    }

    #[cfg(feature = "scram")]
    #[test]
    fn test_nonce_is_cryptographically_random() {
        let n1 = generate_nonce().unwrap();
        let n2 = generate_nonce().unwrap();
        assert_ne!(n1, n2);
        // Base64 of 24 bytes = 32 chars
        assert_eq!(n1.len(), 32);
    }

    #[cfg(not(feature = "scram"))]
    #[test]
    fn test_crypto_not_available_without_feature() {
        let mut client = ScramClient::new("user", "pencil").unwrap();
        let _ = client.client_first_message();

        let nonce = client.client_nonce.clone();
        let server_first =
            BASE64.encode(format!("r={nonce}server,s=QSXCR+Q6sek8bf92,i=4096").as_bytes());

        assert_eq!(
            client.process_server_first(&server_first).unwrap_err(),
            ScramError::CryptoNotAvailable
        );
    }
}
