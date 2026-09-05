# Scorecard: Todo Core and Graph

**Feature ID:** c-todo-core
**Spec file:** docs/specs/c-todo-core.md
**Reviewer agent:** blind verification agent
**Date:** 2026-08-30
**Spec iteration reviewed:** 1

---

## Verdict: PASS

**Summary:** The spec's strongest quality is its transactional graph and recurrence design (§3.2 C4/C5/C6, §4.2, §5.2): every invariant that could corrupt a task graph is pushed into a single transaction with durable constraints, explicit rejection of "race-prone preflight" validation, and a fault-injection/concurrency test matrix that names the five failure stages. Its most critical gap is §7.1, which materially misreports the current state of the codebase — five of the six capabilities it declares "absent" are in fact implemented today, and its proposed Migration 2 and template/occurrence recurrence model collide with the already-shipped `migrations/0002_todo_core.sql` and `todos.recurrence_rule` jsonb model. Secondary gaps: §4 never states configuration precedence or XDG placement for the two new config keys it introduces (C3), and no test in §5 gates the redaction promises made in §3.6/§6.5.

---

## Lens 1 — Temporal and Data Integrity (weight: 35%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| T1 Identity | 3 | §4.2 declares six typed UUID newtypes (`TodoId`…`TodoOccurrenceId`) with the doc comment "selectors are never stored as authority"; §4.3 requires "a resolved immutable UUID and expected version internally" for every mutation and resolves CLI selectors before the use case. §3.2 C2 preserves project UUID across rename and requires `--confirm SELECTOR` to resolve to "the same immutable UUID"; §3.2 C4 gives occurrence graphs fresh todo UUIDs while "retaining template-node IDs and scheduled anchors for history". Conflict identity is explicit (§3.6 `concurrent_update`, optimistic version). Fixtures exist (§5.2 "verify UUID and audit provenance stability"). | — |
| T2 Temporal correctness | 3 | §4.2 splits `TodoDue::Date{date,zone}` from `Timed{at,zone}`, so all-day bounds are structural, not inferred. §3.2 C1.5 stores an IANA zone for timed anchors and recurs in that zone; §3.2 C8 states DST "gap/fold resolution follows the same explicit temporal policy as B4 and never falls back silently to host-local interpretation", and §4.6 forbids re-reading `/etc/localtime` after creation. Vectors are named twice: §5.1 `due_round_trips_date_and_zoned_time` ("include gap/fold fixtures from B4") and `recurrence_uses_schedule_anchor_not_completion_clock` ("including DST vectors"). Transactional semantics are explicit in §3.2 C4: the next value is computed "from the template anchor/RRULE rather than wall-clock completion" inside the locked transaction. | Minor: §3.2 C2 defines `--scope instance|future|instance-and-future` but no test in §5.1–§5.3 exercises the three scopes; add one. |
| T3 Transaction integrity | 3 | §3.2 C1.7 writes todo, relationships, template/first occurrence, reminders and audit "atomically". §3.2 C4 specifies lock template/cursor → compute → validate whole prospective graph → insert all rows → advance cursor → commit, with "Any validation, SQL, serialization, or crash failure rolls back the entire occurrence graph and cursor". §4.2 mandates durable enforcement — "backed by deferred constraint triggers proven in Spike 5; application-only validation is insufficient" and "implementation may not weaken this to a race-prone preflight query". §5.2 supplies two concurrency races (jointly-cyclic edits; completion vs. reparent), fault injection after node/parent-edge/dependency/reminder/audit stages, connection-kill-and-retry, and cap-overflow cursor exactness; §5.1 adds property tests over trees plus DAGs. | — |
| T4 Deletion/audit | 3 | §3.2 C2 makes trash a leaf-only soft delete that "fails if it has live descendants or live dependents" and "preserves audit, recurrence-instance, and reminder records"; restore "validates all graph invariants again". Purge is restricted to an already-trashed todo with no live/trashed descendants, dependency references, recurrence-template/history references, "or undo eligibility", and requires `--confirm` resolving to the same UUID. §4.2 requires audit rows with "one transaction UUID and before/after JSON for every changed aggregate"; §6.5 states there is "no force bypass". §3.6 adds "No error path performs a compensating partial delete." | — |
| T5 Reminder idempotency | 2 | §3.2 C8 draws the right boundary — "Reminder rows are schedules, not delivery claims" — and forbids C from creating deliveries: `eligible_todo_reminders` "must not create `reminder_deliveries` for suppressed candidates", and C "does not mark it delivered, dismissed, or lost". Durable schedule uniqueness is specified (§3.2 C1.4 duplicate schedules "rejected before mutation"; §4.2 "todo reminder uniqueness prevents duplicate equivalent schedules") and §5.2 asserts blocked todos yield "no candidate/delivery row". This meets the 2 anchor for durable unique schedule state, but the delivery-claim ledger and the retry/crash/sleep/DND matrix required for 3 are explicitly deferred to E1–E8 (§7.4, §7.5). | Not blocking at this scope. To reach 3, C would have to own delivery claims, which §7.5 correctly assigns to E. |

**Lens average:** 2.80
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Lens 2 — CLI Usability and Automation (weight: 25%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| C1 Human workflow | 3 | §3.2 C1.2 fixes an explicit prompt order (title → due/zone → priority → project → tags → notes → parent → prerequisites → recurrence → reminders), gives optional prompts an explicit "none", and makes reminder prompting mandatory "even though the default is no reminder". §3.2 C2 replaces Taskwarrior's modifier grammar with explicit set/clear pairs (`--due`/`--clear-due`, `--add-tag`/`--remove-tag`), matching the criteria's "improve: guided discoverability and lower grammar complexity" against the Taskwarrior baseline. Every row of §3.6 carries a concrete Recovery column; §3.2 C1.5 accepts bounded shortcuts (`today`, `tomorrow`, `next fri`) while rejecting unconstrained prose. §4.7 budgets single-item mutations at <100 ms p95. | — |
| C2 Automation | 3 | §3.2 C1.1 gives every promptable value a flag; C1.3 makes `--no-input` "never read stdin" and return `input_required` with field names and exit 64. §4.3 pins the A4 envelope `{schema_version:1, command, ok:true, data}`, a total list ordering (due-nullness, due, descending priority, normalized title, UUID), opaque cursor pagination (default 100 / max 1000), set-like arrays "sorted by immutable ID or normalized name", and "JSON never contains ANSI or localized dates". Ambiguity never mutates: §3.2 C1.6 returns `selector_ambiguous` plus candidates "without mutation", and §5.3.3 asserts "nonzero exits and unchanged database snapshots". Golden contracts in §5.1 `json_order_is_deterministic` and §5.3.5. | — |
| C3 Configuration | 1 | The spec introduces at least two new configuration keys — `todo.date_reminder_time` (§3.2 C8, "default 09:00") and the recurrence materialization cap (§3.2 C4, "a configurable command cap defaulting to 100 occurrences") — but nowhere states which file or XDG root holds them, nor the CLI > env > TOML > default precedence, nor any validation or resolution behavior. §4.6 mentions only `NO_COLOR` and IANA data; §6.1/§6.5 cite "A-foundation redaction" for URLs. §6.4's own criteria-alignment paragraph enumerates T1–T5, C1, C2, C4 and O1–O4 and silently omits C3, so the spec does not even claim coverage, and there is no explicit deferral. §5 contains no resolution test. Meets the 1 anchor (partial XDG/precedence by inheritance only). | Add to §4.4 or §4.6: state that `todo.date_reminder_time` and the materialization cap live in `$XDG_CONFIG_HOME/mg-calr/config.toml` under a `[todo]` table, resolve by CLI flag > env > TOML > default, and are validated at load with a typed error. Add a pure resolution unit test to §5.1. |
| C4 Output/accessibility | 3 | §3.3 fixes a stable semantic row order and spells out states as words (`BLOCKED (2 prerequisites)`, `RECURS`), makes tree indentation "supplemental" with parent/depth also in JSON, and states `--no-color` "loses no meaning" with `No todos match.` as the empty copy. §3.7 gives unique prompt labels with formats and defaults, sets focus order to the §3.2 prompt order, asserts no pointer-only interaction, drops ANSI under `--no-color`/`NO_COLOR`, and requires later Quickshell/TUI clients to "call public commands/application interfaces" and "not query PostgreSQL directly or become the sole recovery path" — the exact public-interface behavior in the 3 anchor. Fixtures: §5.3.1 (no ANSI under both controls, narrow terminal) and §5.4 (40/80/160 columns, long Unicode, combining characters, screen-reader linear output). | — |
| C5 Diagnostics | 2 | §3.2 C2 makes `todo show`/`todo list` explicitly non-mutating; §3.6 routes database-unavailable/migration-missing to "foundation storage error, exit 69" with the actionable step "run doctor/migrate explicitly" and "no automatic migration"; §6.5 forbids sudo and shell invocation, and §3.6 keeps notes and database URLs out of errors. §7.5 explicitly assigns "general doctor" to G. Meets 2 by inheriting a non-mutating diagnostic surface and adding actionable, non-mutating recovery per error, but defines no C-specific check and no prerequisite matrix. | Non-blocking. For 3, add a todo-graph integrity check (orphaned occurrences, dangling template references, migration-2 presence) to G4's matrix and name its stable machine output here. |

**Lens average:** 2.40
**Lens pass:** Yes — avg ≥ 2.0, one 1 (≤ two allowed), no 0s

---

## Lens 3 — Standards Interoperability and Sync (weight: 25%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| I1 Lossless iCalendar | 2 | §6.4 marks this "N/A for local-only todos; no todo is encoded as iCalendar and the design does not modify the accepted event codec boundary", and §7.5 assigns interchange elsewhere. This is a *correct scope deferral*, not a dodge: `feature-tree.md` puts the iCalendar boundary at F1/F2 and orders "C → E → D → F codecs", so no C1–C8 item covers a codec. The §4.2 domain model also does not preclude a later VTODO mapping — due kind/zone, locked priority vocabulary, notes, completion timestamp and a normalized recurrence expression are all retained. Held at 2 rather than 3 because §4.4 and §7.5 overstate the boundary as absolute ("never enter iCalendar"; "iCalendar/JSON interchange … excluded"), which conflicts with the F spec's own corpus (which includes a `VTODO` byte-identity fixture) and with the already-shipped `todo export`/`todo import` JSON interchange in `src/interop.rs`; no unknown-property preservation hook is reserved for todos. | Soften §4.4/§7.5 from "never" to "no C command encodes a todo as iCalendar; VTODO mapping and unknown-property preservation are F1/F2's", and reserve an opaque property bag in §4.2 so F1 does not have to migrate the todo schema. |
| I2 Sync authority | 2 | §4.4 is unambiguous: PostgreSQL "is the sole authority for todos, projects, tags, graph edges, templates, occurrences, reminder schedules, and audit state", the application layer "is the sole mutation authority and owns transaction boundaries", and "Later reminder services and UI clients consume application/public JSON contracts and never write tables directly". §6.4 I2 restates that "no competing vdir or client store is introduced". The durable-vdir half of the 2 anchor is inapplicable because todos do not sync, and the 3 anchor (three-way fingerprints, orchestration) is F8/F9's. | The spec does not acknowledge the existing `todo import` path (`src/main.rs` `TodoCommand::Import`, `src/interop.rs` `TodoProjectionSnapshot`), which ingests an external mg-todo snapshot into the store — a second write path its authority model does not describe. Add a sentence in §4.4 stating that projection import is an application-layer use case subject to the same validation and audit, or that C supersedes it. |
| I3 Conflict/deletion | 2 | §6.4 I3 defers remote reconciliation with an explicit, defensible reason ("N/A … because todos never sync") and supplies the local analogue rather than leaving the section empty: "Local soft-delete, restore, purge preconditions, immutable identity, recurrence history, and audit prevent silent local loss." Backed by §3.2 C2 (trash fails with live descendants/dependents; restore revalidates all invariants), §3.6 (typed conflict at exit 65 for trash/purge/reopen; `concurrent_update` at exit 75 with bounded retry and "optimistic version check/locks"), and §5.2's soft-delete/restore round trip plus purge-eligibility test. | — |
| I4 Scope/network | 3 | §4.4 states "No C command initializes a network client" and "Database access occurs only as part of the explicit local command being run", satisfying the criteria's database-access carve-out. §4.5 forbids "HTTP, CalDAV, iCalendar sync, vdirsyncer, DBus, Quickshell, or notification-backend" dependencies. The 3 anchor's adapter/command test is named explicitly in §5.2: "Assert every non-sync todo command opens no network socket/transport adapter and never touches vdir/sync tables", reinforced by §6.4 I4 and §6.5 ("never invoke sudo, shell commands, a notification backend, or a network client"). §4.4 also forbids reads from materializing recurrence as a hidden side effect. | — |

**Lens average:** 2.25
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s
**Auto-fail triggered:** No

Auto-fail rules walked individually:

| Rule | Result | Where the spec forecloses it |
|---|---|---|
| Silent event/todo loss | Pass | §3.2 C2 trash is leaf-only and preserves audit/occurrence/reminder rows; "Already materialized later occurrences are never silently rewritten"; §3.6 "No error path performs a compensating partial delete"; §6.5 "no force bypass". |
| Unconfirmed overwrite | Pass | §3.2 C2 requires `--scope instance\|future\|instance-and-future` under `--no-input` and asks in guided mode; purge requires `--confirm SELECTOR` resolving to the same UUID; §4.3 requires expected version on every mutation. |
| UID instability | Pass | §4.2 immutable typed UUIDs; §3.2 project rename preserves UUID; §3.2 C4 fresh occurrence UUIDs with retained template-node IDs. |
| Recurrence/exception corruption | Pass | §3.2 C4 one transaction per occurrence graph, full rollback of graph *and* cursor, unique `(template_id, scheduled_anchor)`; §5.2 five-stage fault injection and connection-kill retry. |
| Timezone/DST drift | Pass | §3.2 C1.5/C8 store IANA zone and recur in it; B4 gap/fold policy with no silent host-local fallback; §5.1 DST vectors; §4.6 no `/etc/localtime` re-read. |
| Duplicate reminder delivery | Pass | §3.2 C8 separates schedules from delivery claims and forbids C creating `reminder_deliveries`; §3.2 C1.4 rejects duplicate schedules pre-mutation; §4.2 schedule uniqueness; retries idempotent via the occurrence unique key so copied reminders cannot double. |
| Non-idempotent scans | Pass | §3.2 C4 unique occurrence key "makes retries idempotent"; §5.2 "unique idempotent retry" and §5.3.4 compares two retries for idempotent JSON. |
| Plaintext credentials / secret logging | Pass | §6.1 "No plaintext credential field exists"; §6.5 and §3.6 keep notes, URLs and credentials out of errors and logs. |
| Network outside explicit sync | Pass | §4.4, §4.5, §5.2, §6.4 I4, §6.5 (see I4 row). |
| Automatic conflict overwrite | Pass | §3.6 `concurrent_update` with bounded retry then failure; §4.3 expected-version requirement — no last-writer-wins path. |
| Loss of unsupported iCalendar properties | Pass (inapplicable) | No todo is encoded as iCalendar in C; the codec boundary is untouched (§6.4 I1). |
| *Orchestrator-flagged:* parent completion invariant | Pass | §3.2 C7 checks "every live descendant, not only immediate children"; "Descendant completion never propagates completion upward"; §3.2 C2 complete "fails if the todo is blocked or has any incomplete live descendant"; §3.2 C7 "`--force` is not provided". |

---

## Lens 4 — Operational Security and Reliability (weight: 15%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| O1 Credentials | 2 | §6.1 states "No plaintext credential field exists. Database URLs use A-foundation redaction. Crash reports, fixtures, and benchmarks contain synthetic content only." §6.5 adds that logs and default list/agenda output minimize todo content and "JSON errors expose only safe identifiers/field names", and §3.6 requires errors to "contain safe IDs and field names, never notes, database URLs, or full private task content by default". §4.3 keeps notes out of list/agenda JSON behind `--include-notes`. Held at 2: the 3 anchor requires secret-boundary tests, and no test in §5.1–§5.4 asserts that notes, titles or URLs are absent from any error, log line or benchmark artifact. | Add a §5.1/§5.3 test asserting that a todo with sensitive notes produces an error envelope and a default `todo list` containing neither the notes text nor the database URL. |
| O2 Least privilege | 2 | §4.3 states "the PostgreSQL role remains unprivileged application authority"; §6.5 states mutations "run under the unprivileged application role and never invoke sudo, shell commands, a notification backend, or a network client"; §3.6 forbids automatic migration, routing the user to explicit `doctor`/`migrate`. Held at 2: no role-boundary or clean-machine recovery test is named — §5.2's migration test covers scaffold upgrade and idempotent rerun, not privilege boundaries. | Name a test asserting that C's commands succeed under a role holding only CRUD grants (no DDL/superuser), and that DDL attempts fail with the foundation storage error. |
| O3 Failure contracts | 3 | §3.6 is a complete trigger/presentation/recovery/data-loss table with stable snake_case codes (`input_required`, `selector_ambiguous`, `todo_graph_invalid`, `todo_not_completable`, `recurrence_materialization_failed`, `concurrent_update`, `input_cancelled`) mapped to exits 64/65/66/69/75/130, and every row's data-loss column reads "none". Atomic mutation is specified throughout §3.2. The 3 anchor's fault/concurrency tests are in §5.2 (five-stage injection, connection kill and retry, two races, cap-overflow with exact next cursor), and the Recovery column plus §3.2's bounded-retry semantics serve as the recovery runbook. | — |
| O4 Verification | 2 | §5.1 supplies fourteen named unit tests plus property tests over trees and DAGs; §5.2 opens with "All PostgreSQL tests require the foundation's explicit disposable opt-in and safety-name check" and includes a migration upgrade/idempotency test; §5.3 gives process-level E2E with golden fixtures; §7.3 mandates TDD sub-slices; §4.7 makes "query-plan, boundedness, no-N+1, and atomicity assertions … mandatory correctness gates". Held at 2 for two reasons: the 3 anchor's secret-scan and packaging/clean-machine gates are absent and not deferred anywhere (packaging is H7's, but no secret scan is assigned), and §7.1's misreport of current state (see Feasibility) means the §7.2 delta and the §5.2 migration test are written against a schema baseline that no longer exists. | Correct §7.1/§7.2 against the real tree (see Feasibility caveats), then re-derive the §5.2 migration test from the actual applied migration set 0001–0006. Add a secret-scan gate. |

**Lens average:** 2.25
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Feasibility Check

Source read before completing this table: `src/domain/todo.rs`, `src/domain.rs`, `src/storage.rs`, `src/application.rs`, `src/main.rs`, `src/interop.rs`, `tests/todo_core.rs`, `tests/todo_projection_contract.rs`, `migrations/0001_foundation.sql`, `0002_todo_core.sql`, `0003_todo_recurrence.sql`, `0004_todo_reminders.sql`, `0006_repair_todo_recurrence.sql`, `Cargo.toml`, `Cargo.lock`, `docs/FEATURE-TREE.md`.

| Check | Status | Notes |
|---|---|---|
| Types/models exist or are clearly specified | ✗ | Semantics are clear and correct, but §4.2's snippet is written against the wrong crate. It uses `time::Date` and `time::OffsetDateTime`; `time` appears in neither `Cargo.toml` nor `Cargo.lock`. The project uses `chrono` 0.4 + `chrono-tz` 0.10, and the shipped `src/domain/todo.rs` already models this as `TodoDue::Date{date: NaiveDate, timezone: String}` / `Timed{at: DateTime<FixedOffset>, timezone: String}`. The spec's `IanaZone` type does not exist. This also contradicts §4.5's own instruction to "Prefer existing package dependencies". |
| API/interface changes are feasible with current architecture | ✓ | The domain/application/storage/render authority split matches the code: `src/domain/todo.rs` exists exactly as §4.1 names it, and §4.1's hedge ("If the package remains flat when this slice begins, equivalent namespaced files are acceptable") correctly anticipates that `application`, `storage` and CLI are flat (`src/application.rs`, `src/storage.rs`, `src/main.rs`; no `src/cli.rs` or `src/render.rs`). The `AsyncTodoRepository` trait and `TodoUseCases` already provide the seam §4.3's use-case signatures need. |
| Views/screens fit current navigation pattern | ✓ | `src/main.rs` already exposes a `TodoCommand` subcommand tree (Create/List/Show/Complete/Edit/Trash/Restore/Purge/Export/Import/ScanReminders); the spec's added verbs (`subtask`, `dependency`, `recurrence materialize`, `project`) are ordinary Clap additions. |
| Dependencies are available and version-compatible | ✗ | Two problems. (1) `time` is not a dependency (above). (2) §4.5 requires recurrence to "use the accepted adapter rather than introduce a second RRULE implementation", but no RRULE crate exists in `Cargo.toml` or `Cargo.lock`; the shipped `RecurrenceRule` in `src/domain/todo.rs` is hand-rolled and supports only `Daily|Weekly|Monthly` + interval + count/until. The §3.2 example `--repeat 'FREQ=WEEKLY;BYDAY=FR'` requires `BYDAY` support that the current domain has no representation for. §7.4 does declare the Spike 1/B6 adapter as a blocking dependency, so this is a sequencing gap rather than an impossibility. |
| Platform/renderer requirements are realistic | ✓ | Terminal-only, `--no-color`/`NO_COLOR`, IANA data via `chrono-tz` — all already in use. PostgreSQL 18 target with §4.6's minimum-version documentation clause is reasonable; `tokio-postgres` 0.7 imposes no barrier. |
| Test strategy is executable with current infrastructure | ✓ | `assert_cmd` 2, `predicates` 3 and `tempfile` 3 are dev-dependencies, and the disposable-database opt-in convention §5.2 relies on already exists (`tests/postgres_integration.rs`, `tests/migration_contract.rs`). Fault injection at the five named stages and connection-kill retry are achievable with `tokio-postgres`. Caveat: §5.2's migration test is specified against "migration-1 scaffolding", but the tree is now at 0006. |
| Performance budget is realistic for target hardware | ✓ | §4.7's numbers are internally consistent and hedged correctly ("non-flaky informational gates until reference hardware is recorded", with query-plan/no-N+1/atomicity as the mandatory gates). The 100,000-todo fixture and set-based blocked evaluation are supported by the indexes 0002 already creates on both dependency directions, parent, project and tag join. |
| No undeclared dependency on unbuilt features | ✓ | §7.4 declares A1–A5, B4, Spike 1/B6 and Spike 5 as blockers, and §3.1 handles D10's absence gracefully ("until D10 lands, ambiguity fails without mutation"). Nothing is silently assumed. |

**Feasibility verdict:** Feasible with caveats

**Caveats:**

1. **§7.1 materially misreports current state.** It asserts: "No evidence supplied to this spec shows C1–C8 commands, application use cases, graph enforcement, recurrence materialization, organization tables, or reminder eligibility implemented." Five of those six are implemented today:
   - *C1–C3 commands:* `src/main.rs` `TodoCommand` and `run_todo_command` implement create/list/show/complete/edit/trash/restore/purge with `--yes` confirmation on purge.
   - *Application use cases:* `src/application.rs` provides `TodoUseCases`, `ProjectUseCases` and `TagUseCases` (`create_todo_async`, `edit_todo_async`, `complete_todo_async`, `trash/restore/purge`, `due_reminders_async`, `scan_reminders_async`).
   - *Graph enforcement:* parent-cycle rejection walks ancestors under `FOR UPDATE` and returns `StorageError::Cycle` (`src/storage.rs` ~1636–1673); dependency-cycle rejection uses a `WITH RECURSIVE` reachability query returning `StorageError::DependencyCycle` (`src/storage.rs` ~1733). "DAG cycle rejection" is *not* absent.
   - *Organization tables:* `migrations/0002_todo_core.sql` already creates `projects`, `tags`, `todo_tags`, adds `project_id`/`due_date`/`version`/`trashed_at` to `todos`, renames the dependency columns to `dependent_id`/`prerequisite_id`, and adds the due-representation and due-timezone check constraints — i.e. much of §4.2's "Migration 2 must provide or normalize" list.
   - *Reminder eligibility:* `src/storage.rs` ~1868 already selects due reminders excluding completed, trashed and deleted todos *and* those with an incomplete live prerequisite — the blocked-suppression rule of §3.2 C8. `migrations/0004_todo_reminders.sql` adds `todo_reminders` plus the `reminders_todo_schedule_unique` partial unique index.

   Only two of §7.1's absence claims hold: the C7 parent-completion invariant and blocked-completion prevention are genuinely unimplemented — `complete_todo` (`src/storage.rs` ~1392) sets `completed_at` with no descendant or prerequisite check.

2. **Migration number collision and recurrence-model conflict.** §4.1 and §7.2 call for a new `migrations/0002_todo_core.sql`; that file already exists with different content. More seriously, §4.2's template/occurrence architecture (`todo_recurrence_templates`, `todo_template_nodes`, `todo_template_dependencies`, `todo_occurrences`, unique `(template_id, scheduled_anchor)`) conflicts with the shipped model, in which recurrence is a `todos.recurrence_rule jsonb` column expanded on the fly and explicitly never materialized (`migrations/0003_todo_recurrence.sql`: "instances are never stored"; repaired by `0006`). `Todo::expand_due_instances_indexed` and the agenda projection in `src/application.rs` already consume that model, so adopting §4.2 is a migration-and-rewrite of live behavior, not a greenfield addition. The spec does not acknowledge this and therefore does not plan for it.

3. **Unacknowledged second write path.** `src/interop.rs` (`TodoProjectionSnapshot`) plus `TodoCommand::Export`/`Import` already provide file-based todo JSON interchange that writes todos from an external mg-todo snapshot. This contradicts §7.5's non-goal ("iCalendar/JSON interchange … excluded; todos remain local-only") and is not covered by §4.4's authority model.

None of these makes the design impossible — the invariants, transaction boundaries and CLI contracts all remain buildable — so the verdict is caveated, not Infeasible. But §7.1, §7.2 and §4.2's migration plan must be rewritten against the real tree before an implementer uses them.

---

## Composite Score

| Lens | Average | Weight | Weighted |
|---|---|---|---|
| Temporal and Data Integrity | 2.80 | 35% | 0.980 |
| CLI Usability and Automation | 2.40 | 25% | 0.600 |
| Standards Interoperability and Sync | 2.25 | 25% | 0.563 |
| Operational Security and Reliability | 2.25 | 15% | 0.338 |
| **Composite** | | | **2.48** |

**Pass conditions (from criteria.md):**
- [x] Composite ≥ 2.0 — 2.48
- [x] All lens averages ≥ 2.0 — 2.80 / 2.40 / 2.25 / 2.25
- [x] No criterion scores 0 — lowest is C3 at 1
- [x] No more than two criteria at 1 per lens — one 1 total (C3, Lens 2)
- [x] All auto-fail rules pass — all twelve walked individually above
- [x] Feasibility ≠ Infeasible — Feasible with caveats

**All conditions met:** Yes → PASS

---

## Remediation Brief

N/A — verdict is PASS. The items below are recorded because the Feasibility caveats
are factual errors that will mislead an implementer even though they do not breach a
pass condition.

### Required corrections before implementation

1. **§7.1 — rewrite the current-state analysis against the real tree.** Delete the
   blanket sentence "No evidence supplied to this spec shows C1–C8 commands,
   application use cases, graph enforcement, recurrence materialization, organization
   tables, or reminder eligibility implemented." Replace it with a per-capability
   status: *implemented* — todo CRUD/lifecycle CLI (`src/main.rs` `TodoCommand`),
   `TodoUseCases`/`ProjectUseCases`/`TagUseCases` (`src/application.rs`), parent- and
   dependency-cycle rejection (`src/storage.rs` `StorageError::Cycle` /
   `DependencyCycle`), `projects`/`tags`/`todo_tags` and the due/version/trash columns
   (`migrations/0002_todo_core.sql`), `todo_reminders` with blocked-and-completed
   suppression (`migrations/0004_todo_reminders.sql`, `src/storage.rs` due-reminder
   query); *absent* — the C7 all-descendants parent-completion invariant and
   blocked-completion prevention (`complete_todo` performs neither check), the
   template/occurrence materialization graph, subtask/dependency subcommands, and
   guided prompting.

2. **§4.1, §4.2, §7.2 — resolve the migration collision and the recurrence-model
   conflict.** `migrations/0002_todo_core.sql` already exists with different content
   and must not be redefined; renumber the new schema to `0007` (or later) and state
   which of §4.2's requirements are already satisfied by 0002/0004. Separately, decide
   and state explicitly whether the template/occurrence model *replaces* the shipped
   `todos.recurrence_rule` jsonb + on-the-fly `expand_due_instances` model
   (`0003`/`0006`, `src/domain/todo.rs`, `src/application.rs` agenda projection) or
   coexists with it. If it replaces it, §7.2 must add the data migration and the
   agenda-projection rewrite as delta items.

3. **§4.2 — correct the data-model snippet's crate.** Replace `time::Date` and
   `time::OffsetDateTime` with `chrono::NaiveDate` and `chrono::DateTime<FixedOffset>`
   (or `Utc` where an instant is meant), and either define `IanaZone` as a new
   validated newtype or use the existing `String` + `chrono_tz::Tz` validation from
   `src/domain/todo.rs`. `time` is in neither `Cargo.toml` nor `Cargo.lock`, and §4.5
   forbids adding dependencies where existing ones suffice.

4. **§4.5, §3.2 C1 — reconcile the RRULE example with available capability.** The
   example `--repeat 'FREQ=WEEKLY;BYDAY=FR'` requires `BYDAY`, which the shipped
   hand-rolled `RecurrenceRule` (`Daily|Weekly|Monthly` + interval + count/until)
   cannot express and for which no RRULE crate is present. Either state that C's
   recurrence surface is limited to the existing frequency set until the Spike 1/B6
   adapter lands, or make that adapter a hard precondition in §7.4 and change the
   example accordingly.

### Priority 2 — Should fix for quality

1. **§4.4 or §4.6 — add the missing configuration contract (raises C3 from 1).** State
   where `todo.date_reminder_time` (default 09:00) and the recurrence materialization
   cap (default 100) live — `$XDG_CONFIG_HOME/mg-calr/config.toml`, `[todo]` table —
   their CLI > env > TOML > default precedence, and their validation error. Add a pure
   resolution test to §5.1.
2. **§5.1/§5.3 — add a redaction test (raises O1 toward 3).** Assert that a todo whose
   notes contain a secret produces an error envelope and a default `todo list` output
   containing neither the notes text nor the database URL.
3. **§5.2 — add a least-privilege test (raises O2 toward 3).** Assert C's commands
   succeed under a CRUD-only role and that DDL attempts surface the foundation storage
   error.
4. **§5 — cover `--scope`.** No test exercises `instance` / `future` /
   `instance-and-future`, despite §3.2 C2 defining three distinct rewrite semantics
   and asserting that materialized later occurrences are never silently rewritten.
5. **§4.4 — describe the projection-import write path** (`TodoCommand::Import`,
   `src/interop.rs`) so the "sole mutation authority" claim is complete.

### Priority 3 — Consider for excellence

1. **§4.2 — reserve an opaque property bag on todos** so F1/F2 can add VTODO
   unknown-property preservation without a schema migration, and soften §4.4/§7.5's
   absolute "never enter iCalendar" to a scope statement (raises I1 toward 3).
2. **§3.2 C2 / §7.4 — specify purged-identity tombstones.** Purge currently rejects
   history conflicts but the spec does not say whether a purged UUID is recorded to
   prevent reuse; align with the foundation's planned `purged_identities`.
3. **§7.5 / G4 — name a todo-graph integrity check** (orphaned occurrences, dangling
   template references, migration presence) with stable machine output (raises C5
   toward 3).
