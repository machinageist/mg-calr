# mg-calr

`mg-calr` is a keyboard-first Rust calendar application for a local Linux workstation. PostgreSQL is authoritative for calendars and events. Todo ownership is being extracted to the separate `mg-remindr` application; `mg-calr` can validate and atomically store an immutable `mg.interop/1` projection without connecting to the `mg-remindr` database.

## Implemented surface

The current incremental implementation includes:

- XDG configuration, stable JSON/error envelopes, PostgreSQL diagnostics, and embedded migrations;
- calendar creation/listing and timed or all-day event create/list/show/day-agenda/edit/cancel/restore;
- deterministic combined agenda queries, rendered as a day ordered by the clock
  with times restated in the zone the agenda was asked for, and a bounded
  line-oriented keyboard shell;
- bounded event recurrence — daily, weekly and monthly, with an optional weekday
  set — set with `event create --repeat`, and expanded into occurrences at read
  time rather than stored per day;
- an iCalendar reader that imports `VEVENT` records and refuses by name any RRULE
  part this application cannot represent;
- project and tag CRUD remain available for their current PostgreSQL tables; todo CRUD and todo JSON interchange have moved to `mg-remindr`;
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

The import validates the complete envelope before replacement, rejects stale or conflicting revisions, and never opens an `mg-remindr` database connection. Combined agenda and TUI reads keep calendar/event authority in the mg-calr PostgreSQL database, but read todos only from the validated stored projection. By default they load `$XDG_DATA_HOME/mg-calr/todo-projection.json` (or `~/.local/share/mg-calr/todo-projection.json`); `--todo-projection FILE` selects an explicit imported store. Missing, stale, conflicting, and invalid projections fail closed with distinct diagnostics rather than falling back to legacy todo tables. The imported projection is the only todo representation mg-calr reads; the legacy todo tables remain only as migration residue and are not exposed as mg-calr todo commands or agenda authority.

## Configuration

The optional configuration file is `$XDG_CONFIG_HOME/mg-calr/config.toml` (default `~/.config/mg-calr/config.toml`). Data, state, and cache paths resolve independently under their XDG bases. See `config/example.toml`.

The default calendar PostgreSQL connection uses `/run/postgresql`, the current OS user, database `mg_calr`, and peer authentication. `--database-url URL` overrides `DATABASE_URL`, then TOML configuration. `init` diagnoses only: it never runs `sudo`, creates roles/databases, or applies migrations. `database migrate` applies the embedded schema migrations.

An administrator must provision the peer role and database first. Review and adapt these examples rather than running them blindly:

```bash
sudo -u postgres createuser --login "$USER"
sudo -u postgres createdb --owner "$USER" mg_calr
mg-calr database migrate  # run unprivileged, not through sudo
```

A failure such as `role "<user>" does not exist` means provisioning is incomplete; `doctor`/`status` return a nonzero error with guidance and do not modify the server.

## Development

```bash
cargo fmt --all -- --check
TMPDIR=/dev/shm cargo clippy --workspace --all-targets --all-features -- -D warnings
TMPDIR=/dev/shm cargo test --workspace --all-targets --all-features
git diff --check
```

PostgreSQL integration is opt-in and ignored by default:

```bash
MG_CALR_RUN_DATABASE_TESTS=1 \
MG_CALR_TEST_DATABASE_URL=postgresql:///mg_calr_test \
TMPDIR=/dev/shm cargo test --test postgres_integration -- --ignored
```

Use a disposable database whose effective database name contains `mg_calr_test`; the test applies schema.

## Remaining milestone scope

The projection import is an authority-boundary migration slice, not completion of the product roadmap. Per-occurrence exceptions (`EXDATE`, `RDATE`, `RECURRENCE-ID`) and editing a
single occurrence of a series remain incomplete, as does full event-core
lifecycle. Reminder delivery/service actions, search/bulk safety/audit, lossless iCalendar export, vdirsyncer/iCloud synchronization, backup/restore, a raw-mode TUI, and packaging remain open.

## License

MIT. See `LICENSE`.
