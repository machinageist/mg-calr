# mg-calr

`mg-calr` is a keyboard-first Rust calendar application for a local Linux workstation. One SQLite file is authoritative for calendars and events. Todo ownership is being extracted to the separate `mg-remindr` application; `mg-calr` can validate and atomically store an immutable `mg.interop/1` projection without connecting to the `mg-remindr` database.

## Implemented surface

The current incremental implementation includes:

- XDG configuration, stable JSON/error envelopes, store diagnostics, and embedded migrations;
- calendar creation/listing and timed or all-day event create/list/show/day-agenda/edit/cancel/restore;
- deterministic combined agenda queries, rendered as a day ordered by the clock
  with times restated in the zone the agenda was asked for, and a bounded
  line-oriented keyboard shell;
- bounded event recurrence — daily, weekly and monthly, with an optional weekday
  set — set with `event create --repeat`, and expanded into occurrences at read
  time rather than stored per day;
- an iCalendar reader that imports `VEVENT` records and refuses by name any RRULE
  part this application cannot represent;
- project and tag CRUD remain available for their current tables; todo CRUD and todo JSON interchange have moved to `mg-remindr`;
- calendar/event JSON interchange remains available;
- validated `mg-remindr` projection import with canonical identity, lifecycle, relationship, graph, revision, freshness-order, bounded-input, and conflict checks; and
- crash-safe projection replacement using interprocess advisory locking, atomic rename, file sync, and parent-directory sync.

Only todos carrying a due value appear on a day agenda, and completed ones are hidden
unless `--include-completed` is passed. `mg-remindr` is the producer of that projection;
see its README for how a reminder is kept and refreshed into the calendar.

Run `mg-calr --help` and the relevant subcommand help for the complete current command inventory. Add `--json` where machine-readable output is supported. `--no-input`, `--no-color`, and `NO_COLOR` are supported at their documented boundaries.

### Projection import

Projection refresh is explicit and projection-only:

```bash
mg-calr interop import-todo \
  --input /path/to/mg-remindr-snapshot.json \
  --store /path/to/mg-calr-todo-projection.json
```

The import validates the complete envelope before replacement, rejects stale or conflicting revisions, and never opens an `mg-remindr` database connection. Combined agenda and TUI reads keep calendar/event authority in the mg-calr store, but read todos only from the validated stored projection. By default they load `$XDG_DATA_HOME/mg-calr/todo-projection.json` (or `~/.local/share/mg-calr/todo-projection.json`); `--todo-projection FILE` selects an explicit imported store. Missing, stale, conflicting, and invalid projections fail closed with distinct diagnostics. The imported projection is the only todo representation mg-calr reads, and the store has no todo tables of its own; a missing projection is reported as a missing projection even when the calendar store cannot be opened at all.

## Configuration

The optional configuration file is `$XDG_CONFIG_HOME/mg-calr/config.toml` (default `~/.config/mg-calr/config.toml`). Data, state, and cache paths resolve independently under their XDG bases. See `config/example.toml`.

The store is one SQLite file, `$XDG_DATA_HOME/mg-calr/calr.sqlite` by default, in WAL with foreign keys on. `--db PATH` overrides `MG_CALR_DB`, then TOML configuration. Nothing needs provisioning:

```bash
mg-calr database migrate
```

Opening never migrates, so `init`, `doctor` and `database status` diagnose only. A command opens the store where it needs it, so input the command itself rules out is reported without creating one.

### Coming from the PostgreSQL store

The interchange document is lossless, so the old database's rows move across as they stand — identities, versions and recorded times included:

```bash
mg-calr event export > calendars-and-events.json   # from the PostgreSQL build
mg-calr event import --file calendars-and-events.json
```

## Development

```bash
cargo fmt --all -- --check
TMPDIR=/dev/shm cargo clippy --workspace --all-targets --all-features -- -D warnings
TMPDIR=/dev/shm cargo test --workspace --all-targets --all-features
git diff --check
```

Every test takes a store in a throwaway directory, so the suite needs nothing provisioned and nothing opted into.

## Remaining milestone scope

The projection import is an authority-boundary migration slice, not completion of the product roadmap. Per-occurrence exceptions (`EXDATE`, `RDATE`, `RECURRENCE-ID`) and editing a
single occurrence of a series remain incomplete, as does full event-core
lifecycle. Reminder delivery/service actions, search/bulk safety/audit, lossless iCalendar export, vdirsyncer/iCloud synchronization, backup/restore, a raw-mode TUI, and packaging remain open.

## License

MIT. See `LICENSE`.
