# Scorecard: Event and Calendar Core

**Feature ID:** b-event-calendar-core
**Spec file:** docs/specs/b-event-calendar-core.md
**Reviewer agent:** blind verification agent
**Date:** 2026-08-30
**Spec iteration reviewed:** 2 — blind-review remediation

---

## Verdict: PASS

**Summary:** The temporal and failure-contract work is the strongest part of this spec: §5.0's acceptance vectors are real, independently checkable numbers (I decoded the opaque-property fixture — 28 bytes, base64 and SHA-256 both match exactly; the four DST vectors resolve to the correct UTC instants against the 2026 US transition dates), and §4.4.1 supplies a genuine uncertain-commit runbook rather than operator guesswork. The most critical gap is factual, not architectural: §7.1's "Absent" list is materially wrong against HEAD — event application use cases, JSON DTO projections, day-agenda projections, optimistic version handling, and B tests all ship today — and §4.2's `revision bigint` column would collide with the already-shipped `events.version` optimistic lock without any reconciliation. Passing is conditional on the §7.1/§4.2/§4.1 corrections listed below being applied before implementation begins.

---

## Lens 1 — Temporal and Data Integrity (weight: 35%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| T1 Identity | 3 | §4.2 inv 1 makes `CalendarId`/`EventId`/`ReminderId` typed immutable UUIDv7 and the RFC UID independent, immutable, locally generated, and never recycled after purge; §3.1 restricts `SELECTOR` to an immutable UUID and states a title "is never accepted as mutation authority"; §4.2 inv 10 keeps `OccurrenceKey` stable after a move; inv 13 separates `deleted_at` from remote tombstone and retains a purge identity record. Fixtures are binding, not aspirational: §5.1 `trash_restore_preserve_identity`, `purge_reserves_uid`, `moved_exception_keeps_original_recurrence_id`, plus the §5.0 scoped trash/restore vector asserting IDs/UIDs/lineage. Meets the Excellent column literally (UID + tombstone + conflict identity invariants *with* fixtures). | — |
| T2 Temporal correctness | 3 | §4.2 inv 2–6 fix end-exclusive all-day storage, canonical IANA zones with `EST`-style abbreviations rejected, gap rejection, per-boundary fold resolution, and the `preserve-instant`/`preserve-local` split. §4.2 inv 5 goes past the anchor with `GeneratedLocalTimePolicy::Rfc5545OmitInvalidUseRecordedFold` and tagged `Elapsed` vs `WallClock` end intent that "may not collapse those forms into two instants". I checked §5.0's vectors arithmetically: `2026-03-01 02:30` LA → `10:30Z` (PST) and `03-15`/`03-22` → `09:30Z` (PDT) with `03-08 02:30` correctly in the gap; `01:30 +2h` → `09:30Z`/`11:30Z` displayed `04:30 -07:00`; wall-clock end `03:30` → `10:30Z` (1h elapsed); fall folds `01:30 earlier`/`01:45 later` → `08:30Z`/`09:45Z` = 75 min. `Australia/Lord Howe` is correctly cited as a 30-minute transition. Recurring-exception vectors and transactional semantics are present (§5.0 six-occurrence split). | — |
| T3 Transaction integrity | 2 | Design content is at the Excellent level: §4.2 inv 14 (revision increment + audit + receipt under one transaction ID, exact row-count assertions, serializable retry), §3.2 mandatory `--if-revision` with "no unconditional mutation API or last-writer-wins fallback", §4.4.1 recovery runbook, §5.2 item 4 fault injection after every write point plus connection loss either side of commit, §5.0 stale-overwrite and uncertain-commit-replay vectors, §5.1 `expected_revision_has_no_optional_path`. **Scored down for a factual collision with shipped code:** §4.2's migration implications say "Add `revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0)` to calendars/events", but `migrations/0005_event_lifecycle.sql` already added `events.version bigint NOT NULL DEFAULT 1 CHECK (version >= 1)`, and `src/application.rs` (`cancel_event_async`/`restore_event_async`/`edit_event_async`) plus `AppError::EventVersionConflict` in `src/lib.rs` already drive optimistic locking off it. The spec nowhere reconciles the two, so as written it creates a second competing counter on the same row while `event cancel --version N` keeps advancing the first — the exact unconfirmed-overwrite path the rest of §3.2 forbids. §7.1 compounds this by listing "revisions/concurrency handling" as absent. | In §4.2 migration implications and §7.2, state explicitly whether `events.version` is renamed to `revision` (with the migration and the `EventVersionConflict` mapping) or adopted as-is, and forbid retaining both counters. Correct §7.1 to record optimistic versioning as implemented. |
| T4 Deletion/audit | 3 | §3.3 "Trash, restore, and purge" gives scope-complete restore (occurrence/future/series), a durable `trash_operation` ID, override-stack preservation, and `restore_conflict` with no write on ambiguity; §4.2 inv 13 separates local soft deletion from remote tombstone and retains post-purge identity to prohibit UID reuse; §3.3 step 4 requires trashed state + exact ID + `--yes` + matching revision and writes immutable purge/audit identity *before* deleting payload; step 5 blocks purge under future retention obligations. Immutable provenance and restore/purge eligibility are both explicit. | — |
| T5 Reminder idempotency | 2 | §4.2 inv 12 defines a stable `ReminderInstanceKey(reminder_id, series_id_or_event_id, original_recurrence_id_or_singleton, scheduled_for)` and requires repeated bounded projection to return the same ordered unique keys; §5.0 makes that measurable (projecting the six-occurrence split twice yields one key per effective non-cancelled occurrence, no duplicates across the split) and adds repository-capability and PostgreSQL grant tests proving B cannot write the delivery ledger. §6.3/§5.0 forbid "reminder delivered"/"exactly once" language until E passes. This clears the Acceptable bar for the scope B owns, but B does not itself specify the durable unique claim/delivery state, and the retry/crash/sleep/DND matrix in §5.0 is enumerated as E's release obligation, not a B test — so the Excellent anchor ("proving exactly-once presentation intent") is not met here. Criteria.md permits deferral N/A only for I1–I3, so this is scored on merit rather than waived. | To reach 3, B would need the durable claim state itself, which §4.2 inv 12 correctly refuses. No change required for pass. |

**Lens average:** 2.6
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Lens 2 — CLI Usability and Automation (weight: 25%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| C1 Human workflow | 2 | §3.3 "Create an event" and §3.2 give guided defaults done well: prompts only for missing values, an explicit "Add a reminder? [y/N]" defaulting to none, displayed revision and generated operation ID before commit, and Ctrl-C cancellation with no mutation. §3.7 is a complete trigger/presentation/recovery/data-loss table with actionable recovery for all 17 error classes — the Acceptable anchor is fully met. It does not reach Excellent ("matches benchmark speed") because §3.1 makes a full UUID the only mutation selector in Milestone 2 with short IDs deferred to A3/D10 (Q5), so every `show`/`edit`/`move`/`trash` requires pasting a UUID *plus* `--if-revision N` — measurably slower than the khal/Taskwarrior benchmark named in criteria.md. The tradeoff is deliberate and justified, but it is still a workflow cost, not a stylistic preference. | Optional: name the interim ergonomic mitigation (e.g. echoing a copyable ID+revision pair in every create/list output) so Milestone 2 is not UUID-typing-bound. |
| C2 Automation | 3 | §3.2 guarantees a flag for every prompt, `--clear-*` distinguished from omission, `--no-input` that "never reads stdin or `/dev/tty`" and returns `input_required` naming the exact resolving flags, and one JSON object on stdout with prompts on stderr. §4.3 pins a concrete versioned envelope with a full worked `event.show` example, tagged temporal objects "rather than nullable-field inference", deterministic array ordering, and an additive-only compatibility policy escalating to D9 for removals/narrowing. §5.0/§5.5 require golden JSON, fingerprint, and opaque-byte contracts; §5.3 requires that ambiguous selectors never mutate. All three Excellent clauses met. | — |
| C3 Configuration | 2 | §4.5.1 specifies distinct `XDG_CONFIG_HOME`/`XDG_DATA_HOME`/`XDG_STATE_HOME`/`XDG_CACHE_HOME` roots, an immutable resolved configuration object with no ad hoc path reads, and pure matrix tests over every precedence pair including unset/empty/malformed values, legacy-key rejection with migration warning, and URL redaction — Excellent-level content. **Scored down for misreporting the foundation contract it says B "must consume and contract-test":** it states resolution "is exactly" CLI `--database-url`/`--timezone` > `MG_CALR_DATABASE_URL`/`MG_CALR_TIMEZONE` > TOML, but the implemented foundation reads `DATABASE_URL` (`src/config.rs:169`, `docs/ARCHITECTURE.md:13`, `tests/config_contract.rs:63`), has no `MG_CALR_DATABASE_URL`, no `MG_CALR_TIMEZONE`, and no global `--timezone` flag (`src/main.rs` globals are `--json`, `--no-input`, `--no-color`, `--database-url`). The sibling `specs/a-foundation.md:212-213` correctly labels `MG_CALR_DATABASE_URL` "(target)" against `DATABASE_URL` "(implemented)"; this spec presents the target as current. | In §4.5.1, mark the namespaced variables and the global `--timezone` flag as A-target state not yet implemented, or restate the precedence using the shipped `DATABASE_URL`. Add the `--timezone` global to §7.4 as a blocking A dependency. |
| C4 Output/accessibility | 3 | §3.5 requires word-boundary wrapping with continuation indentation and tables degrading to labeled records "rather than truncate identity, timezone, scope, or errors", with identity/date/timezone/fold/scope legible at 40 columns (§3.8). §3.4 forbids color-only state ("literal text/symbol plus an accessible word"). §3.8 makes `--no-color`, `NO_COLOR`, JSON, Ctrl-C, and help examples contract-tested, and binds future Quickshell/TUI clients to public application/JSON commands with no direct PostgreSQL reads. §5.3 tests 40- and 200-column output under both no-color paths; §5.4 adds screen-reader verification of reading and focus order. Width, focus, Quickshell-public-interface, and accessibility fixtures are all present. | — |
| C5 Diagnostics | 3 | §4.5.1 specifies `mg-calr doctor --component event-core --json` as non-mutating with stable checks (TZDB availability/version, PostgreSQL reachability/version, migration presence, required tables/indexes/constraints, role grants, clock skew, recurrence readiness reported `not_applicable` before Milestone 3), printing exact administrator commands but "never runs them, prompts for a password, invokes sudo, or exposes a database URL". The clean-machine gate supplies the prerequisite matrix and verifies exact recovery text after revoking a grant and removing TZDB. Meets stable machine output + prerequisite matrix + no sudo/secret leakage. | Note only: `doctor` currently accepts no arguments (`src/main.rs` `Command::Doctor`); the `--component` extension of an A-owned command should appear in §7.4. |

**Lens average:** 2.6
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Lens 3 — Standards Interoperability and Sync (weight: 25%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| I1 Lossless iCalendar | 3 | §4.2 inv 8 defines `OpaqueProperties` as a versioned envelope carrying stable property ID, namespace/name, original order, parameters, encoding marker, exact raw bytes, SHA-256, and active/quarantined state, preserved byte-for-byte across show/edit/move/trash/restore, exception creation, and split/merge — with "'Value equivalent' is not a permitted substitute for byte equality". §3.3 step 8 makes a standard-field collision fail as `extension_collision` by default, with `keep-opaque` cancelling the standard change and `quarantine-opaque` moving the envelope without byte change; "Neither choice silently deletes or rewrites unsupported data." §4.2 inv 7 maps the standard metadata set. The §5.0 golden fixture is verifiable and correct — I decoded hex `582d…0d0a` to the 28-byte `X-ODD;X-P=MiXeD:raw\,value\r\n`, and both the stated base64 (`WC1PREQ7…`) and SHA-256 (`931ad824…3287`) match exactly — and it deliberately embeds mixed case, a parameter, an escaped comma, and CRLF. §5.1 `opaque_envelope_is_byte_exact_and_inspectable` binds it across every B transformation and both collision resolutions; §5.2 item 8 adds malformed-RRULE rollback. | — |
| I2 Sync authority | 2 | §4.4 makes local PostgreSQL "the sole application authority" with the CLI reduced to intent-gathering and renderers forbidden from independent queries; §4.5.2 adds a deterministic schema-versioned SHA-256 semantic fingerprint over identity, revision-relevant fields, recurrence intent/exceptions, lifecycle markers, and opaque envelopes, requires any later F proposal to arrive with base fingerprint + expected revision + operation ID, and states "B has no API that accepts 'remote wins'". Above "implicit precedence". It stops short of Excellent because the durable vdir mirror, three-way base storage, and interruption orchestration are explicitly assigned to F4–F12 and "must not be advertised by B" — a defensible deferral with an architecture that does not preclude them, but not delivered here. | — |
| I3 Conflict/deletion | 2 | §4.5.2 makes disagreement stop as `stale_revision`/`base_fingerprint_mismatch` while preserving "both proposal payload and current row", and requires a cross-feature fixture proving a stale proposal "cannot resurrect, delete, or overwrite an event". §4.2 inv 13 and §3.3 step 1/3 keep `deleted_at` and `remote_tombstoned_at` non-aliased, with restore that "never clears a remote tombstone" (§5.1 `trash_restore_preserve_identity` asserts non-aliasing). That is the Acceptable anchor met in full — stops the item, preserves both sides, separates tombstones. Deterministic *resolution* and remote delete/restore round trips are deferred to F4–F12, so the Excellent anchor is not reached; local delete/restore round-trip fixtures (§5.0 scoped trash/restore, §5.2 item 6) exist but only cover the local half. | — |
| I4 Scope/network | 3 | §5.2 item 9 requires asserting that every B command opens a database connection only when invoked and "performs no DNS/socket/network operation other than the explicitly configured PostgreSQL connection"; §5.5 adds a standalone zero-non-PostgreSQL-network-access gate. §3.3 create step 2 parses metadata "without connecting to any network service"; §4.2 inv 1 generates the RFC UID locally with no network lookup; §6.5 parses URLs and attendee URIs as data that is never fetched, opened, executed, or shell-interpolated; §4.7 sets network payload to exactly zero; §7.5 excludes CalDAV/vdir/RSVP/free-busy. Command-level tests proving no network in all non-sync paths are present, which is the Excellent clause. | — |

**Lens average:** 2.5
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s
**Auto-fail triggered:** No — all eleven rules walked individually; see the audit below.

### Auto-fail audit (criteria.md §Auto-fail rules)

| Rule | Result | Controlling spec text |
|---|---|---|
| Silent event/todo loss | Pass | §4.2 inv 8 byte-exact envelopes with collision failing closed; §4.2 inv 11 aborts the whole split on any unrepresentable rule/orphan/duplicate ID; §3.3 step 6 requires `--move-events-to` or `--trash-events` before a calendar with live events can be trashed |
| Unconfirmed overwrite | Pass | §3.2 "`--if-revision` is mandatory … There is no unconditional mutation API or last-writer-wins fallback. `--yes` cannot waive this check"; §5.0 stale-overwrite vector requires exactly one winner and forbids a revision-9 overwrite |
| UID instability | Pass | §4.2 inv 1 (immutable, never recycled); §4.2 inv 11 gates the production split path until the RANGE=THISANDFUTURE spike proves UID mapping rather than inventing it |
| Recurrence/exception corruption | Pass | §4.2 inv 10–11 (override stack, lineage resolution, abort-on-uncertainty); §5.0 six-occurrence split requires the union of both sides to equal the pre-split set |
| Timezone/DST drift | Pass | §4.2 inv 3–6; §5.0 multi-zone vectors; §4.6 forbids silent UTC fallback |
| Duplicate reminder delivery | Pass | §4.2 inv 12 — B "never claims, presents, retries, snoozes, dismisses, or writes delivery state"; §5.0 grant tests prove B cannot insert into the delivery ledger |
| Non-idempotent scans | Pass | No scan exists in B; §4.2 inv 12 requires repeated bounded projection to return the same ordered unique keys |
| Plaintext credentials / secret logging | Pass | §6.5 logs carry "operation, typed IDs, revision, duration, and error code—not event payload or connection secrets"; §4.5.1 doctor never exposes a database URL |
| Network outside explicit sync | Pass | §5.2 item 9 and §5.5; every B command that connects is an explicit database command |
| Automatic conflict overwrite | Pass | §4.5.2 "B has no API that accepts 'remote wins' or suppresses these checks" |
| Loss of unsupported iCalendar properties on round trip | Pass | §4.2 inv 8 and the verified §5.0 opaque-byte fixture |

---

## Lens 4 — Operational Security and Reliability (weight: 15%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| O1 Credentials | 3 | §4.5.1 requires URL redaction in the pure resolution matrix and forbids doctor from exposing a database URL or prompting for a password; §3.7 closes with user content, attendee URIs, descriptions, and URLs never echoed in logs or generic diagnostics; §6.5 restricts log contents to operation/typed IDs/revision/duration/error code; §3.3 step 2 and §3.7 require the `extension_collision` error to carry property ID and hash but "no value"; §5.3 requires `event extensions` to return exact bytes "without logging them"; §5.5 mandates a secret scan. Secret-boundary tests plus sanitized diagnostics — Excellent met. | — |
| O2 Least privilege | 3 | §4.5.1 pins the runtime role to `NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT` over peer auth with an enumerated grant set and explicit denial of DDL, role management, server file, program execution, extension install, network, and delivery-ledger mutation; the migration owner is separately non-superuser and owns only the application schema; initial database/role creation is an administrator step printed by `init`. The clean Arch container gate is executable and documented — revoke one grant, remove TZDB, verify exact recovery text, restore, rerun — and "No test uses root after the explicit provisioning boundary." | — |
| O3 Failure contracts | 3 | §3.7 gives typed presentation and recovery per trigger; §4.3 enumerates stable codes (`stale_revision_manifest`, `base_fingerprint_mismatch`, `restore_conflict`, `split_unrepresentable`, `transaction_retry_exhausted`, `purge_blocked`, …) on the foundation envelope with stable nonzero exit classes; §4.2 inv 14 makes rollback cover every table including receipts; §4.4.1 is a real runbook table that refuses to guess `rolled_back` and forbids telling a user to run SQL, sudo, delete rows, or bypass a revision; §5.2 items 4–5 supply the fault and concurrency matrix. | — |
| O4 Verification | 3 | §5.5 requires `cargo fmt --check`, `clippy … -D warnings`, and full `cargo test`, plus disposable PostgreSQL migration/integration runs, timezone and recurrence property tests, golden JSON/fingerprint/opaque-byte contracts, role-grant assertions, secret scan, the packaged clean-Arch recovery E2E from §4.5.1, and the zero-non-PostgreSQL-network test; CI runs migrations forward and from the oldest supported schema fixture and verifies rollback/backup-restore on synthetic data. "A passing happy-path suite does not override a failed warning, migration, temporal, package, privilege, recovery, or auto-fail gate" closes the loophole the anchor targets. | — |

**Lens average:** 3.0
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Feasibility Check

Source read: `Cargo.toml`, `src/lib.rs`, `src/domain.rs`, `src/application.rs`, `src/config.rs`, `src/main.rs`, `src/storage.rs` (migration registry), `migrations/0001_foundation.sql`, `migrations/0005_event_lifecycle.sql`, `config/example.toml`, `docs/ARCHITECTURE.md`, `README.md`, and the `tests/` suite.

| Check | Status | Notes |
|---|---|---|
| Types/models exist or are clearly specified | ✓ | §4.2's target types are coherent and clearly specified. They are, however, `time`-crate types (`OffsetDateTime`, `PrimitiveDateTime`, `Date`, `Time`) while the shipped `Event`/`EventTime` in `src/domain.rs` are chrono types; the spec hedges this as spike-selected. |
| API/interface changes are feasible with current architecture | ✓ | §4.3's application-boundary signatures fit the existing `EventUseCases` + repository-trait shape in `src/application.rs`. `ExpectedRevision` as a non-`Option` required type is a strict tightening of the current `expected_version: i64` parameters and is implementable. |
| Views/screens fit current navigation pattern | ✓ | §3.2's command tree extends the existing clap `Command::Calendar`/`Command::Event` subcommands (`src/main.rs`), which already carry `create/edit/show/list/day-agenda/cancel/restore` and global `--json`/`--no-input`/`--no-color`/`--database-url`. |
| Dependencies are available and version-compatible | ✗ | `Cargo.toml` ships `chrono 0.4` + `chrono-tz 0.10`; `time` is **not** a dependency. §4.5 says "prefer `time` plus an IANA timezone implementation and do not default to `chrono`", which contradicts ~5,700 lines of shipped chrono-based domain/storage/interop code, and §7.2 lists no migration item for that swap. All other named crates are present and version-plausible (`uuid` v7 feature, `sha2 0.10`, `serde 1`, `serde_json 1`, `thiserror 2`, `clap 4.5`, `tokio-postgres 0.7`). |
| Platform/renderer requirements are realistic | ✓ | Terminal-only, Arch/Hyprland first, PostgreSQL 18 target matches `specs/a-foundation.md:366` and the peer-socket defaults in `config/example.toml`. §4.6's `/etc/localtime` → canonical IANA resolution is not implemented today (timezone is a required explicit argument everywhere) but is straightforward. |
| Test strategy is executable with current infrastructure | ✓ | `tests/postgres_integration.rs` already provides the opt-in disposable `mg_calr_test` harness the spec assumes, and `tests/cli_contract.rs` establishes the `assert_cmd` process-test pattern §5.3 needs. The clean-Arch container gate (§4.5.1) is new CI infrastructure but is described concretely enough to build. |
| Performance budget is realistic for target hardware | ✓ | §4.7's 150 ms p95 for single-record commands and 250 ms p95 for a day/week/month query over 10,000 events on a warm local Unix-socket PostgreSQL are achievable for a Rust CLI with the §4.2 indexes; the 64 MiB RSS target and finite 10,000-occurrence expansion cap are conservative. The spec correctly requires benchmarks to record hardware and database state rather than claim universal latency. |
| No undeclared dependency on unbuilt features | ✗ | §4.5.1 requires `mg-calr doctor --component event-core --json`, a global `--timezone` flag, and `MG_CALR_DATABASE_URL`/`MG_CALR_TIMEZONE`, none of which exist (`Command::Doctor` takes no arguments; `src/config.rs:169` reads `DATABASE_URL`). These A-owned additions appear in §4.5.1 as existing contracts and are absent from §7.4's blocking-dependency list. |

**Feasibility verdict:** Feasible with caveats

**Caveats:**

1. **§7.1 misreports current state.** The "Absent" list names *calendar/event application use cases*, *event renderers/JSON DTOs*, *revisions/concurrency handling*, *projections*, and *"all B tests"* as absent. All five ship at HEAD: `EventUseCases` in `src/application.rs` implements `create_calendar`, `create_event`, `list_calendars_async`, `show_event_async`, `list_events_async`, `cancel_event_async`, `restore_event_async`, `edit_event_async`, and `day_agenda_async`; `EventProjection`/`CalendarProjection` are serializable DTOs; `migrations/0005_event_lifecycle.sql` plus `AppError::EventVersionConflict` implement optimistic concurrency; and `tests/event_domain.rs`, `tests/query_contract.rs`, `tests/projection_agenda_contract.rs`, and the event cases in `tests/cli_contract.rs` are existing B tests. The claims that remain correct are: calendar trash/restore/purge commands, metadata child tables, event recurrence parsing/expansion, exceptions/splits, event reminder CRUD, functional audit writing (`audit_log` appears only in `tests/migration_contract.rs:18` — nothing writes it), and UID reservation after purge.
2. **Migration slot collision.** §4.1 and §7.2 call for `migrations/0002_event_core.sql` / "migration 2", but slot 2 is `0002_todo_core.sql` in the `MIGRATIONS` registry (`src/storage.rs:44-75`, versions 1–6 recorded by version *and name*). A second version-2 name triggers `StorageError::MigrationDrift`. The next free slot is 7.
3. **Duplicate optimistic-lock column** — see T3. `events.version` already exists; §4.2 adds `revision` without reconciling them.
4. **Module layout does not exist.** §4.1's `src/domain/{calendar,event,recurrence,reminder}.rs`, `src/application/{calendar,event}.rs`, `src/storage/{calendar,event}_repository.rs`, `src/cli/*`, and `src/render/*` are absent; the crate is flat (`src/application.rs`, `src/storage.rs`, all CLI in `src/main.rs`, no render module, only `src/domain/todo.rs` as a submodule). The refactor is feasible but §7.2 describes it only as "add … modules … and wire them", understating the relocation of existing event code.
5. **Foundation interface names** — see C3 and the last feasibility row.

---

## Composite Score

| Lens | Average | Weight | Weighted |
|---|---|---|---|
| Temporal and Data Integrity | 2.6 | 35% | 0.910 |
| CLI Usability and Automation | 2.6 | 25% | 0.650 |
| Standards Interoperability and Sync | 2.5 | 25% | 0.625 |
| Operational Security and Reliability | 3.0 | 15% | 0.450 |
| **Composite** | | | **2.635** |

**Pass conditions (from criteria.md):**
- [x] Composite ≥ 2.0 — 2.635
- [x] All lens averages ≥ 2.0 — 2.6 / 2.6 / 2.5 / 3.0
- [x] No criterion scores 0 — lowest is 2
- [x] No more than two criteria at 1 per lens — zero 1s in any lens
- [x] All auto-fail rules pass — all eleven walked individually above
- [x] Feasibility ≠ Infeasible — Feasible with caveats

**All conditions met:** Yes → PASS

---

## Remediation Brief

Not required for the verdict. The items below are corrections a blind reviewer found by reading the source, and they should be applied before implementation starts — items 1–3 will otherwise mislead the implementing agent.

### Priority 1 — Must fix to pass
None. The spec meets every pass condition in criteria.md.

### Priority 2 — Should fix for quality (factual corrections; apply before implementation)

1. **§7.1 "Absent" paragraph — remove the five false claims.** Delete *calendar/event application use cases*, *event renderers/JSON DTOs*, *revisions/concurrency handling*, *projections*, and *all B tests* from the absent list and move them to an "implemented, to be extended" statement citing `src/application.rs` (`EventUseCases`: create/show/list/edit/cancel/restore/day-agenda), `EventProjection`/`CalendarProjection`, `migrations/0005_event_lifecycle.sql` + `AppError::EventVersionConflict`, and `tests/{event_domain,query_contract,projection_agenda_contract,cli_contract}.rs`. Keep the genuinely-absent items (calendar trash/restore/purge, metadata child tables, event recurrence/exceptions/splits, event reminder CRUD, audit writing, post-purge UID reservation).
2. **§4.2 migration implications + §7.2 — reconcile `revision` with the shipped `events.version`.** State whether migration N renames `version` to `revision` (and updates `EventVersionConflict`, `cancel_event`, `restore_event`, `edit_event`, and `--version` CLI arguments accordingly) or adopts `version` under the new semantics. Add an explicit invariant that exactly one optimistic counter exists per row, so the existing `event cancel --version N` path cannot advance a counter the new `--if-revision` path does not check.
3. **§4.1 and §7.2 — renumber the migration.** Replace `migrations/0002_event_core.sql` / "migration 2" with the next free slot (7) and note that `MIGRATIONS` in `src/storage.rs` records version *and* name, so reusing slot 2 fails as `MigrationDrift`.
4. **§4.5.1 — label unimplemented foundation interfaces as target state.** `MG_CALR_DATABASE_URL`, `MG_CALR_TIMEZONE`, the global `--timezone` flag, and `doctor --component` do not exist (`src/config.rs:169` reads `DATABASE_URL`; `Command::Doctor` takes no arguments). Either mark them "(A target)" the way `specs/a-foundation.md:212-213` does, or restate the precedence over the shipped names — and add them to §7.4 as blocking A dependencies.
5. **§4.5 / §4.2 — record the cost of moving off chrono.** If `time` remains the preferred crate, add a §7.2 item covering the rewrite of `EventTime`, `Event`, the repository row mapping in `src/storage.rs`, and `src/interop.rs`, all of which are chrono-based today; otherwise state that `chrono` + `chrono-tz` may satisfy the spike and drop "do not default to `chrono`".

### Priority 3 — Consider for excellence

6. **§4.1 — acknowledge the actual starting layout.** Note that CLI code currently lives in `src/main.rs` with no `src/cli/` or `src/render/` module and that `src/application.rs` / `src/storage.rs` are single files, so the target placement is a split of existing code rather than greenfield addition.
7. **C1 (§3.1/§3.2) — reduce the Milestone 2 UUID burden.** Since short IDs are deferred (Q5), specify that create/list/show output emits a copyable `ID revision` pair in both human and JSON form so the mandatory `SELECTOR --if-revision N` pair can be pasted rather than transcribed.
8. **I2/I3 (§4.5.2) — state the minimum fingerprint acceptance now.** The semantic fingerprint is the seam F depends on; adding one golden fingerprint vector to §5.0 (like the verified opaque-byte vector) would let B prove the canonical encoding is stable before F exists.
