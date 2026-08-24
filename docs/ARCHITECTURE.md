# Foundation architecture

## Current boundaries

- `src/domain.rs`: UUIDv7-backed nominal identifiers and domain errors; no CLI/SQL dependencies.
- `src/config.rs`: pure XDG/config precedence resolution plus filesystem loading at the boundary.
- `src/storage.rs`: PostgreSQL connection translation and embedded migration/status operations.
- `src/main.rs`: Clap parsing, command dispatch, process exit codes, and human/JSON rendering.
- `migrations/`: append-only SQL embedded into the binary.

## Configuration precedence

Database URL: `--database-url` > `DATABASE_URL` > `[database].url` > Unix-socket peer settings. Peer setting fields come from TOML or defaults (`/run/postgresql`, current OS user, `mg_calr`). Connection summaries always redact URLs.

## Migration contract

`database migrate` creates the migration ledger if needed, acquires a transaction-scoped advisory lock, checks each embedded version/name, applies pending SQL and ledger insertion in one transaction, then reports status. Re-running is idempotent. A recorded version with a different name is drift and fails. `database status`, `doctor`, and `init` query only and do not create the ledger.

Migration 1 establishes only identity/integrity scaffolding for calendars, events, todos and their separate dependency graph, reminders/delivery deduplication, and audit records. Later slices must validate richer graph, temporal, recurrence, synchronization, and undo invariants transactionally; table presence is not workflow implementation.

## Connectivity boundary

`version` and `config paths` read environment/config only. `init`, `doctor`, and `database` are explicit database-related operations and may connect. There is no sync or other network client in this slice.
