ALTER TABLE requirements
ADD COLUMN state_version BIGINT NOT NULL DEFAULT 1
CHECK (state_version > 0);
