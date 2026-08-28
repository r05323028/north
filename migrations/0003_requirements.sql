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
