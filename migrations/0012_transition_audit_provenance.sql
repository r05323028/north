ALTER TABLE transition_audit
ADD COLUMN assessment_id TEXT REFERENCES readiness_assessments (id),
ADD COLUMN state_version BIGINT CHECK (
    state_version IS NULL OR state_version > 0
);
