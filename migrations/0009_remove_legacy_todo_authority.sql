-- Remove the obsolete mg-calr todo authority after mg-remindr became the owner.
-- Keep previously applied migrations immutable; this migration is the append-only
-- schema transition. Event reminders remain because reminders also belong to events.
-- Todo-target reminders are obsolete; their deliveries cascade with the rows.
DELETE FROM reminders WHERE todo_id IS NOT NULL;
ALTER TABLE reminders DROP CONSTRAINT IF EXISTS reminders_todo_id_fkey;
DROP TABLE IF EXISTS todo_reminders;
DROP TABLE IF EXISTS todo_tags;
DROP TABLE IF EXISTS todo_dependencies;
DROP TABLE IF EXISTS todos;
