# Spec: PostgreSQL Calendar/Event Persistence

Feature ID: b1-postgres-event-persistence
Parent: B — Calendar and event management
Status: implementation slice

## Purpose

Persist validated calendars and events through PostgreSQL without bypassing domain construction or allowing events to attach to missing/deleted calendars.

## In scope

- Parameterized calendar insertion in a transaction.
- Parameterized event insertion in a transaction.
- `SELECT ... FOR UPDATE` live-calendar validation before event insertion.
- Timed/all-day projection into the foundation schema.
- Standard event metadata and extension properties preservation.
- Async application repository boundary.
- Typed `calendar_not_live` error mapping.
- Deterministic SQL/repository contract tests.

## Explicitly out of scope

- CLI calendar/event commands and agenda views.
- Edit, move, trash, restore, purge, revision, audit, and undo workflows.
- Recurrence expansion and exceptions.
- iCalendar/CalDAV synchronization.
- Reminder delivery and systemd units.
- Runtime PostgreSQL verification when no disposable opt-in database is configured.

## Acceptance criteria

1. SQL uses parameters rather than interpolated user values.
2. Fallible insert paths commit only on success and roll back on errors.
3. Event insertion locks and validates a live parent calendar.
4. Timed and all-day values map to mutually exclusive schema columns.
5. Metadata is preserved in standard columns and extension JSON.
6. Domain/application validation occurs before repository insertion.
7. Contract tests and all available format/test/Clippy/diff gates pass.
8. The slice does not claim full event-core, sync, or reminder completion.

## Current implementation evidence

- `src/storage.rs`
- `src/application.rs`
- `src/lib.rs`
- `tests/repository_contract.rs`

## Next gate

A subsequent slice must expose CLI create/inspect/agenda workflows and runtime-test these repository methods against a disposable PostgreSQL database.
