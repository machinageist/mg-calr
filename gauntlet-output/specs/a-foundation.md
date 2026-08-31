# Spec: Application Foundation

**Feature ID:** a-foundation
**Parent feature:** root
**Spec author agent:** Foundation spec agent (Spec Gauntlet)
**Date:** 2026-08-29
**Iteration:** 2

---

## 1. Purpose

### 1.1 One-sentence job

Give a keyboard-first user and any script a safe, inspectable, non-provisioning base layer — resolved XDG configuration, diagnosed and explicitly migrated PostgreSQL storage, permanently stable typed identity with unambiguous short selectors, deterministic error/exit/JSON contracts, and an immutable transaction/audit record — so that every later calendar, todo, reminder, view, and synchronization slice mutates data without losing, duplicating, or silently overwriting it.

### 1.2 Why it matters

`mg-calr` is a local application whose only authority is a PostgreSQL database the user administers. Every downstream feature (B events, C todos, D views, E reminders, F interchange/sync, G safety, H packaging) reads its paths from A1, its connection and schema from A2, its selectors from A3, its errors and machine contracts from A4, and its provenance from A5. If identity is mutable, a partial migration is possible, a selector matches "the first row", a database URL leaks a password into a log, or a mutation leaves no transaction record, those defects become unfixable once user data exists. The foundation exists to make those failure modes structurally impossible before any user-visible calendar workflow ships, and to make an unprovisioned machine diagnosable without `sudo`, guesswork, or server mutation.

### 1.3 Success signal

On a clean checkout with no PostgreSQL server at all, `cargo test` passes end to end, `mg-calr --json version` and `mg-calr --json config paths` emit schema-version-1 envelopes with four distinct XDG roots and no connection attempt, and `mg-calr --json doctor` returns exit 69 with an ordered prerequisite matrix naming the exact unprivileged administrator commands and no credential text. Against an explicitly named disposable database, `mg-calr database migrate` run twice produces byte-identical `database status` output, one row per embedded migration, and no duplicated schema effect; a selector that matches two rows fails with exit 66 and writes nothing.

### 1.4 Feature and milestone boundary

This spec defines the complete target contract for A1–A5. Delivery is ordered:

- **Milestone 1 (shipped slice):** A1 XDG/TOML precedence, A2 connection/status/migrate/doctor/init, the identity half of A3 (typed immutable UUIDs), and the envelope/exit half of A4.
- **Milestone 1b (this spec's remaining delta):** A3 short-ID resolution, A4 structured redacted diagnostics plus a machine-readable error contract table, A5 functional transaction/audit/receipt recording, and the doctor prerequisite matrix.
- **Deferred, explicitly not A:** calendar/event/todo domain workflows (B, C), views and choosers (D), reminder scanning and delivery (E), iCalendar codec and sync (F), undo/backup/restore commands (G), packaging (H), TUI/Quickshell (I).

A owns the primitives; it never owns a domain workflow. Migration 1 creating a `todos` table is scaffolding, not evidence that todo behavior exists.

---

## 2. User Stories

> As a keyboard-first user on a fresh machine, I want `mg-calr init` to tell me exactly which PostgreSQL role and database an administrator must create, so that I can get to a working install without guessing or running the tool as root.

> As a cautious operator, I want `mg-calr doctor` to be strictly read-only and to name every failed prerequisite with a remedy, so that diagnosing a broken install can never itself change my server.

> As a developer, I want `mg-calr database migrate` to be idempotent, transactional, and advisory-locked, so that a re-run, a concurrent run, or a crash mid-migration can never leave a half-applied schema.

> As a script author, I want a versioned JSON envelope, a stable error code and exit status for every failure, and `--no-input` that never blocks on a prompt, so that automation can branch on outcomes instead of parsing prose.

> As a user typing IDs by hand, I want to paste the first eight hex characters of a UUID and get either exactly one target or an explicit ambiguity error, so that a short selector never silently addresses the wrong record.

> As a privacy-conscious user, I want connection URLs, passwords, and record contents to be absent from every summary, diagnostic, and log line, so that pasting a `doctor` report into a bug tracker cannot leak my credentials.

> As a user who later makes a mistake, I want every mutation to leave an immutable transaction record with before/after state, so that a future undo, restore, or audit review has something truthful to read.

> As a screen-reader user, I want all state, severity, and confirmation to be words rather than colors or cursor positions, so that `NO_COLOR` output and my reader convey exactly the same information.

---

## 3. UX Specification

`mg-calr` is a terminal program. It has no screens, modals, sheets, popovers, drawers, or panels. The subsections below read "view" as "command output surface" and "gesture" as "keystroke and argv".

### 3.1 Screen / view inventory

| Command surface | Invocation | New vs. modified | Output layout |
|---|---|---|---|
| Version | `mg-calr version` | Modified (exists) | one line, or one JSON object |
| Resolved paths | `mg-calr config paths` | Modified (exists) | labeled key/value block, or one JSON object |
| Effective configuration | `mg-calr config show` | **New (A1 delta)** | key / value / source table with every URL redacted |
| Readiness report | `mg-calr init` | Modified (exists) | prerequisite matrix + ordered administrator command examples |
| Diagnosis | `mg-calr doctor [--component …]` | Modified (exists) | ordered check table with status word and remedy |
| Migration status | `mg-calr database status` | Modified (exists) | one row per embedded migration |
| Migration application | `mg-calr database migrate [--dry-run]` | Modified (exists) | plan, then applied rows, then final status |
| Selector diagnosis | `mg-calr id resolve SELECTOR --kind KIND` | **New (A3 delta)** | canonical UUID, kind, minimum unique prefix length |
| Transaction history | `mg-calr audit list …` / `mg-calr audit show TRANSACTION_ID` | **New (A5 delta)** | reverse-chronological rows / one grouped record |

Target command grammar (foundation only; domain commands are specified by B/C/D):

```text
mg-calr [--json] [--no-color] [--no-input] [--database-url URL] [--verbose] <command>

mg-calr version
mg-calr config paths
mg-calr config show [--include-defaults]
mg-calr init [--create-dirs]
mg-calr doctor [--component all|config|paths|timezone|database|migrations|privileges]
mg-calr database status
mg-calr database migrate [--dry-run] [--yes]
mg-calr id resolve SELECTOR --kind calendar|event|todo|reminder|project|tag
mg-calr audit list [--entity-type KIND] [--entity-id UUID] [--since RFC3339] [--limit N]
mg-calr audit show TRANSACTION_ID
```

Rules binding on the whole grammar:

- Every prompt has an equivalent flag. `--no-input` never reads stdin or `/dev/tty`; a missing required value returns `required_input_missing` and names the flags that would resolve it.
- `--json` writes exactly one JSON object to stdout on success and exactly one error envelope to stderr on failure. Prompts and progress text go to stderr and never to stdout.
- Global flags are accepted in any Clap-supported position (`global = true` on `--json`, `--no-input`, `--no-color`, `--database-url`), and `--` terminates option parsing.
- `version`, `config paths`, and `config show` are pure: they read environment, argv, and at most the one TOML file. They never open a socket.
- `init`, `doctor`, `database status`, `database migrate`, `id resolve`, and `audit *` are *explicit database-related commands* and are the only foundation commands permitted to open the configured PostgreSQL connection.
- `database migrate` is the only foundation command permitted to change schema. `init`, `doctor`, and `database status` are read-only and never create the migration ledger.

### 3.2 Interaction flows

**Primary flow — first run on an unprovisioned machine**

1. Clap parses argv locally. `--help`/`--version` short-circuit with exit 0 and no I/O beyond stdout.
2. `ConfigPaths::from_env` resolves four distinct XDG roots. If both the relevant `XDG_*` base and `HOME` are absent, stop with `config_unavailable`, exit 78.
3. The optional `$XDG_CONFIG_HOME/mg-calr/config.toml` is read. Absent is normal. Unreadable is `config_unavailable` (78); malformed is `config_invalid` (78) naming the file path and the parser's line/column.
4. The database connection is resolved by precedence (§4.2) into `ConnectionSettings`, which carries a `ConfigSource` for provenance and exposes only `safe_summary()` for display.
5. `init` runs the same read-only check matrix as `doctor` but **never fails on an unreachable database**: it reports `database_reachable: false`, prints the ordered administrator examples, and exits 0. This is deliberate — `init` is the pre-provisioning command, so a nonzero status would be indistinguishable from a broken install.
6. The user (or an administrator) runs the printed `createuser`/`createdb` commands out of band. `mg-calr` never invokes `sudo`, never spawns a shell, and never provisions a role or database.
7. `doctor` re-runs the matrix. All required checks passing exits 0; any required check failing exits 69 with the failing check IDs and remedies.
8. `database migrate` opens one connection, creates the ledger if absent, opens a transaction, takes `pg_advisory_xact_lock(6851863988)`, and for each embedded migration in ascending version order: reads any recorded name, fails `migration_drift` on mismatch, skips on exact match, otherwise executes the SQL and inserts the ledger row inside the same transaction. Commit is single and final; any failure rolls the whole transaction back. It then re-reads and prints status.
9. `database status` re-run prints byte-identical output to the migrate tail. Re-running `migrate` reports zero pending and changes nothing.

**Branch — guided input.** Foundation commands take no free-form values, so no prompt is required in the shipped slice. Where a future foundation prompt is added it must (a) print the field name, accepted format, and default; (b) be reachable by an equivalent flag; (c) be refused under `--no-input` with `required_input_missing`; (d) treat EOF/Ctrl-C as cancellation with no mutation.

**Branch — destructive confirmation.** `database migrate` on a database whose ledger already records a version that the embedded set does not contain (a downgrade attempt) stops with `migration_unknown_version` and never drops anything. `migrate --dry-run` prints the exact pending plan and exits 0 without opening a write transaction. No foundation command deletes user data; there is no foundation `drop`, `reset`, or `--force`.

**Branch — short-selector resolution (A3 target).** `id resolve` normalizes the selector (lowercase, hyphens stripped, hex-only), rejects fewer than 8 or more than 32 characters, then runs a kind-scoped prefix query. Exactly one match prints the canonical UUID plus the minimum prefix length that is unique in the whole kind. Zero matches is `selector_not_found` (66). Two or more is `selector_ambiguous` (66) reporting the candidate count and the required longer length — never a candidate list of private titles, and never a "first match".

**Cues.** No sound, haptic, or animation is used anywhere. `migrate` prints one line per applied migration as it commits, so a slow migration is visibly progressing without a spinner.

### 3.3 Layout descriptions

Human output is plain text with a stable field order; JSON output carries the same values in the same order. No foundation output contains ANSI escapes today, and any future color is decoration over text that already carries the meaning.

- **`config paths`** — leading label, trailing value, one per line, in fixed order: `config_dir`, `config_file`, `data_dir`, `state_dir`, `cache_dir`. Source: `ConfigPaths` (`src/config.rs`). Empty state is impossible; every path always resolves or the command fails.
- **`config show`** — three columns `SETTING`, `VALUE`, `SOURCE`, where `SOURCE` is one of `cli`, `environment`, `file`, `default` (`ConfigSource`). Any URL renders as `postgresql://<redacted>` plus its source. Empty state: with no config file and no overrides, every row reads `default`.
- **`doctor` / `init`** — an ordered check table `CHECK`, `STATUS`, `DETAIL`, `REMEDY`, where `STATUS` is the literal word `pass`, `warn`, `fail`, or `skipped`. Below it, `Connection: <safe summary>` and, for `init`, the numbered administrator examples. Data source: the check runner over `ConnectionSettings` plus filesystem/TZDB probes. Empty state: never empty; the matrix is fixed and every check reports.
- **`database status` / `migrate`** — one row per embedded migration: `VERSION`, `NAME`, `APPLIED` (`yes`/`no`). Source: `storage::MigrationState` built from `storage::MIGRATIONS` joined to `mg_calr_schema_migrations`. Empty state: on a database with no ledger, all rows read `no` (not an error).
- **`audit list`** — reverse-chronological `OCCURRED_AT`, `TRANSACTION`, `ENTITY_TYPE`, `ENTITY_ID`, `OPERATION`. Empty state prints `No audit records match.` and JSON returns an empty `records` array with the echoed query.
- **`audit show`** — grouped record: transaction identity and actor, then one block per affected entity with `before`/`after` state objects. Empty state: unknown transaction ID is `not_found` (66), not an empty success.

### 3.4 Input & gestures

- Input is argv, environment, an optional TOML file, and (only where a prompt exists) line-oriented stdin. There is no pointer, touch, stylus, controller, voice, or camera input, and none is planned.
- Keyboard: standard shell line editing applies; `Ctrl-C` during any prompt or long migration cancels before commit and exits 130 with no partial write; `Ctrl-D`/EOF is cancellation, never an empty accepted value.
- Shell completion and man pages are H4's deliverable; A only guarantees that the command/flag grammar above is stable enough to generate them.
- "Responsive behavior across screen sizes" maps to terminal width. Human output wraps at word boundaries with continuation indent and never truncates an identifier, status word, check ID, or remedy; tables degrade to labeled records below 60 columns. JSON is width-independent and never wrapped. Nothing depends on terminal width being detectable — an undetectable width falls back to 80 columns.
- `--no-input` is the automation contract: stdin is not read at all, so a foundation command in a pipeline can never hang.

### 3.5 Transitions & animation

N/A — a CLI command replaces terminal output synchronously and this feature introduces no animation, spinner, progress bar, cursor addressing, alternate screen, or timed redraw. Reduced-motion preferences are therefore satisfied by construction. `migrate` reports progress as ordinary appended lines, which remain correct when captured to a file or read by a screen reader.

### 3.6 Error states

| Trigger | Presentation | Recovery | Data-loss risk |
|---|---|---|---|
| `HOME` and required `XDG_*` base both unset | `config_unavailable`, exit 78, names the missing variables | set `HOME` or the XDG base | none — no I/O attempted |
| config file unreadable (permissions/IO) | `config_unavailable`, exit 78, prints path only | fix permissions or remove file | none |
| config file is malformed TOML | `config_invalid`, exit 78, path plus parser line/column | repair or delete the file | none |
| unknown/misspelled TOML key (target) | `config_unknown_key`, exit 78, key name and nearest valid key | correct the key | none |
| config file world/group readable while it contains a URL (target) | `warn` row in `doctor`, remedy `chmod 600` | tighten permissions or move the URL to `MG_CALR_DATABASE_URL` | none |
| database URL unparseable | `database_config_invalid`, exit 69, **URL never echoed** | correct the override | none |
| non-loopback TCP host without TLS (target) | `remote_database_requires_tls`, exit 69, before any socket write | use the local Unix socket, or wait for TLS support | none |
| server down / role missing / database missing | `database_unavailable`, exit 69, redacted summary plus the administrator remedy already in `StorageError::Connect` | administrator provisions role/database or starts the service | none — read-only path, never migrates |
| ledger records a version under a different name | `migration_drift`, exit 69, prints version, recorded name, expected name | inspect manually before any recovery; do not force | none — transaction aborts before any SQL runs |
| ledger records a version the binary does not contain (target) | `migration_unknown_version`, exit 69 | install the newer binary; downgrade is not automatic (G5) | none — nothing dropped |
| migration SQL fails | `database_error`, exit 69, PostgreSQL message | fix the compatibility problem and re-run | none — whole transaction rolls back, no ledger row is written |
| concurrent `migrate` on the same database | second process blocks on the advisory lock, then observes zero pending | none needed | none — serialized by `pg_advisory_xact_lock` |
| selector shorter than 8 hex / non-hex (target) | `selector_invalid`, exit 65, states 8–32 lowercase hex | supply more characters or the full UUID | none |
| selector matches nothing (target) | `selector_not_found`, exit 66 | verify the ID | none |
| selector matches several (target) | `selector_ambiguous`, exit 66, candidate count and minimum unique length, no titles | lengthen the selector or paste the UUID | none — **never mutates, never picks the first match** |
| required input missing under `--no-input` | `required_input_missing`, exit 64, names the flag | supply the flag | none |
| prompt cancelled (Ctrl-C / EOF) | plain cancellation message | re-run | none — cancellation precedes any transaction |
| JSON serialization failure | `serialization_error`, exit 70, best-effort plain message on stderr | report as a defect; human output is not silently substituted | none |

Every error is emitted through the A4 envelope on stderr under `--json` and as `mg-calr: <message>` on stderr otherwise. No error path prints a connection URL, password, TOML file contents, or record payload.

### 3.7 Accessibility

- **Labels, hints, traits.** Every value is preceded by a word label. Check status is the literal word `pass`/`warn`/`fail`/`skipped`; applied state is `yes`/`no`. No glyph, indentation level, cursor position, or color is the sole carrier of meaning.
- **Custom actions for complex interactions.** There are no composite or gesture-driven interactions to expose; each capability is one command with named flags, which is inherently the "custom action" surface.
- **Text scaling / dynamic type.** Delegated to the terminal emulator. `mg-calr` emits no absolute positioning, no box drawing that depends on a monospace cell grid for meaning, and no fixed-width assumption beyond a default of 80 columns for wrapping.
- **Color-independent state.** `--no-color` and `NO_COLOR` (any non-empty value) both suppress color; foundation output contains no ANSI escapes at all today, and a contract test asserts their absence. Severity, applied/pending state, and error identity are words in both modes.
- **Focus order and keyboard navigability.** Output is linear and top-to-bottom in a fixed order, which is the reading order for a screen reader and the tab-free focus order for a terminal. All functionality is reachable without a pointer. `--json` gives assistive tooling a structured alternative to reading a table.
- Long remedies, paths, and messages wrap rather than clip so that a 40-column reader still receives the whole recovery instruction.

---

## 4. Implementation Specification

### 4.1 Architecture placement

The package is a single crate `mg-calr` (`Cargo.toml`, edition 2024, `unsafe_code = "forbid"`, `clippy::all`/`pedantic` denied). Foundation ownership:

- `src/config.rs` — `ConfigPaths`, `ConfigSource`, `ConnectionSettings`, `AppConfig`, `resolve_config` (pure, environment injected as a `HashMap`) and `load` (the only filesystem boundary). Owns A1.
- `src/storage.rs` — `Migration`, `MIGRATIONS`, `MigrationState`, `postgres_config`, `connect`, `ensure_migration_table`, `migration_status`, `migrate`, `doctor`, and `StorageError`. Owns A2. Domain repositories also live here today; the foundation-owned items are the migration/connection block, not the repositories.
- `src/domain.rs` — the `domain_id!` macro and the nominal identifier types `CalendarId`, `EventId`, `ReminderId`, `DeliveryId`, `AuditId` (plus `TodoId`/`ProjectId` re-exported from `src/domain/todo.rs`), `RfcUid`, and `DomainError`. Owns the identity half of A3. Has no SQL, CLI, or network dependency.
- `src/lib.rs` — `AppError`, `AppError::code`, `AppError::exit_code`, `Envelope`, `ErrorEnvelope`, `ErrorBody`. Owns A4.
- `src/main.rs` — Clap `Cli`/`Command`, dispatch, `print_debug`/`print_projection`, `required`/`prompt`, and the single process-boundary error renderer in `main`. Presentation only; it holds no business rule.
- `migrations/*.sql` — append-only SQL embedded with `include_str!`. `0001_foundation.sql` is A2/A5 scaffolding.

**Target additions**, keeping the existing flat-module style:

- `src/domain/selector.rs` — `ShortId`, `Selector<T>`, `Resolution<T>`, normalization and length rules (A3).
- `src/domain/audit.rs` — `AuditTransactionId`, `OperationId`, `RequestFingerprint`, `AuditOperation`, `AuditEntry` (A5).
- `src/storage/selector.rs` — kind-scoped prefix resolution queries (A3).
- `src/storage/audit.rs` — transaction-bound `record_transaction` / `record_entry` / `claim_operation` helpers that every later feature must call inside its own mutation transaction (A5).
- `src/diagnostics.rs` — the check-matrix runner behind `doctor`/`init`, and the redacting structured-diagnostic writer behind `--verbose` (A2/A4).
- `contracts/error-v1.json` — machine-readable `code → {exit, retryable, permitted detail keys}` table loaded by tests instead of duplicated literals (A4).
- `migrations/0006_audit_provenance.sql` — the A5 delta (below).

`src/application.rs`, `src/interop.rs`, and `src/tui.rs` exist in the crate but are **not** foundation surface; A must not grow rules for them.

### 4.2 Data model

**Configuration precedence (A1, binding).** The database connection resolves in exactly this order, first match wins:

1. `--database-url URL` → `ConnectionSettings::Url { source: Cli }`
2. `MG_CALR_DATABASE_URL` (target; application-namespaced) → `source: Environment`
3. `DATABASE_URL` (implemented) → `source: Environment`
4. `[database].url` in the TOML file → `source: File`
5. `[database]` peer fields `socket_dir` / `user` / `dbname` → `source: File`
6. Built-in peer defaults `/run/postgresql`, `$USER`, `mg_calr` → `source: Default`

The four XDG roots resolve independently — `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME`, each falling back under `HOME` to `.config`, `.local/share`, `.local/state`, `.cache`, each suffixed `mg-calr`. Config never falls back to the data dir, and data never falls back to cache. `resolve_config` is pure over an injected variable map so precedence is unit-testable without touching the process environment. Target: `FileConfig` gains `#[serde(deny_unknown_fields)]` so a typo is a named error rather than a silently ignored setting, and a `[compat] config_version` key records the file's schema so a future rename can warn and migrate rather than break.

**Identity (A3, binding).**

```rust
/// Nominal, immutable, database-authoritative identity. Generated once as
/// UUIDv7 and never regenerated by an edit, restore, export, or import.
pub struct CalendarId(Uuid);   // also EventId, TodoId, ReminderId, DeliveryId, AuditId
```

Invariants:

1. Identifier types are distinct at compile time; `CalendarId` cannot be passed where `EventId` is expected. Malformed text is rejected with `DomainError::InvalidIdentifier { kind, value, reason }` in both `FromStr` and `Deserialize`.
2. Identity is UUIDv7, generated locally, never derived from user content, never recycled, and never rewritten by any command including restore and purge.
3. `RfcUid` is a separate, independently stable string derived once from the immutable `EventId` (`RfcUid::for_event`). It is not the primary key and is never regenerated on edit. A UID reserved by a purged record stays reserved (A5 purge ledger) so no later record can reuse it.
4. **Short IDs are selectors, never identity.** A `ShortId` is 8–32 characters of lowercase hex, parsed case-insensitively with hyphens stripped, and is a prefix of the canonical 32-hex-character UUID. It is never stored in any column, never written to `audit_log`, and never appears in a JSON `id` field — only in a separate display-only `short_id` field.
5. Resolution is kind-scoped: a short selector is resolved against one entity kind, so a prefix shared by an event and a todo is not a collision. A 32-hex or canonical-hyphenated selector is matched exactly, never as a prefix.
6. Resolution returns `Unique(T)`, `Ambiguous { candidates, minimum_unique_len }`, or `NotFound`. There is no "first match" path and no API that returns `Vec<T>` to a mutation caller.
7. A mutating command that accepted a short selector re-verifies the resolved canonical ID **inside** its mutation transaction together with the caller's expected revision, so a row inserted between resolution and commit cannot change the target.

```rust
/// A display/entry-only prefix of a canonical UUID. Never persisted.
pub struct ShortId(String);

pub enum Selector<T> { Canonical(T), Short(ShortId) }

pub enum Resolution<T> {
    Unique(T),
    Ambiguous { candidates: usize, minimum_unique_len: usize },
    NotFound,
}
```

**Schema (A2 + A5).** `migrations/0001_foundation.sql` (applied, embedded, ledger version 1, name `foundation`) creates:

- `calendars` — UUID PK, `is_default`, lifecycle timestamps, and `calendars_one_default` — a partial unique index on `(is_default) WHERE is_default AND deleted_at IS NULL` — so at most one live default exists as a database invariant, not an application check.
- `events` — UUID PK, `calendar_id` FK, `rfc_uid text NOT NULL UNIQUE`, and a CHECK enforcing that a row is *either* timed (`timezone`, `starts_at`, `ends_at` all NOT NULL; all-day columns NULL) *or* all-day (`all_day_start`, `all_day_end` NOT NULL; timezone/instant columns NULL) — never both, never neither. Plus `ends_at > starts_at`, `all_day_end > all_day_start`, `extension_properties jsonb NOT NULL DEFAULT '{}'`, and **separate** `deleted_at` (local trash) and `remote_tombstoned_at` (remote deletion) columns.
- `todos`, `todo_dependencies` — identity/parent scaffolding with `parent_id <> id` and `todo_id <> blocked_by_id` checks. Graph acyclicity is C's transactional obligation, not a table property.
- `reminders` — exactly one target (`(event_id IS NOT NULL)::int + (todo_id IS NOT NULL)::int = 1`) and exactly one schedule form (`offset_seconds` xor `absolute_at`).
- `reminder_deliveries` — `UNIQUE (reminder_id, scheduled_for)` plus durable `claimed_at`/`delivered_at`/`dismissed_at`/`snoozed_until`/`deferred_reason`. This unique key is the foundation's contribution to T5: it is the durable claim that makes E's scanner idempotent by construction.
- `audit_log` — `id`, `transaction_id`, `entity_type`, `entity_id`, `operation`, `before_state jsonb`, `after_state jsonb`, `occurred_at`, indexed by `(entity_type, entity_id, occurred_at)`.
- `mg_calr_schema_migrations` — runner-owned ledger `(version bigint PK, name text NOT NULL, applied_at timestamptz)`.

Later migrations 2–5 (`todo_core`, `todo_recurrence`, `todo_reminders`, `event_lifecycle`) are owned by C and B; A owns only the runner contract they obey: strictly increasing unique versions, additive/`IF NOT EXISTS` DDL, no `DROP TABLE`, no `CREATE EXTENSION`, and no destructive rewrite of a timestamp column.

**A5 target migration `0006_audit_provenance.sql`:**

```sql
CREATE TABLE audit_transactions (
    transaction_id uuid PRIMARY KEY,
    operation_id uuid UNIQUE,                     -- caller idempotency key, nullable
    request_fingerprint bytea,                    -- sha256 over canonical request
    command text NOT NULL,                        -- e.g. 'event.edit'
    actor_os_user text NOT NULL,
    application_version text NOT NULL,
    started_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    committed_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    state_schema_version smallint NOT NULL DEFAULT 1,
    CHECK ((operation_id IS NULL) = (request_fingerprint IS NULL))
);
ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS sequence_in_transaction integer NOT NULL DEFAULT 0;
ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS undo_eligible boolean NOT NULL DEFAULT false;
ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS undo_blocked_reason text;
-- referential and append-only guarantees
ALTER TABLE audit_log ADD CONSTRAINT audit_log_transaction_fk
    FOREIGN KEY (transaction_id) REFERENCES audit_transactions(transaction_id);
CREATE UNIQUE INDEX IF NOT EXISTS audit_log_transaction_sequence
    ON audit_log (transaction_id, sequence_in_transaction);
CREATE TABLE purged_identities (           -- tombstone that outlives payload purge
    entity_type text NOT NULL,
    entity_id uuid NOT NULL,
    reserved_uid text,                     -- RFC UID reservation; never reusable
    purged_in_transaction uuid NOT NULL REFERENCES audit_transactions(transaction_id),
    purged_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (entity_type, entity_id)
);
CREATE UNIQUE INDEX IF NOT EXISTS purged_identities_uid ON purged_identities (reserved_uid)
    WHERE reserved_uid IS NOT NULL;
```

Audit invariants: `audit_log` and `audit_transactions` are append-only for the runtime role (`GRANT INSERT, SELECT` only; `UPDATE`/`DELETE` revoked), rows are written *inside* the same transaction as the mutation they describe, `before_state`/`after_state` are the full serialized aggregate at schema version 1, and a *local soft delete*, a *remote tombstone*, and a *purge* are three distinct recorded operations that never alias. Purge writes `purged_identities` before deleting payload, so the identity and its reserved UID survive.

**Temporal invariants owned by A (T2).** Every stored instant is `timestamptz` — no `timestamp without time zone` column may be added by any migration. An all-day boundary is a `date` and is never surrogate-encoded as a midnight instant. A timed row must carry a canonical IANA zone name alongside its instants; fixed abbreviations are rejected at the domain boundary (`EventTime::timed` parses the zone with `chrono_tz` and verifies each boundary's stored offset actually matches that zone at that instant, returning `OffsetTimezoneMismatch` otherwise). `audit_log.occurred_at` and `mg_calr_schema_migrations.applied_at` are server-clock `timestamptz`; `doctor` reports host/server clock skew rather than silently correcting it. A migration may never rewrite an existing timestamp column's type or value without a disposable-database round-trip proof (G5).

### 4.3 API contracts

Pure/library interfaces (authoritative; the CLI is a thin translation):

```rust
config::ConfigPaths::from_env(&HashMap<String,String>) -> Result<ConfigPaths, ConfigError>
config::resolve_config(&HashMap<String,String>, Option<&str>, Option<String>) -> Result<AppConfig, ConfigError>
config::load(Option<String>) -> Result<AppConfig, ConfigError>          // only filesystem boundary
config::ConnectionSettings::safe_summary(&self) -> String                // always redacted

storage::migration_status(&ConnectionSettings) -> Result<Vec<MigrationState>, StorageError>  // read-only
storage::migrate(&ConnectionSettings)          -> Result<Vec<MigrationState>, StorageError>  // only writer
storage::doctor(&ConnectionSettings)           -> Result<Vec<MigrationState>, StorageError>  // read-only

// target
diagnostics::run_checks(&AppConfig, Component) -> Result<CheckReport, AppError>              // read-only
storage::selector::resolve<T: EntityKind>(&ConnectionSettings, &Selector<T>) -> Result<Resolution<T>, StorageError>
storage::audit::begin(&Transaction<'_>, BeginAudit) -> Result<AuditTransactionId, StorageError>
storage::audit::record(&Transaction<'_>, AuditTransactionId, AuditEntry) -> Result<AuditId, StorageError>
storage::audit::claim_operation(&Transaction<'_>, OperationId, RequestFingerprint)
    -> Result<OperationClaim, StorageError>   // Fresh | Replay(result) | Reused(conflict)
```

- **No auth model.** There is no account, session, token, or HTTP endpoint. Authorization is entirely the PostgreSQL role's own grants under Unix-socket peer authentication.
- **No pagination or rate limiting** on the pure functions. `audit list` takes a bounded `--limit` (default 50, maximum 1000) and a `--since` filter and issues one bounded query; there is no cursor, because A owns no snapshot mechanism (D9 documents the same restriction).
- **JSON success envelope (A4, implemented):** `{"schema_version":1,"command":"<dotted.name>","ok":true,"data":{…}}` on stdout.
- **JSON error envelope (A4, implemented):** `{"schema_version":1,"ok":false,"error":{"code":"<stable_snake_case>","message":"<safe text>"}}` on stderr. Target adds an optional non-sensitive `details` object (field names, candidate counts, check IDs, remedies) — additive within schema version 1.
- **Compatibility policy:** within `schema_version: 1`, fields may be added and clients must ignore unknown fields; removing a field, changing a type or meaning, narrowing an enum, or renaming a `command` requires a schema-version bump. Error `code` values are append-only and never re-pointed at a different meaning.
- **Error code → exit status contract** (implemented in `AppError::code`/`AppError::exit_code`, `src/lib.rs`; target moves the table into `contracts/error-v1.json` so tests and docs read one source):

| Class | Exit | Example codes |
|---|---:|---|
| success | 0 | — |
| usage / missing input | 64 | `required_input_missing`, `input_unavailable` |
| validation / domain data | 65 | `invalid_input`, `selector_invalid` |
| selector or record not found | 66 | `event_not_found`, `todo_not_found`, `selector_not_found`, `selector_ambiguous` |
| database / storage unavailable | 69 | `database_unavailable`, `database_config_invalid`, `migration_drift`, `database_error` |
| internal / serialization | 70 | `serialization_error` |
| local file IO | 74 | `projection_missing`, `projection_unavailable`, `projection_write_failed` |
| optimistic concurrency | 75 | `event_version_conflict`, `todo_version_conflict` |
| configuration | 78 | `config_invalid`, `config_unavailable` |
| interrupted | 130 | (signal; no envelope) |

### 4.4 State management

- **Authority.** The user's local PostgreSQL database is the single authority for all persisted state. There is exactly one authority; A forbids any second one. The filesystem holds only *inputs* (the TOML file) and *derived, replaceable* artifacts.
- **Ownership.** `AppConfig` is constructed once per process in `run` (`src/main.rs`) and passed down immutably; there is no global mutable state, no lazily initialized singleton, no ambient environment read below `config::load`, and no cache that outlives the process. Every foundation command is a single-shot process: parse → resolve → act → render → exit.
- **New state container.** Target A5 introduces the audit/receipt writer as a *transaction-bound* helper, not a service object: it is injected as `&Transaction<'_>` so it is impossible to record provenance outside the mutation's own transaction. `OperationClaim` gives later features the exactly-once retry primitive (same operation ID + same fingerprint replays the recorded result; same ID + different fingerprint is a typed conflict and writes nothing).
- **Local vs. server-synced boundary.** All foundation state is local. There is no server-synced state, no remote mirror, and no reconciliation in A. `events.remote_tombstoned_at` exists as a *reserved column* so a later F slice can distinguish remote deletion; A never sets it.
- **Offline / draft persistence.** Every foundation command is offline by definition. There are no drafts: a cancelled prompt or a failed validation persists nothing. There is no autosave, no journal, no background process, no daemon, and no timer.
- Later clients (TUI, Quickshell) must consume the public command/JSON interfaces; they are forbidden from reading PostgreSQL directly, which keeps the authority boundary single even as consumers multiply.

### 4.5 Dependencies

Runtime crates already in `Cargo.toml`: `clap` (argv), `serde`/`serde_json`/`toml` (config and envelopes), `thiserror` (typed errors), `tokio` + `tokio-postgres` (connection and migrations), `uuid` (v7 identity), `chrono`/`chrono-tz` (timestamps and IANA zone validation), `sha2` (fingerprints), `fs2`/`libc`/`rustix` (advisory file locking and directory fsync, used by the non-foundation interop path). Dev-only: `assert_cmd`, `predicates`, `tempfile`.

- **New packages for the target delta:** none required. Short-ID resolution is SQL plus `uuid`; the audit model is SQL plus `serde_json`; structured diagnostics are `serde` plus stderr writing. A logging facade (`tracing`) is a candidate but is deliberately deferred (Q3) rather than added speculatively.
- **New assets:** none. No fonts, images, models, or data files. `contracts/error-v1.json` is generated from the crate's own error table, not a third-party artifact.
- **Infrastructure:** a user-administered PostgreSQL 18-compatible server reachable over a Unix socket. No CDN, no third-party service, no telemetry endpoint, no CalDAV/HTTP client, no DBus, no systemd unit, no notification backend. Adding any of these to the foundation is out of scope by definition.
- **License:** no `LICENSE` file exists yet (MIT vs. Apache-2.0 unresolved). Crate license/provenance audit is a release gate (H2), not a local-implementation gate.

### 4.6 Platform-specific considerations

- **Target platform:** Arch Linux with Hyprland; the code is portable Linux-first. Unix-socket peer authentication and `/run/postgresql` are Arch/Debian-conventional defaults, overridable via `[database].socket_dir`.
- **Version compatibility:** Rust edition 2024 with `rust-version = "1.85"`. PostgreSQL 18 is the compatibility target; `doctor` reports the server version and warns (not fails) below the supported floor so an older server degrades loudly rather than silently.
- **TZDB:** timezone correctness depends on `chrono-tz`'s bundled database. `doctor` reports the TZDB source and version so a stale zone database is visible before it produces wrong future instants. Stored zone *names* plus civil intent — not derived offsets — are what survive a TZDB update.
- **Directory fsync** is `#[cfg(unix)]`-gated in the existing durability helper; a non-Unix build must be explicitly out of support rather than silently less durable.
- **Feature flags / rollout:** none. A CLI has no gradual rollout; correctness is gated by tests and by `migrate` being explicit. The only "flag" is the opt-in integration-test environment pair.
- **No renderer/engine migration concern** applies: there is no GUI toolkit, no web view, and no graphics stack.

### 4.7 Performance budget

- **Startup:** `version` and `config paths` do argv parsing, an environment scan, and at most one small `read_to_string`; target p95 under 20 ms on the supported workstation, which is dominated by process spawn.
- **CPU/render:** output formatting is O(rows) over at most a few dozen foundation rows. No sorting of large sets, no recurrence expansion, no rendering loop.
- **Memory:** peak resident under 32 MiB for any foundation command; the migration set is a handful of embedded `&'static str` values compiled into the binary (all five current migrations total well under 20 KiB).
- **Database work:** `status`/`doctor` = one connection, one `to_regclass` probe, one ordered ledger query. `migrate` = one connection, one transaction, one advisory lock, and one statement batch per pending migration. `id resolve` = one indexed prefix query with a `LIMIT 2` so ambiguity detection never materializes a large candidate set. `audit list` = one bounded, indexed query.
- **Network payload:** exactly zero bytes for `version`/`config`; for explicit database commands, exactly the PostgreSQL wire traffic of the queries above over a local Unix socket by default.
- **Storage:** the ledger is one row per migration. Audit growth is proportional to mutations and is the user's disk cost; retention/pruning policy is G3 and must itself be audited.

---

## 5. Test Specification

### 5.1 Unit tests

Implemented (`tests/config_contract.rs`, `tests/domain_contract.rs`, `tests/migration_contract.rs`):

- `xdg_paths_use_each_distinct_base_directory` — set all four `XDG_*` vars to different roots; assert config/data/state/cache resolve to four distinct `mg-calr` directories and `config_file` is `config.toml` under the config root. Edge: no root aliases another.
- `xdg_paths_fall_back_under_home` — only `HOME` set; assert the four documented fallbacks. Edge: partial environment.
- `database_url_precedence_is_cli_then_environment_then_file_then_peer_default` — one fixture exercises all four sources and asserts `ConfigSource::{Cli, Environment, File, Default}` plus the peer defaults `mg_calr` and `/run/postgresql`. Edge: file URL present but overridden.
- `identifiers_round_trip_without_losing_their_type` — `CalendarId`/`EventId`/`TodoId` survive `Display` → `FromStr`. Edge: type distinctness is a compile-time property.
- `identifiers_serialize_as_canonical_strings_and_validate_on_decode` — JSON encodes as a canonical string and `"not-a-uuid"` fails with `invalid calendar identifier`. Edge: malformed serialized identity.
- `malformed_identifier_returns_typed_error` — `DomainError::InvalidIdentifier { kind: "event", .. }`. Edge: typed, not string, failure.
- `foundation_migration_is_embedded_and_covers_only_foundation_entities` — all six foundation tables present; **no `DROP TABLE`, no `CREATE EXTENSION`**. Edge: destructive SQL smuggled into an embedded migration.
- `migration_versions_are_strictly_increasing_and_unique`; `todo_core_migration_owns_project_schema_without_rewriting_history`; `recurrence_migration_converts_legacy_text_json_without_rewriting_instances`; `reminder_migration_bridges_delivery_identity_without_external_transport` (also asserts the foundation's `UNIQUE (reminder_id, scheduled_for)` and that the SQL contains no `NOTIFY`).

Target additions:

- `unknown_config_key_is_a_named_error` — a typo'd TOML key yields `config_unknown_key` naming the key, not a silently ignored setting.
- `env_precedence_prefers_namespaced_variable` — `MG_CALR_DATABASE_URL` wins over `DATABASE_URL`, both lose to `--database-url`.
- `safe_summary_never_contains_credentials` — property test over generated URLs containing user/password/query strings; assert no substring of the password appears in `safe_summary()`, in `StorageError` Display, or in any envelope message.
- `short_id_normalizes_and_bounds` — case-insensitive, hyphen-stripped, rejects <8, >32, and non-hex with `selector_invalid`.
- `short_id_is_never_identity` — a resolved `Selector::Short` produces a canonical `EventId`; a compile/API test proves no repository method accepts a `ShortId`.
- `resolution_reports_ambiguity_not_a_winner` — two seeded prefixes return `Ambiguous { candidates: 2, minimum_unique_len }`; there is no code path returning one of them.
- `exact_uuid_selector_is_not_a_prefix_match` — a full 32-hex selector matches only itself.
- `error_code_exit_table_is_total_and_stable` — every `AppError` variant maps to a code and exit present in `contracts/error-v1.json`; adding a variant without a table entry fails the build.
- `audit_entry_requires_a_transaction` — the audit writer's type signature admits only `&Transaction<'_>`; a spy proves an entry cannot be written on a bare `Client`.
- `operation_claim_replays_and_rejects` — same ID + same fingerprint returns `Replay`; same ID + different fingerprint returns `Reused` and writes nothing.
- Property test: `resolve_config` over generated environment maps never panics, always yields exactly one `ConfigSource`, and is deterministic for identical inputs.

### 5.2 Integration tests

`tests/postgres_integration.rs` is `#[ignore]`d by default and opt-in only. It asserts `MG_CALR_RUN_DATABASE_TESTS == "1"`, requires `MG_CALR_TEST_DATABASE_URL`, and — critically — `disposable_guard_uses_effective_database_name_not_url_substrings` proves the guard inspects the *parsed effective database name* (`Some("mg_calr_test")`), so `postgresql:///production?application_name=mg_calr_test` is refused. A user's live database is never touched by the default suite.

- `migration_is_idempotent_on_disposable_database` (implemented) — apply, re-apply, assert every embedded migration is `applied` and no duplicate effect.
- Target: apply migrations, assert every foundation table, CHECK constraint, partial unique index (`calendars_one_default`), and the `(reminder_id, scheduled_for)` unique key actually exists in `information_schema`, not merely in the SQL text.
- Target: two concurrent `migrate` processes against the same empty database → one applies, one blocks on the advisory lock then observes zero pending; the ledger has exactly one row per version.
- Target: inject a failure in the middle of a multi-statement migration → assert the transaction rolled back, no ledger row was inserted, and `status` reports the version still pending (**no partial schema**).
- Target: seed a ledger row with a wrong name → `migration_drift` before any DDL executes; seed a ledger version beyond the embedded set → `migration_unknown_version` and nothing dropped.
- Target: seed 1,000 synthetic rows with a shared 8-hex prefix → resolution reports `Ambiguous` with the correct `minimum_unique_len`, and the query plan uses the identity index.
- Target: a mutation plus its audit rows commit or roll back together under injected fault at each write point; `audit_log` `UPDATE`/`DELETE` attempts by the runtime role are refused by grants.
- Target: assert the runtime role is `NOSUPERUSER NOCREATEDB NOCREATEROLE` and that `migrate` succeeds without it — migrations run as the schema owner, never as `postgres`.

### 5.3 UI / E2E tests

"UI" is the process boundary; these are `assert_cmd` process tests in `tests/cli_contract.rs` (implemented unless marked target):

- `version_json_has_a_stable_envelope` — `"schema_version":1`, `"command":"version"`, `"ok":true`.
- `config_paths_json_respects_xdg_without_touching_database` — an injected `XDG_CONFIG_HOME` appears in stdout; no connection is attempted.
- `no_color_environment_is_accepted_for_human_output` — with `NO_COLOR=1`, stdout matches no `\x1b[` sequence.
- `invalid_configuration_is_a_stable_json_error` — malformed TOML → exit **78**, stderr contains `"code":"config_invalid"`.
- `init_reports_unavailable_database_without_failing_or_mutating` — an unreachable `--database-url` still exits 0 with `"database_reachable":false` and `administrator_guidance`.
- `no_input_calendar_create_reports_missing_name_without_database_access` and siblings — `--no-input` fails fast with a named field and never opens a socket.
- Target `doctor_fails_read_only_with_prerequisite_matrix` — unreachable database → exit 69, one row per check with a `fail` status word and a remedy, and no server mutation (verified by an unchanged disposable database).
- Target `doctor_output_never_contains_credentials` — a `--database-url` carrying `user:hunter2@` produces no occurrence of `hunter2` on stdout or stderr in either output mode.
- Target `json_success_goes_to_stdout_and_errors_to_stderr` — exactly one object on the correct stream, and stdout is empty on failure.
- Target `no_color_flag_and_env_are_equivalent` and `narrow_terminal_preserves_identity` — at `COLUMNS=40`, IDs, check IDs, status words, and remedies survive intact.
- Target `ambiguous_selector_never_mutates` — a short selector matching two rows exits 66 and the disposable database is byte-identical afterwards.
- Target `migrate_dry_run_changes_nothing` — `--dry-run` prints the plan, exits 0, and leaves the ledger untouched.
- Target `help_documents_every_flag` — `--help` for each foundation subcommand lists every flag in the §3.1 grammar; a drift between grammar and help fails.

### 5.4 Visual / manual verification

- **Theme variants:** run every foundation command on a light and a dark terminal profile, with default color, `--no-color`, and `NO_COLOR=1`. No status, severity, or applied/pending meaning may disappear in any of the six combinations.
- **Text size extremes:** at the terminal's smallest and largest font sizes, confirm no output depends on a specific cell grid and that wrapped remedies remain readable.
- **Screen size extremes:** 40, 80, and 200 columns. At 40, tables must degrade to labeled records rather than truncating a UUID, a check ID, or a remedy; at 200, output must not stretch into unreadable column gaps.
- **Empty vs. populated:** `database status` against a database with no ledger (all `no`) and one fully migrated (all `yes`); `audit list` with zero records (`No audit records match.`) and with several hundred (bounded by `--limit`).
- **Screen reader:** read `doctor` output with a screen reader and confirm the reading order matches the table order and that every status is spoken as a word.
- **Prerequisite walkthrough:** on a container with no PostgreSQL installed, follow `init` output literally; confirm the printed commands are the complete path to a working `database migrate`, and that no step required `sudo mg-calr`.

### 5.5 Required quality gates

```text
cargo fmt --all -- --check
TMPDIR=/dev/shm cargo clippy --workspace --all-targets --all-features -- -D warnings
TMPDIR=/dev/shm cargo test --workspace --all-targets --all-features
git diff --check
```

Plus, before release: the opt-in disposable-PostgreSQL suite, a secret-scan over source and test fixtures, a check that the default `cargo test` run opens no socket, and a clean-machine smoke test (H7). A green happy-path suite never overrides a failed migration, privilege, redaction, or auto-fail gate.

---

## 6. Compliance & Safety Gate

### 6.1 Sensitive data classification

- [ ] No sensitive data involvement
- [x] **Handles sensitive data** — indirectly and by proximity. The foundation itself stores no calendar content, but it (a) creates the schema that will hold personally revealing titles, locations, attendees, and schedules; (b) resolves and may receive a PostgreSQL connection string that can contain a password; (c) writes `audit_log` rows whose `before_state`/`after_state` will contain full record payloads. Protections: connection strings are never persisted by `mg-calr` and are displayed only through `ConnectionSettings::safe_summary`, which discloses the *source* and redacts the URL; no error, diagnostic, or log line prints a URL, a password, or a record payload; the audit tables inherit the database's own file/role protection and are append-only to the runtime role; the config file is checked for over-permissive modes by `doctor` with a `chmod 600` remedy; a non-loopback TCP connection without TLS is refused rather than transmitting credentials in the clear.
- [x] **Uses synthetic/test data only** — every test fixture is synthetic, the integration suite refuses any database whose effective name is not `mg_calr_test`, and the default suite requires no database at all.

### 6.2 Asset provenance

- [x] **No third-party assets** — no images, fonts, models, icons, sounds, or bundled datasets. The only third-party material is Rust crate source and the IANA timezone database vendored inside `chrono-tz`, which are dependencies rather than user-facing assets. Their licenses, provenance, and update path must pass a dependency audit before public release (H2), and no `LICENSE` file has been chosen for `mg-calr` itself yet.
- [ ] Uses third-party assets

### 6.3 Language / claims audit

- [ ] Makes claims not supported by evidence — **no.** §7 separates implemented from absent; target behavior is labeled as target throughout.
- [ ] Promises capabilities not yet built — **no.** Foundation `--help` text must not advertise calendars, recurrence, reminders, sync, iCalendar, backup, TUI, or Quickshell. `init` prints administrator *examples* and explicitly says `mg-calr` never runs them.
- [ ] Uses language restricted by domain regulations — **no.** There is no health, financial, legal, or safety claim. The word "doctor" is a CLI convention (`brew doctor`, `flutter doctor`), not a medical claim. No security guarantee is asserted beyond what tests prove; in particular the product does not claim encryption at rest or secure erasure.

### 6.4 Regulatory alignment

Walking the Lens 3 criteria by name, per the criteria file's allowance that a foundation spec may mark I1–I3 N/A **only** with explicit deferral plus architecture that does not preclude them:

- **I1 Lossless iCalendar — N/A, explicitly deferred to F1–F3.** The foundation implements no iCalendar parser, serializer, or round trip, so there is no property to drop. Architecture that keeps I1 reachable: `events.extension_properties jsonb NOT NULL DEFAULT '{}'` already exists in migration 1 as the reserved home for unsupported properties; the migration runner forbids `DROP TABLE` and destructive rewrites, so no foundation change can destroy that column's contents; `rfc_uid` is a stable independent column rather than a derived display value; and the migration contract is additive, so F1 can version the envelope shape (raw bytes, order, parameters, hash, quarantine state) without a destructive migration. A's JSON envelope is explicitly *not* an interchange codec and must never be described as one.
- **I2 Sync authority — partially addressed now, orchestration deferred to F4–F12.** A affirmatively establishes the single authority the criterion requires: the local PostgreSQL database is authoritative, `AppConfig` is resolved once and passed immutably, there is no daemon, cache, or second store, and later clients (TUI, Quickshell) are forbidden from reading PostgreSQL directly and must use public commands. What is deferred is the durable vdir mirror, three-way fingerprints, and interruption recovery. Architecture that keeps I2 reachable: `sha2` is already a dependency and A5's `request_fingerprint` establishes the canonical-fingerprint primitive; per-aggregate `version` columns exist for optimistic checks; and no foundation API accepts "remote wins" or an unconditional write, so a future mirror cannot become a competing authority by accident.
- **I3 Conflict/deletion — N/A, explicitly deferred to F8–F10.** There is no remote side in A, so there is nothing to conflict with. Architecture that keeps I3 reachable and forbids the auto-fail: migration 1 already separates `deleted_at` (local trash) from `remote_tombstoned_at` (remote deletion) as distinct columns, and A5 adds `purged_identities` so a purged identity and its reserved RFC UID survive payload deletion — the three states can never alias. A has no code path that clears a tombstone, no automatic overwrite, and no last-writer-wins fallback; `OperationClaim` makes a retried write replay rather than re-apply.
- **I4 Scope/network — applies now and is fully addressed; never N/A.** `version`, `config paths`, and `config show` perform no socket operation of any kind. `init`, `doctor`, `database status`, `database migrate`, `id resolve`, and `audit *` are explicit database-related commands and may open exactly one connection to the configured PostgreSQL server — which the criteria file expressly permits. The default connection is a Unix domain socket at `/run/postgresql`, which is not network access at all. There is no HTTP client, DNS lookup, CalDAV request, vdirsyncer invocation, DBus call, notification transport, telemetry, or update check anywhere in the crate, and no such dependency is listed in `Cargo.toml`. A non-loopback TCP database host is refused unless explicitly allowed and TLS-capable. Test obligation: a process-level assertion that the default `cargo test` run performs no socket operation, and per-command assertions that non-database foundation commands connect to nothing.

**Auto-fail rules, addressed explicitly:**

- *Silent event/todo loss* — no foundation command deletes user rows; migrations are additive with `DROP TABLE` forbidden and contract-tested; a failed migration rolls back entirely; a failed selector resolution writes nothing.
- *Unconfirmed overwrite* — A introduces no overwrite path. `migrate` skips already-applied versions rather than re-running them; `OperationClaim` replays rather than re-applies; version/revision columns exist so B/C mutations must supply an expected version.
- *UID instability* — identity is UUIDv7 assigned once, typed, immutable, and never regenerated on edit, restore, export, or import; `RfcUid` is derived once from the immutable `EventId`; short IDs are selectors that are never stored; `purged_identities.reserved_uid` prevents reuse after purge.
- *Recurrence/exception corruption* — A implements no recurrence; it only guarantees that recurrence columns are additive and that no migration rewrites them destructively.
- *Timezone/DST drift* — every instant column is `timestamptz`, all-day boundaries are `date`, timed rows carry a canonical IANA zone that `EventTime::timed` validates against the stored offset, fixed abbreviations are rejected, and no migration may convert a timestamp column without a proven round trip.
- *Duplicate reminder delivery / non-idempotent scans* — `reminder_deliveries` carries `UNIQUE (reminder_id, scheduled_for)` plus durable claim columns, so the durable unique claim exists before any scanner does; A itself never claims, delivers, or presents, and the reminder migration is contract-tested to contain no `NOTIFY`.
- *Plaintext credentials or secret logging* — `mg-calr` never writes a credential to disk; `safe_summary()` is the only display path; a redaction property test and a secret scan are release gates; the config file's mode is checked; TLS is required for non-loopback hosts.
- *Network access outside explicit synchronization* — see I4 above; the database carve-out is honored exactly and no other network capability exists in the dependency graph.
- *Automatic conflict overwrite* — none exists; drift and reused-operation cases stop with a typed error and no write.
- *Loss of unsupported iCalendar properties on round trip* — A performs no round trip; the reserved `extension_properties` column is protected from destructive migration.

---

## 7. Gap Analysis vs. Current State

### 7.1 What exists today

**Implemented.**

- A1: `src/config.rs` — `ConfigPaths::from_env` (four distinct XDG roots with `HOME` fallbacks), `resolve_config` (pure, injected environment), `load` (single filesystem boundary), `ConnectionSettings` with `ConfigSource` provenance and `safe_summary()` redaction, `config/example.toml` documenting peer fields and warning against committed credentials. Verified by `tests/config_contract.rs`.
- A2: `src/storage.rs` — `MIGRATIONS` (five embedded migrations), `migration_status`, `migrate` (ledger creation, `pg_advisory_xact_lock(6851863988)`, per-version name check, transactional apply, single commit, re-read status), `doctor` (delegates to `migration_status`, read-only), `postgres_config` peer/URL translation, and `StorageError::Connect` carrying administrator guidance. `mg-calr init` reports readiness plus administrator examples and exits 0 even when unreachable. Verified by `tests/migration_contract.rs`, `tests/cli_contract.rs`, and the opt-in `tests/postgres_integration.rs`.
- A3 (identity half only): `src/domain.rs` `domain_id!` generates nominal `CalendarId`, `EventId`, `ReminderId`, `DeliveryId`, `AuditId` (plus `TodoId`/`ProjectId`) as UUIDv7 with typed parse errors; `RfcUid::for_event` derives a stable UID from the immutable event ID. Verified by `tests/domain_contract.rs`.
- A4: `src/lib.rs` — `AppError` with `code()` and `exit_code()` covering the 64/65/66/69/70/74/75/78 classes, `Envelope`/`ErrorEnvelope`/`ErrorBody` at `schema_version: 1`, and the single process-boundary renderer in `src/main.rs::main` that writes JSON errors to stderr and returns `ExitCode::from(error.exit_code())`. Global `--json`, `--no-input`, `--no-color`, `--database-url` are wired; `NO_COLOR` is read.
- A5 (schema only): `migrations/0001_foundation.sql` creates `audit_log` with transaction/entity identity, before/after JSON, and an entity index; it also creates the `reminder_deliveries` durable unique claim and the separate `deleted_at` / `remote_tombstoned_at` columns.

**Prototyped.** `--no-color` is accepted and `NO_COLOR` is read into `_color_disabled` in `src/main.rs`, but no color is emitted anywhere, so the flag is currently a contract placeholder proven only by an absence-of-ANSI test. `doctor`/`init` return a migration list plus a `database_reachable` boolean rather than a per-check matrix. `print_debug` renders human output via `{:#?}` Debug formatting, which is stable enough to test but is not the labeled layout §3.3 specifies.

**Absent.** A3 short-ID normalization, bounds, kind-scoped prefix resolution, ambiguity reporting, and the `id resolve` command (no `ShortId`, `Selector`, or `Resolution` type exists — grep confirms no short-ID code in `src/`). A5 functional behavior: nothing in the crate ever writes an `audit_log` row (`audit_log` appears only in the migration SQL and `AuditId` only in `src/domain.rs`), and there is no `audit_transactions` table, no `mutation_receipts`/`OperationClaim`, no `purged_identities`, and no `audit` command. A4 structured logging: no logging facade at all — the only diagnostic output is `eprintln!` at the process boundary. The `contracts/error-v1.json` machine table, the doctor prerequisite matrix, `config show`, `migrate --dry-run`, the `MG_CALR_DATABASE_URL` namespaced variable, `deny_unknown_fields` on the TOML, the config-file permission check, and the TLS refusal for non-loopback hosts.

**Gated.** The PostgreSQL suite is gated behind `MG_CALR_RUN_DATABASE_TESTS=1` plus `MG_CALR_TEST_DATABASE_URL` and an effective-database-name guard; it is `#[ignore]`d by default so a developer's live database is never touched.

**Out of A's scope but present in the crate:** `src/application.rs`, `src/interop.rs`, `src/tui.rs`, and migrations 2–5 implement B/C/D/F-adjacent behavior. They consume A's contracts; this spec does not govern them.

### 7.2 Delta to spec

**New files/modules:** `src/domain/selector.rs`, `src/domain/audit.rs`, `src/storage/selector.rs`, `src/storage/audit.rs`, `src/diagnostics.rs`, `contracts/error-v1.json`.

**Modified files:** `src/config.rs` (namespaced env variable, `deny_unknown_fields`, `[compat] config_version`, permission probe); `src/storage.rs` (TLS/loopback guard, `migration_unknown_version`, dry-run plan, extract the foundation block as repositories grow); `src/lib.rs` (optional `details` on the error body, table-driven code/exit mapping); `src/main.rs` (`config show`, `id resolve`, `audit list|show`, `doctor --component`, `migrate --dry-run`, labeled human layout replacing `{:#?}`); `README.md` and `docs/ARCHITECTURE.md` (document the new commands and precedence).

**Migrations/schema:** `migrations/0006_audit_provenance.sql` adding `audit_transactions`, `purged_identities`, `audit_log.sequence_in_transaction`/`undo_eligible`/`undo_blocked_reason`, the transaction foreign key, and the append-only grant policy. Strictly additive; no existing column is rewritten.

**Tests:** the target unit, integration, and process tests enumerated in §5.1–5.3, plus the redaction property test, the no-socket assertion for the default suite, and the role-grant assertions.

**New dependencies:** none required. A structured-logging facade remains deferred.

### 7.3 Estimated scope

**M.** The shipped slice (A1, A2, identity-A3, A4 envelopes) is done and tested. The remaining delta is bounded and well-understood: one additive migration, two small domain modules, two storage helpers, a check-matrix runner, and roughly a dozen new commands/flags — no new dependency, no algorithmic risk, no protocol work, and no data rewrite. It is larger than S because A5 touches the transaction discipline every later feature must obey and therefore needs fault-injection integration tests, and because the redaction and no-network guarantees need real adversarial tests rather than assertions. It is smaller than L because nothing here requires an evidence spike.

### 7.4 Blocking dependencies

- **External, for database commands only:** a PostgreSQL 18-compatible server plus an administrator-created peer role and `mg_calr` database. Non-database foundation commands and the default test suite have no such dependency.
- **Internal ordering:** A5's `audit_transactions`/`OperationClaim` must land before B or C acceptance, because those features' expected-revision and receipt contracts (`gauntlet-output/specs/b-event-calendar-core.md` §4.4.1, `c-todo-core.md` §7.4) depend on it. A3 short-ID resolution must land before D10's chooser and before any C/D command advertises short selectors; D9 and D10 own the display/chooser layer above A3's resolver and must not re-implement it.
- **Nothing in A blocks on B–I.** A must never take a dependency on a downstream feature; the direction is one-way by design.
- **Release-only gates:** the MIT vs. Apache-2.0 decision blocks adding `LICENSE` and publishing (H2); G5 must define forward-only/rollback policy before any breaking migration ships to a database holding real data.

---

## 8. Open Questions

- **Q1:** MIT or Apache-2.0? — blocks: adding `LICENSE`, crate metadata, and any public release (H2). Does not block local implementation.
- **Q2:** Should the minimum short-ID length be 8 hex characters, or should it scale with row count (e.g. widen automatically past a threshold)? The spec fixes 8 as the floor and requires deterministic widening in *display*; whether *input* should also be rejected below a computed floor is undecided. — blocks: final A3 tuning, not the ambiguity invariant, which is binding either way.
- **Q3:** Adopt `tracing` for the `--verbose` diagnostic stream, or hand-roll a minimal redacting writer? Adding a facade brings a subscriber ecosystem and a supply-chain surface; hand-rolling keeps the dependency graph minimal but risks re-inventing filtering. — blocks: A4 structured-logging implementation, not the redaction rules, which apply to either choice.
- **Q4:** What is the retention policy for `audit_log`? Unbounded growth is honest but eventually large; any pruning must itself be audited and must not silently destroy the provenance a later undo depends on. — blocks: G3 backup/retention; A5 must ship append-only with no pruning until this is answered.
- **Q5:** Should `migrate` ever support a downgrade path, or is forward-only plus restore-from-backup the permanent policy? — blocks: G5 migration compatibility/rollback specification; until answered, `migration_unknown_version` fails closed and drops nothing.
- **Q6:** Should `init --create-dirs` be allowed to create the XDG data/state directories, given that `init` is otherwise strictly non-mutating? The spec defaults it off and confines it to user-owned local directories, never the database. — blocks: nothing; a default-off flag is safe either way.
