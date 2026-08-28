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
