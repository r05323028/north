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
