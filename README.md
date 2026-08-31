# mg-calr

`mg-calr` is a keyboard-first Rust calendar application for a local Linux workstation. PostgreSQL is authoritative for calendars and events. Todo ownership is being extracted to the separate `mg-todo` application; `mg-calr` can validate and atomically store an immutable `mg.interop/1` projection without connecting to the `mg-todo` database.

## Implemented surface

The current incremental implementation includes:

- XDG configuration, stable JSON/error envelopes, PostgreSQL diagnostics, and embedded migrations;
- calendar creation/listing and timed or all-day event create/list/show/day-agenda/edit/cancel/restore;
- deterministic combined agenda queries and a bounded line-oriented keyboard shell;
- legacy todo/project/tag CRUD, lifecycle, recurrence, dependency, reminder-ledger, and JSON interchange behavior retained during the `mg-todo` migration period;
- calendar/event JSON interchange and read-only `mg.interop/1` snapshot export;
- validated `mg-todo` projection import with canonical identity, lifecycle, relationship, graph, revision, freshness-order, bounded-input, and conflict checks; and
- crash-safe projection replacement using interprocess advisory locking, atomic rename, file sync, and parent-directory sync.

Run `mg-calr --help` and the relevant subcommand help for the complete current command inventory. Add `--json` where machine-readable output is supported. `--no-input`, `--no-color`, and `NO_COLOR` are supported at their documented boundaries.

### Projection import

Projection refresh is explicit and projection-only:

```bash
mg-calr interop import-todo \
  --input /path/to/mg-todo-snapshot.json \
  --store /path/to/mg-calr-todo-projection.json
```

The import validates the complete envelope before replacement, rejects stale or conflicting revisions, and never opens an `mg-todo` database connection. Combined agenda and TUI reads keep calendar/event authority in the mg-calr PostgreSQL database, but read todos only from the validated stored projection. By default they load `$XDG_DATA_HOME/mg-calr/todo-projection.json` (or `~/.local/share/mg-calr/todo-projection.json`); `--todo-projection FILE` selects an explicit imported store. Missing, stale, conflicting, and invalid projections fail closed with distinct diagnostics rather than falling back to legacy todo tables. The existing mg-calr todo schema and commands remain in place for migration compatibility, but they are no longer an agenda read authority.

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

The projection import is an authority-boundary migration slice, not completion of the product roadmap. Event recurrence/exceptions and full event-core lifecycle remain incomplete. Reminder delivery/service actions, search/bulk safety/audit, lossless iCalendar, vdirsyncer/iCloud synchronization, backup/restore, raw-mode TUI, Quickshell integration, and packaging remain open.

No `LICENSE` is included because MIT versus Apache-2.0 remains unresolved.
