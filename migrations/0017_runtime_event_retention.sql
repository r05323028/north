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
