# Event recurrence specification

Status: active change specification

## Desired outcome

`mg-calr` can hold a recurring event as one rule and show every occurrence it
implies, so a repeating schedule is imported once rather than copied per day.
The immediate driver is two iCalendar files in `~/mg-coreforge` whose 36 events
are all recurring across thirteen weeks.

## Current implementation truth

Verified on 2026-09-04 against the live `mg_calr` database at baseline `c22fb3a`:

- `events.recurrence_rule` is a free-form `text` column. Nothing validates it and
  nothing reads it: `recurrence_rule` does not appear anywhere in
  `src/application.rs`, so the agenda matches only an event's base `time`.
- Storing an RRULE today would therefore publish a rule the application ignores,
  showing one occurrence while claiming a repeating series.
- Todos already recur. `RecurrenceRule` in `src/domain/todo.rs` carries frequency,
  interval, count and until, and `expand_due_instances_indexed` walks it. That
  type is part of the `mg-todo` projection payload contract, so it is not free to
  change, and it cannot express a weekday set.
- The source files need a weekday set: `daily-skeleton.ics` repeats on
  `BYDAY=MO,TU,WE,TH,FR,SA`, which no existing rule in this repository can state.
- There is no iCalendar reader. `event import` consumes a document this
  application previously exported.

## Slices

### Slice 1: an event recurrence rule the domain can expand

Give events their own validated recurrence rule carrying frequency, interval,
count, until and an optional weekday set, and expand it into concrete occurrences
inside a bounded window without mutating the stored event.

The rule is separate from the todo rule on purpose. The todo rule is a
cross-application contract with `mg-todo`; the event rule is `mg-calr`'s own, and
only events need a weekday set.

### Slice 2: recurring events on the agenda

Expand event occurrences into agenda rows the way todo occurrences already are,
carrying the occurrence index, and keeping each occurrence's own duration.

### Slice 3: an iCalendar reader

Read `VEVENT` records into events and rules, refusing what cannot be represented
rather than importing a weaker schedule silently.

## Acceptance criteria

1. A weekly rule with a weekday set produces one occurrence per named weekday,
   stops at its count or until bound, and never exceeds the queried window.
2. An occurrence keeps the base event's duration, and a timed occurrence keeps its
   wall time across a daylight-saving transition in its own zone.
3. A stored rule round-trips through export and import unchanged.
4. An event with no rule behaves exactly as it does today.
5. The agenda lists every occurrence inside its window, ordered with the events
   and todos already there, each carrying its occurrence index.
6. Importing the two source files creates 36 events whose expansions match the
   occurrence counts their RRULEs state, and a re-import is refused rather than
   silently duplicating them.
7. An RRULE part this rule cannot represent fails the import with the part named.
8. Focused tests, all targets, strict Clippy, formatting, diff hygiene, and the
   disposable PostgreSQL suite pass.

## Non-goals

- `EXDATE`, `RDATE`, `RECURRENCE-ID`, and per-occurrence exceptions or edits.
- `BYMONTH`, `BYMONTHDAY`, `BYSETPOS`, `BYWEEKNO`, `BYYEARDAY`, `WKST`.
- Editing or cancelling a single occurrence of a series.
- Alarms, attendees, organizers, and free/busy publishing.
- Writing iCalendar out, vdirsyncer, or any network synchronization.
- Changing the `mg-todo` projection contract or its `RecurrenceRule`.
