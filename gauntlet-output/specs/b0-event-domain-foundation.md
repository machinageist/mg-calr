# Spec: Event Domain Foundation

Feature ID: b0-event-domain-foundation
Parent: B — Calendar and event management
Status: implementation slice

## Purpose

Provide a transport-neutral, behavior-tested event value layer that later PostgreSQL CRUD and CLI workflows can consume without duplicating temporal or identity rules.

## In scope

- Typed calendar and event identifiers.
- Calendar name and event title validation.
- Timed event representation with a validated IANA timezone name, fixed-offset instants, and end-after-start validation.
- All-day half-open date ranges.
- Stable RFC UID derivation from immutable event identity.
- Standard metadata containers and defaults.
- Serialization round trips with UID validation on decode.
- A repository interface and application construction boundary that validates before persistence delegation.

## Explicitly out of scope

This slice does not claim to implement event-core completion. The following remain separate slices:

- PostgreSQL repository implementation and transaction boundaries.
- Calendar/event CRUD commands and agenda projections.
- Revision checks, lifecycle mutations, audit, undo, trash, restore, and purge.
- DST local-wall-time gap/fold resolution and recurrence-local semantics.
- RRULE expansion and occurrence exceptions.
- Lossless iCalendar parsing/serialization and opaque property preservation.
- Reminder persistence and delivery.
- Quickshell/TUI interfaces.

## Acceptance criteria

1. Invalid titles and calendar names fail before repository writes.
2. Timed values reject missing/unknown IANA zone names and non-positive ranges.
3. All-day values enforce an exclusive end date after the start.
4. Generated RFC UIDs remain stable when event metadata changes.
5. Deserialization cannot bypass RFC UID validation.
6. Metadata defaults are deterministic and JSON round trips preserve the model.
7. The application boundary constructs validated values before invoking its repository trait.
8. `cargo fmt --check`, workspace tests, strict Clippy, and `git diff --check` pass.

## Current implementation evidence

- `src/domain.rs`
- `src/application.rs`
- `tests/event_domain.rs`

## Known next gates

The full `b-event-calendar-core` contract remains open. It requires real persistence, CLI CRUD, DST fixtures, recurrence, revisions, lifecycle, and agenda projections before that feature can pass.
