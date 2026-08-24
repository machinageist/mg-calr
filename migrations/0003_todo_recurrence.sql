-- Bounded RFC 5545-style recurrence for todos. Rules are validated by the
-- domain before writes and again on rehydration; instances are never stored.
ALTER TABLE todos ADD COLUMN IF NOT EXISTS recurrence_rule jsonb;
ALTER TABLE todos DROP CONSTRAINT IF EXISTS todos_recurrence_rule_check;
ALTER TABLE todos ADD CONSTRAINT todos_recurrence_rule_check CHECK (
    recurrence_rule IS NULL OR (
        jsonb_typeof(recurrence_rule) = 'object'
        AND recurrence_rule ? 'frequency'
        AND recurrence_rule ? 'interval'
        AND ((recurrence_rule ? 'count') OR (recurrence_rule ? 'until'))
    )
);
CREATE INDEX IF NOT EXISTS todos_recurrence_idx
    ON todos (due_date, due_at) WHERE recurrence_rule IS NOT NULL AND completed_at IS NULL AND deleted_at IS NULL;
