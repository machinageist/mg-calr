-- Validated, repeatable todo reminders stored as offsets before the todo due value.
CREATE TABLE IF NOT EXISTS todo_reminders (
    todo_id uuid NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
    minutes_before integer NOT NULL,
    repeatable boolean NOT NULL DEFAULT false,
    PRIMARY KEY (todo_id, minutes_before, repeatable),
    CONSTRAINT todo_reminders_positive_offset CHECK (minutes_before BETWEEN 1 AND 10080)
);
CREATE INDEX IF NOT EXISTS todo_reminders_due_idx ON todo_reminders (minutes_before, todo_id);

-- Bridge todo reminders to the foundation delivery ledger.
ALTER TABLE reminders ADD COLUMN IF NOT EXISTS repeatable boolean NOT NULL DEFAULT false;

-- Legacy databases may already contain duplicate todo identities. Consolidate
-- those identities (and their deliveries) deterministically before adding the
-- uniqueness contract. Keep the lowest-id reminder, and preserve one delivery per
-- scheduled occurrence. The lowest delivery id wins even when the canonical
-- reminder has no delivery, so the subsequent reassignment cannot collide.
WITH duplicate_deliveries AS (
    SELECT delivery.id
    FROM reminder_deliveries delivery
    JOIN reminders reminder ON reminder.id = delivery.reminder_id
    JOIN reminder_deliveries earlier
      ON earlier.scheduled_for = delivery.scheduled_for
    JOIN reminders earlier_reminder ON earlier_reminder.id = earlier.reminder_id
    WHERE reminder.todo_id IS NOT NULL
      AND reminder.offset_seconds IS NOT NULL
      AND reminder.todo_id = earlier_reminder.todo_id
      AND reminder.offset_seconds = earlier_reminder.offset_seconds
      AND reminder.repeatable = earlier_reminder.repeatable
      AND earlier.id < delivery.id
)
DELETE FROM reminder_deliveries delivery
USING duplicate_deliveries duplicate
WHERE delivery.id = duplicate.id;

WITH canonical AS (
    SELECT todo_id, offset_seconds, repeatable, MIN(id::text)::uuid AS keeper_id
    FROM reminders
    WHERE todo_id IS NOT NULL AND offset_seconds IS NOT NULL
    GROUP BY todo_id, offset_seconds, repeatable
)
UPDATE reminder_deliveries delivery
SET reminder_id = canonical.keeper_id
FROM reminders duplicate, canonical
WHERE delivery.reminder_id = duplicate.id
  AND duplicate.todo_id = canonical.todo_id
  AND duplicate.offset_seconds = canonical.offset_seconds
  AND duplicate.repeatable = canonical.repeatable
  AND duplicate.id <> canonical.keeper_id;

WITH canonical AS (
    SELECT todo_id, offset_seconds, repeatable, MIN(id::text)::uuid AS keeper_id
    FROM reminders
    WHERE todo_id IS NOT NULL AND offset_seconds IS NOT NULL
    GROUP BY todo_id, offset_seconds, repeatable
)
DELETE FROM reminders duplicate
USING canonical
WHERE duplicate.todo_id = canonical.todo_id
  AND duplicate.offset_seconds = canonical.offset_seconds
  AND duplicate.repeatable = canonical.repeatable
  AND duplicate.id <> canonical.keeper_id;

CREATE UNIQUE INDEX IF NOT EXISTS reminders_todo_schedule_unique
    ON reminders (todo_id, offset_seconds, repeatable)
    WHERE todo_id IS NOT NULL AND offset_seconds IS NOT NULL;
