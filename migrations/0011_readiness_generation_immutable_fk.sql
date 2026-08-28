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
