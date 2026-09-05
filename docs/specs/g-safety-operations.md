# Spec: Safety and Operations

**Feature ID:** g-safety-operations
**Parent feature:** root
**Spec author agent:** Spec agent G
**Date:** 2026-08-29
**Iteration:** 1

---

## 1. Purpose

### 1.1 One-sentence job

Give the user a way to change many calendar records at once, take back a specific change they regret, keep a restorable copy of everything, diagnose a broken installation, upgrade schema without corrupting data, and recover cleanly from a crash — with no path in any of those that can lose or overwrite data without the user seeing and confirming it first.

### 1.2 Why it matters

Every other feature in `mg-calr` mutates the user's only copy of their schedule. Feature B can trash a recurring master, feature C can complete a subtree, feature F can reconcile a remote proposal, and a migration can rewrite a column. Those features each defend their own transaction, but nothing today defends the *operator* moment: a mistyped filter that matches 400 events, an upgrade applied to the wrong database, a `pg_dump` nobody ever restored, a half-finished batch left behind by a laptop suspend. The product's binding auto-fail rules — **silent event/todo loss**, **unconfirmed overwrite**, and **recurrence/exception corruption** — are exactly the failures that appear at scale and during operations rather than during single-item CRUD. G is the feature that makes those three failures structurally unreachable: no bulk mutation exists that is not dry-run-first, no confirmation can be assumed, no undo is broader than one recorded transaction, no diagnostic writes, and no migration can be applied by a binary that does not understand the schema in front of it.

### 1.3 Success signal

Against a disposable PostgreSQL database seeded with synthetic calendars, events, recurring masters with exceptions, and a stored todo projection: every destructive command run with `--no-input` and no `--yes` exits `confirmation_required` with a byte-identical database and projection file; every bulk plan applied after an intervening edit exits `plan_stale` with zero writes; `doctor` leaves a byte-identical `pg_dump` and never creates the migration ledger; a backup archive with one flipped byte fails `backup verify`; and a binary embedding schema version 5 refuses every mutating command against a database recorded at schema version 99 while still permitting `backup create` and `doctor`.

### 1.4 Feature and milestone boundary

G specifies the complete target contract for G1–G6, delivered in dependency order rather than at once:

- **Prerequisite (feature A5):** functional audit transactions. `audit_log` exists as a table but nothing writes it; G2 undo and G1 chunk provenance are unimplementable until A5 records before/after state under a transaction ID.
- **Milestone G-a (operations floor):** G4 doctor, G5 schema compatibility guard and migration classification, G6 temp-state recovery and crash reporting. These depend only on what exists today plus a small migration, and they are the cheapest defense against operational corruption.
- **Milestone G-b (data custody):** G3 backup, verification, retention, and restore.
- **Milestone G-c (bulk and undo):** G1 dry-run bulk framework and G2 targeted undo, gated on A5 audit and on the B/C aggregate mutation APIs that bulk must call rather than bypass.

G owns no domain semantics. It never writes SQL against `events`, `todos`, exceptions, or reminder rows directly; it composes the application use cases owned by B, C, E, and F. G adds no network client, no daemon, and no privileged operation.

---

## 2. User Stories

> As a keyboard-first user, I want a bulk retag or bulk trash to show me the exact list of affected items and their before/after values before anything is written, so that a wrong filter costs me a glance instead of my calendar.

> As a script author, I want `--no-input` to refuse an unconfirmed destructive command rather than assume yes, and I want a stable plan ID and fingerprint I can pass to the apply step, so that automation is explicit and a plan that drifted cannot be applied blind.

> As a user who just made a mistake, I want to undo one named transaction — the one I can see in `history` — rather than "roll back the last five minutes", so that unrelated work I did afterwards survives.

> As a user with recurring events, I want a bulk operation to refuse to touch a recurring master until I state a scope, so that a batch edit cannot silently rewrite an RRULE or destroy an occurrence exception.

> As an operator, I want a backup whose integrity I can prove by restoring it into a scratch database, and a restore that refuses to overwrite a populated database without explicit acknowledgement, so that "we had backups" is a verified statement.

> As someone whose PostgreSQL is misconfigured, I want `doctor` to tell me exactly what is missing and print the administrator commands without ever running them, changing my server, or asking for sudo, so that diagnosis is always safe to run.

> As a user upgrading or downgrading `mg-calr`, I want an older binary to refuse a newer database rather than write to it, and I want a documented rollback path, so that a version mismatch is an error message instead of data corruption.

> As a screen-reader user, I want dry-run diffs, doctor results, and confirmation prompts to communicate every state in words with a deterministic reading order, so that no decision depends on seeing a color.

---

## 3. UX Specification

### 3.1 Screen / view inventory

Terminal-only. G introduces command views, not graphical screens.

| View | Invocation | Status | Layout |
|---|---|---|---|
| Bulk plan (dry-run diff) | `mg-calr bulk <VERB> …` | New | grouped diff sections + summary footer + plan header |
| Bulk apply confirmation | `mg-calr bulk apply --plan …` | New | re-printed summary + single confirmation line |
| Plan registry | `mg-calr bulk plan list|show|discard` | New | compact table / labeled record |
| Transaction history | `mg-calr history list|show` | New | reverse-chronological table / labeled record with per-entity diff |
| Undo plan and apply | `mg-calr undo …`, `mg-calr undo apply …` | New | reuses the bulk plan and confirmation layout |
| Backup create / list | `mg-calr backup create|list` | New | progress lines + manifest summary / table |
| Backup verify | `mg-calr backup verify …` | New | per-stage check list, same row shape as doctor |
| Backup restore | `mg-calr backup restore …` | New | target summary + irreversibility notice + confirmation |
| Backup prune | `mg-calr backup prune …` | New | dry-run plan of archives to remove + confirmation |
| Doctor report | `mg-calr doctor …` | **Modified** (exists as a migration-status alias) | ordered check rows + prerequisite matrix + remediation block |
| Migration preview | `mg-calr database migrate --dry-run` | **Modified** | pending version table with kind and rollback note |
| Recovery status | `mg-calr recovery status` | New | sections for in-flight locks, partial runs, orphaned artifacts |
| Recovery clean / resume / abandon | `mg-calr recovery clean|resume|abandon` | New | dry-run list + confirmation |

Command grammar (global `--json`, `--no-color`, `--no-input`, `--database-url` remain foundation flags):

```text
mg-calr bulk (edit|move|trash|restore|complete|retag|purge)
    [--filter EXPR] [--calendar CALENDAR_ID] [--project NAME] [--ids-from FILE]
    [mutation flags for the verb]
    [--scope occurrence|future|series] [--limit N]
    [--plan-out FILE] [--json]
mg-calr bulk apply --plan PLAN_ID --plan-fingerprint SHA256
    [--chunk-size N] [--operation-id UUID] [--yes] [--acknowledge-irreversible] [--no-input] [--json]
mg-calr bulk plan list [--json] | show PLAN_ID [--json] | discard PLAN_ID [--yes]

mg-calr history list [--since TS] [--until TS] [--entity ID] [--limit N] [--json]
mg-calr history show TRANSACTION_ID [--json]
mg-calr undo --transaction TRANSACTION_ID [--plan-out FILE] [--json]
mg-calr undo apply --plan PLAN_ID --plan-fingerprint SHA256 [--yes] [--no-input] [--json]

mg-calr backup create [--output-dir DIR] [--label TEXT] [--allow-insecure-permissions] [--json]
mg-calr backup list [--output-dir DIR] [--json]
mg-calr backup verify --archive PATH [--scratch-database-url URL] [--drop-scratch --yes] [--json]
mg-calr backup restore --archive PATH --target-database-url URL
    [--acknowledge-overwrite] [--yes] [--no-input] [--json]
mg-calr backup prune [--output-dir DIR] [--keep N] [--keep-days D] [--yes] [--no-input] [--json]

mg-calr doctor [--component COMPONENT] [--check CHECK_ID]... [--quick]
    [--fail-on never|warn|fail] [--explain CHECK_ID] [--json]
mg-calr database migrate [--dry-run] [--backup-archive PATH]
    [--acknowledge-no-backup] [--yes] [--no-input] [--json]

mg-calr recovery status [--json]
mg-calr recovery clean [--older-than DURATION] [--include-lock-files] [--yes] [--no-input] [--json]
mg-calr recovery resume --run RUN_ID --plan-fingerprint SHA256 [--yes] [--no-input] [--json]
mg-calr recovery abandon --run RUN_ID [--yes] [--no-input] [--json]
```

Binding grammar rules:

- **Dry-run is not a flag; it is the only first phase.** `bulk <VERB>` and `undo --transaction` *compute and print a plan and exit 0 without writing*. There is no `--apply` shortcut on the verb. Writing requires the separate `bulk apply` / `undo apply` command naming an existing plan ID **and** its fingerprint.
- `--yes` confirms only the fully resolved plan already printed, identified by ID and fingerprint. It never resolves an ambiguous selector, never waives a fingerprint check, and never substitutes for `--acknowledge-irreversible` or `--acknowledge-overwrite`.
- **`--no-input` refuses; it never assumes yes.** Any command that would prompt returns `confirmation_required` (exit 64), names the exact flags that would satisfy it, and performs zero writes.
- Every prompt has a corresponding flag; every flag is documented in `--help` with an example.
- `--json` emits exactly one foundation envelope (`{schema_version, command, ok, data}`) on stdout; prompts and progress go to stderr or the controlling terminal.
- Selectors resolve to a frozen set at plan time. Apply re-resolves and compares; it never re-runs the filter to pick up new matches.

### 3.2 Interaction flows

**G1 — bulk plan then apply**

1. Parse flags locally. Resolve configuration. Probe schema compatibility (§G5) before any other query; a pending or ahead schema stops here.
2. Resolve the selector to an ordered set of aggregate IDs with current revisions. Refuse a selector matching zero items (`selector_empty`) and one exceeding `--limit` (default 5000, hard cap 50000) with `bulk_limit_exceeded`.
3. Refuse any item owned by the stored todo projection with `projection_read_only`, naming the `mg-remindr` command that owns it. `mg-calr` never mutates projected todos.
4. For each item, call the owning feature's *validation-only* patch path to compute the after-state. A recurring master or occurrence exception in the set requires an explicit `--scope`; otherwise `recurrence_scope_required`, no plan is stored.
5. Persist the plan: `bulk_plans` row plus one `bulk_plan_items` row per item, with `expires_at` (default 15 minutes, configurable), status `open`, and a fingerprint = SHA-256 over the canonical JSON array of sorted `(entity_type, entity_id, expected_revision, action, after_state_digest)` tuples.
6. Render the diff (§3.3) and the exact apply command including plan ID and fingerprint. Exit 0. **Nothing has been mutated.**
7. `bulk apply` loads the plan, refuses a plan that is expired/applied/discarded, then re-reads every item under one transaction and recomputes the fingerprint. Any divergence — revision changed, item deleted, membership changed — returns `plan_stale` listing the diverged IDs and writes nothing.
8. Confirmation: interactive mode re-prints the summary and asks `Apply N changes to M items? [y/N]`; irreversible verbs (`purge`) require the typed phrase printed in the plan. `--no-input` requires `--yes` (plus `--acknowledge-irreversible` for `purge`).
9. Apply. With `--chunk-size 0` (default) the whole plan commits in one serializable transaction. A non-zero chunk size creates a `bulk_runs` row and one `bulk_run_chunks` row per chunk; each chunk is its own transaction with its own audit transaction ID, recorded before the next chunk begins. A chunk failure stops the run, leaves committed chunks intact, and reports the run ID plus per-chunk undo commands. **Partial application is always recorded and reported; it is never silent.**
10. Mark the plan `applied`, print the result summary, and emit each chunk's transaction ID so G2 can undo it.

**G2 — targeted undo**

1. `history list` shows recorded transactions (ID, time, command, item count, entity kinds, undo eligibility) newest first.
2. `history show TXN` shows every affected entity with before/after values from the audit rows.
3. `undo --transaction TXN` builds a *compensating forward plan* using the same plan machinery: for each audit row, restore `before_state` conditional on the entity's current revision equalling that row's `after_revision`. It prints the plan and exits without writing.
4. Eligibility is evaluated per entity and reported per entity: `eligible`; `undo_stale` when the entity changed after the transaction (the intervening transaction ID is named); `undo_irreversible` when the entity was purged; `unsupported` when the transaction is a schema migration, a restore, or an operation with no recorded before-state. A plan containing any ineligible entity refuses as a whole unless `--skip-ineligible` is passed, which prints exactly what will be skipped.
5. `undo apply` confirms and commits one transaction that **increments revisions and writes new audit rows** referencing `undoes_transaction_id`. It never deletes audit history, never lowers a revision, and never rewrites a UID.
6. Undoing an undo is `undo --transaction <UNDO_TXN>` and is the redo path.
7. There is no `undo --all`, `undo --since`, or point-in-time rollback. Blanket recovery is `backup restore`, which is a different, louder command.

**G3 — backup, verify, restore, prune**

1. `backup create` resolves the backup root (`--output-dir`, else `[backup].dir`, else `$XDG_STATE_HOME/mg-calr/backups`), creates `mg-calr-backup-<UTC>-<short>.part/` with mode 0700, and refuses a group/world-writable parent unless `--allow-insecure-permissions`.
2. It runs `pg_dump --format=custom --no-owner --no-privileges --serializable-deferrable` into `database.dump`, copies the stored todo projection if present, and writes `config.toml.redacted` with any connection URL credential replaced by `REDACTED`.
3. It writes `manifest.json` (schema version, migration ledger snapshot, per-file SHA-256 and byte length, per-table row counts, content fingerprint, mg-calr / pg_dump / server versions, UTC timestamp) and `MANIFEST.sha256`, fsyncs every file, fsyncs the directory, then renames `.part` → final name and fsyncs the parent. A crash before the rename leaves a `.part` directory that `backup list` never shows and `restore` never accepts.
4. `backup verify` recomputes every checksum, parses the dump with `pg_restore --list`, and — when `--scratch-database-url` names an existing empty database whose name contains `mg_calr_verify` — restores into it with `pg_restore --single-transaction --exit-on-error` and runs the invariant suite (ledger equals manifest, row counts equal manifest, FK integrity, one live default calendar, `rfc_uid` uniqueness, reminder single-target, delivery `(reminder_id, scheduled_for)` uniqueness, no orphaned exceptions, content fingerprint match). Without a scratch URL it reports `verified: "checksums_only"` and prints the exact `createdb` command for the operator. **mg-calr never creates or drops a database on its own.**
5. `backup restore` requires `--archive` and an explicit `--target-database-url`; there is no default target. It verifies checksums first, probes the target for `mg_calr_schema_migrations`, and refuses a populated target without `--acknowledge-overwrite`. It restores with `pg_restore --single-transaction --exit-on-error`, so any failure or crash leaves the target unchanged. It then runs the invariant suite and prints the result. It never runs `database migrate` implicitly.
6. `backup prune` is dry-run-first like a bulk plan: it lists exactly which archives would be removed under `--keep`/`--keep-days`, and removes them only after confirmation. It never removes the newest archive, never removes an archive that fails to parse (corrupt archives are reported and kept), never removes anything outside the resolved backup root, and never follows a symlink out of it. Retention never runs automatically as a side effect of another command.

**G4 — doctor**

1. `doctor` connects once, issues `SET TRANSACTION READ ONLY`, and runs the check matrix in fixed order. It never calls the ledger-creating path, never applies a migration, and never writes a file.
2. Each check yields `pass | warn | fail | skipped | not_applicable`, a stable `check_id`, a human title, a detail line, and a remediation block tagged `user_command`, `administrator_command`, or `documentation`.
3. Administrator remediations are **printed, never executed**: `sudo -u postgres createuser --login "$USER"` appears as text with the standing note that mg-calr never invokes sudo.
4. Exit code: 0 when no check failed; 69 when any check is `fail`; `--fail-on warn` promotes warnings; `--fail-on never` always exits 0 for pipelines that only want the JSON.
5. `doctor --explain CHECK_ID` prints the check's rationale and remediation without connecting when the check is local.
6. **Doctor never repairs.** Each remediation names a separate explicit command: `database migrate`, `backup restore`, `recovery clean`, `recovery resume`, or an administrator step.

**G5 — migration compatibility and rollback**

1. Every command that opens a connection first reads `mg_calr_schema_metadata` (falling back to `max(version)` of the ledger for databases predating it).
2. `db_version > EMBEDDED_SCHEMA_VERSION` → `schema_ahead`, exit 69. All mutating commands refuse. Only `version`, `config paths`, `doctor`, `backup create`, and `backup verify` remain available, so the user can preserve data and diagnose before changing binaries.
3. `db_version < EMBEDDED_SCHEMA_VERSION` → read-only commands proceed with a stderr `migration_pending` warning; every mutation refuses with `migration_pending` and names `mg-calr database migrate`.
4. `db_version < MIN_SUPPORTED_SCHEMA_VERSION` → `schema_unsupported`; only `doctor` and `backup create` proceed; the documented path is to install the intermediate release that can migrate.
5. `database migrate --dry-run` lists pending versions with `kind` (`additive`, `rewriting`, `destructive`), the minimum binary each requires, and the rollback note. A `rewriting`/`destructive` migration requires `--backup-archive PATH` whose manifest verifies, or `--acknowledge-no-backup --yes`. `--no-input` without one of those returns `confirmation_required`.
6. **Rollback policy:** migrations are forward-only and append-only. No down-SQL and no `database rollback` command exists. Rollback = stop the newer binary, `backup restore` the verified pre-migration archive into the database, reinstall the older binary. Migration authors must keep migration N readable by N-1's data shape (additive columns, `IF NOT EXISTS`, no destructive rewrite outside an explicit gate), which migrations 0002–0005 already do.

**G6 — crash recovery**

1. `recovery status` reports three sections and exits 0 even when it finds work to do (it is diagnostic): in-flight database state (whether the migration advisory lock is currently held, and by a live backend), partial bulk runs, and orphaned local artifacts.
2. In-flight transactions need no user action: every mg-calr mutation is one transaction and the migration lock is `pg_advisory_xact_lock`, which PostgreSQL releases when the backend terminates. There is no manual unlock step and the spec forbids introducing a session-scoped lock.
3. Orphaned artifacts are only the classes mg-calr creates: projection temp files, projection lock files, `.part` backup directories, exported plan files. `recovery clean` removes only files matching those owned patterns, inside the resolved XDG roots or backup root, that are not symlinks, not flock-held by a live process, and older than `--older-than` (default 24h) — after a printed list and confirmation. Lock files are kept unless `--include-lock-files`. It never touches the current projection store or any archive.
4. A partial bulk run is detected from `bulk_runs`/`bulk_run_chunks` and reported with applied/pending counts, the committed transaction ID per applied chunk, and the exact `recovery resume`, `recovery abandon`, and per-chunk `undo --transaction` commands. `resume` revalidates remaining item revisions and the fingerprint before writing. `abandon` marks the run abandoned and leaves applied chunks intact — **there is no automatic rollback of committed chunks, because that would be a blanket rollback.**
5. For an uncertain commit (process died between commit and response), the authority is B's `mutation status OPERATION_ID`; absence of a receipt means not committed.

### 3.3 Layout descriptions

**Bulk plan diff.** Header: plan ID, verb, item count, selector echo, expiry. Then one section per action, ordered `move`, `edit`, `retag`, `complete`, `restore`, `trash`, `purge`. Each row leads with an action marker and the word, then the immutable ID, then the changed fields as `field: before → after`. Unchanged fields are omitted. Footer: totals per action, an explicit `recurrence: N masters, M exceptions (scope: series)` line when recurrence is involved, and the exact apply command.

```text
plan 018f2a…c31   verb=trash   items=3   expires=2026-08-29T18:12:04Z
selector: --calendar 018f…0002 --filter 'status = cancelled'

trash (3)
  - trash  018f…1001  "Synthetic standup"        rev 4   timed 2026-09-01T15:00Z
  - trash  018f…1002  "Synthetic review"         rev 2   all-day 2026-09-03
  - trash  018f…1003  "Synthetic series master"  rev 9   RECURRING — scope=series, 12 occurrences, 2 exceptions

summary: 3 items, 3 trashed, 0 purged, 1 recurring master (scope=series)
nothing has been written. to apply:
  mg-calr bulk apply --plan 018f2a…c31 --plan-fingerprint 9f2c…7a --yes
```

**Doctor report.** One row per check: status word in a fixed six-column field, check ID, title, detail. Failing and warning checks repeat at the bottom with their remediation block. The prerequisite matrix prints as a table of `check_id | proves | owner | severity`. Redacted connection summary at the top; never a URL.

**Backup manifest summary / verify.** Same row shape as doctor so both are parsed the same way. Verify prints one row per stage (`checksum`, `structure`, `scratch_restore`, `invariants`, `fingerprint`).

**Recovery status.** Three labeled sections; each empty section prints its own empty state rather than being omitted, so the absence of a problem is visible.

**Empty states.** `No plans.` / `No transactions in the selected range.` / `No archives in <dir>.` / `No orphaned artifacts.` / `No partial runs.` / `All checks passed.` JSON returns the same information as an empty array plus the queried scope, never a bare `[]`.

### 3.4 Input & gestures

Keyboard only. There is no pointer, touch, stylus, controller, voice, camera, haptic, or sound interaction anywhere in G. Interaction is: type a command, read the plan, type the apply command or answer a single-key prompt, `Enter` to accept a displayed default, `Ctrl-C`/EOF to cancel. Cancellation before commit yields `input_cancelled`, exits nonzero, writes nothing, and emits no success JSON. Prompts read the controlling terminal, never stdin, so a piped stdin cannot be mistaken for consent; under `--no-input` no terminal is opened at all.

Responsive behavior: output adapts to `COLUMNS` (default 80 when unset or not a TTY). At narrow widths, diff rows wrap on field boundaries with continuation indentation; identity, revision, action word, recurrence scope, and irreversibility notices are never truncated — titles are truncated first, with an ellipsis and the full value available in `--json`. At 40 columns the plan remains fully usable. JSON is width-independent. Progress output (`backup create`, `verify`) is line-oriented, emitted only when stderr is a TTY and `--progress` is not `never`, and never uses cursor addressing.

### 3.5 Transitions & animation

N/A for view transitions — commands print synchronously and replace nothing. The only time-varying output is backup/verify progress, which is a plain appended line per completed stage or per 64 MiB streamed, never a spinner, never a redrawn line, and suppressed entirely when stderr is not a TTY. Reduced-motion needs are therefore satisfied by construction; there is no animation to disable, and `--progress never` exists for users who want silence.

### 3.6 Error states

| Trigger | Code / presentation | Recovery | Data-loss risk |
|---|---|---|---|
| `bulk apply` without a plan | `plan_required`, exit 64 | run the verb first to produce a plan | none |
| plan unknown/expired/already applied | `plan_not_found` / `plan_expired` / `plan_already_applied`, exit 66/75 | re-run the verb to recompute | none |
| entity changed between plan and apply | `plan_stale`, exit 75, lists diverged IDs and revisions | re-plan and re-read | none; zero writes |
| destructive command under `--no-input` without `--yes` | `confirmation_required`, exit 64, names required flags | pass `--yes` (and any acknowledgement) after reading the plan | none |
| purge plan without `--acknowledge-irreversible` | `confirmation_required`, exit 64 | acknowledge explicitly | none |
| recurring master in a scopeless plan | `recurrence_scope_required`, exit 65 | pass `--scope`, re-plan | none; plan not stored |
| selector matches projected todos | `projection_read_only`, exit 65, names the `mg-remindr` command | mutate in the owning app, re-import projection | none |
| plan exceeds limit | `bulk_limit_exceeded`, exit 65 with count and cap | narrow the selector or raise `--limit` deliberately | none |
| undo target changed since | `undo_stale`, exit 75, names intervening transaction | inspect `history show`, re-decide | none |
| undo target purged | `undo_irreversible`, exit 65 | restore from a backup archive instead | none from undo |
| `pg_dump`/`pg_restore` absent or version-mismatched | `backup_tool_missing`, exit 69, prints required version | install matching client tools | none |
| archive checksum mismatch | `backup_checksum_mismatch`, exit 65, names the file | re-create the backup; do not restore this archive | none; restore refused |
| scratch database not supplied | `scratch_database_required`, warn row; verify reports `checksums_only` | create scratch DB, re-verify | none |
| restore target populated without acknowledgement | `restore_target_not_empty`, exit 64 | acknowledge explicitly or pick another target | none; target untouched |
| restore fails mid-stream | `restore_failed`, exit 69; `--single-transaction` rolled it back | fix cause, retry | none; target unchanged |
| prune would delete newest/unparseable archive | `prune_refused`, warn row, archive retained | inspect manually | none |
| database schema newer than binary | `schema_ahead`, exit 69 | use the newer binary, or restore an older archive | none; all mutation refused |
| pending migration and a mutation requested | `migration_pending`, exit 69 | run `database migrate` | none |
| recorded version absent from the embedded list | `migration_unknown_version`, exit 69 | upgrade binary; do not mutate | none |
| rewriting migration without backup or acknowledgement | `migration_backup_required`, exit 64 | supply `--backup-archive` or acknowledge | none |
| migration SQL fails | existing `database_error`, transaction rolled back | fix and rerun; migrate is idempotent | none by transaction contract |
| partial bulk run found at startup | `recovery status` reports it; commands still work | resume, abandon, or undo per chunk | none; state is recorded |
| `recovery clean` candidate is flock-held | skipped with a `warn` row naming the file | retry after the owner exits | none |
| doctor check fails | check row `fail` + remediation; exit 69 | run the named explicit command | none; doctor never writes |

All errors use the foundation error envelope on stderr with a stable code and exit class. Diagnostics never echo event titles, descriptions, locations, attendee URIs, or connection credentials.

### 3.7 Accessibility

- Every status is a word first: `PASS`, `WARN`, `FAIL`, `SKIPPED`, `N/A`; every diff row leads with `add`/`remove`/`change`/`trash`/`purge` in text. The `+ - ~` markers are redundant decoration. Removing color removes nothing.
- `--no-color` and `NO_COLOR` suppress all ANSI output; output is also plain when stdout is not a TTY. Contract tests assert zero escape bytes in every G command under both.
- Confirmation prompts state, in words, the item count, the verb, and whether the action is reversible, e.g. `Purge 12 items? This cannot be undone. Type "purge 12 items" to confirm:`. The prompt never depends on a highlighted default.
- Reading order is deterministic: header, then sections in the fixed order above, then footer. Screen readers get ordinary line-oriented text with no cursor-positioned regions and no live-updating lines.
- Long titles, paths, and remediation commands wrap on word or path-separator boundaries with continuation indentation; identity, revision, scope, and irreversibility text never clip.
- `--json` is the full-fidelity alternative for any user or tool that cannot consume the human layout; it carries every field the human view shows.
- No G interaction requires timing, held keys, or a response within a deadline. A prompt waits indefinitely; automation uses `--no-input`.
- `doctor --explain CHECK_ID` gives a longer text explanation for users who need more than the one-line detail.

---

## 4. Implementation Specification

### 4.1 Architecture placement

Target placement inside the existing single package, following the module boundaries in `docs/ARCHITECTURE.md`:

- `src/domain/safety.rs` — `PlanFingerprint`, `PlanItem`, `PlanStatus`, `UndoEligibility`, `MigrationKind`, `ArtifactClass`; pure, no SQL, no filesystem, no process spawning.
- `src/application/bulk.rs` — plan construction, revalidation, chunked apply orchestration. Calls B/C/F use cases; never emits domain SQL.
- `src/application/undo.rs` — audit-transaction reading, compensating plan construction, eligibility evaluation.
- `src/application/backup.rs` — archive layout, manifest, checksum streaming, subprocess invocation, invariant suite.
- `src/application/doctor.rs` — the check registry and read-only execution.
- `src/application/recovery.rs` — artifact enumeration, partial-run detection, lock inspection.
- `src/storage/plan_repository.rs`, `history_repository.rs`, `recovery_repository.rs` — transaction-bound PostgreSQL access; a `ReadOnlyConnection` newtype used by doctor exposes only query methods.
- `src/cli/bulk.rs`, `undo.rs`, `backup.rs`, `doctor.rs`, `recovery.rs` — argument and prompt translation only.
- `src/render/safety.rs` — human and JSON rendering from shared DTOs.
- `migrations/0006_safety_operations.sql` — plan, run, schema-metadata, and audit-index structures; additive only.

Existing code that moves rather than being duplicated: `storage::migration_status` / `migrate` / `doctor` gain the compatibility probe and the drift check for unknown recorded versions; `interop.rs`'s temp-file, flock, fsync, and rename discipline is factored into a shared `atomic_write` helper that `backup.rs` reuses.

### 4.2 Data model

```rust
/// A frozen, fingerprinted set of intended mutations. Nothing applies without one.
pub struct BulkPlan {
    pub id: PlanId,
    pub verb: BulkVerb,
    pub selector: SelectorEcho,
    pub items: Vec<PlanItem>,
    pub fingerprint: PlanFingerprint, // SHA-256 over canonical sorted tuples
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
    pub status: PlanStatus, // Open | Applied | Discarded | Expired
    pub origin: PlanOrigin, // Bulk | Undo { transaction_id } | Prune
}

/// One aggregate's intended change, bound to the revision observed at plan time.
pub struct PlanItem {
    pub entity_type: EntityType,
    pub entity_id: Uuid,
    pub expected_revision: i64,
    pub action: PlanAction,
    pub after_state_digest: [u8; 32],
    pub recurrence: Option<RecurrenceScopeNote>,
    pub eligibility: ItemEligibility, // Eligible | Stale | Irreversible | Unsupported
}

/// A chunked apply. Its existence is how a partial application is detected.
pub struct BulkRun {
    pub id: RunId,
    pub plan_id: PlanId,
    pub chunk_size: u32,
    pub chunks: Vec<BulkRunChunk>, // Pending | Applied { transaction_id } | Failed { code }
    pub status: RunStatus,         // InProgress | Complete | Failed | Abandoned
}

/// What the binary knows about the schema, and what the database claims.
pub struct SchemaCompatibility {
    pub database_version: i64,
    pub embedded_version: i64,
    pub min_supported_version: i64,
    pub verdict: SchemaVerdict, // Current | Pending | Ahead | Unsupported | UnknownVersion
}

pub enum MigrationKind { Additive, Rewriting, Destructive }

/// A doctor result. Ordering and check_id are part of the public contract.
pub struct DoctorCheck {
    pub check_id: &'static str,
    pub title: &'static str,
    pub status: CheckStatus, // Pass | Warn | Fail | Skipped | NotApplicable
    pub severity: Severity,
    pub detail: String,          // redacted; never a credential or event payload
    pub remediation: Option<Remediation>, // UserCommand | AdministratorCommand | Documentation
}
```

`migrations/0006_safety_operations.sql` (additive; no `DROP`, no rewrite of existing rows):

```sql
CREATE TABLE IF NOT EXISTS mg_calr_schema_metadata (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    schema_version bigint NOT NULL,
    min_binary_version text NOT NULL,
    last_applied_by text NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS bulk_plans (
    id uuid PRIMARY KEY,
    verb text NOT NULL,
    origin text NOT NULL,
    origin_transaction_id uuid,
    selector jsonb NOT NULL,
    fingerprint bytea NOT NULL CHECK (octet_length(fingerprint) = 32),
    item_count integer NOT NULL CHECK (item_count > 0),
    status text NOT NULL CHECK (status IN ('open','applied','discarded','expired')),
    created_by_binary text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at timestamptz NOT NULL,
    CHECK (expires_at > created_at)
);

CREATE TABLE IF NOT EXISTS bulk_plan_items (
    plan_id uuid NOT NULL REFERENCES bulk_plans(id) ON DELETE CASCADE,
    ordinal integer NOT NULL,
    entity_type text NOT NULL,
    entity_id uuid NOT NULL,
    expected_revision bigint NOT NULL CHECK (expected_revision > 0),
    action text NOT NULL,
    after_state_digest bytea NOT NULL CHECK (octet_length(after_state_digest) = 32),
    recurrence_scope text,
    eligibility text NOT NULL,
    PRIMARY KEY (plan_id, ordinal),
    UNIQUE (plan_id, entity_type, entity_id)
);

CREATE TABLE IF NOT EXISTS bulk_runs (
    id uuid PRIMARY KEY,
    plan_id uuid NOT NULL REFERENCES bulk_plans(id),
    chunk_size integer NOT NULL CHECK (chunk_size >= 0),
    status text NOT NULL CHECK (status IN ('in_progress','complete','failed','abandoned')),
    started_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    finished_at timestamptz
);

CREATE TABLE IF NOT EXISTS bulk_run_chunks (
    run_id uuid NOT NULL REFERENCES bulk_runs(id) ON DELETE CASCADE,
    chunk_index integer NOT NULL,
    status text NOT NULL CHECK (status IN ('pending','applied','failed')),
    transaction_id uuid,
    error_code text,
    applied_at timestamptz,
    PRIMARY KEY (run_id, chunk_index),
    CHECK (status <> 'applied' OR transaction_id IS NOT NULL)
);

ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS undoes_transaction_id uuid;
ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS before_revision bigint;
ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS after_revision bigint;
CREATE INDEX IF NOT EXISTS audit_log_transaction_idx ON audit_log (transaction_id, occurred_at);
CREATE INDEX IF NOT EXISTS audit_log_undo_idx ON audit_log (undoes_transaction_id)
    WHERE undoes_transaction_id IS NOT NULL;
```

Binding invariants:

1. A plan is immutable once written. Apply may only transition `open → applied` or `open → discarded/expired`; it never edits items.
2. `expected_revision` is mandatory on every item. There is no unconditional bulk update path and no `Option<ExpectedRevision>` anywhere in the bulk API.
3. The fingerprint covers membership *and* per-item revision *and* after-state digest, so both "a row changed" and "the set changed" are detected.
4. Undo restores `before_state` as a forward change with a new revision and a new audit row; audit rows are append-only and are never deleted, rewritten, or renumbered.
5. Bulk and undo call the owning aggregate's use case. Neither may issue `UPDATE`/`DELETE` against `events`, `todos`, exception, or reminder tables directly; a repository capability test enforces this. This is what makes recurrence/exception corruption unreachable from G.
6. `mg_calr_schema_metadata.schema_version` is written only by `database migrate`, in the same transaction as the ledger insert.
7. `MIGRATIONS` gains `kind` and `min_binary_version`; the embedded list stays append-only and versions strictly increasing (already contract-tested).
8. Doctor's connection type exposes no write method; the check registry is a `const` slice, so check IDs and order are compile-time stable.
9. Every backup artifact has a recorded SHA-256; the manifest's own digest is in `MANIFEST.sha256`; an archive with any mismatch is never restorable.
10. `recovery clean` operates on an allowlist of owned filename patterns rooted at resolved XDG paths; it resolves and rejects symlinks with the same `reject_symlink` discipline `interop.rs` already uses.

### 4.3 API contracts

Application interfaces are authoritative; CLI and SQL are adapters.

```rust
plan_bulk(BulkRequest, Limit) -> Result<BulkPlan, SafetyError>
apply_bulk(PlanId, PlanFingerprint, Confirmation, ChunkSize, OperationId) -> Result<BulkApplyDto, SafetyError>
list_plans(PlanQuery) -> Result<Vec<BulkPlanSummary>, SafetyError>
discard_plan(PlanId, Confirmation) -> Result<(), SafetyError>

list_history(HistoryQuery) -> Result<Vec<TransactionSummary>, SafetyError>
show_transaction(TransactionId) -> Result<TransactionDetailDto, SafetyError>
plan_undo(TransactionId, SkipIneligible) -> Result<BulkPlan, SafetyError>

create_backup(BackupRequest) -> Result<ArchiveManifestDto, BackupError>
list_backups(BackupRoot) -> Result<Vec<ArchiveSummary>, BackupError>
verify_backup(ArchivePath, Option<ScratchDatabaseUrl>, DropScratch) -> Result<VerificationReportDto, BackupError>
restore_backup(ArchivePath, TargetDatabaseUrl, Confirmation, AcknowledgeOverwrite) -> Result<RestoreReportDto, BackupError>
plan_prune(BackupRoot, RetentionPolicy) -> Result<PrunePlanDto, BackupError>
apply_prune(PrunePlanId, Confirmation) -> Result<PruneReportDto, BackupError>

run_doctor(DoctorScope, ReadOnlyConnection) -> Result<DoctorReportDto, DoctorError>
check_schema_compatibility(ReadOnlyConnection) -> Result<SchemaCompatibility, StorageError>
preview_migrations(ReadOnlyConnection) -> Result<Vec<PendingMigrationDto>, StorageError>

recovery_status(ReadOnlyConnection, ResolvedPaths) -> Result<RecoveryReportDto, RecoveryError>
plan_recovery_clean(ResolvedPaths, MaxAge, IncludeLocks) -> Result<CleanPlanDto, RecoveryError>
apply_recovery_clean(CleanPlanId, Confirmation) -> Result<CleanReportDto, RecoveryError>
resume_run(RunId, PlanFingerprint, Confirmation) -> Result<BulkApplyDto, SafetyError>
abandon_run(RunId, Confirmation) -> Result<AbandonReportDto, SafetyError>
```

`Confirmation` has no default constructor; it is built only from an interactive answer or from `--yes` plus a matching fingerprint. `AcknowledgeOverwrite` and `AcknowledgeIrreversible` are separate types, so a compile error — not a code review — prevents a destructive path from taking a plain `bool`.

JSON contracts extend the foundation envelope at `schema_version: 1`. Doctor:

```json
{"schema_version":1,"command":"doctor","ok":true,"data":{
 "connection":"peer socket /run/postgresql db=mg_calr (source: default)",
 "overall":"fail","checks":[
  {"check_id":"pg_server_reachable","title":"PostgreSQL server reachable","status":"pass","severity":"critical","detail":"server version 18.1","remediation":null},
  {"check_id":"schema_compatible","title":"Schema version understood by this binary","status":"fail","severity":"critical","detail":"database schema 99, binary embeds 6","remediation":{"kind":"documentation","commands":[],"text":"Install a newer mg-calr, or restore an archive taken before the upgrade."}},
  {"check_id":"pg_dump_available","title":"pg_dump present and version-compatible","status":"warn","severity":"major","detail":"pg_dump 17.4 is older than server 18.1","remediation":{"kind":"administrator_command","commands":["pacman -S postgresql"],"text":"Install client tools matching the server major version."}}],
 "prerequisites":[{"check_id":"pg_role_exists","proves":"peer role matching the OS user exists","owner":"administrator","severity":"critical"}]}}
```

Error codes: `plan_required`, `plan_not_found`, `plan_expired`, `plan_already_applied`, `plan_stale`, `confirmation_required`, `selector_empty`, `bulk_limit_exceeded`, `recurrence_scope_required`, `projection_read_only`, `undo_stale`, `undo_irreversible`, `undo_unsupported`, `history_not_found`, `backup_tool_missing`, `backup_checksum_mismatch`, `backup_verify_failed`, `scratch_database_required`, `restore_target_not_empty`, `restore_failed`, `prune_refused`, `schema_ahead`, `schema_unsupported`, `migration_pending`, `migration_unknown_version`, `migration_backup_required`, `recovery_state_locked`, `run_not_found`, plus the existing foundation codes. Exit classes follow `src/lib.rs`: 64 missing input/confirmation, 65 invalid request, 66 not found, 69 unavailable/prerequisite failure, 70 serialization, 74 local I/O, 75 conflict/stale, 78 configuration.

Auth: every G operation runs as the existing unprivileged peer role. There is no HTTP endpoint, no token, no rate limit, and no network authorization. `backup`/`verify`/`restore` additionally require the local `pg_dump`/`pg_restore` binaries and, for scratch/target databases, whatever the operator has already provisioned — mg-calr requests no new privilege and never escalates.

### 4.4 State management

PostgreSQL remains the single authority for calendars and events; the validated todo projection file remains the read-only authority for todos. G introduces no third authority: plans, runs, schema metadata, and audit rows live in the same database, and archives are inert files that are only ever read back through an explicit `verify`/`restore`.

Ownership: `application/bulk` owns plan lifecycle and transaction boundaries; `application/backup` owns archive layout and subprocess lifetime; `application/doctor` owns nothing mutable at all. The CLI holds no state between invocations — plan state is durable in the database precisely so that a plan survives the process that created it and can be inspected by another shell.

Local vs. server boundary: everything is local. There is no offline mode because there is no online mode. Draft persistence: a plan *is* the draft, it is explicit and expiring, and cancelling a prompt persists nothing extra. `--plan-out FILE` writes the plan JSON through the same atomic temp-file + fsync + rename discipline `interop.rs` uses, so a crash cannot leave a truncated plan file that looks valid.

Concurrency: two shells may hold plans over the same items; the fingerprint check means at most one applies, and the second gets `plan_stale`. `bulk apply` takes a transaction-scoped advisory lock keyed to the plan ID so the same plan cannot be applied twice concurrently.

### 4.5 Dependencies

- **No new Rust crates are required.** `sha2` (checksums, fingerprints), `fs2` (flock), `rustix` (directory fsync), `serde_json`, `uuid`, `chrono`, `tokio-postgres`, and `thiserror` are already in `Cargo.toml`. `tempfile` moves from dev-dependency to a normal dependency only if scratch-path handling needs it; otherwise the existing `unique_temp_path` helper suffices.
- **New external binaries:** `pg_dump` and `pg_restore` from the PostgreSQL client package. They are invoked with a fixed argument vector — never a shell string, never with user text interpolated into a flag — and their absence is a `warn` in doctor and a hard `backup_tool_missing` in backup commands. Their `--version` output is the only thing doctor executes.
- **Infrastructure:** none new. No CDN, no service, no daemon, no systemd unit (E owns units), no network endpoint.
- **Assets:** none.
- **Feature dependencies:** A5 functional audit for G2; B/C aggregate use cases for G1; F's fingerprint discipline is consumed, not duplicated.

### 4.6 Platform-specific considerations

Arch Linux with PostgreSQL 18 is the first supported target. `pg_dump` must be at least the server's major version; a lower client version is a doctor `warn` and a `backup create` refusal, because a downlevel dump is exactly the kind of quietly incomplete artifact this feature exists to prevent. Directory fsync uses the existing `rustix` path with the non-Linux fallback already present in `interop.rs`; on a filesystem where directory fsync is unavailable, `backup create` records `durability: "file_sync_only"` in the manifest rather than claiming a guarantee it did not make.

Version compatibility is bidirectional and explicit (§G5): the binary declares `EMBEDDED_SCHEMA_VERSION` and `MIN_SUPPORTED_SCHEMA_VERSION`, and refuses rather than guesses outside that window. There are no feature flags for G behavior — a half-enabled safety framework is worse than none. G-a, G-b, and G-c ship as complete slices, and each command exists only once it fully honors the dry-run, confirmation, and refusal contracts. Locale and terminal encoding are not permitted to change parsing: durations, timestamps, and counts are locale-independent.

### 4.7 Performance budget

- `doctor` runs a bounded registry (≤ 40 checks) over one connection with indexed queries; p95 under 300 ms warm. `--quick` skips row-count and per-table checks for a sub-100 ms pipeline probe.
- Bulk plan construction streams the selector result; memory is bounded by `--limit` (default 5000, hard cap 50000 items) at roughly 200 bytes of in-memory state per item, so peak RSS stays under 64 MiB for a maximal plan.
- Plan storage costs ~150 bytes per item in `bulk_plan_items`; expired plans are removed by `bulk plan discard` or by the same explicit prune path, never by a background sweeper.
- `backup create` streams: `pg_dump` writes to the archive file and checksums are computed in 1 MiB chunks, so peak RSS is independent of database size (target < 64 MiB). Elapsed time is proportional to database size and is reported as bytes and duration, not promised as a latency.
- `verify` with a scratch restore costs approximately one restore of the dump plus the invariant queries; it is opt-in precisely because it is the expensive path.
- The schema compatibility probe adds one indexed single-row query (~1 ms) to commands that already connect, and zero to `version` and `config paths`, which still never connect.
- Startup time is otherwise unchanged: no daemon, no cache warm, no background thread.
- **Network payload is exactly zero for every command in this feature.**

---

## 5. Test Specification

### 5.0 Binding acceptance vectors

| Fixture | Input | Exact required result |
|---|---|---|
| refuse-not-assume | every destructive G command with `--no-input` and no `--yes` | exits 64 `confirmation_required`; `pg_dump` checksum of the database and SHA-256 of the projection file are byte-identical before and after; stdout contains no success envelope |
| plan drift | plan 3 events, edit one via `event edit`, then `bulk apply` | exits 75 `plan_stale`, names exactly the diverged ID and both revisions, zero rows changed, plan remains `open` |
| recurrence guard | selector matching a recurring master, no `--scope` | exits 65 `recurrence_scope_required`; no plan row is written; RRULE, EXDATE, and every exception row unchanged |
| opaque bytes through bulk | bulk retag over an event carrying an opaque property envelope | after apply, envelope raw bytes, order, parameters, and SHA-256 are identical |
| targeted undo | transaction T1 edits events A and B; T2 later edits B; `undo --transaction T1` | plan marks A eligible and B `undo_stale`; whole-plan refusal without `--skip-ineligible`; with it, A returns to its pre-T1 value at a new higher revision and B keeps T2's value |
| undo is not rollback | `undo --all`, `undo --since 1h` | argument parsing fails; no such flags exist |
| doctor is inert | run every doctor mode against a freshly migrated database | `pg_dump` custom-format content checksum identical before/after; `mg_calr_schema_migrations` is still absent on a database where doctor ran before any migrate |
| secret containment | config with `url = "postgresql://u:sup3rsecret@h/db"`, run doctor/init/backup/recovery in human and JSON modes | the substring `sup3rsecret` appears in no stdout, stderr, JSON field, manifest, or archive file |
| single-byte corruption | flip one byte in `database.dump`, run `backup verify` | exits 65 `backup_checksum_mismatch` naming the file; `backup restore` on the same archive refuses before touching the target |
| older binary vs newer schema | ledger and metadata record version 99; binary embeds 6 | every mutating command exits 69 `schema_ahead` with zero writes; `doctor` and `backup create` still succeed and the manifest records `produced_by_older_binary: true` |
| unknown recorded version | ledger has version 6 that the binary does not embed | `migration_unknown_version`, exit 69 — not silently ignored |
| partial run visibility | kill the process between chunk 2 and chunk 3 of a 5-chunk run | `recovery status` reports run `in_progress`, 2 applied chunks with their transaction IDs, 3 pending, plus exact resume/abandon/undo commands; `recovery resume` revalidates and completes; `recovery abandon` leaves the 2 applied chunks intact |
| crashed migrate | kill the client mid-`database migrate` | no partial migration is recorded; the advisory lock is not held afterward; rerunning `database migrate` succeeds |
| clean is narrow | scratch dir containing an owned `.tmp`, an owned `.lock` held by a live process, a foreign file, and a symlink to `/etc/passwd` | only the unlocked owned `.tmp` is proposed; the held lock is a `warn`; foreign file and symlink are untouched and the symlink target is never opened |
| restore atomicity | corrupt the dump after the checksum stage in a fault-injection build, run restore | `restore_failed`, target database byte-identical to its pre-restore state |

### 5.1 Unit tests

- `plan_fingerprint_is_canonical_and_order_independent` — the same item set in any input order yields the same fingerprint; changing one revision or one after-state digest changes it.
- `plan_item_requires_expected_revision` — compile/API test proving no constructor produces a plan item without a revision.
- `confirmation_has_no_default_constructor` — `Confirmation`, `AcknowledgeOverwrite`, `AcknowledgeIrreversible` cannot be built from `bool` or `Default`.
- `no_input_maps_to_refusal_not_consent` — a table over every destructive command asserts the `--no-input` + no-`--yes` combination produces `confirmation_required`.
- `undo_eligibility_matrix` — eligible, stale, purged, unsupported, and already-undone transactions each produce their exact classification.
- `undo_plan_is_forward_compensation` — the generated plan increments revisions and contains no audit deletion.
- `schema_verdict_matrix` — current, pending, ahead, unsupported, and unknown-version inputs map to the exact verdict and permitted-command set.
- `migration_kind_gate` — a `Rewriting` pending migration without a verified archive or acknowledgement refuses.
- `retention_policy_never_selects_newest_or_unparseable` — property test over generated archive sets.
- `artifact_classifier_only_matches_owned_patterns` — property test over generated filenames including near-miss and adversarial names.
- `doctor_registry_is_stable` — check IDs are unique, sorted deterministically, and the golden ID list is asserted so a rename is a deliberate breaking change.
- `redaction_of_connection_summary` — URL forms with and without credentials all render through `safe_summary()`.
- `subprocess_argv_is_fixed` — `pg_dump`/`pg_restore` argument vectors contain no user-supplied string in flag position and are never passed to a shell.
- Property tests generate plan/apply/undo sequences and assert: no sequence produces a lower revision, a deleted audit row, or a mutated aggregate without a matching plan item.

### 5.2 Integration tests

Against the explicitly opted-in disposable database (`MG_CALR_RUN_DATABASE_TESTS=1`, `MG_CALR_TEST_DATABASE_URL` containing `mg_calr_test`), ignored by default:

1. Apply migration 0006 twice; assert idempotence, table/index/constraint presence, and that existing rows are untouched.
2. Plan, mutate an item externally, apply → `plan_stale` with zero writes; then re-plan and apply successfully.
3. Chunked apply with an injected fault after chunk 2: assert `bulk_runs`/`bulk_run_chunks` state, that exactly 2 chunk transactions committed, and that `recovery status` reports them.
4. Concurrent `bulk apply` of the same plan from two connections: exactly one applies; the other gets `plan_already_applied` or blocks on the plan advisory lock and then gets it.
5. Undo of a chunk transaction restores exactly that chunk's items.
6. Full backup → verify with scratch restore → invariant suite → restore into a second empty database → compare content fingerprints; then verify a corrupted archive fails.
7. Restore into a populated database without acknowledgement refuses and leaves it unchanged; with acknowledgement, `--single-transaction` semantics are asserted by injecting a mid-restore error.
8. Doctor against: healthy, missing role, missing database, pending migration, ahead schema, missing `pg_dump`, unwritable state dir. Assert exact check statuses, exit codes, and that the database is byte-identical after every run.
9. Assert `doctor`, `database status`, and `init` do not create `mg_calr_schema_migrations` on a fresh database (regression guard on today's behavior).
10. Assert every G command opens a connection only when invoked and performs no DNS lookup or socket operation other than the configured PostgreSQL socket/URL.

### 5.3 UI / E2E tests

Process-level tests via `assert_cmd`:

- Full guided flow: run a bulk verb, capture the plan ID and fingerprint from human output, run apply with `--yes`, assert the result and the recorded transaction.
- `--json --no-input` emits exactly one envelope on stdout and typed errors on stderr for every G command.
- Every prompt has a flag equivalent; `--no-input` closes stdin and cannot hang (each test has a hard timeout).
- `Ctrl-C`/EOF at a confirmation prompt exits nonzero, prints `input_cancelled`, and leaves the database unchanged.
- Piped stdin is never accepted as confirmation: `echo y | mg-calr bulk apply … --no-input` still refuses.
- 40-column and 200-column runs preserve IDs, revisions, action words, scope, and irreversibility text; no ANSI bytes under `--no-color` or `NO_COLOR`; no ANSI when stdout is not a TTY.
- Golden JSON contracts for doctor, plan, verify report, and recovery status; adding an optional field passes, renaming or removing one fails.
- `--help` for every G command lists the dry-run-first rule and at least one runnable example.
- A secret-scan test greps the entire stdout/stderr/artifact surface of a full G session for the fixture credential.

### 5.4 Visual / manual verification

- Inspect the dry-run diff for empty, 1-item, 500-item, recurring-with-exceptions, and long-Unicode-title plans at 40, 80, and 200 columns.
- Inspect doctor output in the healthy state, the "PostgreSQL not installed" state, and the "role missing" state; confirm the administrator commands read as instructions, not as something that already ran.
- Compare light and dark terminal themes with default color, `--no-color`, and `NO_COLOR`; confirm no status, diff direction, or irreversibility notice disappears.
- Read a plan, a doctor report, a verify report, and a recovery status through a screen reader; confirm reading order and that nothing depends on horizontal alignment.
- Run `backup create` against a large synthetic database and watch progress lines with stderr both attached and redirected.
- Confirm empty states print their own sentence rather than nothing, in both human and JSON output.
- Theme, text-scaling, and screen-size matrices beyond terminal width are N/A — there is no GUI surface, no font stack, and no viewport.

### 5.5 Required quality gates

```text
cargo fmt --all -- --check
TMPDIR=/dev/shm cargo clippy --workspace --all-targets --all-features -- -D warnings
TMPDIR=/dev/shm cargo test --workspace --all-targets --all-features
```

Plus, as release gates: the opt-in disposable-PostgreSQL integration suite; the fault-injection suite in 5.2; a clean-Arch container run that installs the package, proves `doctor` fails read-only with the prerequisite matrix, applies the printed administrator steps out of band, migrates unprivileged, and completes a backup → verify → restore round trip; a secret scan over all G output and artifacts; and a test asserting zero non-PostgreSQL network access. A green happy path never overrides a failed safety, migration, privilege, secret, or recovery gate.

---

## 6. Compliance & Safety Gate

### 6.1 Sensitive data classification

- [ ] No sensitive data involvement
- [x] **Handles sensitive data.** A backup archive is the most concentrated copy of the user's schedule that this product produces: event titles, descriptions, locations, URLs, organizer/attendee identifiers, todo notes, and the full audit history. Protections: archives are written under the user's own XDG state directory with directory mode 0700 and file mode 0600; a group- or world-writable output directory is refused without `--allow-insecure-permissions`; the config copy is credential-redacted; nothing outside the database, the config, and the todo projection is ever collected (no `~/.pgpass`, no shell history, no OS credentials); archives are never transmitted anywhere, because G has no network client. Archives are **not encrypted at rest** in v1 — that is stated plainly in `backup create` output and in the docs rather than implied. Diagnostics carry no event payload: doctor reports counts and states, never content.
- [x] Uses synthetic/test data only until compliance gate clears.

### 6.2 Asset provenance

- [x] **No third-party assets.** G ships no images, fonts, models, or data files. Its only external artifacts are the `pg_dump`/`pg_restore` binaries already installed by the operator's PostgreSQL package, which are invoked, not redistributed. Rust crate licenses remain subject to the repository-wide dependency audit; G adds no new crate, so it adds no new license obligation.

### 6.3 Language / claims audit

- [ ] Makes claims not supported by evidence
- [ ] Promises capabilities not yet built
- [ ] Uses language restricted by domain regulations

Specific care taken: `backup verify` reports `checksums_only` versus `scratch_restore_verified` and never says "verified" for the weaker case; the manifest records `durability: "file_sync_only"` when directory fsync is unavailable rather than claiming full durability; purge and restore documentation says data may persist in archives and audit provenance and makes **no claim of secure erasure**; help text shipped in milestone G-a must not advertise bulk, undo, or backup until those slices pass their gates. Section 7 keeps target state and current state strictly separate.

### 6.4 Regulatory alignment

Walking Lens 3 by name:

- **I1 Lossless iCalendar — applies indirectly, addressed.** G defines no codec, but G is the feature most able to destroy round-trip fidelity at scale. It is addressed structurally: bulk and undo mutate only through the owning aggregate's use case, which preserves opaque property envelopes byte-for-byte; no bulk flag can clear or rewrite an extension envelope; an acceptance vector asserts byte, order, parameter, and hash identity across a bulk apply; and the backup content fingerprint and invariant suite cover the extension columns so a restore that dropped them fails verification. Codec ownership stays with F1.
- **I2 Sync authority — applies, addressed.** PostgreSQL remains the sole authority for calendars and events; the validated projection file remains the read-only todo authority and G refuses to mutate it (`projection_read_only`). A backup archive is an inert copy, never a second authority: it is only readable through explicit `verify`/`restore`, both of which name their target explicitly and neither of which can be triggered as a side effect. Plans, runs, and audit rows live in the same authoritative database. `backup restore` is the one operation that can replace authoritative state, which is exactly why it demands an explicit target URL, checksum verification, an overwrite acknowledgement, and a confirmation, and why it runs in a single transaction.
- **I3 Conflict/deletion — applies, addressed.** G never resolves a conflict by picking a winner. A concurrent change between plan and apply stops the operation (`plan_stale`) and preserves both the current row and the untouched plan for re-inspection. An entity changed since a transaction is `undo_stale` and is reported, not overwritten. Local trash, remote tombstone, and purge remain the distinct concepts B and F define; undo never clears a remote tombstone and never resurrects a purged identity. Restore is a whole-database operation that the user explicitly targets, never a silent per-item merge. F retains ownership of three-way reconciliation.
- **I4 Scope/network — applies, never N/A, addressed.** No command in G opens a network socket. `backup create`/`restore` speak to PostgreSQL through the same configured Unix socket or URL as every other database command, and `pg_dump`/`pg_restore` inherit that same connection target — there is no remote fetch, no upload, no telemetry, no update check, and no CalDAV or scheduling traffic. `doctor` executes only `pg_dump --version` / `pg_restore --version` locally and performs no DNS resolution. A dedicated contract test asserts that every G path performs no socket operation other than the configured PostgreSQL connection, and `version`/`config paths` continue to connect to nothing at all.

Other lenses: **T1** identity is preserved by construction — plans reference immutable UUIDs and revisions, undo never rewrites a UID, and restore reproduces identity exactly. **T2** G performs no temporal arithmetic on user data; it moves whole aggregates through their owning use cases, and all G timestamps (`created_at`, `expires_at`, manifest time) are UTC `timestamptz`. **T3** every apply is transactional with mandatory expected revisions; chunk boundaries are recorded before they are crossed; restore is `--single-transaction`; the migration lock is transaction-scoped. **T4** audit rows are append-only, undo is a forward compensating write, purge remains irreversible and is refused by undo, and archives carry the audit history so provenance survives a restore. **T5** G never claims, delivers, or dedupes a reminder; a bulk operation that changes a reminder definition goes through E's owning use case and produces no delivery. **C1–C5** guided plan-then-apply, complete flag coverage, `--no-input` refusal, versioned deterministic JSON, XDG-rooted paths with the existing CLI > env > TOML > default precedence, color-independent output, and a strictly non-mutating doctor with a stable machine contract and a prerequisite matrix. **O1–O4** no credential is stored, copied, or printed; every operation runs unprivileged and prints administrator steps instead of running them; failures are typed with stable codes and atomic effects; and the verification set spans unit, property, contract, fault-injection, clean-machine, and secret-scan gates.

The auto-fail rules, addressed by name: **silent event/todo loss** is unreachable because no mutation exists without a printed plan, a fingerprint check, and a confirmation, and because `recovery clean` touches only mg-calr's own temp artifacts while `backup`/`restore`/`prune` refuse anything unverified. **Unconfirmed overwrite** is unreachable because `--no-input` refuses rather than assuming yes, `--yes` binds to one printed plan, restore requires a separate overwrite acknowledgement, and drift always stops the operation. **Recurrence/exception corruption** is unreachable because G issues no domain SQL, refuses scopeless plans touching recurring masters, and routes every occurrence-affecting change through B's scoped mutation API.

### 6.5 Security controls

- Subprocesses are invoked with a fixed argument vector and no shell; user text never lands in a flag position; the environment passed to `pg_dump`/`pg_restore` is minimized and carries no credential the parent did not already resolve.
- All SQL is parameterized; plan filters compile to enum-driven predicates, never interpolated identifiers.
- Path handling rejects symlinks, requires the resolved path to remain inside the resolved root, and refuses to write over an existing archive name.
- Sizes and counts are validated before allocation: plan limits, archive size reporting, and audit-row caps prevent memory exhaustion from an adversarial or corrupt input.
- Logs contain command, plan/run/transaction IDs, counts, durations, and error codes — never event payload, never a connection string, never a credential.
- No command in G requires, requests, or accepts elevated privilege; there is no `--force` that bypasses a fingerprint, a revision, or an integrity check.

---

## 7. Gap Analysis vs. Current State

### 7.1 What exists today

**Implemented** (verified in the working tree at this date):

- Embedded, append-only migrations 0001–0005 applied inside one transaction guarded by `pg_advisory_xact_lock` in `src/storage.rs::migrate`; re-running is idempotent; a recorded version whose name differs from the embedded name fails as `migration_drift`.
- `database status`, `doctor`, and `init` are query-only and do not create the migration ledger (`src/storage.rs::migration_status`, `doctor`).
- `init` prints administrator provisioning examples and the explicit statement that mg-calr never invokes sudo or provisions roles (`src/main.rs`, ~line 1211).
- Typed errors with stable string codes and stable exit codes (`src/lib.rs::AppError::code`/`exit_code`), and versioned success/error JSON envelopes at `schema_version: 1`.
- Global `--json`, `--no-input`, `--no-color`, `--database-url`; XDG config/data/state/cache resolution (`src/config.rs`) and redacted connection summaries.
- Crash-safe local file replacement for the todo projection: exclusive flock, unique temp path, `sync_all`, atomic `rename`, parent-directory fsync, and symlink rejection (`src/interop.rs`, ~lines 434–500 and 955–1030).
- Optimistic version columns and typed conflict errors for events (migration 0005, `EventVersionConflict`) and todos (`TodoVersionConflict`).
- Exactly one destructive-confirmation gate anywhere in the product: `todo purge --yes`, which refuses without the flag and states that no database was accessed (`src/main.rs`, ~line 978).
- A `--dry-run` flag on `todo scan-reminders` only, which suppresses delivery writes (`src/storage.rs`, ~line 1829).

**Prototyped:** none of G. **Gated:** none of G. **Planned:** all of G1–G6 as specified here.

**Absent:**

- The `audit_log` table is created by `migrations/0001_foundation.sql`, but no code writes or reads it — `grep audit_log src/` returns nothing. There is no functional audit transaction, so there is no history and nothing to undo. This is A5's gap and G2's hard blocker.
- No bulk framework, no plan, no fingerprint, no `--yes` on any command except `todo purge`, no confirmation prompt anywhere, and no multi-item mutation path at all.
- No undo, no history, no `mutation status`.
- No backup, verification, retention, or restore; `pg_dump`/`pg_restore` are never invoked and their presence is never checked.
- `doctor` is currently an alias for `migration_status` that hardcodes `database_reachable: true` and `administrator_guidance: Vec::new()`; it has no check registry, no stable per-check IDs, no prerequisite matrix, no severity, and no remediation. It is non-mutating today, which is correct and must be preserved, but it reports far less than C5 requires.
- No schema compatibility guard of any kind. `migration_status` iterates only over the embedded `MIGRATIONS` slice, so a database recording version 6 against a binary embedding 5 is **silently ignored** — the exact newer-schema-corruption case G5 exists to close. `mg_calr_schema_metadata`, `MigrationKind`, `min_binary_version`, `migrate --dry-run`, and the documented rollback policy are all absent.
- No recovery command, no partial-run concept, no orphaned-artifact enumeration, and no advisory-lock inspection.

### 7.2 Delta to spec

**New files/modules:** `src/domain/safety.rs`; `src/application/{bulk,undo,backup,doctor,recovery}.rs`; `src/storage/{plan_repository,history_repository,recovery_repository}.rs`; `src/cli/{bulk,undo,backup,doctor,recovery}.rs`; `src/render/safety.rs`; `tests/{bulk_safety_contract,undo_contract,backup_contract,doctor_contract,schema_compatibility_contract,recovery_contract}.rs`.

**Modified files:** `src/main.rs` — add the `bulk`, `history`, `undo`, `backup`, `recovery` subcommands, extend `doctor` and `database migrate`, and add the shared confirmation helper. `src/storage.rs` — add the compatibility probe, the unknown-recorded-version drift check, `MigrationKind`/`min_binary_version` on `Migration`, a `ReadOnlyConnection` wrapper, and write `mg_calr_schema_metadata` inside the migrate transaction. `src/interop.rs` — factor the atomic-write/flock/fsync discipline into a shared helper. `src/lib.rs` — add the new error variants, codes, and exit mappings. `src/config.rs` — add the optional `[backup]` section (`dir`, `keep_count`, `keep_days`) and the plan expiry setting. `config/example.toml`, `README.md`, `docs/ARCHITECTURE.md` — document the backup section, the rollback policy, and the doctor contract.

**Migrations/schema:** `migrations/0006_safety_operations.sql` as written in §4.2 — additive only, `IF NOT EXISTS` throughout, no `DROP`, no rewrite of existing rows, consistent with the existing contract test that forbids destructive SQL in the foundation migration.

**New dependencies:** none in Cargo; two new external binaries (`pg_dump`, `pg_restore`) that are detected, version-checked, and reported rather than assumed.

### 7.3 Estimated scope

**XL** overall. The three milestones size very differently and should not be quoted as one number:

- **G-a (doctor, schema compatibility, recovery):** **M**. It is mostly new read-only code plus one small additive migration, and it depends on nothing that does not already exist. It also retires the largest live risk — a binary writing to a schema it does not understand.
- **G-b (backup/verify/restore/prune):** **L**. Subprocess management, streaming checksums, manifest design, the invariant suite, and the restore-refusal matrix are each small, but the fault-injection and clean-machine round-trip gates are the bulk of the work.
- **G-c (bulk and undo):** **L**, and blocked. The framework itself is moderate; what makes it large is that every verb must route through an owning aggregate use case with mandatory expected revisions, and those APIs must exist first.

### 7.4 Blocking dependencies

- **A5 functional audit transactions block G2 entirely** and block chunk provenance in G1. Until something writes `audit_log` with before/after state and a transaction ID, there is no history and no compensating write to build.
- **B's revision-checked aggregate mutation API blocks G1**, because bulk is defined as calling it rather than issuing SQL. The same applies to C for legacy todo tables and to E for reminder definitions.
- G-a blocks nothing and is blocked by nothing; it should land first and independently.
- G3 restore is a prerequisite for the G5 rollback policy being honest — the policy says "restore a verified archive", so the policy is provisional until G-b ships.
- H (packaging) consumes G4's prerequisite matrix for its clean-machine smoke test; G should publish the check IDs before H pins them.
- F's sync work must not begin mutating events before G1's plan discipline exists, or bulk reconciliation will grow its own unreviewed apply path.

### 7.5 Explicit non-goals

- No automatic, scheduled, or background anything: no auto-backup, no auto-prune, no auto-repair, no background compaction, no update check, no telemetry.
- No point-in-time recovery, no WAL archiving, no replication, no continuous backup. `backup` is a `pg_dump` snapshot workflow, and the docs say so.
- No encrypted archives, no remote backup destination, no cloud upload in v1.
- No `database rollback` command and no down-migration SQL, by policy rather than by omission.
- No blanket undo, no "restore to timestamp", no `--force` that bypasses a fingerprint, revision, or integrity check.
- No repair inside `doctor`, ever — including a "safe" fix, a cache rebuild, or ledger creation.
- No privileged operation: mg-calr never runs sudo, never creates or drops a role or database, and never edits `pg_hba.conf`.
- No mutation of the stored todo projection; that authority belongs to `mg-remindr`.
- No GUI, TUI, or Quickshell surface for any G command in this feature; later clients consume the public JSON contracts.

---

## 8. Open Questions

- **Q1:** Should `bulk apply` default to a single transaction for the entire plan with a hard item cap, or to chunking above a threshold? Single-transaction is the strongest integrity story; chunking is friendlier for very large plans and long lock holds. — blocks: §3.2 step 9 default and §4.7 limits; the recorded-partial-run contract holds either way.
- **Q2:** What plan expiry is right by default — 15 minutes, one hour, or session-scoped? Too short is annoying, too long invites applying a stale mental model. — blocks: §4.2 `expires_at` default only.
- **Q3:** Should `backup verify` be allowed to create and drop its own scratch database when the connecting role happens to have `CREATEDB`? Doing so is more convenient; refusing keeps the "mg-calr never creates or drops a database" rule absolute. This spec assumes the strict rule. — blocks: §3.2 G3 step 4.
- **Q4:** Should archives be encrypted at rest in v1 (age/GPG via an external command), or is documenting plaintext plus 0600 permissions the right v1 boundary? — blocks: §6.1 and the `backup create` contract.
- **Q5:** When the schema is behind the binary, should read-only commands proceed with a warning (this spec's choice) or refuse outright? Proceeding keeps `doctor` and agenda reads useful during an interrupted upgrade; refusing is simpler to reason about. — blocks: §3.2 G5 step 3.
- **Q6:** Which exit code should `confirmation_required` carry? This spec uses 64 to match the existing `RequiredInput` mapping in `src/lib.rs`, but a dedicated code would let automation distinguish "you forgot a flag" from "you must confirm". — blocks: §4.3 exit classes.
- **Q7:** How long should applied plans, runs, and audit rows be retained before `history` gets slow, and what explicit command prunes them? Audit retention directly bounds how far back undo can reach. — blocks: §4.7 storage growth and G2's eligibility horizon.
