-- Durable delivery watermarks and server-side event identity protection.
ALTER TABLE server_command_outbox
    ADD COLUMN payload_digest TEXT,
    ADD COLUMN command_identity_digest TEXT;

-- Existing rows predate semantic sent_at normalization. The runtime accepts this
-- legacy full-payload identity while all new rows use the semantic digest.
UPDATE server_command_outbox
SET payload_digest = MD5(payload),
    command_identity_digest = MD5(payload);

ALTER TABLE server_command_outbox
    ALTER COLUMN payload_digest SET NOT NULL,
    ALTER COLUMN command_identity_digest SET NOT NULL,
    ADD CONSTRAINT server_command_outbox_payload_digest_check
        CHECK (CHAR_LENGTH(BTRIM(payload_digest)) > 0),
    ADD CONSTRAINT server_command_outbox_command_identity_digest_check
        CHECK (CHAR_LENGTH(BTRIM(command_identity_digest)) > 0);

CREATE TABLE server_command_tombstones (
    command_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES execution_sessions(id),
    daemon_id TEXT NOT NULL REFERENCES daemon_registrations(daemon_id),
    server_command_seq BIGINT NOT NULL CHECK (server_command_seq > 0),
    payload TEXT,
    payload_digest TEXT NOT NULL CHECK (CHAR_LENGTH(BTRIM(payload_digest)) > 0),
    command_identity_digest TEXT NOT NULL
        CHECK (CHAR_LENGTH(BTRIM(command_identity_digest)) > 0),
    acknowledged_at TIMESTAMPTZ NOT NULL,
    UNIQUE (session_id, server_command_seq)
);

CREATE INDEX server_command_tombstones_session_order_idx
ON server_command_tombstones (session_id, server_command_seq ASC);

CREATE TABLE server_message_command_map (
    session_id TEXT NOT NULL REFERENCES execution_sessions(id),
    message_id TEXT NOT NULL CHECK (CHAR_LENGTH(BTRIM(message_id)) > 0),
    command_id TEXT NOT NULL UNIQUE,
    content_digest TEXT NOT NULL CHECK (CHAR_LENGTH(BTRIM(content_digest)) > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (session_id, message_id)
);

-- Recover logical message mappings from legacy JSON outbox envelopes.
INSERT INTO server_message_command_map (session_id, message_id, command_id, content_digest)
SELECT session_id,
       payload::jsonb #>> '{payload,command,payload,message_id}',
       command_id,
       MD5(payload::jsonb #>> '{payload,command,payload,content}')
FROM server_command_outbox
WHERE payload::jsonb #>> '{payload,command,type}' = 'message.send'
  AND payload::jsonb #>> '{payload,command,payload,message_id}' IS NOT NULL
ON CONFLICT DO NOTHING;

ALTER TABLE execution_sessions
    ADD COLUMN command_ack_through_seq BIGINT NOT NULL DEFAULT 0
        CHECK (command_ack_through_seq >= 0),
    ADD COLUMN event_ack_through_seq BIGINT NOT NULL DEFAULT 0
        CHECK (event_ack_through_seq >= 0),
    ADD COLUMN event_ack_sparse BIGINT[] NOT NULL DEFAULT ARRAY[]::BIGINT[],
    ADD COLUMN repository_ids TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    ADD COLUMN repository_context_initialized BOOLEAN NOT NULL DEFAULT FALSE;

-- Before the general event ledger existed, readiness_assessments was the
-- durable event identity record. Backfill only its contiguous prefix and keep
-- handled rows above a legacy gap as sparse reconciliation state.
WITH legacy_max AS (
    SELECT session_id, MAX(daemon_event_seq) AS max_seq
    FROM readiness_assessments
    GROUP BY session_id
), legacy_state AS (
    SELECT legacy_max.session_id,
           COALESCE(
               (
                   SELECT MIN(g.sequence) - 1
                   FROM generate_series(1, legacy_max.max_seq) AS g(sequence)
                   WHERE NOT EXISTS (
                       SELECT 1
                       FROM readiness_assessments AS assessment
                       WHERE assessment.session_id = legacy_max.session_id
                         AND assessment.daemon_event_seq = g.sequence
                   )
               ),
               legacy_max.max_seq,
               0
           ) AS contiguous_seq
    FROM legacy_max
)
UPDATE execution_sessions AS sessions
SET event_ack_through_seq = legacy.contiguous_seq,
    event_ack_sparse = COALESCE(
        (
            SELECT ARRAY_AGG(assessment.daemon_event_seq ORDER BY assessment.daemon_event_seq)
            FROM readiness_assessments AS assessment
            WHERE assessment.session_id = legacy.session_id
              AND assessment.daemon_event_seq > legacy.contiguous_seq
        ),
        ARRAY[]::BIGINT[]
    )
FROM legacy_state AS legacy
WHERE sessions.id = legacy.session_id;

UPDATE execution_sessions AS sessions
SET command_ack_through_seq = COALESCE((
    SELECT COALESCE(
        MIN(server_command_seq) FILTER (WHERE acknowledged_at IS NULL) - 1,
        MAX(server_command_seq),
        0
    )
    FROM server_command_outbox
    WHERE session_id = sessions.id
), 0);

CREATE TABLE server_event_dedupe (
    event_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES execution_sessions(id),
    daemon_event_seq BIGINT NOT NULL CHECK (daemon_event_seq > 0),
    payload_digest TEXT NOT NULL CHECK (CHAR_LENGTH(BTRIM(payload_digest)) > 0),
    payload TEXT NOT NULL CHECK (CHAR_LENGTH(BTRIM(payload)) > 0),
    legacy_identity BOOLEAN NOT NULL DEFAULT FALSE,
    outcome TEXT NOT NULL CHECK (outcome IN ('accepted', 'rejected')),
    rejection_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (session_id, daemon_event_seq),
    CHECK (
        (outcome = 'accepted' AND rejection_reason IS NULL)
        OR (outcome = 'rejected' AND rejection_reason IS NOT NULL)
    )
);

CREATE INDEX server_event_dedupe_session_order_idx
ON server_event_dedupe (session_id, daemon_event_seq ASC);

-- Legacy readiness rows are already immutable typed event identities, but their
-- old schema did not retain the full envelope digest. Keep an explicit tombstone
-- so their IDs cannot be reused by generic runtime facts.
INSERT INTO server_event_dedupe
    (event_id, session_id, daemon_event_seq, payload_digest, payload,
     legacy_identity, outcome, rejection_reason)
SELECT event_id, session_id, daemon_event_seq, MD5(event_id),
       'legacy-readiness:' || event_id, TRUE, outcome, rejection_reason
FROM readiness_assessments AS assessment
WHERE EXISTS (
    SELECT 1 FROM execution_sessions AS session
    WHERE session.id = assessment.session_id
)
ON CONFLICT DO NOTHING;

CREATE FUNCTION PREVENT_SERVER_COMMAND_OUTBOX_MUTATION()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    IF OLD.command_id IS DISTINCT FROM NEW.command_id
        OR OLD.session_id IS DISTINCT FROM NEW.session_id
        OR OLD.daemon_id IS DISTINCT FROM NEW.daemon_id
        OR OLD.server_command_seq IS DISTINCT FROM NEW.server_command_seq
        OR OLD.payload IS DISTINCT FROM NEW.payload
        OR OLD.payload_digest IS DISTINCT FROM NEW.payload_digest
        OR OLD.command_identity_digest IS DISTINCT FROM NEW.command_identity_digest THEN
        RAISE EXCEPTION 'server command outbox identity and payload are immutable';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER server_command_outbox_immutable
BEFORE UPDATE ON server_command_outbox
FOR EACH ROW EXECUTE FUNCTION PREVENT_SERVER_COMMAND_OUTBOX_MUTATION();
