CREATE TABLE calendars (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    color text,
    is_default boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at timestamptz
);
CREATE UNIQUE INDEX calendars_one_default ON calendars (is_default) WHERE is_default AND deleted_at IS NULL;

CREATE TABLE events (
    id uuid PRIMARY KEY,
    calendar_id uuid NOT NULL REFERENCES calendars(id),
    rfc_uid text NOT NULL UNIQUE,
    title text NOT NULL,
    description text,
    location text,
    url text,
    status text,
    busy boolean NOT NULL DEFAULT true,
    timezone text,
    starts_at timestamptz,
    ends_at timestamptz,
    all_day_start date,
    all_day_end date,
    recurrence_rule text,
    extension_properties jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at timestamptz,
    remote_tombstoned_at timestamptz,
    CHECK ((timezone IS NOT NULL AND starts_at IS NOT NULL AND ends_at IS NOT NULL AND all_day_start IS NULL AND all_day_end IS NULL)
        OR (timezone IS NULL AND starts_at IS NULL AND ends_at IS NULL AND all_day_start IS NOT NULL AND all_day_end IS NOT NULL)),
    CHECK (ends_at IS NULL OR ends_at > starts_at),
    CHECK (all_day_end IS NULL OR all_day_end > all_day_start)
);

CREATE TABLE todos (
    id uuid PRIMARY KEY,
    parent_id uuid REFERENCES todos(id),
    title text NOT NULL,
    notes text,
    due_at timestamptz,
    timezone text,
    priority text NOT NULL DEFAULT 'none' CHECK (priority IN ('none', 'low', 'medium', 'high', 'urgent')),
    project text,
    tags text[] NOT NULL DEFAULT '{}',
    recurrence_rule text,
    completed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at timestamptz,
    CHECK (parent_id IS NULL OR parent_id <> id)
);

CREATE TABLE todo_dependencies (
    todo_id uuid NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
    blocked_by_id uuid NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
    PRIMARY KEY (todo_id, blocked_by_id),
    CHECK (todo_id <> blocked_by_id)
);

CREATE TABLE reminders (
    id uuid PRIMARY KEY,
    event_id uuid REFERENCES events(id) ON DELETE CASCADE,
    todo_id uuid REFERENCES todos(id) ON DELETE CASCADE,
    offset_seconds bigint,
    absolute_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((event_id IS NOT NULL)::integer + (todo_id IS NOT NULL)::integer = 1),
    CHECK ((offset_seconds IS NOT NULL)::integer + (absolute_at IS NOT NULL)::integer = 1)
);

CREATE TABLE reminder_deliveries (
    id uuid PRIMARY KEY,
    reminder_id uuid NOT NULL REFERENCES reminders(id) ON DELETE CASCADE,
    scheduled_for timestamptz NOT NULL,
    claimed_at timestamptz,
    delivered_at timestamptz,
    dismissed_at timestamptz,
    snoozed_until timestamptz,
    deferred_reason text,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (reminder_id, scheduled_for)
);

CREATE TABLE audit_log (
    id uuid PRIMARY KEY,
    transaction_id uuid NOT NULL,
    entity_type text NOT NULL,
    entity_id uuid NOT NULL,
    operation text NOT NULL,
    before_state jsonb,
    after_state jsonb,
    occurred_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX audit_log_entity ON audit_log (entity_type, entity_id, occurred_at);
