# Security Model

## Authentication
- **User Accounts**: Passwords are hashed using **Argon2id**.
- **Operators**: Operator passwords are also hashed using Argon2id. Plaintext fallback is legacy and discouraged.
- **Certificates**: Client certificate authentication (SASL EXTERNAL) is supported via TLS.

## Access Control
- **IP Deny**: The server maintains a list of denied IP addresses/ranges.
- **K-Lines/G-Lines**: Operator bans are supported (`src/handlers/bans`).
- **Rate Limiting**: Per-session message rate limiting prevents flood attacks.

## Protocol Security
- **IRCv3 Compliance**: Supports `labeled-response`, `batch`, `server-time` for robust client synchronization.
- **Typestate Dispatch**: The command registry enforces state requirements (e.g., `PRIVMSG` cannot be sent before registration), preventing state-bypass vulnerabilities.

## Implementation Details
- **Password Module**: `src/security/password.rs` centralizes all hashing logic.
- **Safe Rust**: `unsafe` code is minimized and audited. Dependencies like `ring` and `argon2` are used for crypto primitives.
