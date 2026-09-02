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
