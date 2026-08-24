# Spec: Event and Calendar Core

**Feature ID:** b-event-calendar-core
**Parent feature:** B (B1–B8)
**Spec author agent:** Hermes Agent
**Date:** 2026-08-23
**Iteration:** 2 — blind-review remediation

---

## 1. Purpose

### 1.1 One-sentence job

Let a user safely create, inspect, change, organize, recur, remind, trash, restore, and deliberately purge calendar events through equivalent guided and scriptable interfaces without temporal drift or identity loss.

### 1.2 Why it matters

Events are the first daily-use domain built on the application foundation. `mg-calr` must match mature calendar semantics while remaining keyboard-first, locally authoritative, inspectable, and suitable for later agenda, iCalendar, synchronization, reminder-delivery, TUI, and Quickshell consumers. A weak event model would make later recurrence, sync, and notification work unsafe.

### 1.3 Success signal

Against a disposable PostgreSQL database, synthetic calendar/event CRUD and event-only day/week/month queries produce equivalent human and schema-versioned JSON results; DST gap/fold, all-day, recurrence, exception, trash/restore, and purge fixtures pass without UID change, partial mutation, unconfirmed overwrite, or network access.

### 1.4 Feature and milestone boundary

This specification defines the complete target contract for B1–B8, but implementation remains dependency ordered:

- **Milestone 2 — Event core:** B1, B2, B3, B4, the locally representable portion of B5, non-recurring B8 schedule storage, and event-only day/week/month projections. It includes calendar records; event identity/metadata; guided and non-interactive create; inspect/edit/move/trash/restore/purge; timed/all-day/timezone behavior; and stable JSON. CRUD works with no sync transport or notification backend.
- **Milestone 3 — Recurrence:** B6, B7, recurring-reminder projection from B8, and recurrence-aware mutations. This is gated by the required iCalendar/RRULE and recurring-series mutation evidence spikes.
- **Milestone 5 — Reminder delivery:** scanning, delivery claims, systemd service/timer, actions, catch-up, DND, and notification adapters remain E1–E8. B8 owns event reminder definitions only.
- **Milestone 7 — Interchange:** parsing/serializing unknown iCalendar properties is F1–F3. B5 requires event CRUD to preserve opaque extension data already present; it does not authorize an iCalendar codec.
- **Milestone 8 — Sync:** remote discovery, vdir, conflicts, remote tombstone reconciliation, organizer/attendee scheduling transport, and all network access remain F4–F12.
- The event-only day/week/month application queries needed by Milestone 2 are included. Final terminal formatting, todos in views, filtering/search, and generalized D1–D10 rendering remain feature D.

No later milestone may bypass the event use cases or reinterpret the temporal and identity invariants below.

---

## 2. User Stories

> As a keyboard-first user, I want guided event creation to ask only for missing values and explicitly ask whether I want reminders, so that fast entry remains safe and discoverable.

> As a script author, I want every promptable value represented by a flag, `--no-input` to prohibit prompts, and deterministic JSON, so that automation never blocks or parses presentation text.

> As a traveler, I want each timed event to retain an IANA timezone and make ambiguous or nonexistent wall times explicit, so that editing and DST transitions never silently move it.

> As a user planning multi-day activities, I want all-day events represented as date ranges rather than midnight timestamps, so that they do not drift across timezones.

> As a user with repeating events, I want to edit or delete one occurrence, this and future occurrences, or the whole series, so that history and untouched occurrences remain correct.

> As a cautious user, I want normal deletion to move events to trash and restoration to preserve identity, while purge is explicit and irreversible, so that mistakes are recoverable.

> As an integration author, I want unknown iCalendar fields, organizer/attendee data, RFC UIDs, and event revisions preserved by ordinary local edits, so that a later round trip cannot lose information.

---

## 3. UX Specification

### 3.1 Screen / view inventory

This is a terminal-only feature. It introduces command views, not graphical screens:

| View | Navigation | Status | Layout |
|---|---|---|---|
| Calendar list/show | `calendar list`, `calendar show SELECTOR` | New | compact table or one JSON envelope |
| Calendar create/edit/default/trash/restore/purge | corresponding `calendar` subcommand | New | guided form when fields are missing; result summary |
| Event create | `event create` | New | sequential prompt form or flag-only command |
| Event show | `event show SELECTOR` | New | labeled detail record including temporal interpretation, recurrence, reminders, revision, and trash state |
| Event edit/move | `event edit SELECTOR`, `event move SELECTOR --calendar …` | New | patch form with before/after confirmation for consequential changes |
| Event trash/restore/purge | corresponding `event` subcommand | New | confirmation/recovery summary; trash query is opt-in |
| Event day/week/month projection | `event day`, `event week`, `event month` | New Milestone 2 application queries | chronological rows; month supplies Sunday-first date buckets plus event rows for later D rendering |

`SELECTOR` is an immutable UUID in Milestone 2. A stable short ID, structured search, and interactive chooser become available after A3/D10. Until then, a title or ambiguous text is never accepted as mutation authority.

### 3.2 Command inventory and input contract

Global foundation flags remain `--json`, `--no-color`, and `--database-url`. Mutating commands also accept `--no-input`; destructive commands use `--yes` as specified below. Every mutation of an existing aggregate requires an expected revision and accepts an idempotency key; guided mode supplies both from the snapshot it displays, while `--no-input` requires them as flags.

Calendar commands:

```text
mg-calr calendar list [--include-trash]
mg-calr calendar show CALENDAR_ID [--include-trash]
mg-calr calendar create [--name TEXT] [--color VALUE] [--default [--if-current-default-revision N]] [--operation-id UUID] [--no-input]
mg-calr calendar edit CALENDAR_ID [--name TEXT] [--color VALUE|--clear-color] --if-revision N [--operation-id UUID] [--no-input]
mg-calr calendar default CALENDAR_ID --if-revision N [--if-current-default-revision N] [--operation-id UUID] [--no-input]
mg-calr calendar trash CALENDAR_ID [--move-events-to CALENDAR_ID|--trash-events]
  --if-revision N [--event-revision-manifest SHA256] [--operation-id UUID] [--yes] [--no-input]
mg-calr calendar restore CALENDAR_ID --if-revision N [--operation-id UUID]
  [--as-default [--if-current-default-revision N]] [--no-input]
mg-calr calendar purge CALENDAR_ID --if-revision N --yes [--operation-id UUID] [--no-input]
```

Event commands:

```text
mg-calr event create
  [--calendar CALENDAR_ID] [--title TEXT]
  (--start LOCAL_OR_OFFSET_TIME (--end LOCAL_OR_OFFSET_TIME | --duration DURATION)
    [--timezone IANA_ZONE] [--fold earlier|later]
    [--start-fold earlier|later] [--end-fold earlier|later]
   | --all-day DATE [--through DATE]
   | --all-day-start DATE --all-day-end-exclusive DATE)
  [--description TEXT] [--location TEXT] [--url URL]
  [--status tentative|confirmed|cancelled] [--busy|--free]
  [--category TEXT]... [--organizer URI]
  [--attendee URI]... [--rrule RRULE]
  [--rdate LOCAL_OR_OFFSET_TIME_OR_DATE]... [--exdate RECURRENCE_ID]...
  [--reminder OFFSET_OR_ABSOLUTE]... [--no-reminder]
  [--operation-id UUID] [--no-input]

mg-calr event show EVENT_ID [--include-trash]
mg-calr event edit EVENT_ID --if-revision N [--if-related-revision EVENT_ID=N]...
  [--title TEXT] [--description TEXT|--clear-description]
  [--location TEXT|--clear-location] [--url URL|--clear-url]
  [--status tentative|confirmed|cancelled] [--busy|--free]
  [--set-category TEXT]... [--remove-category TEXT]... [--clear-categories]
  [--organizer URI|--clear-organizer]
  [--add-attendee URI]... [--remove-attendee URI]... [--clear-attendees]
  [--start LOCAL_OR_OFFSET_TIME] [--end LOCAL_OR_OFFSET_TIME|--duration DURATION]
  [--all-day DATE [--through DATE]|--all-day-start DATE --all-day-end-exclusive DATE]
  [--timezone IANA_ZONE] [--timezone-mode preserve-instant|preserve-local]
  [--start-fold earlier|later] [--end-fold earlier|later]
  [--rrule RRULE|--clear-recurrence]
  [--add-rdate LOCAL_OR_OFFSET_TIME_OR_DATE]... [--remove-rdate RECURRENCE_ID]...
  [--add-exdate RECURRENCE_ID]... [--remove-exdate RECURRENCE_ID]...
  [--add-reminder OFFSET_OR_ABSOLUTE]... [--remove-reminder REMINDER_ID]... [--clear-reminders]
  [--scope occurrence|future|series] [--occurrence RECURRENCE_ID]
  [--resolve-extension-collision PROPERTY_ID=keep-opaque|quarantine-opaque]
  [--operation-id UUID] [--no-input]
mg-calr event move EVENT_ID --calendar CALENDAR_ID --if-revision N [--if-related-revision EVENT_ID=N]...
  [--scope occurrence|future|series] [--occurrence RECURRENCE_ID] [--operation-id UUID] [--no-input]
mg-calr event trash EVENT_ID --if-revision N [--if-related-revision EVENT_ID=N]...
  [--scope occurrence|future|series] [--occurrence RECURRENCE_ID] [--operation-id UUID] [--yes] [--no-input]
mg-calr event restore EVENT_ID --if-revision N [--calendar CALENDAR_ID]
  [--scope occurrence|future|series] [--occurrence RECURRENCE_ID]
  [--trash-operation UUID] [--if-related-revision EVENT_ID=N]...
  [--operation-id UUID] [--no-input]
mg-calr event purge EVENT_ID --if-revision N --yes [--operation-id UUID] [--no-input]
mg-calr event extensions EVENT_ID [--include-quarantine] [--json]
mg-calr mutation status OPERATION_ID [--json]
mg-calr event day [DATE] [--timezone IANA_ZONE] [--include-trash]
mg-calr event week [DATE] [--timezone IANA_ZONE] [--include-trash]
mg-calr event month [YYYY-MM] [--timezone IANA_ZONE] [--include-trash]
```

Rules:

- Every prompt has a corresponding flag. Repeatable metadata uses repeatable flags; explicit `--clear-*` flags distinguish clearing from omission.
- `--no-input` never reads stdin or `/dev/tty`. Missing/ambiguous required input returns `input_required` and names the exact flags that can resolve it.
- `--json` uses the foundation envelope. Prompts, when allowed, go to the controlling terminal/stderr; stdout contains exactly one JSON object. Automation should combine `--json --no-input`.
- `create` requires a title, calendar, and one complete temporal form. The calendar defaults to the one live default calendar; absence of a default prompts or fails under `--no-input`.
- Guided creation asks title, calendar, timed versus all-day, temporal fields, optional metadata, recurrence, and finally “Add a reminder? [y/N]”. The default is **no reminder**. `--no-reminder` and one or more `--reminder` flags are mutually exclusive.
- Strict accepted forms and bounded shortcuts are documented and locale-independent: ISO dates, RFC 3339 offset timestamps, `YYYY-MM-DD HH:MM`, durations such as `30m`/`2h`, and `today`, `tomorrow`, `next fri`. There is no unconstrained natural-language parser.
- A mutation selector must resolve to exactly one live or explicitly included trashed object before a transaction begins. No “first match” behavior is permitted.
- Human confirmations include affected object count and scope. `--yes` confirms only the fully resolved target printed by dry validation; it never resolves ambiguity.
- `--if-revision` is mandatory for every existing calendar/event/master mutation in `--no-input` mode and is never inferred from a fresh hidden read. Guided mode reads and displays revision `N`, then submits exactly `ExpectedRevision(N)` after confirmation. Repository update/delete predicates always include `id AND revision`; zero affected rows is `stale_revision`. There is no unconditional mutation API or last-writer-wins fallback. `--yes` cannot waive this check.
- Multi-aggregate mutations bind every row shown at confirmation. Recurrence operations use repeatable `--if-related-revision ID=N`. A calendar operation that moves/trashes member events uses a dry-validation manifest: SHA-256 over sorted `(event_id, revision, action)` tuples; `--no-input` requires `--event-revision-manifest` whenever the set is nonempty, and any membership/revision change fails `stale_revision_manifest`. Setting/replacing a default requires the displayed current default revision as well as the target revision. A first-calendar/default operation explicitly records that no prior default existed. No aggregate is an unversioned side effect of another aggregate's command.
- `--operation-id` is a caller-provided UUID in automation. Guided mode generates and displays one before commit. A durable mutation receipt, request fingerprint, transaction ID, target IDs, before/after revisions, and redacted result are committed atomically with the mutation. Retrying the same ID and fingerprint returns the committed result without another write; reuse with a different fingerprint returns `operation_id_reused`. If no receipt exists after rollback, the same request may safely execute. This is the recovery mechanism for a lost process/database response.
- `--fold` on create is accepted as shorthand only when exactly one timed boundary is ambiguous. If both boundaries are ambiguous, or their intended choices differ, `--start-fold` and `--end-fold` are required. Edit uses the boundary-specific flags shown above. For a recurring event, boundary flags also set the persisted choice for generated future folds; if a source boundary is unique and no flag is supplied, RFC's first occurrence (`earlier`) is recorded explicitly and returned in the create summary rather than left to a library default.

### 3.3 Interaction flows

#### Create a calendar

1. Resolve name/color/default from flags, prompting for missing required name unless `--no-input`.
2. Normalize only surrounding whitespace; preserve user-visible case and reject empty/control-character names.
3. In one transaction, insert the calendar, and if it is the requested default, clear the prior live default and set the new one.
4. If this is the first live calendar, make it default automatically and report that decision.
5. Record an audit transaction and return the persisted row and revision.

#### Create an event

1. Resolve the target calendar and reject a trashed/missing calendar.
2. Parse metadata without connecting to any network service.
3. Resolve the temporal form using Section 4.2. For a DST gap, stop and suggest valid neighboring offsets; for a fold, prompt for each ambiguous boundary or require `--fold`/the boundary-specific flags under `--no-input`.
4. Parse and validate RRULE syntax when recurrence is enabled. Expansion is bounded and deferred to Milestone 3.
5. Collect zero or more reminder definitions. Guided input explicitly asks; the default is zero.
6. Show a human summary when guided. Persist event, metadata, reminder definitions, and audit entries in one transaction.
7. Return immutable event ID, RFC UID, revision, calendar ID, normalized temporal representation, and warnings (if any). No notification is delivered and no sync occurs.

#### Inspect, edit, and move

1. Resolve immutable identity and read a snapshot/revision.
2. `show` uses the same application DTO as JSON and projections and reports opaque-property count, namespace/name, stable property ID, byte length, SHA-256, collision/quarantine state, and sensitivity warning without values. `event extensions` is the authorized read-only diagnostic interface in this feature: after the same local database authorization as `show`, it emits the exact stored opaque envelope and base64 raw bytes in deterministic property-ID order. It performs no iCalendar parsing, serialization, or network access and therefore does not cross into F1 interchange ownership.
3. Build a patch: omission means unchanged; `--clear-*` means NULL/empty. Validate the final aggregate, not fields independently.
4. If a timezone changes, require an explicit timezone mode in non-interactive use; guided mode explains and asks whether to preserve the instant or the wall-clock fields. Gap/fold rules are reapplied.
5. Recurring mutations require explicit scope and occurrence identity under `--no-input`; guided mode asks. Milestone 2 rejects scope options until Milestone 3 rather than approximating them.
6. Submit the mandatory expected revision from Section 3.2. A mismatch returns `stale_revision` with current revision and performs no write. The check applies equally to master, occurrence, future split, move, trash, restore, and purge paths; a multi-master split/merge checks every displayed participant revision under one transaction.
7. Persist the event/reminders/exceptions, revision increment, mutation receipt, and audit rows atomically. Unknown extension envelopes not addressed by an explicit collision resolution are copied byte-for-byte, including ordering, parameters, casing, raw line bytes, and invalid-but-retained payload.
8. If a standard-field patch collides with an opaque entry, default to `extension_collision` and include safe property ID/hash metadata plus the exact `event extensions` command. `keep-opaque` cancels only the colliding standard-field change. `quarantine-opaque` applies the standard edit but moves the exact opaque envelope, without byte change, to a non-serialized quarantine in the same aggregate; it requires explicit confirmation, revision, and audit provenance and remains inspectable/restorable. Neither choice silently deletes or rewrites unsupported data.

#### Trash, restore, and purge

1. `event trash` sets local `deleted_at`, increments revision, and records provenance; it does not set `remote_tombstoned_at`. Trashing an already trashed event is an idempotent success with `changed:false`.
2. Recurring trash uses occurrence/future/series scope and returns a durable `trash_operation` ID. Occurrence deletion adds a cancellation owned by that trash operation while preserving any prior override underneath; it never trashes the master. Future deletion transactionally splits at the original recurrence ID, leaves the historical master live through the occurrence before the cut, and creates the linked future master in local-trash state. Series deletion trashes the master and all linked active split descendants as one revision-checked operation.
3. `event restore` is scope-complete. Series restore resolves trashed identities, verifies the calendar exists/live (or requires `--calendar`), clears local deletion state, and restores all descendants trashed by the named operation. Occurrence restore removes only the cancellation owned by `--trash-operation`, revealing any prior override. Future restore reactivates the exact future master produced at the cut. Each path requires master ID, scope, original recurrence ID where applicable, trash-operation ID when more than one operation could match, and expected revisions for every affected master; ambiguity or intervening incompatible lineage returns `restore_conflict` with no write. Restore increments revisions and preserves IDs, RFC UIDs, exceptions, opaque/quarantined data, audit, and split lineage; it never clears a remote tombstone.
4. `event purge` requires an already trashed event, exact immutable ID, `--yes`, and a matching revision. It writes immutable purge/audit identity before deleting mutable payload and reminder rows. A recurring master cannot purge while a restorable occurrence/future trash operation or retained split descendant references it. Purge is irreversible by CLI and returns `purged:true`; subsequent reuse of its RFC UID is forbidden.
5. Once sync exists, purge is blocked while an unresolved conflict, required remote deletion, or tombstone retention obligation exists. Milestone 2 has no remote state and must not fabricate it.
6. Calendar trash with live events must choose `--move-events-to` or `--trash-events`; otherwise it fails. Calendar purge requires the calendar to be trashed and contain no retained events/tombstone obligations. The last live calendar may be trashed only if its events are handled; the next command requiring a calendar then prompts/fails until one exists.

#### Projection

1. Resolve display timezone from `--timezone`, configuration, then system IANA timezone.
2. Compute a half-open query interval for the requested day/week/month. Week starts Sunday; month output includes Sunday-first date buckets.
3. Query events intersecting the interval; project timed events to the display zone and all-day events by date intersection without converting them to instants.
4. Expand recurrence only within the requested interval and safety cap after Milestone 3.
5. Sort by local date, all-day before timed, start instant, title, then immutable ID. Human and JSON renderers receive this same ordered DTO.
6. Default projections exclude trash. `--include-trash` marks state textually and in JSON rather than with color alone.

### 3.4 Layout descriptions

Human calendar lists use headings `ID`, `DEFAULT`, `NAME`, and optional `COLOR`; “default” is literal text/symbol plus an accessible word, never color-only. Event detail orders identity/revision, title/calendar, temporal range/timezone, status/busy state, location/URL, description, categories, recurrence, reminders, organizer/attendees, then lifecycle state.

Chronological projections expose date headings followed by all-day and timed rows. An empty query prints `No events.` and JSON returns an empty `events` array with the requested interval/timezone. Month application data contains a Sunday-first grid of dates and markers plus an ordered agenda; feature D owns compact terminal-width formatting.

### 3.5 Input, keyboard, and responsive behavior

All actions are keyboard accessible through commands, flags, line prompts, arrows for bounded choices, Enter to accept an explicitly displayed default, and Ctrl-C to cancel before commit. Ctrl-C/EOF produces `input_cancelled`, exits without mutation, and emits no success JSON. There are no pointer, touch, voice, camera, haptic, or sound interactions.

At narrow terminal widths, human detail wraps on word boundaries with continuation indentation; tables degrade to labeled records rather than truncate identity, timezone, scope, or errors. JSON is width-independent. `--no-color` and `NO_COLOR` suppress ANSI output everywhere.

### 3.6 Transitions and animation

N/A — commands replace terminal output synchronously and use no animation. Reduced-motion behavior is inherently satisfied.

### 3.7 Error states

| Trigger | Presentation | Recovery | Data-loss risk |
|---|---|---|---|
| no live/default calendar | `calendar_required` with create/select flags | create calendar or pass `--calendar` | none |
| ambiguous/missing selector | `selector_ambiguous`/`not_found`; candidate data only in guided chooser later | use exact immutable ID | none |
| incomplete `--no-input` request | `input_required`, missing flag list | rerun with named flags | none |
| invalid URL/status/RRULE/reminder/range | typed field error, original safe value and constraint | correct field | none |
| timed/all-day fields mixed | `temporal_form_conflict` | choose exactly one form | none |
| nonexistent DST wall time | `local_time_gap`, zone and valid alternatives | change time/zone | none |
| ambiguous DST wall time | `local_time_fold`, both offsets | add `--fold earlier|later` | none |
| timezone change lacks mode | `timezone_mode_required` | select preserve mode | none |
| stale revision/concurrent write | `stale_revision`, current revision | re-inspect and reapply intentionally | none; no overwrite |
| recurring mutation lacks scope | `recurrence_scope_required` | choose occurrence/future/series | none |
| expansion exceeds cap | `occurrence_limit_exceeded`, interval/cap | narrow interval or explicit higher bounded policy later | none |
| calendar has events | `calendar_not_empty` with counts | move or trash explicitly | none |
| restore target calendar absent/trashed | `restore_calendar_required` | restore/select live calendar | none |
| purge not trashed/not confirmed/retention blocked | typed purge error | trash, pass exact ID/`--yes`, or resolve obligation | none |
| database/transaction interruption | typed storage error with operation ID and exact `mutation status` command; no success claim | follow Section 4.4.1; replay only the same fingerprint or re-inspect revision | none by atomic receipt/replay |
| unsupported extension collides with edited standard field | `extension_collision` with property ID/hash, no value | run `event extensions`; choose `keep-opaque` or non-lossy `quarantine-opaque` explicitly | none; raw bytes remain inspectable |
| scoped restore has ambiguous/intervening lineage | `restore_conflict` with safe lineage/revision IDs | inspect named masters/trash operation; retry only with all accepted revisions | none; no partial merge |

Errors use the foundation JSON error envelope on stderr and stable nonzero exit classes. User content, attendee URIs, descriptions, and URLs are not echoed in logs or generic diagnostics.

### 3.8 Accessibility

- Prompt labels state field, accepted format, and default in text. Choices include names and hotkeys; state never depends on color.
- Focus order follows the guided creation order in Section 3.3 and is deterministic. Error recovery returns focus to the failing field while preserving non-sensitive prior answers in memory only.
- Screen readers receive ordinary terminal text without cursor-position-only interfaces. Decorative symbols always have adjacent words or a plain-text fallback.
- Long descriptions, categories, attendees, and errors wrap rather than clip. Identity, date, timezone, fold, recurrence scope, and confirmation remain visible at 40 columns.
- `--no-color`, `NO_COLOR`, non-interactive flags, JSON, Ctrl-C cancellation, and help examples are contract-tested.
- Future Quickshell/TUI clients must consume public application/JSON commands, expose semantic labels and keyboard focus, and may not read PostgreSQL directly. They are not implemented here.

---

## 4. Implementation Specification

### 4.1 Architecture placement

Target placement in the existing single package:

- `src/domain/calendar.rs`: calendar aggregate, lifecycle, default invariant.
- `src/domain/event.rs`: event aggregate, metadata, temporal forms, lifecycle, patch validation.
- `src/domain/recurrence.rs`: parsed recurrence abstraction, occurrence keys, exclusions/exceptions, split transformation.
- `src/domain/reminder.rs`: event reminder definition and validation; no delivery behavior.
- `src/application/calendar.rs`: calendar commands and transaction boundaries.
- `src/application/event.rs`: create/show/edit/move/trash/restore/purge and event projection use cases.
- `src/storage/calendar_repository.rs` and `event_repository.rs`: PostgreSQL implementations and optimistic revision checks.
- `src/cli/calendar.rs`, `src/cli/event.rs`, `src/cli/prompts.rs`: argument/prompt translation only.
- `src/render/event.rs`: human and JSON rendering from shared DTOs.
- `migrations/0002_event_core.sql`: strengthen foundation schema and add event-core structures.
- A later recurrence migration may be separate if evidence spikes change exception/split representation.

`domain` has no SQL, CLI, system timezone probing, iCalendar crate, vdirsyncer, or notification knowledge. `application` owns authority and transaction boundaries. `storage` maps persisted values. `render` is read-only. `anyhow` remains process-boundary-only; typed errors live in domain/application.

### 4.2 Data model and invariants

The following are semantic target types; exact library types are selected by evidence spike:

```rust
/// A calendar-owned event whose database identity and RFC UID never change.
pub struct Event {
    pub id: EventId,
    pub rfc_uid: RfcUid,
    pub calendar_id: CalendarId,
    pub revision: u64,
    pub title: String,
    pub temporal: EventTemporal,
    pub metadata: EventMetadata,
    pub recurrence: Option<RecurrenceSet>,
    pub reminders: Vec<EventReminder>,
    pub extensions: OpaqueProperties,
    pub lifecycle: EventLifecycle,
}

/// Persist both resolved instants and the civil intent needed to derive
/// recurring occurrences. The intent is authoritative for expansion.
pub enum EventTemporal {
    Timed {
        starts_at: OffsetDateTime,
        ends_at: OffsetDateTime,
        timezone: TzId,
        source_local_start: PrimitiveDateTime,
        generated_start_fold: FoldChoice,
        end_intent: TimedEndIntent,
        generated_policy: GeneratedLocalTimePolicy,
    },
    AllDay {
        start: Date,
        end_exclusive: Date,
    },
}

pub enum TimedEndIntent {
    /// Chosen by --duration; each occurrence preserves exact elapsed time.
    Elapsed { duration: PositiveDuration },
    /// Chosen by --end; each occurrence preserves the civil day offset and
    /// wall-clock end in the master zone, with an independent fold choice.
    WallClock {
        day_offset: i32,
        local_time: Time,
        generated_end_fold: FoldChoice,
    },
}

pub enum GeneratedLocalTimePolicy {
    /// RFC 5545 §3.3.10: invalid date or nonexistent local time is omitted
    /// and does not consume COUNT; a fold uses the persisted boundary choice.
    Rfc5545OmitInvalidUseRecordedFold,
}

/// Identity of a generated occurrence is based on the master's immutable
/// series identity and its original recurrence-id, even after that occurrence moves.
pub struct OccurrenceKey {
    pub series_id: EventId,
    pub original_recurrence_id: RecurrenceId,
}

pub struct EventReminder {
    pub id: ReminderId,
    pub schedule: ReminderSchedule, // relative offset or absolute instant
}
```

Binding invariants:

1. `CalendarId`, `EventId`, and `ReminderId` remain typed immutable UUIDv7 values. RFC UID is independent, globally unique, immutable, never recycled after purge, and generated locally with a non-secret product-owned suffix/domain form that requires no network lookup.
2. Exactly one temporal variant exists. Timed `end > start`; all-day `end_exclusive > start`. A one-day event is `[date, date + 1 day)`. `--through` is inclusive user syntax converted to end-exclusive storage. Timed creation permanently records whether the user supplied `--duration` or `--end`; serialization, edits, exceptions, and splits may not collapse those forms into two instants.
3. PostgreSQL `timestamptz` stores timed instants; an exact canonical IANA TZDB identifier stores presentation/recurrence zone. Fixed abbreviations such as `EST` are rejected. All-day values use `date` and store no timezone/UTC-midnight surrogate.
4. Parsing a local wall time performs timezone resolution against the configured TZDB. Gaps are rejected. Folds require explicit earlier/later resolution for each ambiguous boundary and persist both the choice and resulting instant; the zone remains attached. A supplied RFC 3339 offset plus `--timezone` must agree for that local instant or fail.
5. A timed non-recurring event is projected by instant. A recurring timed event persists its local start seed, generated-start fold choice, end intent, generated-end fold choice when applicable, zone, and `GeneratedLocalTimePolicy::Rfc5545OmitInvalidUseRecordedFold`; no field may be reconstructed from the two instants. It expands from civil seeds in the master zone and never by adding fixed UTC seconds between starts. Per RFC 5545 §3.3.10, a generated candidate whose calendar date is invalid or whose local start is in a DST gap is omitted and does **not** consume `COUNT`. A fold resolves with the recorded start-fold choice (explicit input, or persisted `earlier`/first occurrence default). With `Elapsed`, end is `resolved_start + duration`. With `WallClock`, end uses the recorded civil day offset/local time and end-fold choice; a nonexistent end, non-positive resolved range, or overflow invalidates and omits that candidate without consuming `COUNT`. `EXDATE` and exceptions address the original local recurrence ID whether or not neighboring candidates were omitted. All-day recurrence advances civil dates and likewise omits invalid generated dates without consuming `COUNT`.
6. Changing only an event's timezone requires either `preserve-instant` (instants unchanged, displayed wall time changes) or `preserve-local` (wall fields unchanged, instants recomputed). The operation records the choice. It never silently chooses across a gap/fold.
7. Metadata covers title, description, location, URL, status, busy/free, categories, RRULE/recurrence dates, zero or more alarms, organizer, and attendees. Categories are ordered deterministically in output and de-duplicated by exact normalized value. Organizer/attendee values are preserved data in v1; active invitations/RSVP are excluded.
8. `OpaqueProperties` is a lossless, versioned envelope owned by the later F1 codec. Each entry has a stable property ID, namespace/name metadata, original order, parameters, encoding marker, exact raw bytes, SHA-256, and active/quarantined state. Ordinary create initializes it empty. Show/edit/move/trash/restore, exception creation, and split/merge transformations preserve every envelope field and raw byte exactly. A standard-field collision fails unless the explicit non-lossy resolution in Section 3.3 is selected. “Value equivalent” is not a permitted substitute for byte equality in B acceptance fixtures.
9. Recurrence masters are stored once; generated future occurrences are derived and bounded by a required query interval. No unbounded occurrence table or “expand forever” operation is permitted.
10. An occurrence exception stores its `OccurrenceKey`, original recurrence-id, prior override stack, effective override/cancellation, owning operation ID, and revision. Moving an occurrence changes its temporal value but not its original recurrence-id/identity. Cancelling/restoring an occurrence pushes/removes only the named operation layer, so a preexisting moved or metadata override cannot be destroyed by trash/restore.
11. Occurrence, future, and series mutations are distinct typed operations. “Future” is a serializable transaction over a locked lineage: verify every expected revision; prove the cut is an occurrence of the unsplit effective series; snapshot source rule/end intent/extensions; make the old master produce exactly original recurrence IDs `< cut`; create a linked future master producing exactly IDs `>= cut`; partition exceptions by original recurrence ID; clone reminder definitions with new IDs but unchanged schedules; copy opaque envelopes byte-for-byte; write parent/child/cut/operation provenance; and commit both revisions, audit, and receipt together. `COUNT` is repartitioned by the number of valid, non-gap candidates before the cut; `UNTIL`, `RDATE`, and `EXDATE` are normalized so the two sides are disjoint and their union equals the pre-split effective set. Any unrepresentable rule, orphaned exception, duplicate/missing recurrence ID, UID mapping uncertainty, or injected failure aborts the entire transaction. Historical occurrence keys resolve through lineage and never silently retarget. Exact external UID/`RANGE=THISANDFUTURE` mapping must be fixed by the required spike before Milestone 3; until it is proven, the production split path remains gated rather than inventing UID behavior.
12. Reminder definitions belong to the event aggregate for transactional CRUD. Relative reminders project from each effective occurrence start; absolute reminders are permitted only for non-recurring events unless the recurrence spike defines an unambiguous per-occurrence mapping. Every projection has a stable `ReminderInstanceKey(reminder_id, series_id_or_event_id, original_recurrence_id_or_singleton, scheduled_for)` and repeated bounded projection returns the same ordered unique keys. B never claims, presents, retries, snoozes, dismisses, or writes delivery state; those operations remain E-only and cannot mutate the event alarm.
13. Local soft deletion (`deleted_at`) and remote tombstone state are distinct. Restore preserves ID/UID/audit/split lineage. Purge retains a minimal immutable identity/provenance record sufficient to prohibit UID reuse and, once sync exists, reconcile deletion.
14. Every aggregate mutation increments `revision` and writes audit before/after state plus its mutation receipt under one transaction ID. `ExpectedRevision` is a required application type, not `Option`; SQL predicates include every affected ID/revision and assert the exact row count. Constraints and serializable transaction/retry policy enforce optimistic concurrency. A failure rolls back all event, reminder, exception, split, extension/quarantine, calendar-default, audit, purge-ledger, and receipt changes.

Migration implications:

- Add `revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0)` to calendars/events and explicit normalized lifecycle checks.
- Add structured categories, organizer, and attendee storage; prefer child tables where ordering/identity matters instead of an opaque JSON shortcut.
- Add event exception/split lineage tables with uniqueness on `(series_id, original_recurrence_id)` and foreign keys that cannot orphan a split.
- Add persisted local recurrence seed, boundary fold choices, generated-gap policy, and tagged elapsed-versus-wall-clock end intent; database checks reject missing intent for recurring timed masters.
- Add mutation receipts keyed by operation UUID plus request fingerprint, and trash-operation/exception-layer provenance needed for deterministic retry and scope-complete restore.
- Add an RFC UID reservation/purge ledger that survives payload purge.
- Strengthen reminders so relative/absolute forms, target ownership, and recurrence restrictions are constrained where PostgreSQL can express them.
- Preserve the foundation's `extension_properties`; a later codec migration may version its shape without dropping source data.
- Add indexes for live calendar membership, timed interval overlap, all-day date overlap, RFC UID, trashed state, exception lookup, and reminder ownership.
- Do not destructively rewrite foundation timestamps until a migration round-trip and disposable-database validation prove equivalence.

### 4.3 API contracts

Application interfaces, not CLI or SQL, are authoritative:

```rust
create_calendar(CreateCalendar, ExpectedPriorDefault, OperationId) -> Result<CalendarDto, CalendarError>
patch_calendar(CalendarId, CalendarPatch, ExpectedRevision, OperationId) -> Result<CalendarDto, CalendarError>
set_default_calendar(CalendarId, ExpectedRevisions, OperationId) -> Result<CalendarDto, CalendarError>
trash_calendar(CalendarTrash, ExpectedRevisionManifest, OperationId) -> Result<MutationDto, CalendarError>
restore_calendar(CalendarRestore, ExpectedRevisions, OperationId) -> Result<CalendarDto, CalendarError>
purge_calendar(CalendarId, PurgeConfirmation, ExpectedRevision, OperationId) -> Result<PurgeDto, CalendarError>
create_event(CreateEvent, OperationId) -> Result<EventDto, EventError>
get_event(EventId, IncludeTrash) -> Result<EventDto, EventError>
patch_event(EventTarget, EventPatch, ExpectedRevisions, OperationId) -> Result<EventDto, EventError>
trash_event(EventTarget, ExpectedRevisions, OperationId) -> Result<MutationDto, EventError>
restore_event(RestoreTarget, TrashOperationId, ExpectedRevisions, OperationId) -> Result<EventDto, EventError>
purge_event(EventId, PurgeConfirmation, ExpectedRevision, OperationId) -> Result<PurgeDto, EventError>
query_events(EventWindow) -> Result<EventProjectionDto, EventError>
inspect_extensions(EventId, IncludeQuarantine) -> Result<ExtensionDiagnosticDto, EventError>
mutation_status(OperationId) -> Result<MutationStatusDto, EventError>
```

`EventTarget` and `RestoreTarget` are either a whole non-recurring event/master or a recurrence master plus occurrence key and explicit `OccurrenceScope`. `ExpectedRevision`, nonempty `ExpectedRevisions`, and `ExpectedRevisionManifest` have no unchecked/default constructor. `ExpectedPriorDefault` is either `NoneExpected` (transactionally assert none exists) or the displayed calendar ID/revision; it prevents calendar creation from silently replacing a concurrent default. There is no parallel unchecked overload. Repositories expose transaction-bound methods and never accept user text selectors.

JSON success retains `{schema_version:1, command, ok:true, data}`. Event data uses explicit tagged temporal objects rather than nullable-field inference:

```json
{"schema_version":1,"command":"event.show","ok":true,"data":{"id":"018f0000-0000-7000-8000-000000000001","rfc_uid":"synthetic-uid@example.invalid","revision":3,"calendar_id":"018f0000-0000-7000-8000-000000000002","title":"Synthetic event","temporal":{"kind":"timed","start":"2026-11-01T08:30:00Z","end":"2026-11-01T09:30:00Z","timezone":"America/Los_Angeles","source_local_start":"2026-11-01T01:30:00","generated_start_fold":"earlier","end_intent":{"kind":"elapsed","duration":"PT1H"},"generated_policy":"rfc5545_omit_invalid_use_recorded_fold"},"status":"confirmed","availability":"busy","categories":[],"reminders":[],"trashed":false}}
```

All-day JSON uses `{"kind":"all_day","start":"2026-08-23","end_exclusive":"2026-08-24"}`. Recurrence output includes canonical RRULE text, series ID, original recurrence ID for occurrences, scope lineage when split, and an `is_exception` boolean. Dates and array ordering are deterministic. New optional fields may be added within schema version 1; removal, type/meaning change, or enum narrowing requires a schema-version compatibility decision in D9.

Typed error codes include those in Section 3.7 plus `invalid_calendar`, `invalid_event`, `invalid_recurrence`, `extension_collision`, `operation_id_reused`, `stale_revision_manifest`, `base_fingerprint_mismatch`, `restore_conflict`, `split_unrepresentable`, `transaction_retry_exhausted`, `purge_blocked`, and foundation storage/serialization errors. Calendar/event content requires only the local PostgreSQL role's existing permissions. There is no account, HTTP endpoint, rate limit, or network authorization in this feature.

### 4.4 State management and authority

PostgreSQL 18-compatible local storage is the sole application authority. The CLI gathers an intent, invokes one application use case, and renders its returned DTO. Human and JSON output never perform independent business queries. Domain values validate temporal and recurrence invariants; application services own mutation scope and transaction orchestration; database constraints are the final local-integrity backstop.

Prompts are ephemeral and are not drafts. Cancellation or validation failure stores nothing. There is no hidden autosave, background process, remote fallback, vdir write, or notification side effect. Later vdir/sync state is a durable mirror and may propose reconciled mutations only through the same application boundary. Later Quickshell/TUI clients invoke public commands/interfaces and never query the database directly.

#### 4.4.1 Transaction interruption and recovery runbook

The following runbook is part of the executable CLI contract, not operator guesswork:

| Observed failure | Required inspection | Deterministic next action |
|---|---|---|
| validation, stale revision, serialization, or injected pre-commit fault | `mutation status OPERATION_ID --json` returns `not_found` and event revision is unchanged | correct/re-inspect, then retry with the same operation ID and current explicitly accepted revision |
| connection/process loss with unknown commit outcome | run `mutation status OPERATION_ID --json` after database connectivity returns | `committed` returns transaction ID, target IDs/revisions, and original redacted result; `not_found` permits the exact same request/operation ID retry |
| exact retry after commit | same command and operation ID/request fingerprint | return `replayed:true` and the original result; no revision/audit/reminder/exception is added |
| operation ID reused for different intent | status shows the original fingerprint/targets but no payload | choose a new operation ID; `operation_id_reused` performs no write |
| serializable/deadlock retry exhausted | status inspection as above | CLI may automatically retry only the same fingerprint within a bounded three-attempt policy; afterward return `transaction_retry_exhausted` and the same status command |
| interrupted future split, scoped restore, or purge | status plus `event show` for every returned target/lineage ID | observe either the complete before-state or complete after-state; any mixed state is a release-blocking integrity failure, never a manual SQL repair instruction |

`mutation status` is read-only, payload-free, stable JSON and never guesses `rolled_back`: absence is reported as `not_found` because a rolled-back receipt cannot commit. Audit transaction lookup must agree with a committed receipt. Repair guidance never asks the application user to run SQL, use sudo, delete rows, or bypass a revision.

### 4.5 Dependencies

Required before Milestone 2:

- A1–A4 implemented foundation contracts, plus an A5 audit transaction design sufficient for event mutations.
- PostgreSQL 18-compatible disposable integration database and migration harness.
- A bounded evidence-backed IANA timezone crate/TZDB strategy; prefer `time` plus an IANA timezone implementation and do not default to `chrono`.

Required before Milestone 3:

- Spike 1: compare maintained iCalendar/RRULE libraries for full recurrence, exclusions, recurrence IDs, timezone/DST behavior, malformed errors, unknown preservation, and fixture/property testing.
- Spike 2: decide and fixture occurrence-only/future/series transforms, moved DST exceptions, split UID interoperability, and deletion round trip.

No network, CalDAV, vdirsyncer, DBus, systemd, Quickshell, TUI, or credential dependency is introduced. Crate additions require maintenance/license/security review and lockfile audit.

#### 4.5.1 Foundation integration, role, and clean-machine contract

Feature A continues to own configuration, `init`, `doctor`, migrations, and PostgreSQL provisioning; B must consume and contract-test those public interfaces rather than duplicate them:

- Resolution is exactly CLI `--database-url` / `--timezone` > `MG_CALR_DATABASE_URL` / `MG_CALR_TIMEZONE` > XDG config TOML > documented default/system discovery. Config uses distinct `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, and `XDG_CACHE_HOME` roots; B commands receive a resolved immutable configuration object and never read ad hoc paths. Pure matrix tests cover every precedence pair, unset/empty/malformed values, legacy-key rejection/migration warning, and URL redaction.
- `mg-calr doctor --component event-core --json` remains non-mutating and reports stable checks for canonical TZDB availability/version, PostgreSQL reachability/version, migration 0002 presence, required tables/indexes/constraints, application-role grants, clock skew visibility, and recurrence backend readiness (`not_applicable` before Milestone 3). It prints exact administrator commands for missing database/role/schema prerequisites but never runs them, prompts for a password, invokes sudo, or exposes a database URL.
- The runtime application role is `NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT`, connects through peer authentication, and receives only schema usage plus DML on B aggregate/receipt/audit/ledger objects and sequence usage where needed. It has no schema DDL, role management, server file, program execution, extension-install, network, or notification-delivery-ledger mutation privilege. The separate migration owner is also non-superuser/no-create-role and owns only the application schema; initial database/role creation is an administrator step printed by `init`.
- A clean Arch Linux container/VM gate starts with no application database or config, installs the built package, verifies `doctor` fails read-only with the documented prerequisite matrix, applies the printed administrator setup out of band, runs migrations as the migration owner, then runs B CRUD/recovery as the unprivileged runtime OS user. It finally revokes one grant and removes TZDB in separate fixtures, verifies exact doctor recovery text, restores the prerequisite, and reruns the synthetic E2E suite. No test uses root after the explicit provisioning boundary.

#### 4.5.2 Interchange and sync seam without scope transfer

B implements no iCalendar codec, vdir mirror, network adapter, or conflict resolver. It does, however, make later F behavior safe: every event snapshot/receipt exposes a deterministic SHA-256 semantic fingerprint over identity, revision-relevant standard fields, recurrence intent/exceptions, lifecycle markers, and exact active/quarantined opaque envelopes. The canonical encoding is schema-versioned and golden-tested. A later F proposal must enter B with base fingerprint, expected revision, and operation ID; disagreement stops as `stale_revision`/`base_fingerprint_mismatch` and preserves both proposal payload and current row outside B's event mutation transaction. B has no API that accepts “remote wins” or suppresses these checks. Local trash/restore never clears `remote_tombstoned_at`; a cross-feature contract fixture proves a stale proposal cannot resurrect, delete, or overwrite an event. Durable vdir state, three-way base storage, interruption orchestration, conflict UI, and remote delete round trips remain F4–F12 acceptance work and must not be advertised by B.

### 4.6 Platform-specific considerations

Arch Linux/Hyprland is first supported. System timezone discovery must resolve `/etc/localtime`/system configuration to a canonical IANA name and return actionable `timezone_unavailable` rather than silently using UTC. `--timezone` always overrides discovery. The core accepts injected clock, TZDB, locale-independent parser, and repository interfaces for portable tests.

TZDB updates can change future civil projections. Persist the canonical zone and source wall-time recurrence semantics, expose TZDB version in diagnostics later, and recompute derived occurrences rather than rewriting stored event instants silently. Feature flags may isolate candidate recurrence backends during spikes, but only one implementation is enabled in production and persisted data cannot depend on an undocumented crate representation.

### 4.7 Performance budget

- Calendar list and single-event show/create/edit should complete within 150 ms at p95 on the supported local machine with a warm local database, excluding interactive think time.
- A day/week/month query over 10,000 stored events should complete within 250 ms at p95 and use bounded result memory; benchmarks record hardware and database state rather than claiming universal latency.
- Recurrence expansion always requires a finite interval and defaults to a hard cap of 10,000 produced occurrences per request. It streams/iterates rather than materializing unbounded futures.
- Normal command peak resident memory target is below 64 MiB for the synthetic 10,000-event benchmark. No daemon/cache is added and startup performs no recurrence precomputation.
- Metadata/opaque property size limits must be configurable and validated before allocation; initial implementation caps an individual text field at 1 MiB, aggregate opaque event payload at 4 MiB, 1,000 categories/attendees, and 100 reminders, returning a typed limit error.
- Network payload is exactly zero. Storage grows with masters, explicit exceptions, reminders, audits, and tombstones—not generated future occurrences. Index effectiveness is checked with representative `EXPLAIN` plans in integration CI.

---

## 5. Test Specification

### 5.0 Binding acceptance vectors

All fixture assertions compare complete ordered DTOs, revisions, lineage rows, audit/receipt counts, and exact bytes—not merely event counts or “no panic.” Tests pin the fixture TZDB release and record it in failure output.

| Fixture | Input | Exact required result |
|---|---|---|
| generated spring gap / `COUNT` | `America/Los_Angeles`, local start `2026-03-01 02:30`, `FREQ=WEEKLY;COUNT=3`, elapsed `30m` | recurrence IDs/local starts are exactly `2026-03-01 02:30`, `2026-03-15 02:30`, `2026-03-22 02:30`; UTC starts exactly `10:30Z`, `09:30Z`, `09:30Z`; nonexistent `2026-03-08 02:30` is absent and did not consume `COUNT` |
| elapsed duration across spring DST | local start `2026-03-08 01:30`, `--duration 2h` | start `2026-03-08T09:30:00Z`, end `2026-03-08T11:30:00Z`, displayed end `04:30 -07:00`; elapsed time exactly two hours |
| wall-clock end across spring DST | same start, `--end '2026-03-08 03:30'` | start `09:30Z`, end `10:30Z`, displayed end `03:30 -07:00`; elapsed time exactly one hour and persisted intent remains `WallClock` |
| independent fall folds | start `2026-11-01 01:30 --start-fold earlier`, wall end `01:45 --end-fold later` | start `08:30Z`, end `09:45Z`, positive 75-minute occurrence; choices survive DTO/storage round trip |
| generated wall end in gap | weekly valid start whose derived `WallClock` end lands at `2026-03-08 02:30` | entire candidate is omitted, does not consume `COUNT`, and no truncated/negative occurrence is emitted |
| six-occurrence split | daily local IDs March 1–6, `COUNT=6`; March 2 moved; March 5 cancelled; cut March 4; one relative reminder | old side IDs exactly March 1–3 with moved March 2; future side IDs exactly March 4–6 with cancelled March 5; no duplicate/missing ID; one reminder definition per side with distinct reminder IDs/equal schedule; union of effective IDs and values equals pre-split set |
| scoped trash/restore | trash March 2 occurrence over its moved exception, trash future at March 4, then restore each by trash-operation ID | occurrence trash hides March 2 without deleting its move; occurrence restore reveals the same moved value; future trash leaves March 1–3 live; future restore reactivates the exact March 4 child; IDs, UIDs, revisions, and lineage remain inspectable |
| opaque byte preservation | active raw bytes hex `582d4f44443b582d503d4d695865443a7261775c2c76616c75650d0a`, base64 `WC1PREQ7WC1QPU1pWGVEOnJhd1wsdmFsdWUNCg==` (28 bytes, SHA-256 `931ad82471abcbd2972912f6076e393c51eb78b0e893d7e428ea7e8aec103287`) | after metadata edit, move, occurrence override, split, trash, and restore every derived/retained envelope has the exact hex/hash/order/parameters; collision default writes nothing; quarantine changes only state and remains returned by `event extensions --include-quarantine` |
| stale overwrite | two requests read revision 7 and submit different patches with distinct operation IDs | exactly one reaches revision 8; the other is `stale_revision`; final payload is the winner only; one mutation receipt/audit transaction exists; no revision-9 last-writer overwrite |
| uncertain commit replay | kill client after database commit before response, then rerun identical operation ID/fingerprint | status is `committed`; retry returns `replayed:true`; target revision, audit, reminder, exception, split, and receipt counts do not increase |

The DST vectors above are also run against at least `Europe/Berlin`, `Australia/Lord_Howe` (30-minute transition), and a non-DST zone from the pinned TZDB using transition-derived expected instants. Property generators classify every local boundary as unique/fold/gap through an independent oracle and assert the same omit/fold/end-intent rules.

Reminder boundary acceptance is measurable without moving E into B: projecting the six-occurrence split twice yields one identical `ReminderInstanceKey` per effective, non-cancelled occurrence and no duplicate keys across the split. Repository capability and PostgreSQL grant tests prove B cannot insert/update the delivery ledger or invoke a presenter. The release-level E dependency must separately run its durable unique-claim matrix for duplicate scans, crash before/after claim and presentation, retry, suspend/resume catch-up, DND deferral, snooze, and dismiss; until E passes, B help and success output say “schedule stored,” never “reminder delivered” or “exactly once.”

### 5.1 Unit tests

- `timed_event_requires_zone_and_ordered_instants`: reject missing zone and non-positive ranges; accept canonical IANA zone.
- `all_day_is_end_exclusive_and_timezone_free`: one-day and multi-day boundaries survive serialization; reject mixed timed/all-day fields.
- `dst_gap_is_rejected`: synthetic `America/Los_Angeles` spring-forward local time returns both context and no persisted value.
- `dst_fold_requires_choice`: ambiguous fall-back time fails without fold, while earlier/later map to distinct expected instants and round-trip the chosen interpretation.
- `timezone_change_modes_are_distinct`: preserve-instant and preserve-local produce the documented results; gap/fold cannot be bypassed.
- `projection_intersection_is_half_open`: events ending at window start or starting at window end are excluded; overlapping timed/all-day events are included.
- `calendar_default_is_unique`: first calendar defaults; setting another atomically moves default; trashed defaults do not satisfy selection.
- `event_patch_preserves_omitted_and_opaque_fields`: omission changes nothing and unknown-property envelopes remain byte-for-byte identical.
- `event_revision_prevents_lost_update`: stale expected revision returns an error and leaves before-state intact.
- `trash_restore_preserve_identity`: EventId/RFC UID remain constant, local and remote deletion states do not alias, second trash is idempotent.
- `purge_reserves_uid`: purge requires trashed state/confirmation and UID cannot be reused.
- `rrule_requires_bounded_window`: no expansion without finite bounds; cap is enforced deterministically.
- `generated_gap_is_omitted_without_consuming_count`: RFC-invalid generated dates/gaps match the first acceptance vector exactly.
- `recurrence_end_intent_survives_dst`: elapsed and wall-clock end vectors produce distinct exact ends and round-trip their tagged intent.
- `recurrence_uses_wall_clock_across_dst`: weekly timed fixtures retain intended local time while UTC offset changes.
- `moved_exception_keeps_original_recurrence_id`: occurrence identity is stable after move.
- `series_split_is_semantically_partitioned`: every occurrence belongs to exactly one side, history is unchanged, future exceptions/reminders transfer per policy.
- `scoped_trash_restore_preserves_override_stack`: occurrence and future deletion are reversible without erasing prior exceptions or lineage.
- `opaque_envelope_is_byte_exact_and_inspectable`: the fixed hex/hash fixture survives every B transformation and both collision resolutions.
- `expected_revision_has_no_optional_path`: compile/API tests and repository spies prove no existing-object mutation can omit or bypass `ExpectedRevision`.
- `operation_replay_is_idempotent`: same ID/fingerprint returns the prior DTO; different fingerprint fails without write.
- `reminder_default_is_empty`: create with no reminder produces zero definitions; relative projections use effective occurrence start.
- Property tests generate valid date ranges, timezone transitions, recurrence windows, patches, trash/restore sequences, and assert invariants/no panic.

### 5.2 Integration tests

Using the foundation's explicitly opted-in disposable `mg_calr_test` database:

1. Apply migrations twice; assert schema/index/constraint presence and no duplicate effects.
2. Create two calendars concurrently as default; assert exactly one live default and one transaction receives a deterministic retry/conflict result.
3. Create timed and all-day events with metadata/reminders; read through repositories and application DTOs; assert audit rows share transaction identity.
4. Inject one fault after each of: aggregate row, metadata, reminder, exception partition, child master, extension copy/quarantine, audit, purge-ledger write, and receipt write, plus connection loss immediately before and after commit acknowledgement. Pre-commit faults leave the complete before-state and no receipt; post-commit loss leaves the complete after-state and one committed receipt discoverable by status. Repeat for create, edit, move, each trash/restore scope, split, and purge; mixed lineage/counts are forbidden.
5. Run two edits from the same revision; assert one commit and one `stale_revision`, never last-writer overwrite.
6. Trash/restore across calendar lifecycle and every recurrence scope; assert identity, override stack, extension/quarantine bytes, metadata, split lineage, and audit retention. Purge leaves only permitted identity/provenance, blocks UID reuse, and refuses retained restorable lineage.
7. Query DST/all-day boundary fixtures in multiple display zones and compare exact ordered DTOs.
8. Milestone 3: exercise every Section 5.0 recurrence vector, RFC recurrence/exclusion/exception fixtures, future split, moved DST exception, malformed RRULE rollback, cap enforcement, and no occurrence materialization. The split suite remains a release gate until its required UID/RANGE spike has selected a proven adapter; it is never weakened to fit an adapter.
9. Assert every B command uses a database connection only when invoked and performs no DNS/socket/network operation other than the explicitly configured PostgreSQL connection.

### 5.3 UI / E2E tests

Process tests cover:

- guided calendar/event creation with synthetic input, default-no-reminder prompt, cancellation, and exact persisted result;
- every guided field mapped to a non-interactive flag; `--no-input` closes stdin and cannot hang;
- `--json --no-input` emits exactly one envelope on stdout and stable typed errors on stderr;
- human and JSON day/week/month outputs deserialize from/compare to the same application projection fixture;
- 40-column and 200-column output retain IDs, timezone/fold, scope, and recovery text without ANSI under both `--no-color` and `NO_COLOR`;
- ambiguous selectors never mutate; until short IDs land, non-UUID selectors fail;
- purge fails without trashed state and `--yes`; Ctrl-C before commit leaves database unchanged;
- recurrence edits require explicit scope non-interactively and display scope in confirmation.
- every existing-object mutation without `--if-revision` fails before opening a transaction; a guided race between display and confirmation returns `stale_revision` rather than overwriting;
- `event extensions` returns the exact fixed base64/hex-correlated bytes and collision recovery text without logging them; `mutation status` proves both uncertain-commit outcomes and replay behavior.

### 5.4 Visual / manual verification

- Inspect `--help` and shell examples for all prompt/flag pairs and explicit end-exclusive/all-day language.
- Test empty, single-event, dense, trashed, all-day, fold/gap, long Unicode title, long unbroken URL, and recurrence exception displays at 40/80/200 columns.
- Use a screen reader with guided create/edit, errors, confirmations, and projection output; verify logical reading/focus order and no color-only state.
- Verify light/dark terminal themes with default color, `--no-color`, and `NO_COLOR`; no meaning may disappear.
- Run with system timezone unavailable and with explicit `--timezone`; verify actionable recovery and no UTC fallback.

### 5.5 Required quality gates

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Additionally run disposable PostgreSQL migration/integration tests, timezone and recurrence property tests, golden JSON/fingerprint/opaque-byte contracts, synthetic RFC fixtures after Milestone 3, role-grant assertions, secret scan, packaged clean-Arch recovery E2E from Section 4.5.1, and a test proving zero non-PostgreSQL network access. CI runs migrations both forward and from the oldest supported schema fixture and verifies rollback/backup-restore instructions on synthetic data. A passing happy-path suite does not override a failed warning, migration, temporal, package, privilege, recovery, or auto-fail gate.

---

## 6. Compliance & Safety Gate

### 6.1 Sensitive data classification

- [ ] No sensitive data involvement
- [x] Handles sensitive data — event titles, descriptions, locations, URLs, organizer/attendee identifiers, schedules, and categories may reveal personal information. Store them only in the user-controlled local PostgreSQL database and explicit audit/tombstone records; use least-privilege peer auth; redact database URLs; do not include event content in default logs/errors; do not access network; use synthetic fixtures.
- [x] Uses synthetic/test data only until compliance gate clears

Purge removes mutable event content when retention/sync obligations permit, but documents that PostgreSQL backups and immutable audit provenance may retain data according to later G3 policy. No claim of secure erasure is made.

### 6.2 Asset provenance

- [x] No third-party assets
- [ ] Uses third-party assets

Rust crate code/data, including TZDB data, is a dependency rather than a user-facing asset; its license, provenance, update method, and supply-chain state must pass dependency audit before release.

### 6.3 Language / claims audit

- [ ] Make claims not supported by evidence
- [ ] Promise capabilities not yet built
- [ ] Use language restricted by domain regulations

Current behavior is explicitly separated from target state in Section 7. CLI help shipped in Milestone 2 must not advertise recurrence execution, notifications, iCalendar round trip, sync, TUI, or Quickshell until those slices pass.

### 6.4 Criteria alignment

- **T1 Identity:** immutable typed UUIDs, independent stable RFC UID, occurrence/split identity, tombstone/purge reservation, and fixtures are binding.
- **T2 Temporal correctness:** exclusive all-day dates, IANA zones, source civil seeds, persisted generated-fold/gap policy, distinct elapsed/wall-clock ends, RFC-invalid omission without `COUNT` consumption, and exact multi-zone DST fixtures prevent drift.
- **T3 Transaction integrity:** mandatory expected revisions/manifests, operation receipts/replay, aggregate writes, reminders, splits, constraints, audit, fault rollback, uncertain-commit recovery, and concurrency tests prohibit partial or last-writer mutation.
- **T4 Deletion/audit:** local trash, remote tombstone, occurrence override layers, restorable future splits, series restore, purge eligibility, immutable operation provenance, and exact restore-conflict behavior are distinct.
- **T5 Reminder idempotency:** B8 emits stable unique reminder-instance keys but has neither repository capability nor database grant to deliver; E owns and must pass the separately enumerated durable claim/delivery crash/retry/sleep/DND matrix before delivery is advertised.
- **C1–C5:** guided defaults, fully enumerated clear/set flags, no-input, stable errors/JSON, no-color, narrow-width output, XDG precedence matrix, non-mutating component doctor, and exact recovery commands are specified while A retains implementation ownership of shared foundation commands.
- **I1 Lossless iCalendar:** standard metadata is mapped; opaque envelopes have exact bytes/order/hash, survive every B transformation, and have a current read/quarantine recovery path. F1 retains codec/round-trip ownership; B does not claim import/export.
- **I2 Sync authority:** PostgreSQL remains authority; schema-versioned semantic fingerprints and revision-checked proposal entry prevent a later durable vdir mirror from becoming a competing authority, while F retains mirror/orchestration ownership.
- **I3 Conflict/deletion:** B distinguishes local deletion, remote tombstone, and purge obligations and rejects stale/base-mismatched proposals without overwrite; F later owns preserved-proposal storage, three-way resolution, and remote delete/restore round trips.
- **I4 Scope/network:** all B paths prohibit network. Active CalDAV scheduling is deferred.
- **O1–O4:** no credential dependency, explicit runtime/migration role grants, non-mutating doctor recovery, typed/atomic failures, operation-status runbook, synthetic isolated tests, crate/package audit, secret scan, and clean-machine recovery E2E are required.

Any silent data loss, unconfirmed overwrite, UID instability, recurrence corruption, timezone/DST drift, duplicate reminder presentation, plaintext secret logging, non-sync network access, automatic conflict overwrite, or opaque-property loss is an automatic failure and blocks implementation acceptance.

### 6.5 Security controls

- Validate length/count before allocation and database writes; reject NUL/control characters where unsafe for terminal output and sanitize terminal control sequences during rendering without altering stored text.
- Parse URLs and organizer/attendee URIs as data; never fetch, open, execute, or shell-interpolate them.
- Parameterize all SQL. Dynamic ordering/filter identifiers come from enums, never user text.
- Use cryptographically non-predictive/UUIDv7 identity generation as provided by the audited UUID dependency; selectors convey no authorization but must never collide ambiguously.
- Confirmations display sanitized target identity and scope. Logs contain operation, typed IDs, revision, duration, and error code—not event payload or connection secrets.
- Resource caps and bounded recurrence prevent memory/CPU denial from malformed or adversarial local/imported records.

---

## 7. Gap Analysis vs. Current State

### 7.1 What exists today

**Implemented foundation only:** `src/domain.rs` defines typed calendar/event/reminder UUID identities; `src/lib.rs` defines version-1 success/error envelopes and foundation error exits; `migrations/0001_foundation.sql` scaffolds calendars, mutually exclusive timed/all-day events, RFC UID, metadata columns, extension JSON, reminders, delivery ledger, audit, and separate local/remote deletion timestamps. Foundation configuration, PostgreSQL migration, doctor/init, and isolated tests are described in `gauntlet-output/specs/a-foundation.md`.

**Absent:** calendar/event application use cases, prompts/flags, event renderers/JSON DTOs, system-timezone semantics, revisions/concurrency handling, calendar lifecycle commands, metadata child structures, recurrence parsing/expansion, exceptions/splits, event reminder CRUD, projections, functional audit mutation recording, UID reservation after purge, and all B tests. Foundation columns are scaffolding and are not evidence that B behavior exists.

### 7.2 Delta to spec

- Add domain/application/CLI/render/repository modules in Section 4.1 and wire them through the existing single binary/package.
- Add migration 2 for revisions, metadata structures, event indexes, lifecycle/purge identity, exceptions, split lineage, and stronger reminder constraints.
- Implement strict temporal parsing, canonical IANA timezone discovery/injection, DST resolution, timezone edit modes, and date/interval projection.
- Implement transactional calendar and event CRUD/scoped trash/restore/purge with mandatory expected revisions, operation receipts/replay, complete audit records, and an inspectable recovery path.
- Persist recurrence source wall time, generated fold/gap policy, elapsed-versus-wall-clock end intent, layered exceptions, split/trash lineage, and byte-exact opaque/quarantine envelopes.
- Define stable event/calendar/projection JSON DTOs and process error contracts.
- Run and decide the timezone, iCalendar/RRULE, and recurring mutation spikes before their dependent production slices; then implement bounded RRULE expansion, exceptions, scopes, and recurring reminder projection.
- Add all unit/property/integration/process/accessibility/performance/security gates in Section 5.

### 7.3 Estimated scope

**XL** for the complete B1–B8 target because it spans temporal modeling, transactional CRUD, metadata preservation, recurrence algebra, occurrence identity/splitting, reminder definitions, projection queries, and stable CLI/JSON contracts. Delivery must remain at least two slices: Milestone 2 non-recurring event core, then Milestone 3 recurrence. B8 delivery behavior stays in Milestone 5.

### 7.4 Blocking dependencies

- A1–A4 foundation must remain stable; A5 must provide transactional audit semantics before event mutation acceptance.
- Timezone crate/TZDB evidence and synthetic DST fixtures block B4 implementation.
- Required Spike 1 and Spike 2 decisions block B6/B7 and recurring B8 implementation.
- D9 owns suite-wide JSON compatibility policy, but Milestone 2 must establish additive version-1 event DTOs that D9 can adopt rather than replace.
- F1–F3 block claims of actual lossless iCalendar import/export; F4–F12 block remote discovery/sync/tombstone reconciliation.
- E1–E8 block notification delivery. G2 blocks generalized audit-based undo; B's explicit trash restore operations remain part of B and are not deferred to G2.

### 7.5 Explicit non-goals

- No notification scanning/presentation, snooze, dismiss, DND, action service, or systemd units.
- No iCalendar/JSON import/export interchange command, vdir mirror, vdirsyncer, iCloud/CalDAV request, conflict resolution, or remote calendar discovery. The local `event extensions` diagnostic reads stored opaque envelopes only and is not a codec or interchange path.
- No active invitation sending, RSVP, cancellation transport, scheduling inbox/outbox, or free/busy lookup.
- No todo behavior, combined agenda, full-text search, generalized filters, bulk mutation, or targeted undo command.
- No full-screen TUI, Quickshell pill/card implementation, web UI, file attachments, or direct database access by future clients.
- No unconstrained natural language, SQLite/embedded PostgreSQL, sudo/root provisioning, background sync, or background process.
- No locale-specific calendar arithmetic in v1; strict/bounded input and Sunday-first week/month semantics are authoritative.
- No unbounded recurrence materialization or promise of cross-platform behavior beyond a portable Linux-oriented core.

---

## 8. Open Questions

- **Q1:** Which maintained IANA timezone implementation and TZDB update strategy passes the B4 evidence fixture set? — blocks: B4 production dependency selection, not the semantics in this spec.
- **Q2:** Which iCalendar/RRULE adapter passes full RRULE, exclusions, recurrence IDs, malformed input, organizer/attendee, timezone, and unknown-property spike tests? — blocks: B6/B7 implementation and F1 codec selection.
- **Q3:** What exact RFC UID and `RECURRENCE-ID;RANGE=THISANDFUTURE` mapping interoperates safely for a local transactional future split while preserving historical occurrence identity? — blocks: B7 Milestone 3; escalate rather than guessing if the spike cannot prove it.
- **Q4:** What default finite recurrence query cap and user-overridable upper policy should production expose after benchmarks? — blocks: final B6 CLI tuning; the no-unbounded-expansion invariant is already binding.
- **Q5:** Short-ID encoding remains deferred to A3/D10. Until selected, exact UUID is the only mutation selector. — blocks: chooser/short-selector convenience, not Milestone 2 correctness.
