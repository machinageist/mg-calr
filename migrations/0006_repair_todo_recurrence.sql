-- Migration 1 created todos.recurrence_rule as text, while released migration
-- 3 attempted ADD COLUMN IF NOT EXISTS jsonb. Repair existing and fresh ledgers
-- append-only by converting the extant text column in this new migration.
ALTER TABLE todos
    ALTER COLUMN recurrence_rule TYPE jsonb
    USING CASE
        WHEN recurrence_rule IS NULL THEN NULL
        ELSE recurrence_rule::jsonb
    END;
ALTER TABLE todos DROP CONSTRAINT IF EXISTS todos_recurrence_rule_check;
ALTER TABLE todos ADD CONSTRAINT todos_recurrence_rule_check CHECK (
    recurrence_rule IS NULL OR (
        jsonb_typeof(recurrence_rule) = 'object'
        AND recurrence_rule ? 'frequency'
        AND recurrence_rule ? 'interval'
        AND ((recurrence_rule ? 'count') OR (recurrence_rule ? 'until'))
    )
);
DROP INDEX IF EXISTS todos_recurrence_idx;
CREATE INDEX todos_recurrence_idx
    ON todos (due_date, due_at)
    WHERE recurrence_rule IS NOT NULL AND completed_at IS NULL AND deleted_at IS NULL;
