# Spec: Todo Core and Graph

**Feature ID:** c-todo-core
**Parent feature:** C (C1–C8)
**Spec author agent:** Hermes Agent
**Date:** 2026-08-23
**Iteration:** 1

---

## 1. Purpose

### 1.1 One-sentence job

Let a user safely create, organize, recur, nest, block, remind, complete, trash, restore, and purge local todos through guided CLI flows or deterministic automation without corrupting task graphs or silently losing scheduled work.

### 1.2 Why it matters

`mg-calr` is intended to be the user's local daily-driver calendar and task authority. Todo core must match Taskwarrior-class organization and dependency semantics while remaining discoverable, keyboard-first, scriptable, and suitable for later agenda, reminder, TUI, and Quickshell clients. Tree parentage, dependency edges, recurrence, and reminder eligibility interact; specifying them as one transactional slice prevents impossible completion states, partial recurring graphs, and notifications for work that cannot yet be acted on.

### 1.3 Success signal

Against a disposable PostgreSQL database, CLI and application contract tests prove C1–C8 and Milestone 4 todo-core acceptance end to end: cycles and impossible completion edges are rejected deterministically, every scheduled recurrence occurrence is created as one complete graph or not at all, parent completion requires an explicit action after all descendants complete, and blocked todos remain query-visible while returning no reminder-delivery candidates.

### 1.4 Feature coverage

| Feature | Binding outcome in this spec |
|---|---|
| C1 | Guided creation, complete non-interactive flags, explicit reminder choice, and atomic validation/write |
| C2 | Inspect/edit/complete/reopen/trash/restore/restricted purge with stable selectors and audit |
| C3 | One optional project, tags, locked priority vocabulary, notes, and date-only/zoned due values |
| C4 | Schedule-anchored recurrence templates, bounded materialization, immutable instance history, atomic graph copy |
| C5 | Arbitrarily deep single-parent subtask tree with cycle-safe add/move/detach |
| C6 | Separate dependency DAG, deterministic cycle/deadlock rejection, visible derived blocking |
| C7 | All descendants must be complete; parent completion remains explicit and never propagates silently |
| C8 | Zero-or-many schedules, guided default of none, and suppression for blocked/completed/trashed todos |

---

## 2. User Stories

> As a keyboard-first user, I want guided todo creation to ask for missing fields, including whether I want reminders, so that I can capture work without memorizing flags.

> As an automation author, I want every promptable value to have a flag, `--no-input` to prohibit prompts, and versioned JSON on success and failure, so that scripts cannot hang or parse prose.

> As a planner, I want one optional project, tags, priority, notes, date-only or zoned timed due values, and stable selectors, so that I can organize and retrieve work without a terse modifier language.

> As a user with repeating routines, I want occurrences to follow the recurrence schedule rather than my completion time and to receive a fresh valid subtask/dependency graph, so that late completion does not drift or erase scheduled work.

> As a user managing complex work, I want arbitrary subtask depth and a separate dependency DAG, so that hierarchy and blocking express different relationships without allowing cycles or deadlocks.

> As a user completing a project, I want descendants to be complete before the parent can complete and still want to complete the parent explicitly, so that a checked child does not silently close its ancestors.

> As an accessibility or integration user, I want plain, color-independent blocked/completed/reminder states and the same public JSON/application contracts for CLI, agenda, TUI, and Quickshell clients, so that no database-only or pointer-only path becomes authoritative.

---

## 3. UX Specification

### 3.1 Screen / view inventory

This slice is terminal-only and introduces or modifies these command views:

- **Guided todo form** — `mg-calr todo add`; new sequential prompt flow, not a full-screen modal.
- **Todo detail** — `mg-calr todo show SELECTOR`; new read-only field, ancestry, descendant, dependency, recurrence, reminder, and audit-summary view.
- **Todo list** — `mg-calr todo list [FILTERS]`; new line-oriented list containing open, blocked, completed, or trashed items as explicitly filtered. Blocked items are never hidden by default.
- **Mutation result** — `todo edit|complete|reopen|trash|restore|purge`, `todo subtask ...`, `todo dependency ...`, and `todo recurrence materialize`; concise human confirmation or one JSON envelope.
- **Project inventory** — `mg-calr todo project create|list|rename|archive`; line-oriented project management. Archiving prevents new assignment but does not mutate existing todos.
- **Interactive chooser** — used only when input is allowed and a structured search returns multiple candidates. D10 owns the reusable chooser; until D10 lands, ambiguity fails without mutation.
- **Combined agenda projection** — no new renderer in C. C publishes `TodoAgendaItem` values consumed by D5; blocked tasks remain present with an explicit `blocked` state.

No graphical screen, modal, sheet, pointer interaction, sound, haptic, or animation is introduced.

### 3.2 Interaction flows

#### C1 — guided and non-interactive creation

1. `todo add` accepts `--title`, `--due`, `--timezone`, `--priority`, `--project`, repeated `--tag`, `--notes`, `--parent`, repeated `--depends-on`, `--repeat`, `--repeat-until`, repeated `--remind-before`, repeated `--remind-at`, `--no-reminder`, and `--no-input`.
2. With input allowed, omitted fields are prompted in this order: title; due kind/value and timezone if timed; priority; project; tags; notes; parent; prerequisites; recurrence; reminders. Optional prompts offer an explicit “none.” Reminder prompting is mandatory even though the default is no reminder.
3. `--no-input` never reads stdin. Missing title, recurrence anchor, timezone for an explicitly zoned ambiguous timestamp, or any other conditionally required value returns `input_required` with field names and exit 64. Optional omitted values take documented defaults: no due, priority `none`, no project/tags/notes/parent/dependencies/recurrence/reminders.
4. `--remind-before` and `--remind-at` may repeat. `--no-reminder` conflicts with either reminder flag. Duplicate reminder schedules are rejected before mutation.
5. A recurring todo requires a due anchor. Date-only anchors recur as civil dates; timed anchors store an IANA timezone and recur in that zone. Strict ISO forms and bounded shortcuts (`today`, `tomorrow`, `next fri`) are accepted; unconstrained natural language is rejected.
6. Selectors resolve before writing. Ambiguous selectors prompt only when input is allowed; with `--no-input` they return `selector_ambiguous` and candidates without mutation.
7. The application validates the prospective parent/dependency graph and writes the todo, relationships, recurrence template/first occurrence, reminders, and one audit transaction atomically.

Examples:

```text
mg-calr todo add
mg-calr todo add --title "Submit report" --due 2026-08-28T16:00 \
  --timezone America/Los_Angeles --priority high --project work \
  --tag finance --remind-before 30m --no-input
mg-calr todo add --title "Weekly review" --due "next fri" \
  --repeat 'FREQ=WEEKLY;BYDAY=FR' --no-reminder --no-input --json
```

#### C2/C3 — inspect, organize, edit, complete, trash, restore, purge

- `todo show` and `todo list` are non-mutating. List filters include `--status`, `--project`, `--tag`, `--priority`, `--due-before`, `--due-after`, `--blocked`, `--parent`, and `--include-trash`; D6/D7 later generalize filters/search.
- `todo edit SELECTOR` uses explicit set/clear pairs (`--title`, `--due`/`--clear-due`, `--project`/`--clear-project`, `--notes`/`--clear-notes`, `--add-tag`/`--remove-tag`, reminder add/remove flags). Empty title is invalid. Priority is exactly `none|low|medium|high|urgent`.
- Editing a recurring instance requires `--scope instance|future|instance-and-future` under `--no-input`; guided mode asks. `instance` never changes the template. `future` changes the template and unmaterialized occurrences only. `instance-and-future` changes the selected occurrence plus the template. Already materialized later occurrences are never silently rewritten; modifying them requires separate selected mutations or the later G1 bulk framework.
- `todo complete SELECTOR` is always an explicit mutation. It fails if the todo is blocked or has any incomplete live descendant. It never auto-completes ancestors. Repeating occurrence completion does not calculate a next due value from completion time.
- `todo reopen SELECTOR` fails if a live completed dependent or completed ancestor would become impossible. No dependent or ancestor is silently reopened.
- `todo trash SELECTOR` soft-deletes only the selected leaf and fails if it has live descendants or live dependents. It preserves audit, recurrence-instance, and reminder records. `todo restore` validates all graph invariants again before restoring.
- `todo purge SELECTOR --confirm SELECTOR` permanently deletes only an already-trashed todo that has no live/trashed descendants, dependency references, recurrence-template/history references, or undo eligibility. Selector text after `--confirm` must resolve to the same immutable UUID. Recursive/bulk purge belongs to G1 and is not implied by this command.
- Project rename preserves project UUID. Project archive does not detach todos. Tags use Unicode-preserving display text and a deterministic case-folded uniqueness key; assignment order does not affect JSON.

#### C4/C5/C6 — recurrence, nested subtasks, and dependencies

- Parentage is a tree: a todo has at most one parent and any depth. `todo subtask add PARENT [creation flags]`, `todo subtask move CHILD --parent PARENT`, and `todo subtask detach CHILD` use the same validation and prompt/JSON contracts as creation/editing.
- Dependencies are a separate directed graph where `dependent -> prerequisite`. `todo dependency add DEPENDENT --on PREREQUISITE` and `remove` never change parentage.
- An edge is rejected if it is self-referential, would form a dependency cycle, or names an ancestor as a prerequisite of its descendant. The last rule prevents the deadlock in which a parent cannot complete until its descendant completes while that descendant is blocked on the parent. Reparenting performs the same combined tree/DAG deadlock validation.
- A todo is blocked when it is open and at least one live prerequisite is not complete. Blocking is derived transitively for presentation but stored edges remain direct. A trashed prerequisite is not treated as completed; trash is prevented while live dependents exist.
- `todo recurrence materialize --through TIMESTAMP` is the explicit bounded maintenance command. It creates all due occurrence graphs through the inclusive bound, in schedule order, with one transaction per occurrence graph and a configurable command cap defaulting to 100 occurrences. Exceeding the cap returns `recurrence_limit` with the next cursor and no partial graph; already committed earlier occurrence graphs are reported. `--atomic-all` is intentionally absent because an unbounded multi-occurrence transaction is unsafe.
- Creation materializes the first scheduled graph. Later E2 reminder scanning will call the same application use case before selecting reminders; reads never create occurrences as a hidden side effect.
- Every occurrence graph receives fresh todo UUIDs and one occurrence UUID while retaining template-node IDs and scheduled anchors for history. Its parent edges are copied by mapping template-node IDs to fresh todo IDs. Internal dependency edges are copied through that map. Explicit external prerequisite IDs are copied only if the referenced todo still exists; purge is prohibited while a template references it.
- A unique `(recurrence_template_id, scheduled_anchor)` constraint makes retries idempotent. The transaction locks the template/cursor, computes the next schedule value from the template anchor/RRULE rather than wall-clock completion, validates the entire prospective graph, inserts all nodes/parent edges/dependencies/reminders/history/audit rows, advances the cursor, and commits. Any validation, SQL, serialization, or crash failure rolls back the entire occurrence graph and cursor.
- Template graph edits use `--scope future` or `instance-and-future`; they validate the complete template DAG in one transaction and affect only future materializations except for the explicitly selected current instance. Historical instances are immutable evidence except through ordinary instance-scoped edits.

#### C7/C8 — completion and reminder eligibility

- Parent completion checks every live descendant, not only immediate children. Descendant completion never propagates completion upward.
- A blocked todo cannot complete unless its prerequisites are completed first. `--force` is not provided.
- Each todo may have zero or more reminders. Relative reminders are defined against the due value. For a date-only due value, relative offsets use the configured local `todo.date_reminder_time` (default 09:00) and the todo's stored IANA zone; DST gap/fold resolution follows the same explicit temporal policy as B4 and never falls back silently to host-local interpretation. Absolute reminders store an instant and display zone.
- Reminder rows are schedules, not delivery claims. C exposes `eligible_todo_reminders(now, horizon)`. It excludes completed, trashed, or blocked todos and includes the suppression reason in inspection/JSON. It must not create `reminder_deliveries` for suppressed candidates. When a task becomes unblocked, E5 later decides whether a missed reminder is still relevant; C does not mark it delivered, dismissed, or lost.

### 3.3 Layout descriptions

Human list rows use a stable semantic order: short ID, completion marker, title, due value/zone, priority word, project, tags, and state words such as `BLOCKED (2 prerequisites)` or `RECURS`. Detail output is grouped as identity/status, organization, time/recurrence, hierarchy, dependencies, reminders, and history. Tree indentation is supplemental; every row also prints parent/depth data in JSON, and `--no-color` loses no meaning. Empty list output says `No todos match.` and JSON returns an empty array.

`TodoAgendaItem` contains immutable todo ID, short ID, title, due kind/value/zone, priority, project, tags, completion, blocked boolean, direct unmet prerequisite count, parent ID, depth, and recurrence occurrence identity. D5 may interleave it with events but must not infer or mutate domain state.

### 3.4 Input & gestures

All interactions are keyboard and stdin/argv based. `Ctrl-C` during prompts returns exit 130 before opening a write transaction or rolls back an open transaction. EOF returns `input_cancelled`. Prompts echo ordinary text; no secret input exists. Flags are accepted in any Clap-supported order, repeated values preserve semantic sets rather than display order, and `--` terminates option parsing. Narrow terminals wrap at word boundaries; machine output never wraps.

### 3.5 Transitions & animation

N/A — terminal commands have no navigation or in-view animation. Reduced-motion behavior is inherently satisfied.

### 3.6 Error states

| Trigger | Presentation | Recovery | Data loss risk |
|---|---|---|---|
| Required title/conditional flag missing under `--no-input` | `input_required`, exit 64, field list | provide flags or allow prompts | none |
| Invalid priority/date/time/timezone/RRULE/reminder | field-specific typed error, exit 65 | correct value | none |
| Missing or ambiguous selector | candidate-safe `not_found`/`selector_ambiguous`, exit 66 | use UUID/longer short ID or chooser | none; no mutation |
| Parent cycle, dependency cycle, or ancestry deadlock | `todo_graph_invalid` with edge/path, exit 65 | remove/reparent offending edge | none; transaction rollback |
| Complete blocked todo/incomplete parent | `todo_not_completable` with unmet IDs/counts, exit 65 | complete prerequisites/descendants | none |
| Recurrence graph validation/insert failure | `recurrence_materialization_failed` with template and scheduled anchor | fix template/reference and retry | none; graph and cursor rollback |
| Concurrent graph/template mutation | retry bounded three times, then `concurrent_update`, exit 75 | rerun after inspecting current JSON | none; optimistic version check/locks |
| Trash/purge/reopen would invalidate references/history | typed conflict, exit 65 | detach relationships or use eligible operation | none |
| Database unavailable/migration missing | foundation storage error, exit 69 | run doctor/migrate explicitly | none; no automatic migration |
| Prompt cancel/EOF | plain cancellation / JSON error, exit 130/64 | rerun | none |

Errors follow A4 envelopes and contain safe IDs and field names, never notes, database URLs, or full private task content by default. No error path performs a compensating partial delete.

### 3.7 Accessibility

- Prompts have unique textual labels, expected formats, defaults, and an explicit `none` option; no information depends on color, indentation, emoji, sound, or animation.
- Focus order is the prompt order in 3.2; all commands and recovery actions are keyboard invocable. No pointer-only interaction exists.
- Human rows spell out `BLOCKED`, `COMPLETE`, `TRASHED`, and reminder suppression reasons. ANSI is absent under `--no-color` or `NO_COLOR`.
- Output respects terminal width, does not overwrite prior lines, and remains usable with a screen reader. JSON gives equivalent semantic fields rather than preformatted glyphs.
- Later Quickshell/TUI clients must call public commands/application interfaces and expose pill/card, direct keybind, command-palette, and deep CLI/TUI actions. They must not query PostgreSQL directly or become the sole recovery path.

---

## 4. Implementation Specification

### 4.1 Architecture placement

Building on `a-foundation.md` and its implemented flat modules:

- `src/domain/todo.rs` (or the equivalent split from `src/domain.rs`): typed todo, project, tag, template, occurrence, due, priority, reminder, parent, dependency, and validation types; no SQL/CLI/render knowledge.
- `src/application/todo.rs`: command/query use cases, transaction boundaries, combined tree/DAG validation, recurrence materialization, blocked/completion rules, reminder eligibility, and audit events.
- `src/storage/todo.rs`: PostgreSQL repositories, recursive/iterative queries, locking, optimistic versions, and migration adapters.
- `src/cli/todo.rs`: Clap arguments, prompts, selector/chooser integration, and application request translation.
- `src/render/todo.rs`: human and stable JSON projections only; never business mutations.
- `migrations/0002_todo_core.sql`: todo-core tables/columns, constraints, indexes, and replacement/refinement of migration-1 todo scaffolding without destructive data loss.
- `tests/todo_cli.rs`, `tests/todo_postgres.rs`, and synthetic fixtures under `tests/fixtures/todos/`.

If the package remains flat when this slice begins, equivalent namespaced files are acceptable; the domain/application/storage/render authority boundaries are not.

### 4.2 Data model

Representative Rust domain types:

```rust
/// Immutable application-authority identity; selectors are never stored as authority.
pub struct TodoId(pub uuid::Uuid);
pub struct ProjectId(pub uuid::Uuid);
pub struct TagId(pub uuid::Uuid);
pub struct TodoTemplateId(pub uuid::Uuid);
pub struct TodoTemplateNodeId(pub uuid::Uuid);
pub struct TodoOccurrenceId(pub uuid::Uuid);

pub enum Priority { None, Low, Medium, High, Urgent }

/// Date-only values remain civil dates; timed values retain an instant and IANA zone.
pub enum TodoDue {
    Date { date: time::Date, zone: IanaZone },
    Timed { at: time::OffsetDateTime, zone: IanaZone },
}

pub struct Todo {
    pub id: TodoId,
    pub title: String,
    pub due: Option<TodoDue>,
    pub priority: Priority,
    pub project_id: Option<ProjectId>,
    pub notes: Option<String>,
    pub parent_id: Option<TodoId>,
    pub occurrence_id: Option<TodoOccurrenceId>,
    pub completed_at: Option<time::OffsetDateTime>,
    pub trashed_at: Option<time::OffsetDateTime>,
    pub version: i64,
}

/// Direct edge: dependent cannot complete while prerequisite is incomplete.
pub struct TodoDependency {
    pub dependent_id: TodoId,
    pub prerequisite_id: TodoId,
}

pub struct TodoReminderSchedule {
    pub id: ReminderId,
    pub todo_id: TodoId,
    pub schedule: ReminderSchedule,
}
```

Migration 2 must provide or normalize:

- `projects(id, name, normalized_name, archived_at, version, timestamps)` and `tags(id, name, normalized_name)` with deterministic uniqueness.
- `todos`: immutable UUID; title; mutually exclusive date-only/timed due representation; IANA zone; fixed priority check; optional project FK; notes; nullable parent FK; completion/trash timestamps; recurrence template-node/occurrence FKs; optimistic `version`; timestamps.
- `todo_tags(todo_id, tag_id)` unique join table.
- `todo_dependencies(dependent_id, prerequisite_id)` with unique edge and no-self check. Full DAG/combined-deadlock checks run under application transaction and are backed by deferred constraint triggers proven in Spike 5; application-only validation is insufficient.
- `todo_recurrence_templates`, `todo_template_nodes`, `todo_template_dependencies`, and `todo_occurrences`. Templates store an immutable anchor, IANA zone/date semantics, normalized recurrence expression, cursor/version, and active/retired state. Nodes store copied todo fields and parent template-node ID. Template dependencies target either a template node or an immutable external todo, never both.
- Unique `(template_id, scheduled_anchor)` occurrence key and unique `(occurrence_id, template_node_id)` instance mapping.
- Foundation `reminders` rows remain exactly-one-target schedules; todo reminder uniqueness prevents duplicate equivalent schedules.
- Audit rows use one transaction UUID and before/after JSON for every changed aggregate. Notes are included in protected local audit state but omitted from ordinary logs/errors.
- Indexes cover live status/due, parent, project, tag join, both dependency directions, occurrence/template mapping, normalized names, and eligible reminder joins.

Parent-cycle and DAG checks must be correct under concurrent transactions. The Spike 5 result chooses deferred triggers, serializable transaction/advisory aggregate locks, or an equally strong PostgreSQL design; implementation may not weaken this to a race-prone preflight query.

### 4.3 API contracts

Application interfaces are local Rust use cases, not network endpoints:

```rust
pub async fn create_todo(tx: &mut UnitOfWork, req: CreateTodo) -> Result<TodoView, TodoError>;
pub async fn edit_todo(tx: &mut UnitOfWork, id: TodoId, req: EditTodo) -> Result<TodoView, TodoError>;
pub async fn complete_todo(tx: &mut UnitOfWork, id: TodoId, expected_version: i64) -> Result<TodoView, TodoError>;
pub async fn mutate_relationship(tx: &mut UnitOfWork, req: RelationshipMutation) -> Result<GraphView, TodoError>;
pub async fn materialize_occurrences(tx_factory: &UnitOfWorkFactory, req: MaterializeRequest) -> Result<MaterializeReport, TodoError>;
pub async fn query_todos(repo: &dyn TodoReadRepository, filter: TodoFilter) -> Result<Page<TodoView>, TodoError>;
pub async fn agenda_todos(repo: &dyn TodoReadRepository, range: TimeRange) -> Result<Vec<TodoAgendaItem>, TodoError>;
pub async fn eligible_todo_reminders(repo: &dyn TodoReadRepository, range: TimeRange) -> Result<Vec<EligibleTodoReminder>, TodoError>;
```

All mutations require a resolved immutable UUID and expected version internally. CLI selectors are resolved before the use case. List JSON is deterministically ordered by due-nullness, due value, descending priority, normalized title, then UUID; pagination uses an opaque stable cursor and defaults to 100/max 1000. No rate limiting or authentication applies to this local process, but the PostgreSQL role remains unprivileged application authority.

JSON uses the A4 envelope `{schema_version:1, command, ok:true, data}` or the stable error envelope on stderr. Todo objects include full UUID, short ID, version, fields, direct relationship IDs, computed `blocked`, unmet prerequisite IDs, `completable`, reminder schedules plus suppression status, recurrence template/occurrence IDs, and timestamps. Notes appear only in detail output by default; list/agenda JSON require `--include-notes`. Set-like arrays are sorted by immutable ID or normalized name. JSON never contains ANSI or localized dates.

### 4.4 State management and authority

PostgreSQL 18-compatible local storage is the sole authority for todos, projects, tags, graph edges, templates, occurrences, reminder schedules, and audit state. The application layer is the sole mutation authority and owns transaction boundaries. Domain validation is pure; storage enforces durable constraints; CLI/render are adapters.

Todos are local-only and never enter iCalendar, the durable vdir mirror, vdirsyncer, iCloud, or conflict reconciliation. No C command initializes a network client. Database access occurs only as part of the explicit local command being run. Later reminder services and UI clients consume application/public JSON contracts and never write tables directly. Reads do not materialize recurrence or mutate delivery state.

### 4.5 Dependencies

Required predecessors are A1–A5 contracts, B4 temporal policy for zoned due/DST behavior, the recurrence parser/expander decision from Spike 1/B6 where reused, and successful PostgreSQL task-graph Spike 5. Prefer existing package dependencies; recurrence must use the accepted adapter rather than introduce a second RRULE implementation. No HTTP, CalDAV, iCalendar sync, vdirsyncer, DBus, Quickshell, or notification-backend dependency is permitted.

### 4.6 Platform-specific considerations

Arch/Hyprland is the first supported environment, but todo domain/application/storage behavior is Linux-desktop agnostic. Timezone resolution uses IANA data and stores the chosen zone; it never assumes `/etc/localtime` after creation. Terminal color honors `--no-color` and `NO_COLOR`. No feature flag may create an alternate task authority. PostgreSQL 18 is the target; integration tests document the minimum compatible PostgreSQL version if lower versions are later claimed.

### 4.7 Performance budget

- List/agenda queries paginate and use indexed due/status joins; on the synthetic 100,000-todo fixture, first-page p95 is under 150 ms on CI reference hardware and `EXPLAIN` shows no unbounded sequential scan over dependency edges.
- Blocked-state evaluation for a page is set-based, not N+1. Graph validation is `O(V+E)` in affected connected components and iterative, so arbitrary depth cannot overflow the Rust stack.
- A 1,000-node/5,000-edge occurrence graph materializes in one transaction with bounded memory under 64 MiB above process baseline and p95 under 2 seconds on reference CI PostgreSQL; rollback leaves zero rows for its occurrence ID.
- `todo show` and single-item mutations target under 100 ms p95 excluding process startup/database connection on reference CI hardware.
- Storage is proportional to materialized occurrences and audit history. No unbounded future occurrence materialization, daemon, network payload, or startup cache is introduced.
- Benchmarks are non-flaky informational gates until reference hardware is recorded; query-plan, boundedness, no-N+1, and atomicity assertions are mandatory correctness gates.

---

## 5. Test Specification

### 5.1 Unit tests

- `priority_accepts_only_locked_vocabulary`: parse each allowed value and reject unknown/case-ambiguous inputs.
- `due_round_trips_date_and_zoned_time`: preserve date-only values and timed IANA zone across DST; include gap/fold fixtures from B4.
- `bounded_shortcuts_are_deterministic`: inject clock/zone and test today, tomorrow, next Friday; reject free prose.
- `parent_cycle_returns_path`: build a deep tree, attempt ancestor reparent, assert stable `todo_graph_invalid` and cycle path.
- `dependency_cycle_returns_path`: add a closing edge to a DAG and assert deterministic canonical path independent of insertion order.
- `ancestor_prerequisite_deadlock_rejected`: reject descendant depending on ancestor; permit parent depending on completed/open descendant subject to normal blocking.
- `blocked_is_transitive_but_edges_are_direct`: derive blocked state through prerequisites without inventing persisted transitive edges.
- `parent_requires_all_descendants_and_explicit_action`: completing final descendant leaves every ancestor open; parent completion then succeeds.
- `reopen_rejects_completed_dependent_or_ancestor_conflict`: no silent propagation.
- `recurrence_uses_schedule_anchor_not_completion_clock`: late completion/materialization retains RRULE-derived anchors, including DST vectors.
- `template_graph_copy_remaps_internal_edges`: every node gets fresh UUID; parent/internal dependencies map correctly; external prerequisite stays stable.
- `reminder_eligibility_suppresses_blocked_complete_trash`: schedules remain inspectable while eligible output omits them and states reason.
- `json_order_is_deterministic`: golden object/array ordering, null/date/timestamp representation, suppression fields, and error codes.
- Property tests generate parent trees plus dependency DAGs, apply valid/invalid edge/reparent operations, and assert acyclicity, no ancestry deadlock, and completion invariants.

### 5.2 Integration tests

All PostgreSQL tests require the foundation's explicit disposable opt-in and safety-name check.

- Create the same todo through guided simulated stdin and all flags; assert equivalent domain rows/audit and that `--no-input` never reads stdin.
- CRUD round trip project/tags/notes/due/priority/reminders; soft-delete/restore; purge eligibility; verify UUID and audit provenance stability.
- Race two parent/dependency edits that are individually valid but jointly cyclic; exactly one commits and the final graph is valid.
- Race completion against prerequisite/reparent mutation; assert serializable valid outcome and no impossible completed state.
- Materialize a multi-level recurring template with internal and external dependencies; assert fresh IDs, scheduled-anchor history, copied reminders, and unique idempotent retry.
- Inject failure after node, parent-edge, dependency, reminder, and audit insertion stages; each rollback leaves no partial occurrence rows and does not advance the cursor.
- Kill the client connection during materialization, reconnect, retry, and assert one complete occurrence graph.
- Materialize through a bound with more than the cap; assert committed complete graphs are reported, next cursor is exact, and no partial graph exists.
- Query blocked todos through list and `agenda_todos`; assert they remain visible while `eligible_todo_reminders` yields no candidate/delivery row.
- Assert every non-sync todo command opens no network socket/transport adapter and never touches vdir/sync tables.
- Migration test upgrades migration-1 scaffolding with synthetic rows without destructive loss and reruns idempotently.

### 5.3 UI / E2E tests

Process-level CLI tests cover:

1. `todo add` guided prompt order, explicit no-reminder answer, cancellation, narrow terminal, and no ANSI under both color controls.
2. Complete a child chain, verify parents remain open, explicitly complete parents, and compare human/JSON states.
3. Attempt parent/dependency cycles, ambiguous selector mutation, blocked completion, unsafe trash, and purge without exact confirmation; verify nonzero exits and unchanged database snapshots.
4. Create/materialize recurrence, inspect template/occurrence history, and compare two retries for idempotent JSON.
5. Golden `todo show`, empty/populated `todo list`, project inventory, blocked agenda projection, and stable stdout/stderr separation.

A later D/I test suite owns full combined agenda rendering and graphical TUI/Quickshell interaction; C contract fixtures are reused there.

### 5.4 Visual / manual verification

- Inspect light/dark terminal themes only to confirm no state relies on foreground color; `--no-color` is normative.
- Test 40-, 80-, and 160-column terminals, long Unicode titles/tags, combining characters, and 1,000-level nesting without stack overflow.
- Test screen-reader-friendly linear output, prompt labels, empty/populated/blocked/completed/trashed states, and JSON equivalence.
- Verify `--help` documents all prompt flags, set/clear pairs, recurrence scope, selector ambiguity, reminder defaults, and destructive confirmation.

---

## 6. Compliance & Safety Gate

### 6.1 Sensitive data classification

- [ ] No sensitive data involvement
- [x] Handles sensitive data — titles, notes, projects, tags, due dates, dependency relationships, and reminder schedules may expose personal or work plans. They remain in the local PostgreSQL authority/audit store, use least-privilege peer authentication, are omitted or minimized in logs/errors/list JSON, and never sync to iCloud.
- [x] Uses synthetic/test data only until compliance gate clears

No plaintext credential field exists. Database URLs use A-foundation redaction. Crash reports, fixtures, and benchmarks contain synthetic content only.

### 6.2 Asset provenance

- [x] No third-party assets
- [ ] Uses third-party assets

Crate licenses remain subject to the repository-wide dependency/license gate; this slice adds no images, fonts, models, external datasets, or service content.

### 6.3 Language / claims audit

- [x] No unsupported implementation claim is made; Section 7 marks todo core absent.
- [x] User-facing capability statements are target-state requirements, not claims that the current binary already supports them.
- [x] No regulated medical, legal, financial, or safety claim is introduced.

### 6.4 Regulatory alignment

- **I1 Lossless iCalendar:** N/A for local-only todos; no todo is encoded as iCalendar and the design does not modify the accepted event codec boundary.
- **I2 Sync authority:** PostgreSQL is explicitly the only todo authority; no competing vdir or client store is introduced.
- **I3 Conflict/deletion:** N/A for remote reconciliation because todos never sync. Local soft-delete, restore, purge preconditions, immutable identity, recurrence history, and audit prevent silent local loss.
- **I4 Scope/network:** every C path is local and constructs no network/sync transport. Contract tests enforce no network access and later clients use public interfaces.

Criteria alignment also covers T1 immutable typed IDs; T2 date/timed zone and schedule semantics; T3 transactional graph/recurrence writes plus concurrency tests; T4 soft delete/audit/purge; T5 suppression without delivery claims; C1 guided recovery; C2 flags/`--no-input`/versioned JSON; C4 color-independent accessible output; O1 redaction; O2 unprivileged role; O3 typed atomic failures; and O4 unit/property/contract/isolated integration gates. Any design allowing silent todo loss, recurrence corruption, timezone/DST drift, duplicate reminder claims, hidden network access, plaintext secrets, or partial recurring graphs fails this spec.

### 6.5 Operational security

- Mutations run under the unprivileged application role and never invoke sudo, shell commands, a notification backend, or a network client.
- SQL uses typed parameters; notes, titles, selectors, and recurrence text are data, never executable SQL or shell fragments.
- Logs and default list/agenda output minimize todo content; database URLs and credentials use A-foundation redaction, and JSON errors expose only safe identifiers/field names.
- Recursive graph and recurrence inputs are handled iteratively with transaction, occurrence-count, pagination, and memory bounds to resist stack/CPU/memory exhaustion. Exact business depth remains arbitrary rather than silently truncated.
- Purge requires exact confirmation and rejects audit/history/reference conflicts. There is no force bypass for graph, completion, authority, or recurrence integrity.

---

## 7. Gap Analysis vs. Current State

### 7.1 What exists today

**Implemented foundation:** `docs/specs/a-foundation.md` records typed `TodoId`/`ReminderId`, PostgreSQL connection/migration contracts, migration-1 `todos`, `todo_dependencies`, `reminders`, `reminder_deliveries`, audit scaffolding, A4 JSON/error envelopes, and isolated database-test conventions.

**Absent todo behavior:** the same foundation spec explicitly says DAG cycle rejection, parent completion, recurrence, scanner behavior, and functional audit/undo remain later slices; its gap analysis states all todo workflows are absent. No evidence supplied to this spec shows C1–C8 commands, application use cases, graph enforcement, recurrence materialization, organization tables, or reminder eligibility implemented.

### 7.2 Delta to spec

- Add todo domain/application/storage/CLI/render modules and public query/mutation contracts.
- Add Migration 2 for projects, tags, due/organization fields, template/occurrence graph, durable concurrency constraints, indexes, and non-destructive scaffold upgrade.
- Implement guided prompts, complete flags, selectors, `--no-input`, recurrence scope, stable human/JSON output, and destructive confirmations.
- Implement transactional CRUD, soft delete/restore/restricted purge, audit, arbitrary parent tree, separate dependency DAG, combined deadlock checks, blocked/completion rules, and optimistic concurrency.
- Implement bounded RRULE-based template materialization with atomic graph copy, history, idempotent keys, and crash recovery.
- Implement todo reminder schedule CRUD, suppression/eligibility query, and `TodoAgendaItem` projection.
- Add unit/property/process/disposable-PostgreSQL tests, synthetic fixtures, fault injection, concurrency tests, query-plan checks, and documentation/help.
- No new production network, sync, notification, graphical, or credential integration is needed.

### 7.3 Estimated scope

**XL.** C1–C8 form a tightly coupled aggregate spanning schema evolution, transactional graph theory, recurrence/time semantics, CLI prompting/automation, stable projections, concurrency, and fault-injection tests. Implement in TDD sub-slices (basic CRUD/organization; tree; DAG/deadlock/completion; recurrence template/materialization; reminder eligibility; CLI/golden integration) while preserving this single aggregate contract.

### 7.4 Blocking dependencies

- A1–A5 must supply configuration, migrations, typed IDs/selectors, error/JSON envelopes, and transactional audit authority; A3 short-ID resolution and A5 functional audit behavior cannot remain scaffold-only for final C acceptance.
- B4's accepted IANA timezone/DST gap/fold policy must be reusable for timed/date reminder semantics.
- Spike 1/B6 must select or define the normalized recurrence adapter if todo recurrence shares RRULE parsing/expansion; C must not create a competing implementation.
- Required Spike 5 must prove PostgreSQL designs for arbitrary nesting, cycle rejection, parent completion, atomic graph copy, and concurrent edits before production migration constraints are finalized.
- D5 consumes `TodoAgendaItem` for the polished combined event/todo view; E1–E8 consume reminder eligibility and own delivery claims, scanner, catch-up, DND, snooze/dismiss, action service, and notification backends.
- G1/G2/G5/G6 own bulk dry-run, targeted undo policy, migration rollback/compatibility, and broader interrupted-operation recovery; this slice still guarantees atomic local writes and safe retry.

### 7.5 Non-goals

- Event/calendar CRUD, full combined-view rendering, generalized full-text search/filter language, and interactive chooser implementation remain B/D responsibilities; C only provides todo filters, selectors, and the combined-view projection seam required by Milestone 4.
- Notification delivery claims, systemd scanner/timer, action callbacks, snooze/dismiss, sleep catch-up, DND, and backend adapters remain E responsibilities. C owns schedules and suppression eligibility only.
- iCalendar/JSON interchange, CalDAV/vdirsyncer/iCloud sync, invitations, and any network transport are excluded; todos remain local-only.
- Bulk recursive mutation/dry-run, targeted undo eligibility, backup/restore, general doctor, and migration rollback policy remain G responsibilities.
- Full-screen TUI and Quickshell pill/card implementation remain deferred I1/I2 consumers of the public contracts. Direct database access by either is forbidden.
- File attachments, unconstrained natural-language parsing, automatic ancestor completion, force-completion, recursive purge, and unbounded future occurrence materialization are not v1 todo-core capabilities.

---

## 8. Open Questions

N/A — locked product decisions and this spec resolve C1–C8 behavior. Implementation choices for the PostgreSQL constraint mechanism and recurrence crate are evidence-gated by Spikes 5 and 1 respectively; those gates may select an equivalent mechanism but may not weaken the invariants, atomicity, CLI, or authority contracts above.
