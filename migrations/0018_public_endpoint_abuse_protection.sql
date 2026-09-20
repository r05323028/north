-- Persist canonical client CIDR for durable daemon setup quotas.
-- Legacy rows remain NULL and are excluded from new keyed quota counts.
ALTER TABLE daemon_setup_requests
    ADD COLUMN client_network_key CIDR;

CREATE INDEX daemon_setup_requests_client_network_key_expires_at_idx
    ON daemon_setup_requests (client_network_key, expires_at)
    WHERE claimed_at IS NULL;
