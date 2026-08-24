# mg-calr

`mg-calr` is a keyboard-first Rust calendar/reminder/todo application for a local Linux workstation. This repository currently contains **only the first deployable foundation slice**: XDG configuration, typed identifiers/errors, PostgreSQL connection and embedded schema migration infrastructure, and stable foundation diagnostics.

## Implemented commands

```text
mg-calr version
mg-calr config paths
mg-calr init
mg-calr doctor
mg-calr database status
mg-calr database migrate
```

Add `--json` for the versioned JSON envelope. `--no-color` and `NO_COLOR` are accepted globally; current foundation human output intentionally emits no ANSI styling. `--database-url URL` overrides `DATABASE_URL`, then TOML configuration. Commands other than `init`, `doctor`, and `database ...` do not connect to PostgreSQL or make network requests.

## Configuration

The optional configuration file is `$XDG_CONFIG_HOME/mg-calr/config.toml` (default `~/.config/mg-calr/config.toml`). Data, state, and cache paths resolve independently under their XDG bases. See `config/example.toml`.

The default PostgreSQL connection uses `/run/postgresql`, the current OS user, database `mg_calr`, and peer authentication. `init` diagnoses only: it never runs `sudo`, creates roles/databases, or applies migrations. `database migrate` is the only foundation command that changes database schema.

An administrator must provision the peer role and database first. Review and adapt these examples rather than running them blindly:

```bash
sudo -u postgres createuser --login "$USER"
sudo -u postgres createdb --owner "$USER" mg_calr
mg-calr database migrate  # run unprivileged, not through sudo
```

A failure such as `role "<user>" does not exist` means provisioning is incomplete; `doctor`/`status` return a nonzero error with this guidance and do not modify the server.

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

PostgreSQL integration is opt-in and ignored by default:

```bash
MG_CALR_RUN_DATABASE_TESTS=1 \
MG_CALR_TEST_DATABASE_URL=postgresql:///mg_calr_test \
cargo test --test postgres_integration -- --ignored
```

Use a disposable database whose URL contains `mg_calr_test`; the test applies schema.

## Not implemented yet

Event/todo CRUD, agenda views, recurrence behavior, reminders scanning/notifications, audit-backed undo, iCalendar, sync, backup/restore, TUI, Quickshell, packaging, and remote integrations are deferred. Schema tables are a migration foundation, not claims that those workflows exist.

No `LICENSE` is included because MIT versus Apache-2.0 remains unresolved.
