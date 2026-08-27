-- User-owned daemon setup requests and durable runtime ownership.
CREATE TABLE daemon_setup_requests (
    id TEXT PRIMARY KEY,
    request_token_hash BYTEA NOT NULL UNIQUE,
    label TEXT NOT NULL CHECK (char_length(btrim(label)) BETWEEN 1 AND 100),
    created_by TEXT REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMPTZ NOT NULL,
    approved_at TIMESTAMPTZ,
    claimed_at TIMESTAMPTZ,
    daemon_id TEXT
);

CREATE TABLE daemon_registrations (
    daemon_id TEXT PRIMARY KEY,
    credential_hash BYTEA NOT NULL UNIQUE,
    label TEXT NOT NULL CHECK (char_length(btrim(label)) BETWEEN 1 AND 100),
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    revoked_at TIMESTAMPTZ,
    last_seen_at TIMESTAMPTZ,
    connected_at TIMESTAMPTZ,
    connection_id TEXT UNIQUE,
    protocol_version TEXT NOT NULL CHECK (char_length(btrim(protocol_version)) > 0),
    capabilities TEXT NOT NULL DEFAULT '[]'
);

ALTER TABLE daemon_setup_requests
    ADD CONSTRAINT daemon_setup_requests_daemon_fk
    FOREIGN KEY (daemon_id) REFERENCES daemon_registrations(daemon_id);

CREATE TABLE execution_sessions (
    id TEXT PRIMARY KEY,
    daemon_id TEXT REFERENCES daemon_registrations(daemon_id),
    state TEXT NOT NULL DEFAULT 'Idle'
        CHECK (state IN ('Idle', 'Running', 'Retrying', 'Failed', 'Completed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE server_command_outbox (
    command_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES execution_sessions(id) ON DELETE CASCADE,
    daemon_id TEXT NOT NULL REFERENCES daemon_registrations(daemon_id),
    server_command_seq BIGINT NOT NULL CHECK (server_command_seq > 0),
    payload TEXT NOT NULL,
    acknowledged_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (session_id, server_command_seq)
);
