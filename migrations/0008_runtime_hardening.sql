ALTER TABLE verification_codes
ADD COLUMN failed_attempts INTEGER NOT NULL DEFAULT 0
CHECK (failed_attempts >= 0);

CREATE INDEX daemon_setup_requests_expires_at_idx
ON daemon_setup_requests (expires_at);
