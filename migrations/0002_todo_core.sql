-- Todo core foundation. This migration only adds columns/relations/indexes;
-- migration 1 rows remain readable and are never rewritten or deleted.
CREATE TABLE IF NOT EXISTS projects (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    normalized_name text NOT NULL,
    archived_at timestamptz,
    version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX IF NOT EXISTS projects_normalized_name_unique
    ON projects (normalized_name) WHERE archived_at IS NULL;

CREATE TABLE IF NOT EXISTS tags (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    normalized_name text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX IF NOT EXISTS tags_normalized_name_unique ON tags (normalized_name);

-- Migration 1 already owns the locked priority vocabulary; this slice preserves
-- that constraint while extending the row with project/due/version fields.
ALTER TABLE todos ADD COLUMN IF NOT EXISTS project_id uuid REFERENCES projects(id);
ALTER TABLE todos ADD COLUMN IF NOT EXISTS due_date date;
ALTER TABLE todos ADD COLUMN IF NOT EXISTS version bigint NOT NULL DEFAULT 1;
ALTER TABLE todos ADD COLUMN IF NOT EXISTS trashed_at timestamptz;
ALTER TABLE todos ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP;

CREATE TABLE IF NOT EXISTS todo_tags (
    todo_id uuid NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
    tag_id uuid NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (todo_id, tag_id)
);

-- Migration 1's dependency columns are renamed to the domain names without
-- rewriting rows; PostgreSQL preserves the existing primary key and FKs.
DO $$
DECLARE
    _dependency_mismatch boolean;
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = current_schema() AND table_name = 'todo_dependencies' AND column_name = 'todo_id'
    ) AND EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = current_schema() AND table_name = 'todo_dependencies' AND column_name = 'dependent_id'
    ) THEN
        EXECUTE $query$
            SELECT EXISTS (
                SELECT 1 FROM todo_dependencies
                WHERE todo_id <> dependent_id
                   OR blocked_by_id <> prerequisite_id
            )
        $query$ INTO STRICT _dependency_mismatch;
        IF _dependency_mismatch THEN
            RAISE EXCEPTION
                'todo_core migration refused: legacy and canonical dependency columns disagree';
        END IF;
        ALTER TABLE todo_dependencies
            DROP COLUMN todo_id CASCADE,
            DROP COLUMN blocked_by_id CASCADE;
    END IF;
END $$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = current_schema() AND table_name = 'todo_dependencies' AND column_name = 'dependent_id'
    ) AND EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = current_schema() AND table_name = 'todo_dependencies' AND column_name = 'todo_id'
    ) THEN
        ALTER TABLE todo_dependencies RENAME COLUMN todo_id TO dependent_id;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = current_schema() AND table_name = 'todo_dependencies' AND column_name = 'prerequisite_id'
    ) AND EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = current_schema() AND table_name = 'todo_dependencies' AND column_name = 'blocked_by_id'
    ) THEN
        ALTER TABLE todo_dependencies RENAME COLUMN blocked_by_id TO prerequisite_id;
    END IF;
END $$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'todo_dependencies_pkey') THEN
        ALTER TABLE todo_dependencies
            ADD CONSTRAINT todo_dependencies_pkey PRIMARY KEY (dependent_id, prerequisite_id);
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'todo_dependencies_dependent_id_fkey') THEN
        ALTER TABLE todo_dependencies
            ADD CONSTRAINT todo_dependencies_dependent_id_fkey
            FOREIGN KEY (dependent_id) REFERENCES todos(id) ON DELETE CASCADE;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'todo_dependencies_prerequisite_id_fkey') THEN
        ALTER TABLE todo_dependencies
            ADD CONSTRAINT todo_dependencies_prerequisite_id_fkey
            FOREIGN KEY (prerequisite_id) REFERENCES todos(id) ON DELETE CASCADE;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'todo_dependencies_check') THEN
        ALTER TABLE todo_dependencies
            ADD CONSTRAINT todo_dependencies_check CHECK (dependent_id <> prerequisite_id);
    END IF;
END $$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM todos
        WHERE (due_at IS NULL) <> (timezone IS NULL)
    ) THEN
        RAISE EXCEPTION
            'todo_core migration refused: existing todos contain inconsistent due_at/timezone pairs';
    END IF;
END $$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'todos_due_representation_check') THEN
        ALTER TABLE todos ADD CONSTRAINT todos_due_representation_check
            CHECK (NOT (due_date IS NOT NULL AND due_at IS NOT NULL));
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'todos_due_timezone_check') THEN
        ALTER TABLE todos ADD CONSTRAINT todos_due_timezone_check
            CHECK ((due_at IS NULL AND timezone IS NULL) OR (due_at IS NOT NULL AND timezone IS NOT NULL));
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'todos_version_positive_check') THEN
        ALTER TABLE todos ADD CONSTRAINT todos_version_positive_check CHECK (version > 0);
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS todo_dependencies_prerequisite_idx
    ON todo_dependencies (prerequisite_id, dependent_id);
CREATE INDEX IF NOT EXISTS todos_live_due_idx
    ON todos (due_date, due_at) WHERE completed_at IS NULL AND deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS todos_parent_idx ON todos (parent_id);
CREATE INDEX IF NOT EXISTS todos_project_idx ON todos (project_id);
CREATE INDEX IF NOT EXISTS todo_tags_tag_idx ON todo_tags (tag_id, todo_id);
CREATE INDEX IF NOT EXISTS todo_dependencies_dependent_idx
    ON todo_dependencies (dependent_id, prerequisite_id);
