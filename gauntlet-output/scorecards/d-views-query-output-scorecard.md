# Scorecard: Views, Querying, and Output

**Feature ID:** d-views-query-output
**Spec file:** gauntlet-output/specs/d-views-query-output.md
**Reviewer agent:** Verification Agent (blind review)
**Date:** 2026-08-30
**Spec iteration reviewed:** 2

---

## Verdict: PASS

**Summary:** The spec's core — C2 automation and C4 output/accessibility — is genuinely
excellent: a versioned JSON envelope with a checked-in JSON Schema, an ABNF for the human
row, a machine-readable error/exit table, manifest-driven byte goldens, an RFC 8785
fingerprint that excludes render mode, and an ambiguity rule that makes a multi-candidate
selector a hard error in every non-interactive path (§4.3, §3.2, §3.3, §3.7). The most
critical gap is §7.1: its "current state" is factually wrong — it declares D1–D10 commands,
application projection types, and a query repository "absent" when `event day-agenda`,
`EventProjection`, `list_events_async`, and `tests/query_contract.rs` were already committed
29 minutes before the spec was recorded, and today `mg-calr agenda`, `AgendaUseCases`,
`AgendaItem`/`AgendaOutput`, and two agenda contract test files exist. Second, §4.4's
"PostgreSQL is authoritative … no local index outside PostgreSQL" is contradicted by the
shipped `ProjectionAgendaRepository`, which sources agenda todos from a local
`mg.interop/1` JSON file, not the database.

---

## Lens 1 — Temporal and Data Integrity (weight: 35%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| T1 Identity | 3 | §4.2 `ProjectionIdentity` gives four distinct, non-interchangeable variants — `Event{event_id, rfc_uid, recurrence_id}`, `Todo{todo_id, instance_id}`, `Tombstone{tombstone_id, deleted_entity_id}`, `ConflictBranch{conflict_id, branch_id, entity_id}` — and separates `ProjectionVersion` (revision) from identity. §3.2(D10.1) makes short IDs "selectors only … never stored as identity", 8–32 hex chars, lowercase output. §4.5 adds the tombstone tuple `(tombstone_id, deleted_entity_id, source_revision, deletion_provenance)` and the branch tuple with `content_fingerprint`. Fixtures exist: §4.3 `manifest-v1.json` carries "expected ordered complete identities"; §5.1 `identity_variants_never_collapse` and `short_prefix_resolution`; §5.2(1) seeds colliding UUID prefixes and two same-UID branches. Meets the "plus explicit RFC UID/tombstone/conflict identity invariants and fixtures" column literally. | — |
| T2 Temporal correctness | 3 | §3.2 converts civil ranges to half-open instants and states a local day may be 23/24/25 hours; all-day bounds stay civil and are never routed through midnight instants. §3.3 mandates a per-row numeric UTC offset + IANA zone plus `fold=0/1`, retained at every width, with the worked example `01:30 -07:00 … fold=0` / `01:30 -08:00 … fold=1` ordered by instant not wall clock. §3.6 gives typed `local_time_nonexistent` and `local_time_ambiguous` rather than a silent offset choice. §4.3 makes `fold` mandatory on every local datetime and forbids fabricating all-day as midnight UTC. §5.1 supplies the gap (23h), fold (25h), exclusive-all-day, and `recurrence_exception_identity_is_preserved` (moved/cancelled/deleted/fold/orphan) vectors; §4.3 binds them to one `REPEATABLE READ` snapshot. Meets the excellent column. | — |
| T3 Transaction integrity | 3 | D writes nothing, but read-transaction integrity is specified concretely: §4.3 opens one read-only `REPEATABLE READ` transaction, captures database `now` and `snapshot_token` once, and commits without writes; over-limit fails atomically with `query_limit_exceeded` rather than returning a truncated page. §4.3 "Same-query projection invariant" adds a contract-test repository that *panics on a second query*. Concurrency/fault coverage: §5.2(4) holds a `SnapshotPageSession` open across concurrent insert/update/delete and asserts no duplicate, omission, or replacement, then asserts resume fails after close/abort/timeout; §5.2(5) asserts `selection_stale` with no mutation or audit row; §5.2(9) corrupts fixtures inside isolated transactions and asserts whole-projection failure; §5.2(6) hashes state across "every success and injected failure path". | — |
| T4 Deletion/audit | 2 | §4.5 "Tombstones and conflicts" specifies the tri-state eligibility, distinct tombstone/branch identities, and immutable provenance (golden fixtures assert "unchanged provenance/fingerprints/audit rows before and after the query"). §3.6 records "Data-loss risk: none" for all 14 error rows and §3.6 closes "No command in this feature writes events, todos, audit records, sync state, or reminder delivery state." The excellent column additionally requires "safe restore/purge/undo eligibility", which §4.5 explicitly declines: "Deterministic resolution, restore, purge, undo, and delete/export round trips remain explicit F/deletion commands and are not claimed here." Honest and appropriate for a read-only view, but the 3-anchor is not met. | To reach 3, §4.5 would need to state the eligibility predicate a later restore/purge command must evaluate against the tri-state (e.g. which `(tombstone_id, source_revision)` pairs are restorable and what makes one ineligible), even though the command itself stays in F. |
| T5 Reminder idempotency | 2 | §4.5 "Reminder identity and delivery idempotency" names the durable uniqueness constraint precisely — `(reminder_definition_id, occurrence_or_instance_identity, scheduled_instant, delivery_channel)` — and makes it a hard D-completion gate, while confining D to an observational boundary: repository returns only typed definition IDs plus an aggregate boolean; renderers "cannot inspect or emit payloads, delivery destinations, DND state, claim tokens, or secret material". §5.1 `reminder_query_is_observational` and §5.2(6) assert claim/outcome/audit rows are byte-equal across success and injected failure. The excellent column's "retry/crash/sleep/DND test matrix proving exactly-once presentation intent" is explicitly out of scope: "Reminder exactly-once presentation and sleep/DND retry behavior remain B8/C8/E responsibilities." Durable claim state is specified as a required contract, not demonstrated here. | To reach 3, §4.5 would need D's own sleep/suspend vector — e.g. a test asserting that a view snapshot taken across a host suspend/resume boundary still reports identical `has_reminder` membership and leaves claim rows untouched — rather than deferring the entire matrix. |

**Lens average:** 2.60
**Lens pass:** Yes — avg 2.60 ≥ 2.0, zero 1s, zero 0s

---

## Lens 2 — CLI Usability and Automation (weight: 25%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| C1 Human workflow | 3 | §3.1 gives defaults that remove ceremony: bare `mg-calr` is today, `week`/`month` default to today/current month. §3.2 "Structured filtering (D6)" deliberately rejects the Taskwarrior grammar the competitive baseline flags — typed long flags with a stated boolean law (AND across fields, OR within a repeated field) and "no eval syntax, SQL fragment, regex, shell expansion". §3.1 bounds date shortcuts to `today`/`tomorrow`/`next fri` with no open-ended NL parsing. §3.6 supplies a Recovery column for all 14 error rows (e.g. `terminal_too_narrow` → "use `--json`, pipe, or set valid width"). §3.4 makes the chooser fully keyboard-driven. Benchmark speed is encoded, not asserted: §4.8 p95 ≤100 ms day query, ≤200 ms first human byte. | — |
| C2 Automation | 3 | Primary criterion, and the strongest section of the spec. Versioned deterministic JSON: §4.3 "Stable JSON (D9)" pins `schema_version:1`, a full worked envelope, and binding rules on field names, enum strings, item ordering, null-vs-absent, RFC 3339 formats, and canonical lowercase UUIDs. Golden contracts: §4.3 "Machine-readable contract artifacts" requires six checked-in artifacts — `view-result-v1.schema.json` (2020-12, `unevaluatedProperties:false` inside identity/time unions so a missing fold or source revision cannot pass), `error-v1.schema.json`, `human-row-v1.abnf`, `manifest-v1.json`, byte-stable `*.json`/`*.txt` goldens, and concurrency schedules — with CI cross-validating ABNF-extracted human identities against the JSON identities and checking the manifest is bijective (§5.1 `machine_contracts_are_closed_and_complete`). Documented compatibility: §4.3 "New optional fields may be added within v1; removals, meaning/type changes, enum removals, or order changes require a schema-version change." Determinism: §4.2 `query_fingerprint` is SHA-256 over RFC 8785 canonical JSON and explicitly excludes render mode, width, color, and generated time, so the same result rendered twice fingerprints identically. Selectors never mutate ambiguously: §3.2(D10.2) returns `selector_ambiguous` under `--no-input`, JSON, piped/non-TTY, *and every bulk command*; §3.2(D10.4) re-resolves the full UUID inside the mutation transaction and fails rather than substituting; §3.2(D10.5) forbids accepting a prefix that was unique only in a previously displayed subset. Excellent column met on every clause. | — |
| C3 Configuration | 3 | §4.6 "Configuration and readiness contract" gives four distinct XDG roots with explicit roles (`XDG_CACHE_HOME` "is never authoritative") and a per-setting precedence matrix — timezone/width/color/limit/database profile each mapped across CLI, env, TOML, default — under the stated law `CLI flag > documented environment variable > TOML profile > compiled default`, with "absence differs from an explicitly empty/invalid value, which fails instead of falling through". Pure resolution tests: "Table-driven tests isolate the process environment and cover every pairwise precedence, unset/empty/malformed values, missing files, symlinks, non-UTF-8 paths where supported"; §5.1 `configuration_precedence_matrix` asserts "no writes". Redaction: "redacted database URLs" in that same matrix, plus "Configuration contents, query text, and credentials never enter fingerprints or diagnostics". Compatibility policy is documented for the surface D owns (§4.3 v1 evolution rules; §4.6 `database.schema_revision` and `contracts.installed` doctor checks). | — |
| C4 Output/accessibility | 3 | Primary criterion, handled with real rigor. Color: §3.1 and §3.7 disable ANSI under `--no-color`, `NO_COLOR` ("presence wins"), `TERM=dumb` (§4.6 color row), non-TTY, and always under `--json`; §5.1 `color_policy_precedence` asserts every combination is ANSI-free and that "color never changes plain-text tokens". Color-independent state: §3.3 makes `[E]`/`[T]` and `[ACTIVE]`/`[COMPLETED]`/`[CANCELLED]` mandatory tokens "always printed and never inferred from color or strike-through"; §3.2 month cells carry `E`/`T`/`B`/`.` markers with brackets for the current date. Width: §3.3 pins precedence `--width` > positive `COLUMNS` > terminal detection > 80, clamps 40–240, and defines three named degradation tiers (80+, 60–79, 40–59) with an explicit never-truncate set — kind, state, time/date, offset/fold, complete identity token, `BLOCKED`, result metadata, ambiguity indicators — then fails `terminal_too_narrow` below 40 "rather than emit misleading layout". Width counts display cells, not bytes or scalar values, and user text is escaped so ESC/bidi/newline cannot alter terminal structure. Focus: §3.7 chooser focus uses "both `>` and the word `selected`", focus starts on the first result, "keyboard order equals visual order". Quickshell public interface: §3.7 final bullet and §4.7 make JSON the nonvisual boundary and forbid clients reading PostgreSQL directly. Accessibility fixtures: §5.3 PTY at 40/59/60/79/80/240 in color and no-color, piped screen-reader linear-order check asserting no cursor repaint; §5.1 `width_is_grapheme_and_cell_aware` and `projection_state_matrix_is_accessible`; §4.3's ABNF makes the human row machine-checkable rather than snapshot-only. | — |
| C5 Diagnostics | 3 | §4.6 specifies `mg-calr doctor --check views [--json]` as non-mutating with ten stable check IDs (`database.connect`, `database.read_only_role`, `database.schema_revision`, `timezone.database`, `recurrence.adapter`, `reminder.identity_uniqueness`, `ical.preservation_adapter`, `conflict.typed_state`, `terminal.width`, `contracts.installed`) and a fixed machine row shape `{id,status:"pass"|"fail"|"blocked",required_for,code,recovery}` that "contains no secret". No sudo or secret leakage: "D never invokes package managers, `sudo`, role creation, migrations, or secret prompts"; doctor "emits exact separately copyable role/database/migration commands … never executes them, never requests sudo, and never prints a credential". §5.2(7) runs doctor on clean, partial, unsafe-role, and ready installations and schema-validates the matrix. §4.6 closes the honesty loop: "Help marks commands unavailable until required checks pass rather than claiming partial capability." | — |

**Lens average:** 3.00
**Lens pass:** Yes — avg 3.00 ≥ 2.0, zero 1s, zero 0s

---

## Lens 3 — Standards Interoperability and Sync (weight: 25%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| I1 Lossless iCalendar | 3 | §4.5 "Lossless iCalendar boundary" states D "never parses, normalizes, serializes, drops, merges, indexes, logs, or returns the opaque property store", and §4.2 separately forbids the search migration from leaking search vectors through JSON or indexing "secret/unknown-property blobs". The excellent column's golden/property tests across malformed/edge fixtures are specified concretely: §4.3 requires `tests/fixtures/views/preservation/*.ics` containing "unknown `X-` properties, parameters, folded lines, malformed-but-preserved opaque values, and expected source-byte/property digests"; §4.5 captures canonical property multiset and source-byte digests before the query, runs every D view/filter/search/JSON/human/error path, re-exports through the owning B/F adapter, and requires identical "names, values, parameters, multiplicity, ordering/folding metadata, and malformed-but-preserved opaque bytes", with "Any digest change, even on serialization failure or cancellation, fails D acceptance." §6.4 I1 restates this without claiming D exports iCalendar. | — |
| I2 Sync authority | 2 | §4.4 asserts single authority: "PostgreSQL is authoritative … There is no daemon, global mutable cache, draft, local index outside PostgreSQL, or background refresh", and §6.4 I2 adds "no cache, direct client DB access, vdir, or competing search service is introduced. Snapshot and source-revision fingerprints are carried through the public projection." §4.3's `snapshot_token` plus `ProjectionVersion` revision give a two-part fingerprint carried into the public projection, and §4.3's pagination rule refuses to re-query at a new snapshot with an old cursor. The excellent column's "three-way fingerprints, interruption recovery, explicit orchestration" is not met — three-way (local/remote/base) reconciliation and orchestration are deferred to F (§4.6, §7.4), and only D's own snapshot interruption (`snapshot_expired`) is covered. **Scored down for a factual conflict:** the shipped `ProjectionAgendaRepository` (`/home/mgeist/geist/calendar/src/storage.rs:2277`) sources agenda todos from a local `mg.interop/1` JSON file via `TodoProjectionSnapshot::load`, and `/home/mgeist/geist/calendar/tests/projection_agenda_contract.rs:330` (`agenda_repository_source_has_no_legacy_todo_database_read`) makes the *absence* of a todo database read a binding contract. The spec never names `src/interop.rs` or that projection, so §4.4's "no local index outside PostgreSQL" and §3.2 step 3's "one read-only application query … projects live events plus eligible todos" cannot both be executed against the current code. | §4.4 and §6.4 I2 must reconcile with the existing todo interop projection: either declare that D displaces `ProjectionAgendaRepository` and returns todos to PostgreSQL (and say what happens to `mg.interop/1`), or extend the snapshot model to two sources and define what `snapshot_token`/`query_fingerprint` mean when one source is a file revision (`interop.rs` already exposes `TodoProjectionSnapshot::revision()`). |
| I3 Conflict/deletion | 2 | §4.5 "Tombstones and conflicts" fully meets the acceptable column: repository eligibility returns a typed tri-state; D fails the *whole* result with `projection_integrity_error` on a live-plus-tombstone same-revision contradiction, an empty conflict set, a duplicated branch identity, or an implicit winner; "Selector resolution searches only authoritative live eligible rows and cannot resurrect a tombstone or select one conflict branch." Golden fixtures cover local-delete/remote-update, remote-delete/local-update, delete/delete, moved recurrence exception versus deleted master, and two same-UID branches, asserting default exclusion, distinct preserved identities, unchanged provenance, and "no chosen winner". The excellent column's "deterministic resolution and delete/restore round-trip fixtures" is explicitly declined in the same section: those "remain explicit F/deletion commands and are not claimed here". | To reach 3 without overreaching scope, §4.5 could add a read-side round-trip assertion — query, run an F restore in a fixture, re-query, and assert the restored row reappears under exactly its original `(entity_id, rfc_uid, recurrence_id)` with no new UID — proving D's projection survives the round trip rather than deferring the whole clause. |
| I4 Scope/network | 3 | §5.2(10) is an adapter/command test proving the boundary, not an assertion: "Deny outbound networking (namespace/socket test) and assert every D and D-doctor command succeeds/fails without network attempts; only the explicitly selected local PostgreSQL connection is permitted." Reinforced by §4.6 ("PostgreSQL full-text search is preferred; no external search service or network dependency" and "No images, fonts, web assets, HTTP client, sync client, or Quickshell dependency"), §4.6's role matrix denying "sync/network capabilities", and §6.4 I4. Database access is confined to explicit database-backed view commands, satisfying the auto-fail carve-out. | — |

**Lens average:** 2.50
**Lens pass:** Yes — avg 2.50 ≥ 2.0, zero 1s, zero 0s
**Auto-fail triggered:** No — all eleven rules walked individually; see table below

### Auto-fail walk

| Rule | Result | Where the design forbids it |
|---|---|---|
| Silent event/todo loss | Pass | §4.3 over-limit is `query_limit_exceeded` with "no partial success", never a truncated page; §4.5 a missing/duplicate exception identity is `projection_integrity_error`, "not an omission"; §3.6 malformed row → "no item silently omitted" |
| Unconfirmed overwrite | Pass | §3.6 closing line: no command in D writes events, todos, audit, sync, or reminder delivery state; §4.3 repository "commits/rolls back without writes" |
| UID instability | Pass | §4.2 event occurrence identity `(event_id, rfc_uid, recurrence_id)` "remains stable across revisions"; §3.2(D10.1) short IDs "are selectors only and are never stored as identity"; §4.5 forbids "manufacturing a second UID" and recycling an RFC UID |
| Recurrence/exception corruption | Pass | §4.5 D "never reparses RRULE text, invents recurrence IDs, or rewrites an exception onto its master"; moved exceptions retain both recurrence ID and resolved instant; deleted exceptions "never reappear as generated master occurrences" |
| Timezone/DST drift | Pass | §3.2 invalid zone "never silently falls back to UTC or a fixed offset"; §3.6 typed `local_time_nonexistent`/`local_time_ambiguous`; §3.3 fold bit and offset retained at every width; §5.1 gap/fold vectors |
| Duplicate reminder delivery | Pass | §4.5 D "neither creates, claims, acknowledges, retries, resets, nor deletes those rows"; a concurrent snapshot "reads definition membership only and cannot change delivery eligibility" |
| Non-idempotent scans | Pass | §4.4 query state is immutable and command-scoped; every path is read-only; §4.3 same `QueryResult` fingerprints identically on re-render |
| Plaintext credentials / secret logging | Pass | §3.6 errors exclude URLs, SQL, credentials, notes, extension blobs; §4.6 credentials never enter fingerprints or diagnostics; doctor "never prints a credential" |
| Network outside explicit sync | Pass | §5.2(10) network-denial test; only the explicitly configured local PostgreSQL connection, which is a database-related command |
| Automatic conflict overwrite | Pass | §4.5 an implicit winner is `projection_integrity_error`; selectors "cannot resurrect a tombstone or select one conflict branch" |
| Loss of unsupported iCalendar properties on round trip | Pass | §4.5 before/after source-byte and property-multiset digests across every D path plus re-export; any digest change fails acceptance |

---

## Lens 4 — Operational Security and Reliability (weight: 15%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| O1 Credentials | 3 | §3.6 closing: "Errors never include database URLs, SQL, credentials, unescaped notes, or extension-property contents." §4.6: "redacted database URLs" is an explicit test case in the config matrix, and "Configuration contents, query text, and credentials never enter fingerprints or diagnostics". §4.2 the fingerprint preimage excludes titles, notes, raw iCalendar properties, reminder payloads, and credentials. §4.5 renderers cannot emit "claim tokens, or secret material". §3.3 defines `snapshot_token` as "an opaque non-secret correlation value, not a database transaction ID or credential". Secret-boundary tests: §5.3 "assert it never prompts, stdout is empty on error, stderr is one valid envelope" and "assert conventional termination without panic, stack trace, secret, or retry loop". Sanitized artifacts: §4.3 goldens substitute only manifest-declared volatile fields. §6.1 extends redaction to debug tracing ("typed IDs/counts and redacted predicates"). | — |
| O2 Least privilege | 3 | §4.6 states the runtime role exactly — "`CONNECT`, schema `USAGE`, and `SELECT` only", with no `INSERT`/`UPDATE`/`DELETE`/`TRUNCATE`, DDL, role, replication, bypass-RLS, or network-sync privilege — and adds an enforcement gate: "Startup verifies effective read-only transaction mode and fails `database_role_unsafe` if the configured D profile can mutate projection/audit/reminder/sync tables." Clean-machine recovery is both executable and admin-run: doctor "emits exact separately copyable role/database/migration commands from versioned documentation, never executes them, never requests sudo". §5.2(7) executes the role matrix, proving "explicit denial of DML, DDL, reminder claim, audit write, tombstone restore/purge, conflict resolution, and sync/network capabilities". | — |
| O3 Failure contracts | 3 | §3.6 tabulates 14 error conditions with trigger, typed code, recovery, and data-loss risk. §4.3 makes the mapping machine data rather than prose: `contracts/view-errors-v1.json` carries `code`, `exit`, `retryable`, permitted detail keys, and a recovery text key, with stable exits (64 usage, 66 selector, 69 database/snapshot, 70 internal) and "Human and JSON tests load this table rather than duplicate literals." Atomicity: over-limit and integrity contradictions fail the whole result. Fault/concurrency tests: §5.2(4) concurrent DML across a held page session, §5.2(5) `selection_stale`, §5.2(6) injected failures including forced renderer failure and transaction cancellation, §5.2(9) corrupted fixtures, §5.3 broken pipe and SIGINT. Recovery runbook: §3.6 Recovery column plus §4.6 doctor's copyable recovery commands. | — |
| O4 Verification | 2 | The acceptable column is met well beyond the bar: §5.1 names 19 unit tests with the invariant each proves, §5.2 defines 10 opt-in integration scenarios gated on a disposable database "visibly named `mg_calr_test`", §5.3 adds PTY E2E, §5.4 a manual matrix, and §4.3 adds CI schema/ABNF/manifest-bijection gates. The excellent column names four CI gates, of which two are absent and one is unclaimable: **no migration gate** — §7.2 adds a forward index migration and §4.8 requires reporting its cost, but §5 never names a migration up/idempotency/drift test even though `tests/migration_contract.rs` already exists in the repo to extend; **no secret-scan gate** — redaction is asserted inside individual tests but no CI scan over goldens/artifacts is specified, despite §4.3 goldens embedding real-shaped identity and `snapshot_token` values; package gate is legitimately H's scope. **Also scored down for a factual error:** §7.1 states "Current passing foundation tests do not demonstrate any view/query capability", which is false — `tests/query_contract.rs` (247 lines) was committed in `d0f75f1` 29 minutes *before* this spec was recorded in `2dd47ec`, and `tests/agenda_contract.rs` and `tests/projection_agenda_contract.rs` now add 15 further view/query tests. The stated verification delta is therefore wrong. | Add to §5.2 a migration gate for the new index migration (apply on a clean database, re-apply for idempotency, assert checksum/drift behavior matches `tests/migration_contract.rs`, and record the measured storage cost §4.8 demands). Add to §4.3 or §5.2 a CI secret-scan gate over `contracts/` and `tests/fixtures/views/` asserting no connection string, credential, or non-manifest-declared volatile value is committed. Rewrite §7.1's final bullet against the real test suite. |

**Lens average:** 2.75
**Lens pass:** Yes — avg 2.75 ≥ 2.0, zero 1s, zero 0s

---

## Feasibility Check

Verified against `/home/mgeist/geist/calendar` at HEAD `6e855f9` (2026-08-27). The spec was
recorded in `2dd47ec` (2026-08-23 21:49), 29 minutes after `d0f75f1` "feat: add calendar
event query and CLI slice" (21:20). Errors marked **(false when written)** predate the spec;
errors marked **(post-dates spec)** arose in later commits.

| Check | Status | Notes |
|---|---|---|
| Types/models exist or are clearly specified | ✓ | The proposed types are clearly specified (§4.2) and non-conflicting with `src/domain.rs` (`RfcUid`, `EventTime` timed-with-IANA vs all-day-exclusive, `Event`, `EventStatus`) and `src/domain/todo.rs`. **But §7.1 materially misreports the baseline:** it calls `src/domain.rs` "nominal UUID identifiers/errors" (it is 401 lines with the full event aggregate), `src/storage.rs` "connects/migrates" (2911 lines, four repositories, optimistic locking, import/export), and `src/main.rs` "foundation diagnostics and the schema-v1 envelope" (1321 lines, 13 subcommand groups). It lists "application/domain projection types" as Absent, but `src/application.rs` defines `EventProjection`, `CalendarProjection`, `TodoQueryProjection`, `AgendaItem`, and `AgendaOutput`. **(false when written** for `EventProjection`/`CalendarProjection`; the agenda types post-date it.**)** |
| API/interface changes are feasible with current architecture | ✓ | `execute_view`/`render_human`/`render_json` (§4.3) fit the existing `EventUseCases`/`AgendaUseCases` + repository-trait pattern; `AsyncAgendaRepository` already models a read-only query boundary. One concrete conflict: §3.6 and §4.3 mandate exit **64** for "invalid date, zone, filter, range, width, or empty search", but `src/lib.rs:107-124` returns **65** for `Domain`/`Todo`/`InvalidInput` and reserves 64 for `RequiredInput`/`Input` only. §7.2 claims the change "preserv[es] the foundation envelope"; it does not preserve the foundation *exit mapping*. |
| Views/screens fit current navigation pattern | ✓ | The clap subcommand tree already carries global `--json`, `--no-input`, `--no-color`, `--database-url` (`src/main.rs:29-40`), so §3.1's global projection flags slot in. `--width` and `NO_COLOR`-driven rendering are new. §7.1's "Absent: D1–D10 commands" is wrong: `event day-agenda --date --timezone` (D2) shipped in `d0f75f1` **(false when written)**, and `agenda --start --end --timezone --include-completed/--include-trashed/--include-blocked` (D1/D5 partial) plus `tui` shipped later **(post-dates spec)**. Correctly absent, verified by grep: `week`, `month`, `search`, `--choose`/chooser, `--width`, `contracts/`, `tests/fixtures/`. |
| Dependencies are available and version-compatible | ✓ | Present and plausible in `Cargo.toml`: `chrono 0.4` + `chrono-tz 0.10` (IANA zones — already chosen, so §4.6's deferral to B4/B6 evidence is moot), `sha2 0.10` (the §4.2 SHA-256 fingerprint is directly implementable), `serde_json 1`, `uuid 1` (v7), `tokio-postgres 0.7` (supports `build_transaction().read_only(true)` with `RepeatableRead`, so §4.3's snapshot is achievable), `clap 4.5`; dev: `assert_cmd 2`, `predicates 3`, `tempfile 3`. **Undeclared:** §4.6 names only a grapheme/width crate and a terminal-capability crate (correctly deferred to Q1), but §4.3/§5.3 additionally require a JSON Schema 2020-12 validator, an ABNF validator, an RFC 8785 canonicalizer with an independent cross-implementation, and a PTY driver — none are listed in §4.6 or §7.2. Note `Cargo.toml` sets `unsafe_code = "forbid"` and `clippy::pedantic = "deny"`, which constrains PTY crate choice. |
| Platform/renderer requirements are realistic | ✓ | §4.7's Arch/Hyprland UTF-8 target with a portable Linux core, dumb-terminal and pipe support, is realistic and matches the repo. `--no-color`/`NO_COLOR` are parsed today but inert — `src/main.rs:1160` computes `let _color_disabled = cli.no_color \|\| std::env::var_os("NO_COLOR").is_some();` and discards the value — so §7.1's "width/color implementation beyond foundation ANSI absence" being absent is accurate. |
| Test strategy is executable with current infrastructure | ✓ | `assert_cmd`/`predicates`/`tempfile` and the opt-in `tests/postgres_integration.rs` harness support §5.2's disposable-database pattern directly. §5.3's PTY harness, §5.2(7)'s role matrix, §5.2(8)'s `EXPLAIN (ANALYZE, BUFFERS)` gate, and §5.2(10)'s network-namespace denial are all executable but are new tooling not present and not budgeted in §4.6. §7.1's "Current passing foundation tests do not demonstrate any view/query capability" is **false when written** (`tests/query_contract.rs`, 247 lines) and further wrong now (`tests/agenda_contract.rs`, 7 tests covering stable serializable ordering, DST-boundary rejection, timezone-scoped recurrence membership, lifecycle filtering; `tests/projection_agenda_contract.rs`, 8 tests). |
| Performance budget is realistic for target hardware | ✓ | The p95 targets (≤100 ms day, ≤150 ms week, ≤250 ms month, ≤300 ms filter/search over a 100k-item corpus) and ≤32 MiB/500 items are realistic for local PostgreSQL on a workstation *given* the indexes §7.2 adds. Two gaps make them unachievable against today's code: the schema has **no range index on `events(starts_at)`/`all_day_start`** (`0001_foundation.sql` creates only `calendars_one_default` and `audit_log_entity`) and **no `tsvector`/GIN anywhere** (grep across `migrations/` and `src/` returns none, so §7.1's "no search index" is correct); and `ProjectionAgendaRepository::agenda_events` calls `list_events_with_trashed(None, include_trashed)` — it loads *every* event and filters in memory, which violates §4.8's "Rendering is O(returned items + displayed graphemes), not O(total database rows)". §7.2 does not name that repository change. |
| No undeclared dependency on unbuilt features | ✓ | §7.4 is thorough on unbuilt work: A3/A4, B1–B8, C1–C8, the F lossless-property and typed tombstone/conflict adapters, and evidence-backed timezone/RRULE choices, with an explicit statement of what blocks D-complete versus what can land first. The gap is the mirror case — an **undeclared conflict with a built feature**: the shipped agenda sources todos from a local `mg.interop/1` file (`src/storage.rs:2277`, `src/interop.rs:577`), and `tests/projection_agenda_contract.rs:330` makes the absence of a todo database read binding, none of which the spec mentions **(post-dates spec)**. |

**Feasibility verdict:** Feasible with caveats

**Caveats:**
1. **Two authorities, unreconciled.** §3.2 step 3 ("one read-only application query … projects live events plus eligible todos"), §4.3's single `snapshot_token`, §4.4's "no local index outside PostgreSQL", and §4.3's "contract-test repository panics on a second query" cannot be implemented over `ProjectionAgendaRepository`, which issues one PostgreSQL read plus one file load from two independent sources with two independent revisions. This must be resolved before implementation starts.
2. **§7.1 is not a usable baseline.** It understates three of four source files, wrongly lists shipped commands, projection types, and the query repository as absent, and wrongly claims no existing test demonstrates view/query capability. Some of this was already false when the spec was committed. An implementer sizing work from §7.1 would rebuild what exists.
3. **Exit-code conflict.** §3.6/§4.3's exit 64 for validation errors contradicts `src/lib.rs:107` (65) for the same class of error; §7.2 must state whether the foundation mapping changes or D conforms to it.
4. **Unbudgeted test tooling.** JSON Schema, ABNF, RFC 8785, and PTY tooling are required by §4.3/§5.3 but appear in neither §4.6 nor §7.2.
5. **Load-everything repository.** §4.8's budgets require pushing range/filter predicates into SQL; the current `list_events_with_trashed(None, …)` path is not named in §7.2.

---

## Composite Score

| Lens | Average | Weight | Weighted |
|---|---|---|---|
| Temporal and Data Integrity | 2.60 | 35% | 0.910 |
| CLI Usability and Automation | 3.00 | 25% | 0.750 |
| Standards Interoperability and Sync | 2.50 | 25% | 0.625 |
| Operational Security and Reliability | 2.75 | 15% | 0.4125 |
| **Composite** | | | **2.70** |

**Pass conditions (from criteria.md):**
- [x] Composite ≥ 2.0 — 2.70
- [x] All lens averages ≥ 2.0 — 2.60 / 3.00 / 2.50 / 2.75
- [x] No criterion scores 0 — lowest is 2 (T4, T5, I2, I3, O4)
- [x] No more than two criteria at 1 per lens — zero 1s in every lens
- [x] All auto-fail rules pass — all eleven walked individually above
- [x] Feasibility ≠ Infeasible — Feasible with caveats

**All conditions met:** Yes → PASS

---

## Remediation Brief (non-blocking — verdict is PASS)

No Priority 1 items: nothing here blocks the pass. Items 1 and 2 below should nonetheless be
corrected before an implementation agent is handed this spec, because both would cause
misdirected work.

### Priority 2 — Should fix for quality

1. **§7.1 — rewrite the current-state inventory against HEAD.** Replace the four bullets with
   verified state. Specifically: `src/domain.rs` (401 lines) holds `RfcUid`, `Calendar`,
   `EventTime`, `EventStatus`, `Alarm`, `EventMetadata`, `Event`, with `src/domain/todo.rs`
   (569 lines) alongside; `src/storage.rs` (2911 lines) holds
   `PostgresCalendarEventRepository`, `PostgresTodoRepository`, `PostgresProjectRepository`,
   and `ProjectionAgendaRepository`; `src/main.rs` (1321 lines) dispatches calendar, event,
   todo, agenda, interop, project, tag, and tui subcommands. Move from "Absent" to
   "implemented, partial": D2 (`event day-agenda`), D1/D5 partial (`agenda` with
   `--start/--end/--timezone` and three lifecycle flags), application projection types
   (`EventProjection`, `CalendarProjection`, `TodoQueryProjection`, `AgendaItem`,
   `AgendaOutput`), and the agenda JSON item shape (`AgendaItem` derives `Serialize` inside
   the schema-v1 envelope; `tests/agenda_contract.rs:36` already asserts
   `json["items"][1]["kind"] == "todo"`). Delete the claim that no passing test demonstrates
   view/query capability and cite `tests/query_contract.rs`, `tests/agenda_contract.rs`, and
   `tests/projection_agenda_contract.rs`. Keep as correctly absent: D3 week, D4 month, D6
   filter flags, D7 search, D8 width rendering, D10 chooser, the `contracts/` artifacts,
   `tests/fixtures/`, the snapshot page-session API, SELECT-only role checks, and any
   `tsvector`/GIN index.
2. **§4.4 and §6.4 I2 — reconcile the todo authority.** State explicitly how D relates to the
   shipped `mg.interop/1` todo projection (`src/storage.rs:2277`, `src/interop.rs`), which
   `tests/projection_agenda_contract.rs:330` makes binding. Choose one and say so: (a) D
   displaces it and todos return to PostgreSQL, in which case §7.2 must list removing
   `ProjectionAgendaRepository` and that test; or (b) the projection file remains a second
   source, in which case §4.3 must define what `snapshot_token` and `query_fingerprint` cover
   when one source is a file revision (`TodoProjectionSnapshot::revision()` already exists),
   and §4.3's "contract-test repository panics on a second query" must be restated as one
   query per source.
3. **§3.6 / §4.3 / §7.2 — resolve the exit-code conflict.** Validation errors exit 65 today
   (`src/lib.rs:107-124`), not the 64 the spec mandates. Either change the spec to 65 or add
   to §7.2 an explicit, breaking-change-flagged item that remaps `Domain`/`Todo`/`InvalidInput`
   to 64 across the whole binary.
4. **§5.2 — add the missing O4 gates.** Add a migration gate for the new index migration
   (clean apply, idempotent re-apply, checksum/drift behavior consistent with the existing
   `tests/migration_contract.rs`, plus the measured storage cost §4.8 requires) and a CI
   secret-scan gate over `contracts/` and `tests/fixtures/views/`.
5. **§4.6 / §7.2 — declare the remaining test tooling.** JSON Schema 2020-12 validator, ABNF
   validator, RFC 8785 canonicalizer plus its independent cross-implementation, and a PTY
   driver are required by §4.3 and §5.3 but named nowhere as dependencies. Note the
   `unsafe_code = "forbid"` and `clippy::pedantic = "deny"` lints when selecting them.
6. **§7.2 / §4.8 — name the repository predicate pushdown.** `agenda_events` currently calls
   `list_events_with_trashed(None, include_trashed)` and filters in memory, which cannot meet
   §4.8's "O(returned items), not O(total database rows)". Add the pushdown as an explicit
   delta item.

### Priority 3 — Consider for excellence

7. **T4 → 3 (§4.5):** specify the eligibility predicate a later F restore/purge command
   evaluates against the tri-state — which `(tombstone_id, source_revision)` pairs are
   restorable and what makes one ineligible — without moving the command itself into D.
8. **T5 → 3 (§4.5):** add one D-owned sleep/suspend vector asserting that a view snapshot
   spanning a host suspend/resume reports identical `has_reminder` membership and leaves
   claim/outcome rows byte-equal.
9. **I3 → 3 (§4.5):** add a read-side round-trip fixture — query, run an F restore, re-query,
   assert the row returns under exactly its original `(entity_id, rfc_uid, recurrence_id)`
   with no new UID.
10. **§4.6:** `chrono-tz 0.10` is already the committed IANA source, so "The chosen IANA
    timezone … dependencies come from B4/B6 evidence, not this spec" is stale; state the
    existing choice and confine the deferral to RRULE.
