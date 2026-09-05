# Event recurrence implementation plan

Baseline: `c22fb3a`

## Dependency order

1. The rule and its expansion, proven in the domain alone.
2. Persistence, so a rule survives a restart and a round trip.
3. The agenda, so occurrences are actually visible.
4. The iCalendar reader, so the source files can land.

## Slice 1: rule and expansion

- `src/domain.rs`: `EventRecurrence` with frequency, interval, count, until and an
  optional weekday set, validated on construction; and expansion over a bounded
  window returning indexed `EventTime` values that keep the base duration.
- `tests/event_domain.rs`: weekday sets, count and until bounds, window clipping,
  duration preservation, a daylight-saving transition, and rejected rules.

## Slice 2: persistence

- `migrations/0008_event_recurrence.sql`: retype `events.recurrence_rule` to
  `jsonb` with a shape check, the way migration 6 did for todos. Existing rows
  hold no rule, so nothing is converted or invented.
- `src/storage.rs`: read and write the rule; extend the schema verification.
- Prove a round trip and a restart against disposable PostgreSQL.

## Slice 3: agenda

- `src/application.rs`: expand event occurrences inside the query window, carrying
  the occurrence index, and leave an event without a rule on its current path.
- `tests/agenda_contract.rs`: ordering against todos, window edges, and an
  unchanged non-recurring event.

## Slice 4: iCalendar reader

- `src/ics.rs`: unfold lines, read `VEVENT` into title, description, `DTSTART`,
  `DTEND` with `TZID`, and `RRULE`. Reject unsupported RRULE parts by name.
- `src/main.rs`: `event import-ics --file --calendar`.
- `tests/ics_contract.rs`: fixtures from the two source files, unsupported parts,
  malformed input, and idempotence on re-import.

## Verification

```bash
TMPDIR=/dev/shm cargo test --all-targets
TMPDIR=/dev/shm cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
MG_CALR_RUN_DATABASE_TESTS=1 MG_CALR_TEST_DATABASE_URL=postgresql:///mg_calr_test \
  TMPDIR=/dev/shm cargo test --test postgres_integration -- --ignored
git diff --check
```

## Stop conditions

Stop and cut a prerequisite slice rather than expanding if the work needs a
changed `mg-remindr` projection contract, a per-occurrence exception model, a second
writable authority for events, or a network fetch.
