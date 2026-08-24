-- Event optimistic lifecycle state was added to the foundation table in place.
-- This migration is intentionally idempotent for databases created at version 1.
ALTER TABLE events ADD COLUMN IF NOT EXISTS version bigint NOT NULL DEFAULT 1;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'events_version_positive_check'
          AND conrelid = 'events'::regclass
    ) THEN
        ALTER TABLE events ADD CONSTRAINT events_version_positive_check CHECK (version >= 1);
    END IF;
END $$;