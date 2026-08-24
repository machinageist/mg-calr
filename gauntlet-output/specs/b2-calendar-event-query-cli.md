# B2 Calendar/Event Query and CLI Slice

## Claim boundary

This contract covers a narrow, deployable PostgreSQL-backed calendar and event workflow:

- create and list live calendars;
- create, show, and list live events;
- query one local-day agenda;
- preserve timed and all-day temporal forms;
- expose the same transport-neutral projections to human and versioned JSON renderers;
- reject inconsistent RFC3339 offset/IANA timezone pairs;
- return deterministic ordering independent of repository iteration order;
- prove PostgreSQL runtime behavior against an explicitly opted-in disposable database with cleanup.

It does **not** claim event update/delete/restore, recurrence expansion or exceptions, attendee workflows, reminders, todos, week/month views, search, CalDAV, import/export, backup/restore, Quickshell integration, or packaging.

## Required behavior

1. `calendar create` validates a nonempty name through the domain constructor and persists transactionally.
2. `calendar list` returns live calendars ordered by case-folded name and stable ID.
3. `event create` requires exactly one temporal form:
   - timed: RFC3339 start/end plus an IANA timezone;
   - all-day: start and exclusive end dates.
4. Timed event offsets must match the named timezone at each supplied instant. Valid aliases such as `US/Pacific` and valid differing offsets across a DST fold are accepted; silent UTC fallback is forbidden.
5. `event show`, `event list`, and `event day-agenda` read through the transport-independent repository/use-case boundary.
6. Event ordering is total and deterministic at the application boundary: all-day before timed, temporal start, case-folded title, then stable ID.
7. Human and JSON output derive from the same `CalendarProjection` and `EventProjection` values.
8. SQL is parameterized. Event insertion validates and locks the live parent calendar in the same transaction.
9. Unknown or soft-deleted parent calendars fail with a typed `CalendarNotLive` error.
10. Live integration tests require both explicit opt-in and a parsed effective database name exactly equal to `mg_calr_test`; they never drop the database and remove inserted rows even when the exercised workflow fails.

## Verification

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
MG_CALR_RUN_DATABASE_TESTS=1 \
MG_CALR_TEST_DATABASE_URL='postgresql:///mg_calr_test?host=/run/postgresql' \
cargo test --test postgres_integration -- --ignored --nocapture
git diff --check
```

Before and after the live integration test, `calendars`, `events`, `todos`, and `reminders` in `mg_calr_test` must retain their baseline row counts.
