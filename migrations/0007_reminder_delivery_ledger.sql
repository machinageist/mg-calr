-- Durable reminder delivery identity and state foundation.
-- Presentation and claim behavior lands in later E slices; this migration makes
-- those operations idempotent and preserves legacy delivery provenance.
ALTER TABLE reminder_deliveries
    ADD COLUMN IF NOT EXISTS schedule_ref text,
    ADD COLUMN IF NOT EXISTS occurrence_key text,
    ADD COLUMN IF NOT EXISTS channel text,
    ADD COLUMN IF NOT EXISTS state text,
    ADD COLUMN IF NOT EXISTS attempts integer NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS claim_owner text,
    ADD COLUMN IF NOT EXISTS claim_fence bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS claim_expires_at timestamptz,
    ADD COLUMN IF NOT EXISTS next_attempt_at timestamptz,
    ADD COLUMN IF NOT EXISTS deferred_until timestamptz,
    ADD COLUMN IF NOT EXISTS supersedes uuid,
    ADD COLUMN IF NOT EXISTS folded_into uuid,
    ADD COLUMN IF NOT EXISTS backend_handle bigint,
    ADD COLUMN IF NOT EXISTS terminal_reason text,
    ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP;

UPDATE reminder_deliveries
SET schedule_ref = COALESCE(schedule_ref, 'mg-calr:reminder:' || reminder_id::text),
    occurrence_key = COALESCE(occurrence_key, 'singleton'),
    channel = COALESCE(channel, 'freedesktop'),
    state = COALESCE(
        state,
        CASE
            WHEN dismissed_at IS NOT NULL THEN 'dismissed'
            WHEN snoozed_until IS NOT NULL THEN 'snoozed'
            WHEN delivered_at IS NOT NULL THEN 'presented'
            WHEN claimed_at IS NOT NULL THEN 'unconfirmed_lost'
            ELSE 'pending'
        END
    ),
    terminal_reason = CASE
        WHEN state IS NULL AND claimed_at IS NOT NULL AND delivered_at IS NULL
            THEN COALESCE(terminal_reason, 'backfilled_unconfirmed')
        ELSE terminal_reason
    END;

-- Delivery did not exist before this feature. Historical due rows remain
-- visible, but upgrading must never emit a burst of old notifications.
UPDATE reminder_deliveries
SET state = 'expired',
    terminal_reason = 'backfilled_before_delivery_existed',
    updated_at = CURRENT_TIMESTAMP
WHERE state = 'pending' AND scheduled_for < CURRENT_TIMESTAMP;

ALTER TABLE reminder_deliveries
    ALTER COLUMN schedule_ref SET NOT NULL,
    ALTER COLUMN occurrence_key SET NOT NULL,
    ALTER COLUMN channel SET NOT NULL,
    ALTER COLUMN state SET NOT NULL,
    ALTER COLUMN reminder_id DROP NOT NULL;

ALTER TABLE reminder_deliveries
    DROP CONSTRAINT IF EXISTS reminder_deliveries_reminder_id_fkey,
    DROP CONSTRAINT IF EXISTS reminder_deliveries_reminder_id_scheduled_for_key;

ALTER TABLE reminder_deliveries
    ADD CONSTRAINT reminder_deliveries_reminder_id_fkey
        FOREIGN KEY (reminder_id) REFERENCES reminders(id) ON DELETE SET NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'reminder_deliveries'::regclass
          AND conname = 'reminder_deliveries_channel_check'
    ) THEN
        ALTER TABLE reminder_deliveries
            ADD CONSTRAINT reminder_deliveries_channel_check
            CHECK (channel IN ('freedesktop', 'log', 'null'));
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'reminder_deliveries'::regclass
          AND conname = 'reminder_deliveries_state_check'
    ) THEN
        ALTER TABLE reminder_deliveries
            ADD CONSTRAINT reminder_deliveries_state_check
            CHECK (state IN (
                'pending', 'claimed', 'presented', 'deferred', 'snoozed',
                'dismissed', 'folded', 'expired', 'failed', 'revoked',
                'unconfirmed_lost'
            ));
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'reminder_deliveries'::regclass
          AND conname = 'reminder_deliveries_attempts_check'
    ) THEN
        ALTER TABLE reminder_deliveries
            ADD CONSTRAINT reminder_deliveries_attempts_check CHECK (attempts >= 0);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'reminder_deliveries'::regclass
          AND conname = 'reminder_deliveries_claim_fence_check'
    ) THEN
        ALTER TABLE reminder_deliveries
            ADD CONSTRAINT reminder_deliveries_claim_fence_check CHECK (claim_fence >= 0);
    END IF;
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS reminder_deliveries_claim_key
    ON reminder_deliveries (schedule_ref, occurrence_key, scheduled_for, channel);
CREATE INDEX IF NOT EXISTS reminder_deliveries_due
    ON reminder_deliveries (state, scheduled_for)
    WHERE state IN ('pending', 'deferred', 'claimed');

CREATE TABLE IF NOT EXISTS reminder_digests (
    id uuid PRIMARY KEY,
    kind text NOT NULL,
    window_start timestamptz NOT NULL,
    window_end timestamptz NOT NULL,
    delivery_count integer NOT NULL CHECK (delivery_count >= 0),
    state text NOT NULL,
    presented_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (kind, window_start, window_end),
    CHECK (window_end > window_start)
);

CREATE TABLE IF NOT EXISTS reminder_dnd_windows (
    id uuid PRIMARY KEY,
    starts_at timestamptz NOT NULL,
    ends_at timestamptz,
    source text NOT NULL CHECK (source IN ('manual', 'quiet_hours')),
    reason text,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (ends_at IS NULL OR ends_at > starts_at)
);

CREATE TABLE IF NOT EXISTS reminder_scanner_runs (
    id uuid PRIMARY KEY,
    owner text NOT NULL,
    started_at timestamptz NOT NULL,
    heartbeat_at timestamptz NOT NULL,
    stopped_at timestamptz,
    stop_reason text
);
