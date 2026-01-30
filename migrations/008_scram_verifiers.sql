-- SCRAM-SHA-256 verifiers for SASL authentication (RFC 5802/7677)
-- Accounts without these columns fall back to PLAIN-only authentication.

ALTER TABLE accounts ADD COLUMN scram_salt BLOB;
ALTER TABLE accounts ADD COLUMN scram_iterations INTEGER;
ALTER TABLE accounts ADD COLUMN scram_hashed_password BLOB;
