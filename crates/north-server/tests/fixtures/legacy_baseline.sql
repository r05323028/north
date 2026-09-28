-- North 0.1.0 initial schema baseline.
-- Consolidated from unreleased migrations in original version order.
-- After first public release, keep this file immutable and append later versions.

-- Consolidated source: 0001_email_auth.sql
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL CHECK (role IN ('Owner', 'Admin', 'RequirementManager', 'Requester')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE verification_codes (
    id BIGSERIAL PRIMARY KEY,
    email TEXT NOT NULL,
    code_hash BYTEA NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX verification_codes_one_active_per_email
    ON verification_codes (email)
    WHERE used_at IS NULL;

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash BYTEA NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    invalidated_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE instance_settings (
    id SMALLINT PRIMARY KEY CHECK (id = 1),
    owner_user_id TEXT REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO instance_settings (id) VALUES (1);

-- Consolidated source: 0002_role_model.sql
-- Role values are present in the bootstrap users table. This migration makes
-- the low-privilege default and constraint explicit for all new accounts.
ALTER TABLE users
    ALTER COLUMN role SET DEFAULT 'Requester';

ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_role_check;

ALTER TABLE users
    ADD CONSTRAINT users_role_check
    CHECK (role IN ('Owner', 'Admin', 'RequirementManager', 'Requester'));

-- Consolidated source: 0003_requirements.sql
CREATE TABLE requirements (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL
    CHECK (CHAR_LENGTH(BTRIM(title)) BETWEEN 1 AND 500),
    description TEXT NOT NULL
    CHECK (CHAR_LENGTH(BTRIM(description)) BETWEEN 1 AND 10000),
    summary TEXT NOT NULL DEFAULT '', -- noqa: RF04
    acceptance_criteria TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    assumptions TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    open_questions TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    status TEXT NOT NULL DEFAULT 'Draft'
    CHECK (status IN ('Draft', 'Discussing', 'Ready', 'Accepted', 'Rejected')),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_by TEXT NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX requirements_status_updated_at_idx
ON requirements (status, updated_at DESC, id ASC);

CREATE INDEX requirements_created_by_updated_at_idx
ON requirements (created_by, updated_at DESC, id ASC);

CREATE TABLE transition_audit (
    id BIGSERIAL PRIMARY KEY,
    requirement_id TEXT NOT NULL
    REFERENCES requirements (id) ON DELETE CASCADE,
    actor_id TEXT NOT NULL,
    transition TEXT NOT NULL,
    from_status TEXT NOT NULL
    CHECK (
        from_status IN ('Draft', 'Discussing', 'Ready', 'Accepted', 'Rejected')
    ),
    to_status TEXT NOT NULL
    CHECK (
        to_status IN ('Draft', 'Discussing', 'Ready', 'Accepted', 'Rejected')
    ),
    feedback TEXT CHECK (
        feedback IS NULL OR CHAR_LENGTH(BTRIM(feedback)) BETWEEN 1 AND 10000
    ),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX transition_audit_requirement_created_at_idx
ON transition_audit (requirement_id, created_at ASC, id ASC);

-- Consolidated source: 0004_conversations.sql
CREATE TABLE conversations (
    id TEXT PRIMARY KEY,
    requirement_id TEXT NOT NULL UNIQUE REFERENCES requirements (id)
    ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Backfill conversations for requirements created before this migration.
INSERT INTO conversations (id, requirement_id)
SELECT
    MD5('conversation:' || id) AS id,
    id AS requirement_id
FROM requirements;

CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations (id)
    ON DELETE CASCADE,
    author_user_id TEXT REFERENCES users (id),
    kind TEXT NOT NULL CHECK (kind IN ('requester', 'agent', 'system')),
    body TEXT NOT NULL CHECK (CHAR_LENGTH(BTRIM(body)) BETWEEN 1 AND 100000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (kind <> 'requester' OR author_user_id IS NOT NULL)
);

CREATE INDEX messages_conversation_created_at_idx
ON messages (conversation_id, created_at ASC, id ASC);

-- Consolidated source: 0005_readiness_assessments.sql
CREATE TABLE readiness_assessments (
    id TEXT PRIMARY KEY,
    event_id TEXT NOT NULL UNIQUE,
    session_id TEXT NOT NULL,
    daemon_event_seq BIGINT NOT NULL CHECK (daemon_event_seq > 0),
    event_requirement_id TEXT NOT NULL,
    requirement_id TEXT REFERENCES requirements (id) ON DELETE SET NULL,
    requirement_revision BIGINT NOT NULL CHECK (requirement_revision > 0),
    verdict TEXT NOT NULL CHECK (verdict IN ('ready', 'needs_clarification')),
    blockers TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    assumptions TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    repositories_reviewed JSONB NOT NULL
    CHECK (JSONB_TYPEOF(repositories_reviewed) = 'array'),
    outcome TEXT NOT NULL CHECK (outcome IN ('accepted', 'rejected')),
    rejection_reason TEXT,
    assessed_at_ms BIGINT NOT NULL CHECK (assessed_at_ms >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (
        (outcome = 'accepted' AND rejection_reason IS NULL)
        OR (outcome = 'rejected' AND rejection_reason IS NOT NULL)
    ),
    UNIQUE (session_id, daemon_event_seq)
);

CREATE INDEX readiness_assessments_requirement_revision_idx
ON readiness_assessments (
    requirement_id,
    requirement_revision,
    outcome,
    created_at DESC,
    id ASC
);

CREATE FUNCTION PREVENT_READINESS_ASSESSMENT_MUTATION()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    IF pg_trigger_depth() > 1 THEN
        IF TG_OP = 'DELETE' THEN
            RETURN OLD;
        END IF;
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'readiness assessments are immutable';
END;
$function$;

CREATE TRIGGER readiness_assessments_immutable
BEFORE UPDATE OR DELETE ON readiness_assessments
FOR EACH ROW EXECUTE FUNCTION PREVENT_READINESS_ASSESSMENT_MUTATION();

-- Consolidated source: 0007_daemon_runtime.sql
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

-- Consolidated source: 0008_runtime_hardening.sql
ALTER TABLE verification_codes
ADD COLUMN failed_attempts INTEGER NOT NULL DEFAULT 0
CHECK (failed_attempts >= 0);

CREATE INDEX daemon_setup_requests_expires_at_idx
ON daemon_setup_requests (expires_at);

-- Consolidated source: 0009_execution_session_requirement.sql
ALTER TABLE execution_sessions
ADD COLUMN requirement_id TEXT REFERENCES requirements (id);

CREATE INDEX execution_sessions_requirement_id_idx
ON execution_sessions (requirement_id);

-- Consolidated source: 0010_requirement_state_version.sql
ALTER TABLE requirements
ADD COLUMN state_version BIGINT NOT NULL DEFAULT 1
CHECK (state_version > 0);

-- Consolidated source: 0011_readiness_generation_immutable_fk.sql
ALTER TABLE readiness_assessments
ADD COLUMN accepted_state_version BIGINT
CHECK (accepted_state_version IS NULL OR accepted_state_version > 0),
ADD COLUMN generation_unknown BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE readiness_assessments
DISABLE TRIGGER readiness_assessments_immutable;

UPDATE readiness_assessments AS assessment
SET accepted_state_version = requirement.state_version
FROM requirements AS requirement
WHERE
    assessment.outcome = 'accepted'
    AND assessment.requirement_id = requirement.id
    AND requirement.status = 'Ready'
    AND assessment.requirement_revision = requirement.revision
    AND (
        SELECT COUNT(*)
        FROM readiness_assessments AS candidate
        WHERE
            candidate.requirement_id = assessment.requirement_id
            AND candidate.requirement_revision = assessment.requirement_revision
            AND candidate.outcome = 'accepted'
    ) = 1;

UPDATE readiness_assessments
SET generation_unknown = TRUE
WHERE outcome = 'accepted' AND accepted_state_version IS NULL;

ALTER TABLE readiness_assessments
ENABLE TRIGGER readiness_assessments_immutable;

ALTER TABLE readiness_assessments
ADD CONSTRAINT readiness_assessments_generation_consistency
CHECK (
    (
        outcome = 'accepted'
        AND (
            (accepted_state_version IS NOT NULL AND generation_unknown = FALSE)
            OR (accepted_state_version IS NULL AND generation_unknown = TRUE)
        )
    )
    OR (
        outcome = 'rejected'
        AND accepted_state_version IS NULL
        AND generation_unknown = FALSE
    )
);

ALTER TABLE readiness_assessments
DROP CONSTRAINT readiness_assessments_requirement_id_fkey;

ALTER TABLE readiness_assessments
ADD CONSTRAINT readiness_assessments_requirement_id_fkey
FOREIGN KEY (requirement_id) REFERENCES requirements (id);

CREATE UNIQUE INDEX readiness_assessments_accepted_generation_idx
ON readiness_assessments (requirement_id, accepted_state_version)
WHERE outcome = 'accepted'
AND accepted_state_version IS NOT NULL
AND generation_unknown = FALSE;

-- Consolidated source: 0012_transition_audit_provenance.sql
ALTER TABLE transition_audit
ADD COLUMN assessment_id TEXT REFERENCES readiness_assessments (id),
ADD COLUMN state_version BIGINT CHECK (
    state_version IS NULL OR state_version > 0
);

-- Consolidated source: 0013_repositories.sql
CREATE TABLE repositories (
    id TEXT PRIMARY KEY
        CHECK (id = LOWER(id)
            AND id ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'),
    name TEXT NOT NULL
        CHECK (OCTET_LENGTH(BTRIM(name)) BETWEEN 1 AND 100),
    name_normalized TEXT NOT NULL UNIQUE
        CHECK (CHAR_LENGTH(BTRIM(name_normalized)) BETWEEN 1 AND 400),
    url TEXT NOT NULL
        CHECK (OCTET_LENGTH(BTRIM(url)) BETWEEN 1 AND 2048),
    description TEXT NOT NULL DEFAULT ''
        CHECK (OCTET_LENGTH(BTRIM(description)) BETWEEN 0 AND 10000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    disabled_at TIMESTAMPTZ
);

CREATE INDEX repositories_catalog_order_idx
ON repositories (name_normalized ASC, id ASC);

CREATE FUNCTION PREVENT_REPOSITORY_URL_MUTATION()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $function$
BEGIN
    IF OLD.id IS DISTINCT FROM NEW.id
        OR OLD.url IS DISTINCT FROM NEW.url
        OR OLD.created_at IS DISTINCT FROM NEW.created_at THEN
        RAISE EXCEPTION 'repository identity, URL, and created_at are immutable';
    END IF;
    RETURN NEW;
END;
$function$;

CREATE TRIGGER repositories_url_immutable
BEFORE UPDATE ON repositories
FOR EACH ROW EXECUTE FUNCTION PREVENT_REPOSITORY_URL_MUTATION();

-- Consolidated source: 0014_protocol_delivery.sql
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

-- Consolidated source: 0015_clarification_runtime.sql
ALTER TABLE execution_sessions
    ADD COLUMN start_message_id TEXT REFERENCES messages(id),
    ADD COLUMN start_context JSONB,
    ADD COLUMN start_command_id TEXT,
    ADD COLUMN cancel_command_id TEXT,
    ADD COLUMN cancel_requested BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN runtime_id TEXT,
    ADD COLUMN terminal_summary TEXT,
    ADD COLUMN failure_reason TEXT,
    ADD COLUMN started_at TIMESTAMPTZ,
    ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ADD COLUMN last_activity_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    ADD CONSTRAINT execution_sessions_clarification_context_check
    CHECK (
        (start_message_id IS NULL AND start_context IS NULL)
        OR (
            start_message_id IS NOT NULL
            AND start_context IS NOT NULL
            AND JSONB_TYPEOF(start_context) = 'object'
        )
    );

CREATE UNIQUE INDEX execution_sessions_clarification_slot_idx
ON execution_sessions (requirement_id)
WHERE start_message_id IS NOT NULL
  AND state NOT IN ('Completed', 'Failed');

CREATE INDEX execution_sessions_clarification_latest_idx
ON execution_sessions (requirement_id, created_at DESC, id DESC)
WHERE start_message_id IS NOT NULL;

ALTER TABLE messages
    ADD COLUMN source_event_id TEXT UNIQUE;

CREATE TABLE clarification_activities (
    id BIGSERIAL PRIMARY KEY,
    event_id TEXT NOT NULL UNIQUE,
    session_id TEXT NOT NULL REFERENCES execution_sessions(id) ON DELETE CASCADE,
    activity TEXT NOT NULL CHECK (CHAR_LENGTH(BTRIM(activity)) BETWEEN 1 AND 1000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX clarification_activities_session_order_idx
ON clarification_activities (session_id, created_at ASC, id ASC);

-- Consolidated source: 0016_execution_retry_authority.sql
-- Durable server-owned execution attempt accounting and retry scheduling.
ALTER TABLE execution_sessions
    ADD COLUMN attempt_count BIGINT NOT NULL DEFAULT 0
        CHECK (attempt_count >= 0),
    ADD COLUMN max_attempts BIGINT NOT NULL DEFAULT 3
        CHECK (max_attempts > 0),
    ADD COLUMN next_retry_at TIMESTAMPTZ,
    ADD COLUMN failure_class TEXT,
    ADD COLUMN current_attempt_id TEXT;

-- Legacy failure text was operational-only. Replace it with a bounded safe
-- classification before enforcing the public/privacy boundary.
UPDATE execution_sessions
SET failure_class = CASE WHEN state = 'Failed' THEN 'runtime_failure' END,
    failure_reason = CASE
        WHEN state = 'Failed' AND failure_reason IS NOT NULL THEN 'runtime_failure'
        ELSE NULL
    END;

ALTER TABLE execution_sessions
    ADD CONSTRAINT execution_sessions_failure_class_check
        CHECK (
            failure_class IS NULL
            OR failure_class IN (
                'runtime_failure',
                'execution_outcome_unknown',
                'retry_exhausted',
                'owner_unavailable',
                'cancelled'
            )
        ),
    ADD CONSTRAINT execution_sessions_failure_reason_check
        CHECK (
            failure_reason IS NULL
            OR CHAR_LENGTH(BTRIM(failure_reason)) BETWEEN 1 AND 256
        );

CREATE TABLE execution_attempts (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES execution_sessions(id) ON DELETE CASCADE,
    attempt_number BIGINT NOT NULL CHECK (attempt_number > 0),
    command_id TEXT NOT NULL UNIQUE,
    command_kind TEXT NOT NULL CHECK (command_kind IN ('session.start', 'session.resume')),
    outcome TEXT CHECK (outcome IN ('completed', 'failed')),
    failure_event_id TEXT UNIQUE,
    failure_class TEXT,
    failure_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    closed_at TIMESTAMPTZ,
    UNIQUE (session_id, attempt_number),
    CHECK (
        failure_class IS NULL
        OR failure_class IN (
            'runtime_failure',
            'execution_outcome_unknown',
            'retry_exhausted',
            'owner_unavailable',
            'cancelled'
        )
    ),
    CHECK (
        failure_reason IS NULL
        OR CHAR_LENGTH(BTRIM(failure_reason)) BETWEEN 1 AND 256
    ),
    CHECK (
        (outcome = 'failed' AND failure_event_id IS NOT NULL
            AND failure_class IS NOT NULL AND failure_reason IS NOT NULL)
        OR (outcome = 'completed' AND failure_event_id IS NULL
            AND failure_class IS NULL AND failure_reason IS NULL)
        OR (outcome IS NULL AND failure_event_id IS NULL
            AND failure_class IS NULL AND failure_reason IS NULL)
    )
);

CREATE INDEX execution_attempts_session_order_idx
ON execution_attempts (session_id, attempt_number ASC);

-- Recover execution commands whose durable payload is still available. This
-- deliberately ignores non-execution commands and cannot invent attempts.
WITH command_rows AS (
    SELECT command_id, session_id, server_command_seq, created_at, payload
    FROM server_command_outbox
    WHERE payload IS NOT NULL
    UNION ALL
    SELECT command_id, session_id, server_command_seq, acknowledged_at AS created_at, payload
    FROM server_command_tombstones
    WHERE payload IS NOT NULL
), execution_commands AS (
    SELECT command_id, session_id, server_command_seq, created_at,
           payload::jsonb #>> '{payload,command,type}' AS command_kind
    FROM command_rows
    WHERE pg_input_is_valid(payload, 'jsonb')
), resumes AS (
    SELECT command_id, session_id, created_at,
           ROW_NUMBER() OVER (
               PARTITION BY session_id
               ORDER BY server_command_seq ASC, command_id ASC
           ) AS resume_number
    FROM execution_commands
    WHERE command_kind = 'session.resume'
), starts AS (
    SELECT id AS session_id, start_command_id
    FROM execution_sessions
    WHERE start_command_id IS NOT NULL
      AND BTRIM(start_command_id) <> ''
)
-- Start identity is authoritative even after payload compaction. Resume rows
-- are numbered after it, preventing compacted starts from colliding with N+1.
INSERT INTO execution_attempts
    (id, session_id, attempt_number, command_id, command_kind, created_at)
SELECT MD5('execution-attempt:' || starts.session_id || ':1'),
       starts.session_id, 1, starts.start_command_id, 'session.start',
       sessions.created_at
FROM starts
JOIN execution_sessions AS sessions ON sessions.id = starts.session_id
ON CONFLICT (command_id) DO NOTHING;

WITH command_rows AS (
    SELECT command_id, session_id, server_command_seq, created_at, payload
    FROM server_command_outbox
    WHERE payload IS NOT NULL
    UNION ALL
    SELECT command_id, session_id, server_command_seq, acknowledged_at AS created_at, payload
    FROM server_command_tombstones
    WHERE payload IS NOT NULL
), execution_commands AS (
    SELECT command_id, session_id, server_command_seq, created_at,
           payload::jsonb #>> '{payload,command,type}' AS command_kind
    FROM command_rows
    WHERE pg_input_is_valid(payload, 'jsonb')
), resumes AS (
    SELECT command_id, session_id, created_at,
           ROW_NUMBER() OVER (
               PARTITION BY session_id
               ORDER BY server_command_seq ASC, command_id ASC
           ) AS resume_number
    FROM execution_commands
    WHERE command_kind = 'session.resume'
), starts AS (
    SELECT id AS session_id, start_command_id
    FROM execution_sessions
    WHERE start_command_id IS NOT NULL
      AND BTRIM(start_command_id) <> ''
)
INSERT INTO execution_attempts
    (id, session_id, attempt_number, command_id, command_kind, created_at)
SELECT MD5('execution-attempt:' || resumes.session_id || ':'
           || (resumes.resume_number + CASE WHEN starts.start_command_id IS NULL THEN 0 ELSE 1 END)),
       resumes.session_id,
       resumes.resume_number + CASE WHEN starts.start_command_id IS NULL THEN 0 ELSE 1 END,
       resumes.command_id, 'session.resume', resumes.created_at
FROM resumes
LEFT JOIN starts ON starts.session_id = resumes.session_id
ON CONFLICT (command_id) DO NOTHING;

UPDATE execution_sessions AS sessions
SET attempt_count = COALESCE(
    (
        SELECT COUNT(*)
        FROM execution_attempts AS attempts
        WHERE attempts.session_id = sessions.id
    ),
    0
);

UPDATE execution_sessions AS sessions
SET current_attempt_id = (
    SELECT attempts.id
    FROM execution_attempts AS attempts
    WHERE attempts.session_id = sessions.id
    ORDER BY attempts.attempt_number DESC
    LIMIT 1
)
WHERE sessions.state IN ('Idle', 'Running')
  AND sessions.start_command_id IS NOT NULL
  AND EXISTS (
      SELECT 1 FROM execution_attempts AS attempts
      WHERE attempts.session_id = sessions.id
  );

ALTER TABLE execution_sessions
    ADD CONSTRAINT execution_sessions_current_attempt_fk
    FOREIGN KEY (current_attempt_id) REFERENCES execution_attempts(id)
    ON DELETE SET NULL;

CREATE INDEX execution_sessions_retry_due_idx
ON execution_sessions (state, next_retry_at, id)
WHERE state = 'Retrying' AND next_retry_at IS NOT NULL;

-- Consolidated source: 0017_runtime_event_retention.sql
-- Ephemeral runtime telemetry retention.
--
-- clarification_activities is the only allowlisted ephemeral class. Durable
-- coordination state (execution sessions, attempts, retry scheduling, outbox,
-- dedupe records, watermarks) is intentionally untouched by retention.
ALTER TABLE clarification_activities
    ADD COLUMN expires_at TIMESTAMPTZ;

-- Deterministic backfill: the 0.1.0 default retention window is 7 days.
UPDATE clarification_activities
SET expires_at = created_at + INTERVAL '7 days';

ALTER TABLE clarification_activities
    ALTER COLUMN expires_at SET NOT NULL;

CREATE INDEX clarification_activities_expires_at_idx
ON clarification_activities (expires_at ASC, id ASC);

-- Consolidated source: 0018_public_endpoint_abuse_protection.sql
-- Persist canonical client CIDR for durable daemon setup quotas.
-- Legacy rows remain NULL and are excluded from new keyed quota counts.
ALTER TABLE daemon_setup_requests
    ADD COLUMN client_network_key CIDR;

CREATE INDEX daemon_setup_requests_client_network_key_expires_at_idx
    ON daemon_setup_requests (client_network_key, expires_at)
    WHERE claimed_at IS NULL;

-- Consolidated source: 0019_otp_hmac_hardening.sql
-- Invalidate verification codes written before keyed OTP storage was deployed.
-- Rows remain for audit/history; only active legacy codes are consumed.
UPDATE verification_codes
SET used_at = CURRENT_TIMESTAMP
WHERE used_at IS NULL
  AND expires_at > CURRENT_TIMESTAMP;
