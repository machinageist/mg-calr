-- Validated, repeatable todo reminders stored as offsets before the todo due value.
CREATE TABLE IF NOT EXISTS todo_reminders (
    todo_id uuid NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
    minutes_before integer NOT NULL,
    repeatable boolean NOT NULL DEFAULT false,
    PRIMARY KEY (todo_id, minutes_before, repeatable),
    CONSTRAINT todo_reminders_positive_offset CHECK (minutes_before BETWEEN 1 AND 10080)
);
CREATE INDEX IF NOT EXISTS todo_reminders_due_idx ON todo_reminders (minutes_before, todo_id);
