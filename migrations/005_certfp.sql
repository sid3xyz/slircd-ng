-- Add certificate fingerprint column for SASL EXTERNAL authentication
-- Allows users to authenticate using their TLS client certificate

-- Add certfp column to accounts (nullable - not all accounts have certs)
ALTER TABLE accounts ADD COLUMN certfp TEXT;

-- Create index for efficient certfp lookups during SASL EXTERNAL
CREATE UNIQUE INDEX idx_accounts_certfp ON accounts(certfp) WHERE certfp IS NOT NULL;
