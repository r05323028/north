ALTER TABLE execution_sessions
ADD COLUMN requirement_id TEXT REFERENCES requirements (id);

CREATE INDEX execution_sessions_requirement_id_idx
ON execution_sessions (requirement_id);
