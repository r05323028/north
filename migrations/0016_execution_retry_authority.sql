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
