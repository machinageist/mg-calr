# Spec: First Deployable Application Foundation

**Feature ID:** A-foundation
**Parent:** A (A1–A5 foundation subset)
**Date:** 2026-08-23
**Iteration:** 1

## 1. Purpose

### 1.1 Job
Provide a safe, scriptable local foundation that resolves configuration, diagnoses PostgreSQL readiness, and applies an embedded baseline schema without implying later calendar workflows exist.

### 1.2 Why
All event, todo, reminder, audit, and integration slices need stable identity, storage, error, path, and automation contracts before user mutations are safe.

### 1.3 Success signal
On a clean checkout, all default tests run without a database; stable version/config JSON works without connecting; an explicitly selected disposable PostgreSQL database can report and idempotently apply migration 1.

## 2. User stories

- As a user, I can print the version and distinct resolved XDG paths without database or network access.
- As an operator, I can run `init`/`doctor` and receive actionable role/database provisioning guidance without sudo or server mutation.
- As a developer, I can explicitly run `database migrate` and safely rerun it without duplicate schema effects.
- As an integration author, I receive a versioned JSON envelope and deterministic error code/exit status.
- As a privacy-conscious user, connection URLs and credentials never appear in summaries.
- As a tester, normal tests never access my live database; integration requires explicit disposable opt-in.

## 3. UX specification

### 3.1 Command inventory

- `mg-calr version`: build version.
- `mg-calr config paths`: config/data/state/cache paths.
- `mg-calr init`: non-provisioning readiness diagnosis and explicit administrator examples.
- `mg-calr doctor`: non-mutating connection/migration diagnosis.
- `mg-calr database status`: query-only migration status.
- `mg-calr database migrate`: explicit schema mutation.
- Global `--json`, `--no-color`, `--database-url URL`; `NO_COLOR` and `DATABASE_URL` environment behavior.

### 3.2 Flows

1. Parse arguments locally.
2. Resolve XDG roots and optional TOML. Invalid TOML fails with exit 78 and `config_invalid` under JSON.
3. Non-database commands render without a connection.
4. Explicit database-related commands connect using CLI > environment > file URL, otherwise Unix-socket peer settings.
5. Connection failure exits 69 with guidance that an administrator must create the matching role/database; no provisioning is attempted.
6. Migrate acquires a transactional advisory lock, verifies version/name, applies pending SQL and ledger record atomically, commits, then reports state.

### 3.3 Layout/input/animation/accessibility

Terminal-only. Human output is plain text and JSON is one compact object. All functions are keyboard invocable. No gesture, sound, animation, modal, color-only meaning, or pointer requirement applies. `--no-color` and `NO_COLOR` are accepted; foundation output contains no ANSI escapes. Error recovery is described in text and fields, not color.

### 3.4 Error states

| Trigger | Presentation | Recovery | Data-loss risk |
|---|---|---|---|
| Missing XDG bases and HOME | typed config error / JSON code | set HOME or XDG roots | none |
| unreadable/invalid TOML | path-aware/parse error; exit 78 | repair/remove file | none |
| invalid URL | database config error; exit 69 | correct override | none |
| role/database/server unavailable | redacted actionable error; exit 69 | administrator provisions/starts service | none; query never migrates |
| recorded version/name mismatch | `migration_drift`; exit 69 | inspect before manual recovery | none; transaction stops |
| SQL failure | database error; transaction rollback | correct compatibility/problem and rerun | no partial migration by contract |

## 4. Implementation specification

### 4.1 Placement

- `src/domain.rs`: nominal UUIDv7 identifiers and `DomainError`.
- `src/config.rs`: pure resolution and filesystem loader.
- `src/storage.rs`: connection adapter and migration runner/status.
- `src/main.rs`: CLI/process rendering only.
- `migrations/0001_foundation.sql`: embedded baseline.

### 4.2 Data model

Migration 1 creates:

- `calendars` with immutable UUID identity and one-live-default index.
- `events` with independent RFC UID, calendar FK, exclusive timed/all-day representation, extension JSON, soft-delete and remote-tombstone distinction.
- `todos` with self-parent foundation and fixed priority vocabulary; `todo_dependencies` is separate.
- `reminders` targeting exactly one event/todo and defining exactly one offset/absolute schedule.
- `reminder_deliveries` with `(reminder_id, scheduled_for)` uniqueness and durable claim/deliver/dismiss/snooze/defer fields.
- `audit_log` with transaction/entity identity and before/after JSON.
- `mg_calr_schema_migrations` is runner-owned and records ordered version/name/application time.

Schema is scaffolding only. DAG cycle rejection, parent completion, recurrence, sync reconciliation, scanner behavior, and undo eligibility require later transactional application slices/evidence spikes.

### 4.3 Contracts

- `ConfigPaths::from_env(map)` and `resolve_config(map, TOML?, CLI URL?)` are deterministic and testable without mutating process environment.
- IDs (`CalendarId`, `EventId`, `TodoId`, `ReminderId`, `DeliveryId`, `AuditId`) cannot substitute for one another at compile time and reject malformed UUID text with `DomainError`.
- JSON success: `{schema_version:1, command, ok:true, data}`.
- JSON error: `{schema_version:1, ok:false, error:{code,message}}` on stderr.
- Config errors exit 78; storage errors 69; serialization errors 70.
- URL summaries disclose source only and explicitly redact credentials.

### 4.4 State and dependencies

PostgreSQL 18-compatible local state is authoritative. No server, global mutable state, background process, HTTP client, notification client, or sync transport exists. Dependencies are Clap, Serde/JSON/TOML, thiserror, Tokio/tokio-postgres, and UUID. Test-only dependencies are assert_cmd, predicates, tempfile.

### 4.5 Performance

Version/config commands perform environment parsing and at most one small TOML read. Database status performs one connection, existence query, and ordered ledger query. One small migration is embedded. No network payload exists. Startup target is normal Rust CLI startup; no daemon/cache is introduced.

## 5. Test specification

### 5.1 Unit/contract tests

- Distinct XDG roots and HOME fallbacks.
- Database precedence CLI > env > TOML > peer default.
- Typed identifier round-trip and malformed error.
- Embedded migration list order, required foundation tables, and absence of destructive/extension SQL.
- Version/config stable JSON, invalid TOML JSON error/exit, and NO_COLOR ANSI absence.

### 5.2 Integration

`postgres_integration` is ignored by default, requires `MG_CALR_RUN_DATABASE_TESTS=1` and `MG_CALR_TEST_DATABASE_URL`, refuses URLs not visibly containing `mg_calr_test`, applies migration twice, and asserts all entries remain applied. It is intentionally not run against the user's live server.

### 5.3 UI/manual

N/A — terminal contracts are exercised by process tests. Manually inspect `--help`, JSON stdout/stderr separation, and a live connection failure for actionable, redacted text. No theme or viewport matrix applies in this slice.

## 6. Compliance and safety gate

- Calendar content can later be sensitive; this slice uses synthetic data and creates schema only.
- No credentials are stored. TOML permits a URL but examples omit secrets; output redacts every URL.
- No third-party assets. Crate licenses must be audited before release; repository LICENSE remains intentionally unresolved.
- No user-facing claim promises later CRUD, recurrence, reminders, sync, backup, TUI, or Quickshell.
- Criteria alignment: T1/T3/T4/T5 foundations, C2/C3/C5 contracts, I4 no-network boundary, and O1–O4 least privilege/testing are explicit. I1–I3 are deferred without introducing a competing codec/sync authority.

## 7. Gap analysis

### 7.1 Current state

Implemented in this slice: package/module boundary, XDG/TOML/override resolution, typed IDs/errors, redacted peer/URL connection configuration, migration runner/status/doctor/init, baseline schema, stable JSON, NO_COLOR behavior, default-isolated tests, and docs.

### 7.2 Remaining delta

A3 short-ID encoding/resolution and A5 functional audit transaction/undo behavior remain absent. Logging policy beyond deterministic CLI errors, production migration rollback policy, and CI PostgreSQL service validation remain future A/G/H slices. All B–I product workflows remain absent.

### 7.3 Scope

M — coherent cross-cutting foundation with one embedded migration and process contracts, deliberately excluding domain workflows.

### 7.4 Dependencies

PostgreSQL 18 installation and administrator-created peer role/database are external prerequisites only for explicit database commands/integration. License decision is not required for local implementation but blocks public release.

## 8. Open questions

- MIT versus Apache-2.0 remains unresolved and blocks adding `LICENSE`/release metadata.
- Short-ID encoding is deferred to its own evidence-backed slice.
- Migration downgrade/forward-only production policy needs G5 specification before a breaking migration.
