-- Migration 1 created events.recurrence_rule as free-form text that nothing read.
-- Convert it in place to the validated rule the domain expands. No stored row
-- carries a rule today, so nothing is converted and no schedule is invented.
ALTER TABLE events
    ALTER COLUMN recurrence_rule TYPE jsonb
    USING CASE
        WHEN recurrence_rule IS NULL THEN NULL
        ELSE recurrence_rule::jsonb
    END;
ALTER TABLE events DROP CONSTRAINT IF EXISTS events_recurrence_rule_check;
ALTER TABLE events ADD CONSTRAINT events_recurrence_rule_check CHECK (
    recurrence_rule IS NULL OR (
        jsonb_typeof(recurrence_rule) = 'object'
        AND recurrence_rule ? 'frequency'
        AND recurrence_rule ? 'interval'
        AND ((recurrence_rule ? 'count') OR (recurrence_rule ? 'until'))
    )
);
DROP INDEX IF EXISTS events_recurrence_idx;
CREATE INDEX events_recurrence_idx
    ON events (starts_at, all_day_start)
    WHERE recurrence_rule IS NOT NULL AND deleted_at IS NULL;
