# Spec: Import, Export, and Synchronization

**Feature ID:** f-import-export-sync
**Parent feature:** root
**Spec author agent:** Spec agent F
**Date:** 2026-08-29
**Iteration:** 1

---

## 1. Purpose

### 1.1 One-sentence job

Let a user move calendar and todo data between the authoritative `mg-calr` PostgreSQL database and the outside world — iCalendar files, versioned JSON, and an iCloud CalDAV account reached through vdirsyncer — without ever losing a property, silently overwriting a change, or letting the tool touch the network outside an explicitly requested synchronization.

### 1.2 Why it matters

`mg-calr` is a local-first calendar whose value depends on not being a data prison. The user's real calendar already lives on iCloud, shared with people who use Apple Calendar, so a local tool that cannot round-trip that data is a toy. At the same time, every synchronization tool the user has been burned by fails in one of four ways: it drops properties it did not understand (`X-APPLE-*`, `ATTACH`, custom parameters), it resolves conflicts by picking a winner, it deletes local data because a remote listing looked empty, or it keeps an app password in a config file. This feature exists to make each of those failures structurally impossible: a lossless residual store so unknown data survives, three-way fingerprints so a conflict is detected rather than resolved, tombstones separate from soft deletes so an absence is never read as a deletion, and an external secret-command indirection so `mg-calr` never holds or logs a credential. It also fixes the scope trap: `mg-calr` writes and reads a durable vdir mirror; **vdirsyncer** owns the network.

### 1.3 Success signal

Against the synthetic Apple-generated `.ics` corpus and a disposable PostgreSQL database: import → export produces items whose canonical content-line multiset, parameter values, ordering metadata, organizer/attendee parameters, and opaque residual bytes are identical to the source (byte-identical for items unedited since import); a scripted three-way fixture in which both sides changed halts with `sync_conflict` and leaves both the database row and the mirror file byte-unchanged; a `SIGKILL` at every journal phase leaves a resumable run with no partial item; and the network-denial suite proves that every command other than an explicit `sync run`/`sync discover`/`sync doctor --probe-transport` completes normally inside an empty network namespace.

---

## 2. User Stories

> As a keyboard-first user, I want `mg-calr sync run` to pull my iCloud calendars into PostgreSQL and push my local edits back, so that my terminal calendar and my phone agree without me leaving the terminal.

> As a user with years of Apple Calendar history, I want every property `mg-calr` does not understand — `X-APPLE-TRAVEL-TIME`, `ATTACH`, structured location parameters — to come back out exactly as it went in, so that syncing through `mg-calr` never degrades my data for the people I share calendars with.

> As a cautious user, I want a run where both my laptop and my phone changed the same event to stop on that event, keep both versions, and tell me the exact command to inspect them, so that no edit of mine is ever silently replaced.

> As a user who deleted an event on my phone, I want `mg-calr` to record a tombstone and move the local event into a restorable state rather than hard-deleting it, and I want `sync tombstones restore` to bring it back byte-for-byte, so that a mistaken deletion is recoverable on either side.

> As a script author, I want `mg-calr ical export --json --no-input` and `mg-calr sync run --json --no-input --dry-run` to emit one versioned deterministic envelope, never prompt, and return a stable exit code per outcome class, so that I can put synchronization in a systemd timer without parsing prose.

> As a screen-reader user on a slow first sync, I want `--progress plain` to emit append-only phase lines with counts and no ANSI redraws, so that I can follow a long run without a terminal that overwrites itself.

> As a privacy-conscious user, I want my iCloud app-specific password to live in `pass` and be named in my config only as a command to run, and I want every URL in every diagnostic to be redacted, so that no credential is ever written to my config, my logs, my JSON output, or a bug report.

> As an administrator debugging a broken setup, I want `mg-calr sync doctor` to tell me what is missing — vdirsyncer absent, vdir root unwritable, secret command non-executable, collection unmapped — without connecting to iCloud, mutating anything, or asking for sudo.

---

## 3. UX Specification

This is a terminal product. There are no graphical screens, modals, sheets, popovers, or drawers. "Views" below are command output surfaces.

### 3.1 Screen / view inventory

| View | Command / navigation path | New vs. modified | Layout pattern |
|---|---|---|---|
| iCalendar export | `mg-calr ical export …` | New | streamed `.ics` to stdout or a directory tree; `--json` emits a manifest envelope |
| iCalendar import plan | `mg-calr ical import … --dry-run` | New | per-item action table, then a totals footer |
| iCalendar import result | `mg-calr ical import …` | New | same table with applied outcomes; conflicts listed separately |
| iCalendar validate | `mg-calr ical validate --file …` | New | one diagnostic list; no database access |
| Item property inventory | `mg-calr ical inspect SELECTOR` | New | labeled record: supported properties, then residual entries by index/name/length/SHA-256 (values withheld unless `--show-values`) |
| JSON interchange export | `mg-calr interop export --json` | **Modified** (exists) | one `mg.interop/1` envelope; gains `ical` residual carriage |
| JSON schema print | `mg-calr interop schema --name mg.ical/1` | New | one JSON Schema document on stdout |
| Todo projection import | `mg-calr interop import-todo …` | **Modified** (exists) | unchanged behavior; gains shared redaction/exit-code contract |
| Sync status | `mg-calr sync status` | New | per-collection table: mapped calendar, tracked items, pending local, pending mirror, conflicts, tombstones, last run |
| Sync doctor | `mg-calr sync doctor` | New | prerequisite matrix with stable check IDs, pass/fail/blocked, and recovery text |
| Sync config scaffold | `mg-calr sync init-config` | New | printed TOML for `mg-calr` plus printed vdirsyncer config; writes only with `--write PATH --yes` |
| Collection discovery | `mg-calr sync discover` | New | remote collection list (display name, key, proposed calendar mapping) |
| Sync plan | `mg-calr sync run --dry-run` | New | phase-ordered action plan and totals; no mutation |
| Sync run | `mg-calr sync run` | New | live phase progress, then the same plan table with outcomes |
| Conflict list / show | `mg-calr sync conflicts list|show ID` | New | list table; `show` renders a three-column base/local/mirror property diff |
| Conflict resolve | `mg-calr sync conflicts resolve ID --take …` | New | confirmation summary then result record |
| Tombstone list / restore / purge | `mg-calr sync tombstones …` | New | table plus per-action confirmation |
| Mirror verification | `mg-calr sync verify` | New | offline digest audit table |

Global flags from feature A apply to all of the above: `--json`, `--no-input`, `--no-color`, `--database-url`. `NO_COLOR` is honored identically to `--no-color`.

**Command grammar.**

```text
mg-calr ical export [--calendar CALENDAR_ID]... [--uid RFC_UID]...
    [--from DATE] [--to DATE] [--include-trashed] [--include-tombstones]
    [--fidelity verbatim|canonical] [--out FILE|--out-dir DIR|-] [--json]
mg-calr ical import (--file FILE|--dir DIR|-) --calendar CALENDAR_ID
    [--dry-run] [--on-existing stop|update|skip] [--if-revision UID=N]...
    [--allow-malformed-residual] [--yes] [--no-input] [--json]
mg-calr ical validate (--file FILE|--dir DIR|-) [--json]
mg-calr ical inspect SELECTOR [--show-values] [--include-quarantine] [--json]

mg-calr interop export --json
mg-calr interop import-todo --input FILE --store FILE
mg-calr interop schema --name mg.interop/1|mg.ical/1|mg.sync/1

mg-calr sync status [--remote NAME] [--collection KEY] [--json]
mg-calr sync doctor [--remote NAME] [--probe-secret] [--probe-transport] [--json]
mg-calr sync init-config [--remote NAME] [--write PATH --yes] [--json]
mg-calr sync discover --remote NAME [--json]
mg-calr sync map --remote NAME --collection KEY --calendar CALENDAR_ID [--yes]
mg-calr sync run [--remote NAME] [--collection KEY]... [--dry-run]
    [--direction both|pull|push] [--no-transport]
    [--deletions hold|propagate] [--confirm-deletions N]
    [--progress auto|plain|never] [--timeout SECONDS]
    [--yes] [--no-input] [--json]
mg-calr sync resume [--run RUN_ID] [--yes] [--no-input] [--json]
mg-calr sync verify [--collection KEY] [--repair-ledger] [--json]
mg-calr sync conflicts list [--collection KEY] [--json]
mg-calr sync conflicts show CONFLICT_ID [--side base|local|mirror] [--json]
mg-calr sync conflicts resolve CONFLICT_ID
    (--take local|--take mirror|--keep-both|--merge-file FILE)
    --if-revision N --if-mirror-digest SHA256 [--yes] [--no-input] [--json]
mg-calr sync tombstones list [--include-acknowledged] [--json]
mg-calr sync tombstones restore TOMBSTONE_ID --if-digest SHA256 [--yes] [--no-input]
mg-calr sync tombstones purge TOMBSTONE_ID --yes [--no-input]
```

Grammar rules:

- Every prompt has an equivalent flag. `--no-input` never reads stdin or `/dev/tty`; a missing required value returns `input_required` naming the exact flags that resolve it.
- `--yes` confirms only the fully resolved target already printed by dry validation. It never resolves ambiguity and never waives a revision or digest check.
- `SELECTOR` is an immutable event UUID or, once A3 lands, a short ID. An RFC UID is accepted only via the explicit `--uid` flag, never positionally, because UIDs are foreign-controlled strings.
- `--json` writes exactly one envelope to stdout; prompts, progress, and diagnostics go to stderr. `--json --no-input` is the supported automation combination.
- `mg-calr ical export -` and `mg-calr ical import -` use stdout/stdin so the codec composes with pipes without touching the vdir mirror.

**Exit codes** (extending feature A's scheme in `src/lib.rs`):

| Code | Meaning | Example |
|---:|---|---|
| 0 | success, including "no changes" | clean `sync run` |
| 64 | usage / required input missing under `--no-input` | `sync run --no-input` with pending deletions and no `--confirm-deletions` |
| 65 | invalid input data | unparsable `.ics`, `ical_invalid` |
| 66 | selector not found | unknown conflict ID |
| 69 | service unavailable | vdirsyncer missing, database unreachable, transport failed |
| 70 | internal serialization failure | envelope encoding error |
| 74 | I/O failure at the vdir/ledger boundary | mirror directory unwritable |
| 75 | conflict or stale precondition | `sync_conflicts_present`, `stale_revision`, `mirror_digest_mismatch` |
| 78 | configuration error | `secret_in_config`, unmapped collection, invalid `password_command` |

Exit 75 is chosen deliberately for "conflicts exist": a timer-driven `sync run` that halts on conflicts is not a crash and not a success, and a script must be able to distinguish it from a transport outage (69).

### 3.2 Interaction flows

**Primary flow — first synchronization.**

1. `mg-calr sync doctor` runs entirely offline. It checks: config present and free of literal secrets; `vdir_root` exists/creatable and is not a symlink; vdirsyncer present on `PATH` and its version parsed; `password_command` resolvable and executable; collections mapped to live calendars; migration `0006_sync` applied; ledger readable. Each row prints a stable check ID, a status, and, if failing, an exact recovery command. Nothing is mutated and no socket is opened.
2. `mg-calr sync init-config --remote icloud` prints two blocks: the `[sync.remote.icloud]` TOML for `mg-calr` and the corresponding vdirsyncer `config` block whose `password.fetch` is the *same* `["command", …]` indirection. It writes nothing without `--write PATH --yes`, and refuses to write over an existing file.
3. `mg-calr sync discover --remote icloud` is the first command permitted to cause network traffic. It spawns `vdirsyncer discover` and renders the returned collections with proposed calendar mappings. Before spawning it prints the redacted target (`https://caldav.icloud.com/… as u***@example.com`) and, interactively, asks for confirmation; under `--no-input` it requires `--yes`.
4. `mg-calr sync map --remote icloud --collection home --calendar CALENDAR_ID` writes one `sync_collections` row in one transaction. Mapping a collection to a calendar that already has a mapping fails `collection_already_mapped`.
5. `mg-calr sync run --dry-run` performs pull transport (vdirsyncer), then classifies every item three ways and prints the plan with zero database or mirror mutation.
6. `mg-calr sync run` executes the plan through the journal described in §4.4, reporting progress per phase, then prints the outcome table and totals.

**Steady-state flow — `sync run` phase machine.** Each phase writes its transition to `sync_runs.phase` before doing work, so an interrupted run is always resumable:

1. `planning` — load the ledger, scan the mirror, read authoritative rows, compute the plan digest. Read-only.
2. `pull_transport` — spawn vdirsyncer to bring the remote into the mirror. Skipped entirely with `--no-transport` or `--direction push`. This is the only phase in the pull half that can cause network traffic, and `mg-calr` itself opens no socket: it spawns a process.
3. `import` — for each item, three-way classify; apply mirror→database fast-forwards through feature B's revision-checked application boundary; record conflicts; record remote-deletion tombstones.
4. `export` — apply database→mirror fast-forwards by writing item files atomically (temp file, `fsync`, `rename`, parent directory `fsync`) and committing the ledger row only after the `fsync` returns.
5. `push_transport` — spawn vdirsyncer to push the mirror to the remote. Skipped under `--no-transport` or `--direction pull`.
6. `finalizing` — advance base fingerprints for converged items only, write run counts, mark `completed`.

**Branch — conflicts detected.** Classification stops the item. The database row and the mirror file are both left byte-unchanged, a `sync_conflicts` row is written with base/local/mirror digests and the local revision, and the item is excluded from both fast-forward directions. The run continues for other items and finishes with exit 75 and a footer naming the count and the exact `mg-calr sync conflicts list` command. Conflicts never block unrelated items, and a conflicted item is never pushed.

**Branch — resolution.** `sync conflicts show ID` renders a three-column property diff (base, local, mirror) with residual entries shown by index/name/hash. `sync conflicts resolve ID --take mirror --if-revision 7 --if-mirror-digest ab12…` requires both preconditions; a mismatch fails `stale_revision` or `mirror_digest_mismatch` with no write. `--take local` and `--take mirror` write the chosen side through feature B's `patch_event` with the expected revision, preserving the loser's exact bytes in `sync_item_bytes` and the conflict row forever. `--keep-both` writes the mirror side as a **new** event with a new `EventId` and a new RFC UID, records provenance linking it to the conflict, and leaves the local event untouched; it never reuses or reassigns the original UID. `--merge-file FILE` accepts a hand-edited `.ics` whose UID must equal the conflicted UID and whose residual must be a superset of both sides' residual entries unless `--allow-residual-drop --yes` is given, in which case the dropped entries are listed by name/hash before confirmation and the originals stay in the conflict record.

**Branch — deletions.** Local trash (`events.deleted_at`) is a local lifecycle state and by default does **not** delete the mirror file. A run with pending trashed items prints "N local deletions held" and exits 0 with the items untouched. `--deletions propagate` moves each mirror file into the mirror's `.mg-calr-holding/` directory (never `unlink`), writes a `sync_tombstones` row with the held bytes digest, and lets the next `push_transport` remove it remotely; interactively it asks for confirmation and lists the items, and under `--no-input` it additionally requires `--confirm-deletions N` where `N` is the exact expected count from the plan. A count mismatch fails `deletion_count_mismatch` with no write. Remote deletion (mirror file gone, base present, local semantically unchanged) sets `events.remote_tombstoned_at`, writes a `remote_delete` tombstone with the held base bytes, and leaves the row otherwise intact — never a hard delete.

**Branch — interruption.** Any `sync run` that dies leaves `sync_runs.phase` at its last committed transition and `sync_run_steps` rows with `applied_at IS NULL` for unfinished work. The next `sync run` refuses to start (`sync_run_interrupted`, exit 75) and names `mg-calr sync resume`. `sync resume` re-derives each unfinished step's current state, compares the on-disk digest to `intended_digest`, and either completes or re-plans that step. No step is ever assumed applied.

**Branch — `ical import` on an existing UID.** Default `--on-existing stop` returns `ical_uid_exists`. `update` requires `--if-revision UID=N` per affected item under `--no-input`. `skip` counts and lists skipped UIDs. Import is never a blind upsert.

No haptics, sound, or animation exist in any flow. The only motion is optional in-place progress redraw, disabled by `--progress plain|never`, `--no-color`, `NO_COLOR`, `TERM=dumb`, and any non-TTY stdout.

### 3.3 Layout descriptions

**`sync run` plan/outcome table.** Component hierarchy, top to bottom: (1) a header line naming remote, collection, direction, and mode (`dry-run` or `apply`) — remote is the config key, never a URL; (2) one row per item, leading to trailing: `ACTION`, `UID` (truncated with an ellipsis only in the middle, never at the identity-bearing head), `TITLE`, `OUTCOME`; (3) a blank line; (4) a totals footer `pulled / pushed / unchanged / conflicts / deletions-held / tombstones`; (5) if conflicts exist, one recovery line with the literal command. Data source: the `SyncPlan` DTO from `application::sync`, which is also the sole source for the `--json` envelope. Empty state prints `Nothing to synchronize.` and JSON returns empty arrays with the run identity and counts all zero.

**`sync status` table.** Columns: `COLLECTION`, `CALENDAR`, `TRACKED`, `PENDING-LOCAL`, `PENDING-MIRROR`, `CONFLICTS`, `TOMBSTONES`, `LAST-RUN`. Data source: `sync_collections` joined to aggregate counts over `sync_items`, `sync_conflicts`, `sync_tombstones`. Empty state: `No collections are mapped. Run: mg-calr sync discover --remote NAME`.

**`sync conflicts show` diff.** A labeled record, not a colorized unified diff: for each differing property, three lines prefixed with the words `base:`, `local:`, `mirror:` and the property name in the leading column. Residual entries render as `residual[NN] X-APPLE-TRAVEL-TIME  87 bytes  sha256:ab12…` with values withheld unless `--show-values`. A property present on only one side renders explicitly as `(absent)` rather than an empty cell. Data source: the `ConflictDetail` DTO, decoded from `sync_item_bytes` for all three sides.

**`ical inspect` record.** Order: identity (event UUID, RFC UID, revision, collection), supported property table, residual entry table, quarantine table (only with `--include-quarantine`), then a footer with `residual_entry_count` and the item's byte fingerprint. This is the read surface that makes losslessness auditable by a human.

**`sync doctor` matrix.** One row per check: `CHECK-ID`, `STATUS` (`pass`/`fail`/`blocked`), `DETAIL`. Failing rows are followed by an indented `recovery:` line containing an exact command. Check IDs are stable and versioned: `config.present`, `config.no_literal_secret`, `secret.command_executable`, `secret.command_exit` (only with `--probe-secret`), `vdir.root_writable`, `vdir.no_symlink`, `transport.vdirsyncer_present`, `transport.vdirsyncer_version`, `transport.reachable` (only with `--probe-transport`), `ledger.migration_applied`, `collections.mapped`, `collections.calendars_live`, `ledger.consistent`, `run.no_interrupted`.

**Progress surface.** `--progress auto` on a TTY: a single line rewritten in place, `phase 3/6 import  142/1180 items  38 pushed  2 conflicts`. `--progress plain`: one append-only line per phase transition and one per 100 items, no carriage returns, no ANSI. `--progress never`: nothing. Under `--json`, progress is newline-delimited JSON objects on **stderr** (`{"schema_version":1,"kind":"progress","phase":"import","done":142,"total":1180}`) so stdout stays a single envelope.

### 3.4 Input & gestures

- Input is keyboard-only: subcommands, flags, single-line prompts, `y`/`n` confirmations with the safe default shown in brackets, and `Ctrl-C` to abort before commit.
- There is no pointer, touch, stylus, controller, voice, or camera input, and no TUI key map: this feature ships no raw-mode interface. The bounded shell in `src/tui.rs` is read-only and gains no sync commands.
- There are no application-level keyboard shortcuts; the shell's own history and completion apply. Feature H owns shell completions, which must include every flag above.
- "Responsive behavior across screen sizes" is terminal width. At ≥100 columns tables render in full. Below that, the `UID`, `TITLE`, and `DETAIL` columns wrap with continuation indentation; identity, action, outcome, digests, revisions, and recovery commands are never truncated. At <40 columns tables degrade to labeled records, one field per line. JSON output is width-independent.
- `Ctrl-C` during `planning`, `import`, or `export` aborts at the next step boundary, leaves a resumable journal, prints `input_cancelled`, and emits no success envelope. `Ctrl-C` during a transport phase sends `SIGTERM` to vdirsyncer, waits up to five seconds, then `SIGKILL`, and records the phase as interrupted.

### 3.5 Transitions & animation

There are no navigation transitions, view animations, or motion effects. The only time-varying output is the optional in-place progress line in §3.3, which is a redraw of one line and not an animation. The reduced-motion alternative is `--progress plain` (append-only lines) or `--progress never`; both are also selected automatically when stdout is not a TTY, when `--no-color`/`NO_COLOR` is set, when `TERM=dumb`, and when `--json` is used. No information exists only in the animated surface: every count in the progress line reappears in the final totals footer and in the JSON envelope.

### 3.6 Error states

Errors use feature A's JSON error envelope on stderr (`{schema_version, ok:false, error:{code, message}}`) plus the exit codes in §3.1. Presentation is inline text at the point of failure for per-item problems and a final block for run-level problems; there are no toasts, banners, or modals in a terminal, and a per-item failure must not abort the surrounding run unless it threatens integrity.

| Trigger | Code | Presentation | Recovery path | Data-loss risk |
|---|---|---|---|---|
| literal `password`/`token` key in config | `secret_in_config` (78) | run-level, before any other work; the offending key name only, never its value | move the secret to `password_command` | none |
| `password_command` missing/non-executable | `secret_command_unavailable` (78) | doctor row + run-level | fix the command path | none |
| `password_command` fails or emits nothing (`--probe-secret`) | `secret_command_failed` (78) | doctor row with exit status and byte length only | fix the secret store | none |
| vdirsyncer not on `PATH` | `transport_unavailable` (69) | run-level with install guidance | install vdirsyncer | none |
| vdirsyncer exits nonzero | `transport_failed` (69) | run-level; captured stderr passed through the URL/secret redactor | rerun; inspect vdirsyncer separately | none; the mirror is unchanged or partially updated but the ledger base is not advanced |
| transport phase times out | `transport_timeout` (69) | run-level | rerun `sync resume` | none |
| unparsable item in mirror or import file | `ical_invalid` (65) | inline per item, item skipped, run continues | fix or remove the item; `ical validate` | none; the item is never imported and never rewritten |
| item parses but residual cannot be represented | `residual_unrepresentable` (65) | inline; item quarantined, never dropped | `ical inspect --include-quarantine` | none |
| both sides changed | `sync_conflict` (75) | inline per item; run-level footer with count | `sync conflicts show|resolve` | none; both sides preserved verbatim |
| local update vs. remote delete | `sync_conflict` kind `update_delete` (75) | inline | resolve explicitly | none; deletion is not applied |
| two mirror files claim one UID | `uid_collision` (75) | inline; both files preserved | resolve explicitly | none |
| stale revision at resolution | `stale_revision` (75) | run-level, before any write | re-inspect and reapply | none |
| mirror changed since the plan | `mirror_digest_mismatch` (75) | run-level, before any write | rerun `sync run --dry-run` | none |
| pending deletions without confirmation | `deletion_confirmation_required` (64) | run-level with the item list and count | `--deletions propagate --confirm-deletions N` | none; deletions held |
| deletion count differs from plan | `deletion_count_mismatch` (75) | run-level | re-plan | none |
| previous run interrupted | `sync_run_interrupted` (75) | run-level naming the run ID | `mg-calr sync resume` | none; journal is authoritative |
| vdir root unwritable / is a symlink | `vdir_unavailable` (74) | run-level | fix permissions/path | none |
| mirror file digest disagrees with ledger (`sync verify`) | `mirror_drift` (75) | per item table | `sync verify --repair-ledger` re-derives the ledger from bytes; never rewrites the file | none |
| collection not mapped | `collection_unmapped` (78) | run-level | `sync map` | none |
| calendar for a mapped collection is trashed | `collection_calendar_not_live` (78) | run-level | restore or remap the calendar | none |
| import UID already present | `ical_uid_exists` (75) | inline per item | `--on-existing update --if-revision UID=N` | none |
| `--no-input` missing a required value | `input_required` (64) | run-level naming exact flags | supply the flags | none |
| user cancels | `input_cancelled` (64) | run-level | rerun | none |

Every message is plain text and states the object, the reason, and the next command. No error message ever contains a credential, a URL with userinfo, a query string, a raw `.ics` payload, or event content beyond a title that the user already sees in their own terminal.

### 3.7 Accessibility

- **Labels, hints, traits.** Every interactive element is a prompt line carrying its field name, accepted values, and default in words: `Propagate 3 local deletions to iCloud? [y/N]`. Confirmation prompts always state the count and the direction of the irreversible half. Tables print explicit header words; no column meaning depends on position alone in the degraded <40-column form.
- **Custom actions for complex interactions.** The two complex interactions are conflict resolution and deletion propagation. Neither is a gesture; each decomposes into a named flag (`--take local`, `--take mirror`, `--keep-both`, `--merge-file`, `--deletions propagate --confirm-deletions N`) so it is reachable non-interactively and describable by a screen reader as a discrete choice with an enumerated answer set.
- **Text scaling / dynamic type.** Terminal font size is the user's; output must not depend on it. All layout is character-cell based with no box-drawing that breaks at narrow widths; `--width` (or `COLUMNS`) overrides detection so a user at a large font can force a narrow layout.
- **Color-independent state.** Every state is a word first: `CONFLICT`, `PUSHED`, `PULLED`, `HELD`, `TOMBSTONED`, `SKIPPED`, `UNCHANGED`. Color, when enabled, only re-emphasizes the word. `--no-color`, `NO_COLOR`, `TERM=dumb`, and non-TTY stdout all produce identical text with zero ANSI bytes; a contract test asserts byte-equality of the two renderings after ANSI stripping.
- **Focus order and keyboard navigability.** Prompts are strictly sequential in the order printed; there is no hidden focus. A validation failure re-asks the same field, preserving previously entered non-secret answers in memory only. `Ctrl-C` and EOF are honored at every prompt.
- **Screen-reader behavior.** `--progress plain` guarantees append-only output with no carriage returns, cursor movement, or line rewrites, so a screen reader never re-reads a mutating line. Long-running phases emit a line at least every 100 items or 5 seconds so silence is never ambiguous. Item titles and UIDs are printed with terminal control characters escaped (`\u{XX}` form) so a malicious remote `.ics` cannot inject escape sequences into the reading surface.

---

## 4. Implementation Specification

### 4.1 Architecture placement

Target placement in the existing single package, extending the modules in `docs/ARCHITECTURE.md`:

- `src/interop.rs` → split into `src/interop/mod.rs` (existing `mg.interop/1` snapshot/projection code, moved unchanged), `src/interop/ical/mod.rs` (codec entry points), `src/interop/ical/lexer.rs` (RFC 5545 content-line unfolding/folding, byte-preserving), `src/interop/ical/model.rs` (`IcalItem`, `Residual`, `CalendarUser`), `src/interop/ical/decode.rs`, `src/interop/ical/encode.rs`, `src/interop/ical/fingerprint.rs` (`mg.icalfp/1` canonical encoding).
- `src/vdir.rs` — durable mirror: collection layout, atomic item write, item read, holding area, symlink refusal. Reuses the atomic-write and advisory-lock primitives already proven in `src/interop.rs` (`ProjectionLock`, `unique_temp_path`, `sync_parent_directory`, `reject_symlink`), which move to `src/fsutil.rs` and are shared rather than duplicated.
- `src/sync/mod.rs` — orchestration: run phases, journal, resume.
- `src/sync/classify.rs` — pure three-way classification. No I/O, no database, no clock; fully unit-testable.
- `src/sync/reconcile.rs` — applies a classified plan through feature B's application boundary.
- `src/sync/transport.rs` — the **only** module permitted to spawn a process or reach a network. Defines `trait Transport { fn pull(..); fn push(..); fn discover(..); }` with exactly two implementations: `VdirsyncerTransport` and `NoTransport` (returns `TransportDisabled` for every call).
- `src/sync/secret.rs` — `SecretCommand` type, argv validation, optional probe, and the `Redactor`.
- `src/application/sync.rs` — use cases and DTOs consumed identically by the human renderer and the JSON envelope.
- `src/cli/sync.rs`, `src/cli/ical.rs` — argument and prompt translation only.
- `migrations/0006_sync.sql` — ledger, residual, and calendar-user tables.

Layering rules: `interop::ical` has no database, filesystem, network, or clock dependency and is a pure `&[u8] ⇄ IcalItem` codec. `vdir` has filesystem only. `sync::classify` is pure. `sync::transport` is the sole process/network chokepoint. `application` owns transactions. `cli` renders. `domain` gains no knowledge of iCalendar, vdir, or vdirsyncer.

### 4.2 Data model

**Codec types** (`src/interop/ical/model.rs`):

```rust
/// One parsed iCalendar component together with everything mg-calr did not map.
/// Reconstructing `source_bytes` from `supported` + `residual` is a test invariant.
pub struct IcalItem {
    /// Exact bytes of the enclosing VCALENDAR as received, never normalized.
    pub source_bytes: Vec<u8>,
    pub source_byte_fp: [u8; 32],
    /// Typed properties mg-calr understands and may edit locally.
    pub supported: SupportedProperties,
    /// Every content line and sub-component mg-calr did not map, in source order.
    pub residual: Residual,
    /// ORGANIZER/ATTENDEE with parameters preserved verbatim (F11).
    pub calendar_users: Vec<CalendarUser>,
}

/// Ordered, byte-exact carriage for unmapped iCalendar data.
pub struct Residual {
    pub version: u16,
    pub entries: Vec<ResidualEntry>,
}

/// One unmapped content line or nested component, recorded well enough to re-emit
/// it at its original position with its original parameters, casing, and folding.
pub struct ResidualEntry {
    /// Stable identity used by diagnostics and collision reporting.
    pub property_id: ResidualId,
    /// Zero-based position within the owning component in the source document.
    pub ordinal: u32,
    /// Dotted component path, e.g. "VCALENDAR/VEVENT/VALARM".
    pub component_path: String,
    /// Uppercased property or component name for grouping only; never re-emitted.
    pub normalized_name: String,
    /// Exact unfolded bytes of the content line, including original name casing,
    /// parameter order, quoting, and value encoding.
    pub raw_unfolded: Vec<u8>,
    /// Exact original folding offsets so verbatim re-emission is byte-identical.
    pub fold_offsets: Vec<u32>,
    pub sha256: [u8; 32],
    pub state: ResidualState, // Active | Quarantined { reason, at, operation_id }
}

/// A calendar-user property whose parameters are data, not semantics, in v1.
pub struct CalendarUser {
    pub kind: CalendarUserKind, // Organizer | Attendee
    pub ordinal: u32,
    pub cal_address: String,
    /// Parameters in source order; names uppercased for lookup, values verbatim.
    pub parameters: Vec<(String, Vec<u8>)>,
    pub raw_unfolded: Vec<u8>,
}
```

**Sync types** (`src/sync/classify.rs`):

```rust
/// Two independent digests per side. Semantic drives classification; byte drives
/// durability checks and the verbatim export guarantee.
pub struct Fingerprints {
    /// SHA-256 over the `mg.icalfp/1` canonical encoding, excluding the versioned
    /// volatile property set (DTSTAMP, LAST-MODIFIED, PRODID, SEQUENCE-only bumps).
    pub semantic: [u8; 32],
    /// SHA-256 over the exact stored bytes.
    pub byte: [u8; 32],
}

/// The three-way input. `base` is the last state both sides agreed on.
pub struct ThreeWay {
    pub base: Option<Fingerprints>,
    pub local: Option<Fingerprints>,
    pub mirror: Option<Fingerprints>,
}

/// Total, deterministic classification. Every variant is exhaustively fixture-tested.
pub enum Classification {
    Unchanged,
    CreateLocalFromMirror,     // base none, local none, mirror some
    CreateMirrorFromLocal,     // base none, local some, mirror none
    FastForwardToLocal,        // local == base, mirror != base
    FastForwardToMirror,       // mirror == base, local != base
    Converged,                 // local != base, mirror != base, local == mirror
    RemoteDeleted,             // mirror none, base some, local == base
    LocalDeletionPending,      // local trashed, mirror == base
    Conflict(ConflictKind),    // everything else
}

pub enum ConflictKind { UpdateUpdate, UpdateDelete, DeleteUpdate, UidCollision, MalformedMirror }
```

`classify(three_way, local_lifecycle, mirror_present) -> Classification` is a pure total function. **There is no variant, parameter, or configuration value that resolves a `Conflict` automatically.** `Converged` is the only case where both sides changed and no user decision is needed, and it is reached only on exact semantic equality.

**Migration `0006_sync.sql`** (append-only, applied by the existing embedded runner):

```sql
-- Byte-exact carriage for everything the codec does not map (F1).
CREATE TABLE event_ical_residual (
    event_id uuid PRIMARY KEY REFERENCES events(id) ON DELETE CASCADE,
    residual_version int NOT NULL,
    source_bytes bytea,
    source_byte_fp bytea NOT NULL,
    entries jsonb NOT NULL,           -- ordered entries; raw bytes base64-encoded
    entry_count int NOT NULL CHECK (entry_count >= 0),
    quarantined jsonb NOT NULL DEFAULT '[]'::jsonb,
    CHECK (jsonb_typeof(entries) = 'array')
);

-- Parameter-preserving ORGANIZER/ATTENDEE storage (F11).
CREATE TABLE event_calendar_users (
    id uuid PRIMARY KEY,
    event_id uuid NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    role_kind text NOT NULL CHECK (role_kind IN ('organizer','attendee')),
    ordinal int NOT NULL CHECK (ordinal >= 0),
    cal_address text NOT NULL,
    parameters jsonb NOT NULL,        -- ordered [{name, raw_name, values}]
    raw_line bytea NOT NULL,
    UNIQUE (event_id, role_kind, ordinal)
);
CREATE UNIQUE INDEX event_one_organizer ON event_calendar_users (event_id)
    WHERE role_kind = 'organizer';

-- The misleading name is corrected: this column never held unknown properties.
ALTER TABLE events RENAME COLUMN extension_properties TO typed_metadata;

CREATE TABLE sync_collections (
    id uuid PRIMARY KEY,
    calendar_id uuid NOT NULL REFERENCES calendars(id),
    remote_name text NOT NULL,        -- config key only; never a URL
    collection_key text NOT NULL,
    transport text NOT NULL DEFAULT 'vdirsyncer',
    vdir_path text NOT NULL,
    enabled boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (remote_name, collection_key),
    UNIQUE (calendar_id, remote_name)
);

-- Content-addressed store for base/loser/held bytes. Nothing is ever discarded.
CREATE TABLE sync_item_bytes (
    digest bytea PRIMARY KEY CHECK (octet_length(digest) = 32),
    bytes bytea NOT NULL,
    byte_length int NOT NULL CHECK (byte_length = octet_length(bytes)),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE sync_items (
    id uuid PRIMARY KEY,
    collection_id uuid NOT NULL REFERENCES sync_collections(id),
    rfc_uid text NOT NULL,
    event_id uuid REFERENCES events(id),
    vdir_filename text NOT NULL,
    base_semantic_fp bytea, base_byte_fp bytea,
    base_bytes_digest bytea REFERENCES sync_item_bytes(digest),
    local_semantic_fp_at_sync bytea, local_revision_at_sync bigint,
    mirror_byte_fp_at_sync bytea,
    state text NOT NULL CHECK (state IN
        ('tracked','conflicted','pending_local','pending_mirror','tombstoned')),
    last_synced_at timestamptz,
    UNIQUE (collection_id, rfc_uid),
    UNIQUE (collection_id, vdir_filename),
    CHECK (base_semantic_fp IS NULL OR base_bytes_digest IS NOT NULL)
);

CREATE TABLE sync_runs (
    id uuid PRIMARY KEY,
    remote_name text NOT NULL,
    started_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    finished_at timestamptz,
    phase text NOT NULL CHECK (phase IN ('planning','pull_transport','import',
        'export','push_transport','finalizing','completed','interrupted')),
    direction text NOT NULL CHECK (direction IN ('both','pull','push')),
    dry_run boolean NOT NULL,
    plan_digest bytea,
    counts jsonb NOT NULL DEFAULT '{}'::jsonb
);
CREATE UNIQUE INDEX sync_runs_one_active ON sync_runs (remote_name)
    WHERE finished_at IS NULL;

-- Write-ahead journal: one row per intended item action, written before the action.
CREATE TABLE sync_run_steps (
    id uuid PRIMARY KEY,
    run_id uuid NOT NULL REFERENCES sync_runs(id) ON DELETE CASCADE,
    sync_item_id uuid REFERENCES sync_items(id),
    seq int NOT NULL,
    action text NOT NULL,
    intended_digest bytea,
    applied_at timestamptz,
    outcome text,
    UNIQUE (run_id, seq)
);

CREATE TABLE sync_conflicts (
    id uuid PRIMARY KEY,
    sync_item_id uuid NOT NULL REFERENCES sync_items(id),
    run_id uuid NOT NULL REFERENCES sync_runs(id),
    detected_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    kind text NOT NULL CHECK (kind IN ('update_update','update_delete',
        'delete_update','uid_collision','malformed_mirror')),
    base_digest bytea REFERENCES sync_item_bytes(digest),
    local_digest bytea NOT NULL REFERENCES sync_item_bytes(digest),
    mirror_digest bytea REFERENCES sync_item_bytes(digest),
    local_revision bigint NOT NULL,
    resolved_at timestamptz,
    resolution text CHECK (resolution IN ('take_local','take_mirror','keep_both','merge')),
    resolved_by_operation uuid,
    CHECK ((resolved_at IS NULL) = (resolution IS NULL))
);
CREATE UNIQUE INDEX sync_conflicts_one_open ON sync_conflicts (sync_item_id)
    WHERE resolved_at IS NULL;

-- Tombstones are a synchronization fact and are NOT events.deleted_at (soft delete).
CREATE TABLE sync_tombstones (
    id uuid PRIMARY KEY,
    collection_id uuid NOT NULL REFERENCES sync_collections(id),
    rfc_uid text NOT NULL,
    origin text NOT NULL CHECK (origin IN
        ('local_purge','local_trash_propagated','remote_delete')),
    event_id uuid,
    deleted_at timestamptz NOT NULL,
    observed_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    base_digest bytea REFERENCES sync_item_bytes(digest),
    held_bytes_digest bytea REFERENCES sync_item_bytes(digest),
    acknowledged_at timestamptz,
    retention_until timestamptz NOT NULL,
    UNIQUE (collection_id, rfc_uid, deleted_at)
);
```

Binding data invariants:

1. **PostgreSQL is the sole authority.** The vdir mirror is a derived, durable cache. Any disagreement is resolved by re-deriving the ledger from bytes (`sync verify --repair-ledger`), never by treating the mirror as truth outside an explicit classified fast-forward.
2. **Identity is layered and immutable.** `EventId` (UUIDv7) is internal identity; `RfcUid` is interchange identity; `(collection_id, rfc_uid)` is mirror identity. Import never regenerates an existing event's `RfcUid`; `RfcUid::for_event` applies only to events `mg-calr` itself creates. A foreign UID is stored verbatim, including case and any character RFC 5545 permits. A purged UID is reserved (feature B's purge ledger) and can never be reassigned.
3. **Residual is append-preserving.** No code path deletes a `ResidualEntry`. Quarantine changes `state` only. Import of an item whose residual is a strict subset of the stored residual is a `residual_regression` conflict, not a silent drop.
4. **Tombstone ≠ soft delete.** `events.deleted_at` is a restorable local trash state with no synchronization meaning. `events.remote_tombstoned_at` records that the remote no longer has the item. `sync_tombstones` records the synchronization event with its origin, held bytes, and retention. A local trash never writes `remote_tombstoned_at`; a remote delete never writes `deleted_at`. Restoring from trash never clears a tombstone, and restoring a tombstone never clears trash.
5. **Nothing is unlinked.** Mirror deletions move files into `<vdir_root>/<collection>/.mg-calr-holding/<digest>.ics`; database losers move into `sync_item_bytes`. Only `sync tombstones purge --yes` after `retention_until` removes held bytes, and it records the removal.
6. **Every mutation is transactional and revision-checked.** Reconciliation calls feature B's `patch_event`/`create_event` with `ExpectedRevision` and an `OperationId`. There is no repository method in this feature that writes an event without a revision predicate.

### 4.3 API contracts

Application interfaces are authoritative; the CLI is a thin translation layer.

```rust
// Codec — pure, no I/O.
pub fn decode_ics(bytes: &[u8]) -> Result<Vec<IcalItem>, IcalError>;
pub fn encode_ics(item: &IcalItem, fidelity: Fidelity) -> Result<Vec<u8>, IcalError>;
pub fn fingerprint(item: &IcalItem) -> Fingerprints;

// Mirror — filesystem only.
pub fn read_collection(path: &Path) -> Result<Vec<MirrorItem>, VdirError>;
pub fn write_item(path: &Path, filename: &str, bytes: &[u8]) -> Result<[u8; 32], VdirError>;
pub fn hold_item(path: &Path, filename: &str) -> Result<[u8; 32], VdirError>;

// Classification — pure, total.
pub fn classify(w: &ThreeWay, local: LocalLifecycle, mirror: MirrorPresence) -> Classification;

// Application use cases.
pub async fn plan_sync(SyncRequest) -> Result<SyncPlan, SyncError>;
pub async fn apply_sync(SyncPlan, Confirmations, OperationId) -> Result<SyncOutcome, SyncError>;
pub async fn resume_sync(RunId, OperationId) -> Result<SyncOutcome, SyncError>;
pub async fn verify_mirror(CollectionKey, RepairLedger) -> Result<VerifyReport, SyncError>;
pub async fn list_conflicts(Filter) -> Result<Vec<ConflictSummary>, SyncError>;
pub async fn show_conflict(ConflictId) -> Result<ConflictDetail, SyncError>;
pub async fn resolve_conflict(ConflictId, Resolution, ExpectedRevision,
                              ExpectedMirrorDigest, OperationId) -> Result<ResolutionOutcome, SyncError>;
pub async fn list_tombstones(Filter) -> Result<Vec<TombstoneSummary>, SyncError>;
pub async fn restore_tombstone(TombstoneId, ExpectedDigest, OperationId) -> Result<EventDto, SyncError>;
pub async fn purge_tombstone(TombstoneId, PurgeConfirmation, OperationId) -> Result<PurgeDto, SyncError>;
pub async fn import_ics(ImportRequest, OperationId) -> Result<ImportReport, SyncError>;
pub async fn export_ics(ExportRequest) -> Result<ExportStream, SyncError>;
pub async fn sync_doctor(DoctorRequest) -> Result<DoctorReport, SyncError>;
```

`Resolution` is `TakeLocal | TakeMirror | KeepBoth | Merge(Vec<u8>)`. There is deliberately no `Auto` variant and no `policy` field anywhere in `SyncRequest`. `ExpectedRevision` and `ExpectedMirrorDigest` have no defaulting constructor. `Confirmations` carries `deletions: DeletionPolicy::{Hold, Propagate{expected_count: u32}}`; `Hold` is the default and `Propagate` cannot be expressed without a count.

**Error cases.** Every code in §3.6 is a variant of a `thiserror` enum wired into `AppError::code()`/`exit_code()` in `src/lib.rs`, keeping the existing scheme. Codec errors carry the component path and content-line ordinal, never the value.

**Auth / permissions.** No account, token, or HTTP authorization exists in `mg-calr`. Database access uses the existing unprivileged peer-auth role, which needs `SELECT`/`INSERT`/`UPDATE` on the tables in §4.2 and no DDL, role, or superuser privilege. Remote authorization is entirely vdirsyncer's, using a credential `mg-calr` never reads on the common path.

**Pagination / rate limiting.** `sync status`, `conflicts list`, and `tombstones list` accept `--limit`/`--offset` with a deterministic total order (`collection_key, rfc_uid, id`). There is no client-side rate limiting because there is no client-side network call; vdirsyncer owns retry and backoff against iCloud.

**JSON contracts.** Three versioned schemas printable via `interop schema`: `mg.interop/1` (existing snapshot, extended with an optional per-event `ical_residual` block), `mg.ical/1` (a JSON rendering of `IcalItem` with residual bytes base64-encoded so JSON interchange is exactly as lossless as `.ics`), and `mg.sync/1` (`SyncPlan`, `SyncOutcome`, `ConflictSummary`, `ConflictDetail`, `TombstoneSummary`, `DoctorReport`, and the progress object). All are additive within a major version: adding an optional field is allowed; removing, retyping, or narrowing an enum requires a version bump. Golden files under `contracts/` are asserted byte-for-byte.

Example `mg.sync/1` outcome:

```json
{"schema_version":1,"command":"sync.run","ok":true,"data":{
 "run_id":"018f0000-0000-7000-8000-0000000000aa","remote":"icloud","dry_run":false,
 "phase":"completed","counts":{"pulled":12,"pushed":3,"unchanged":140,"conflicts":1,
 "deletions_held":2,"tombstones_created":1,"skipped_invalid":0},
 "conflicts":[{"conflict_id":"018f0000-0000-7000-8000-0000000000bb",
   "uid":"synthetic-uid@example.invalid","kind":"update_update",
   "base_digest":"sha256:1f0a…","local_digest":"sha256:44c1…","mirror_digest":"sha256:9e77…",
   "local_revision":7}],
 "recovery":"mg-calr sync conflicts show 018f0000-0000-7000-8000-0000000000bb"}}
```

### 4.4 State management

**Ownership.** `application::sync` owns all sync state and transaction boundaries. `sync_runs`/`sync_run_steps` are the durable orchestration state; there is no in-memory run state that survives a process, and there is no daemon, background thread, or watcher.

**New state container.** `SyncSession` is created per invocation, injected with a `Transport`, a `Clock`, a `Redactor`, a repository handle, and a `VdirRoot`. It is constructed in `main.rs` for `sync` subcommands only; every other command path constructs the session type not at all, and the `Transport` it would need is `NoTransport` by construction in tests.

**Local vs. synced boundary.** PostgreSQL rows are local authority. The vdir mirror is synced state. The ledger is the mapping between them and holds the only copy of the *base*. Anything derivable is recomputed rather than cached; the only caches are `sync_item_bytes` (content-addressed, deduplicated, append-only) and the mirror itself.

**Offline / draft persistence.** Everything except the two transport phases works fully offline: `ical export`, `ical import`, `ical validate`, `ical inspect`, `sync status`, `sync doctor` (without probes), `sync verify`, `sync conflicts *`, `sync tombstones *`, and `sync run --no-transport` all operate on the database and the mirror alone. There are no drafts: a cancelled prompt persists nothing.

**Interruption recovery (the durability contract).** Ordering per item is strictly: (1) insert the `sync_run_steps` row with `intended_digest`, commit; (2) perform the filesystem or database action; (3) set `applied_at`/`outcome` and advance `sync_items`, commit. A crash between (1) and (2) leaves an unapplied step whose intended digest is compared against the on-disk bytes at resume. A crash between (2) and (3) is detected because the on-disk digest already equals `intended_digest`, so the step is completed idempotently rather than re-executed. Mirror writes are atomic (`O_CREAT|O_EXCL` temp with mode `0600`, `write_all`, `fsync`, `rename`, parent-directory `fsync`) exactly as `TodoProjectionSnapshot::store` already does, so a torn file is impossible. `sync_runs_one_active` guarantees at most one live run per remote; a second invocation fails `sync_run_interrupted` rather than racing. Advisory locking on the vdir root uses the existing `ProjectionLock` pattern so a concurrent `mg-calr` on the same mirror blocks rather than interleaves.

**Base advancement.** The base is advanced only in `finalizing`, and only for items whose applied outcome is known-good. A conflicted, skipped, invalid, or unresolved item keeps its old base forever, so a later run re-detects the same conflict rather than forgetting it.

### 4.5 Dependencies

**New Rust crates** (each requires license/maintenance/supply-chain review before adoption; versions pinned in `Cargo.lock`):

- An RFC 5545 parsing foundation. The evidence spike in §8 Q1 must confirm byte-level residual preservation, unknown-property retention, parameter order/casing retention, and malformed-input behavior. If no crate passes, the fallback — explicitly acceptable — is a hand-written content-line lexer in `src/interop/ical/lexer.rs`, since unfolding and parameter splitting are a small, well-specified grammar and full semantic parsing is not required for residual carriage.
- An RRULE implementation shared with feature B6 (not selected here; B owns that decision).
- `zeroize` for the credential buffer used only on the `--probe-secret` path.
- `base64` for residual carriage in JSON.
- Already present and reused: `sha2`, `serde`, `serde_json`, `fs2`, `rustix`, `libc`, `tokio-postgres`, `uuid`, `chrono`, `chrono-tz`.

**Explicitly not added: any HTTP, TLS, CalDAV, DNS, or socket crate.** `Cargo.toml` currently contains none, and a dependency-graph test in §5.2 asserts it stays that way.

**External runtime program.** `vdirsyncer` (Python) is a runtime prerequisite for the two transport phases and for `sync discover`. It is not a build dependency, is not vendored, and its absence degrades `mg-calr` to full offline operation rather than breaking it. Feature H packaging declares it an optional dependency.

**Infrastructure.** No CDN, no third-party service, no new server. Database changes are the one new migration in §4.2. The user's iCloud account is a third-party service reached exclusively by vdirsyncer.

**Assets.** A synthetic `.ics` fixture corpus under `tests/fixtures/ical/` — authored for this project, not scraped from real calendars.

### 4.6 Platform-specific considerations

- **Renderer/engine migration.** N/A in the graphical sense. The relevant equivalent is the codec backend: if the §8 Q1 spike selects a crate and it later proves lossy, the hand-written lexer fallback must be swappable behind `interop::ical`'s public functions without touching `sync` or `application`. That is why the codec's public surface is three functions.
- **Version compatibility.** PostgreSQL 18-compatible SQL, no version-specific features beyond what migrations 1–5 already use. Rust edition 2024, MSRV 1.85 as in `Cargo.toml`. vdirsyncer ≥ 0.19 is the tested floor; `transport.vdirsyncer_version` records the detected version, and an untested version emits a `blocked` doctor row rather than proceeding silently. iCloud CalDAV requires an app-specific password; standard account passwords will fail and the doctor text says so.
- **Filesystem.** The vdir mirror requires a POSIX filesystem supporting `rename` atomicity, directory `fsync`, and `O_NOFOLLOW`; the existing `sync_parent_directory` already handles the non-Unix fallback. A vdir root on a network filesystem is detected where possible and produces a `blocked` doctor row, because atomic rename guarantees do not hold there.
- **Feature flags / gradual rollout.** Three gates, in dependency order: (a) the codec (`ical *` commands) ships and is useful with no sync; (b) the mirror + offline reconciliation (`sync run --no-transport`) ships next; (c) transport (`sync discover`, unflagged `sync run`) ships last and is compiled behind the `sync-transport` Cargo feature, enabled by default in the package but disabled in the default test profile so the test binary literally cannot spawn a transport. Help text marks a command unavailable rather than claiming partial capability.
- **Case-insensitive filesystems.** vdir filenames derive from a hash of the UID rather than from the UID itself, so a case-only UID difference cannot collide on a case-insensitive filesystem; the UID→filename map is the ledger, not the filesystem.

### 4.7 Performance budget

Measured on a documented baseline workstation against a local PostgreSQL over a Unix socket, with a synthetic corpus of 10,000 events across 5 collections:

- **Memory.** Item processing is streaming: one item's bytes, its parsed form, and its fingerprints at a time. Peak resident memory attributable to a full `sync run` target ≤128 MiB at 10,000 items; `ical export` and `ical import` target ≤64 MiB because they never hold the whole document. A hard per-item cap of 1 MiB and a per-collection cap of 256 MiB of mirror bytes are validated *before* allocation and produce `ical_item_too_large` rather than an OOM.
- **CPU / render time.** `sync run --dry-run` (planning only) over 10,000 tracked items targets ≤2 s p95, dominated by SHA-256 over mirror bytes; `sync status` ≤150 ms p95 (aggregate queries only, no mirror scan); `ical export` ≥2,000 items/s in verbatim fidelity, where the work is a byte copy.
- **Network payload.** Zero bytes originate from `mg-calr`. Transport payload is vdirsyncer's and is bounded by the remote's collection size; `mg-calr` neither measures nor budgets it, and must not claim to.
- **Storage.** The mirror is roughly the size of the remote data. `sync_item_bytes` adds one base copy per tracked item plus one copy per conflict loser and per held deletion, deduplicated by digest — expect ~1.2× the mirror for a healthy tracked set. `event_ical_residual.source_bytes` adds another copy for foreign-origin items; the §8 Q3 decision may make it optional per collection. `retention_until` on tombstones bounds unbounded growth, defaulting to 90 days.
- **Startup.** Zero impact on non-sync commands: no module in this feature is initialized unless its subcommand is dispatched, and `version`/`config paths` remain database-free and filesystem-light exactly as today.

---

## 5. Test Specification

All tests use synthetic data. Integration tests remain opt-in behind `MG_CALR_RUN_DATABASE_TESTS=1` against a disposable database whose name contains `mg_calr_test`, matching the existing convention in `README.md`.

### 5.1 Unit tests

Codec (`src/interop/ical/`):

- `unfold_refold_is_byte_identical` — setup: fixture lines folded at 73, 75, and mid-UTF-8-sequence octets; assert unfold→refold reproduces the exact original bytes; edge: a fold inside a multi-byte grapheme.
- `unknown_property_is_retained_verbatim` — setup: `X-APPLE-TRAVEL-TIME;X-ADDRESS="1 Main St":PT15M`; assert the `ResidualEntry` raw bytes, parameter quoting, and name casing are unchanged; edge: a parameter value containing an escaped comma.
- `unknown_component_is_retained` — setup: a `VEVENT` containing an unrecognized `X-VENDOR-BLOCK` sub-component; assert the whole block survives with its `component_path`; edge: nesting two levels deep.
- `residual_ordinals_preserve_position` — assert re-emission restores original ordering among mapped and unmapped lines; edge: an unmapped line between two mapped ones.
- `malformed_item_is_quarantined_not_dropped` — setup: a property with an invalid value type; assert `ResidualState::Quarantined` and that `entry_count` is unchanged; edge: a truncated `BEGIN` with no `END`, which must fail the item (65) without touching neighbors.
- `organizer_attendee_parameters_round_trip` — setup: `ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=TENTATIVE;RSVP=TRUE;CN="Ada, L.";X-NUM-GUESTS=2:mailto:ada@example.invalid`; assert parameter order, quoting, comma escaping, and `X-` parameter all survive; edge: two `DELEGATED-TO` values on one parameter.
- `partstat_is_never_mutated_locally` — assert no codec or reconciliation path writes `PARTSTAT`; edge: an item whose local edit touches `SUMMARY` only.
- `uid_is_stored_verbatim` — setup: UIDs with mixed case, `@`, and a 200-character length; assert no normalization, lowercasing, or regeneration; edge: a UID differing from another only by case.
- `semantic_fingerprint_ignores_volatile_properties` — setup: two items differing only in `DTSTAMP`/`LAST-MODIFIED`; assert equal `semantic`, differing `byte`; edge: `SEQUENCE` bumped with no content change.
- `semantic_fingerprint_includes_residual` — setup: two items differing only in one `X-` property; assert differing `semantic`; edge: same property, different parameter order.
- `fingerprint_is_canonical_across_property_order` — assert reordering mapped properties does not change `semantic` but does change `byte`.

Classification (`src/sync/classify.rs`) — a table-driven exhaustive matrix over `base ∈ {none, A}`, `local ∈ {none, A, B, trashed}`, `mirror ∈ {none, A, C}`:

- `classification_matrix_is_total_and_deterministic` — every cell has exactly one expected `Classification`; the function is called twice with shuffled inputs and must agree.
- `both_changed_is_always_conflict` — for every `local ≠ base ≠ mirror` with `local ≠ mirror`, assert `Conflict`, never a fast-forward.
- `identical_change_is_converged_not_conflict` — `local == mirror ≠ base` yields `Converged`.
- `remote_absence_with_no_base_is_create_not_delete` — an item present locally with no base and no mirror is `CreateMirrorFromLocal`; **an absent mirror is never read as a deletion without a base**.
- `local_trash_never_produces_a_mirror_delete_action` — assert `LocalDeletionPending`, and that the plan contains no delete step without `DeletionPolicy::Propagate`.
- `no_input_yields_automatic_resolution` — a compile-and-API test plus a source-grep contract test asserting no `Classification` consumer maps `Conflict` to a write, and that the `Resolution` enum has no auto variant.

Secrets (`src/sync/secret.rs`):

- `literal_secret_key_in_config_is_rejected` — `password`, `pass`, `token`, `app_password`, `secret` under any `[sync.remote.*]` table fails `secret_in_config`; assert the value never reaches the error string.
- `secret_command_rejects_shell_strings` — a string (not an argv array) is rejected; assert no shell is invoked.
- `secret_never_appears_in_debug_or_display` — `format!("{:?}", credential)` and `{}` both yield `<redacted>`.
- `redactor_strips_userinfo_query_and_fragment` — property test over generated URLs asserting the output contains no `@`-userinfo, no `?`, no `#`, and no substring of the secret.

Ledger/policy:

- `deletion_policy_requires_explicit_count` — `DeletionPolicy::Propagate` cannot be constructed without a count; a mismatched count fails before any write.
- `tombstone_and_soft_delete_are_independent` — setting one never sets or clears the other, across all four combinations.
- `retention_bounds_are_enforced` — a tombstone before `retention_until` refuses purge.

### 5.2 Integration tests

Against the disposable database plus a temporary vdir root:

1. **Golden round trip.** For each of ~30 corpus files (Apple-style `VEVENT` with `X-APPLE-*`, a recurring master with `EXDATE`/`RECURRENCE-ID`, all-day, `VALARM` with `ATTACH`, `VTIMEZONE`, a `VTODO`, a CRLF-less file, a file with a BOM, a file folded at odd offsets, a file with an unknown sub-component, a malformed-but-parseable file): import → export → assert **byte identity** for unedited items, and assert canonical content-line multiset equality after a local `SUMMARY` edit.
2. **Property test.** Generate random valid iCalendar documents including random unknown properties/parameters; assert `decode → encode → decode` is a fixed point and that the residual entry multiset is preserved. Assert no generated input panics.
3. **Malformed corpus.** Fuzz-style seeds (truncated, wrong line endings, duplicate `UID`, missing `DTSTART`, 10 MiB single property, deeply nested components) each produce a typed error or a quarantined entry; none panics, none drops a neighboring item, none writes partial state.
4. **Three-way fixtures.** A scripted 12-case matrix over base/local/mirror producing every `Classification`. For each conflict case assert: exit 75, a `sync_conflicts` row with all three digests, the event row's `version` and every column unchanged, and the mirror file's bytes unchanged (`byte_fp` equality before/after).
5. **Interruption recovery.** Inject a `SIGKILL` after each of: journal step insert, temp file write, `fsync`, `rename`, ledger update, base advancement — and after each phase transition. Assert the next `sync run` fails `sync_run_interrupted`, that `sync resume` completes, and that the final state equals the uninterrupted run's state exactly (all digests, revisions, and counts). Repeat for pull and push directions.
6. **Delete/restore round trip.** (a) Remote delete: remove a mirror file, run, assert `remote_tombstoned_at` set, a `remote_delete` tombstone with held bytes, and the event row otherwise unchanged; `sync tombstones restore` reproduces the exact original bytes and clears the tombstone. (b) Local trash propagate: assert the mirror file moved to holding (still readable), a `local_trash_propagated` tombstone, and that restore brings back a byte-identical file. (c) Delete/update: assert `Conflict(UpdateDelete)`, no deletion applied, both sides retained.
7. **Resolution fixtures.** For each of `--take local`, `--take mirror`, `--keep-both`, `--merge-file`: assert the winner is written through a revision-checked patch, the loser's bytes remain retrievable from `sync_item_bytes`, the conflict row is marked resolved with the operation ID, and — for `--keep-both` — that a new `EventId` and a new `RfcUid` were minted and the original UID was untouched. Assert a stale `--if-revision` and a stale `--if-mirror-digest` each fail with zero writes.
8. **No-network proof (I4).** (a) A dependency-graph test asserts the binary's transitive dependencies contain no HTTP/TLS/DNS/socket crate from a denylist. (b) A source-contract test — in the style of the existing `tests/interop_contract.rs` — asserts `std::process::Command` and `std::net` appear only in `src/sync/transport.rs`. (c) Every command except `sync discover`, `sync doctor --probe-transport`, and `sync run` without `--no-transport` is executed inside an empty network namespace (`unshare -rn`, skipped with a recorded reason where unavailable) and must succeed, including full database access over the Unix socket. (d) The default test profile builds without the `sync-transport` feature, so `NoTransport` is the only implementation linked and every transport call returns `TransportDisabled`. (e) A spawn-counting `Transport` double asserts zero spawns for `sync run --no-transport`, `sync status`, `sync verify`, `sync conflicts *`, `sync tombstones *`, `ical *`, and `interop *`.
9. **Secret boundary.** Run `sync doctor --probe-secret`, `sync discover`, and a failing `sync run` with a fixture secret command emitting a known sentinel string; assert the sentinel appears in **no** stdout byte, stderr byte, JSON envelope, database row, ledger column, mirror file, temp file, holding file, or panic message. Assert the same for a URL containing embedded userinfo. Run the repository's secret scanner over all produced artifacts as an assertion, not a warning.
10. **Transport isolation.** With a stub `vdirsyncer` on `PATH`: assert `mg-calr` passes no credential on the command line or via environment; assert the generated vdirsyncer config contains only the `password.fetch` command indirection; assert nonzero exit maps to `transport_failed` with redacted captured stderr; assert timeout kills the child and records `interrupted`.
11. **Migration.** Apply `0006_sync.sql` twice; assert idempotence, constraint/index presence, and that the `extension_properties` → `typed_metadata` rename preserves every existing row's contents.
12. **Concurrency.** Two `sync run` processes on one remote: assert one proceeds and one fails on `sync_runs_one_active` with no mirror write. Two processes on one vdir root: assert advisory-lock serialization.
13. **Authority.** Assert no code path outside an explicit classified fast-forward writes an event from the mirror; a mirror file edited by hand and never classified changes no database row until a `sync run` classifies it.

### 5.3 UI / E2E tests

Process-level tests with `assert_cmd`, extending `tests/interop_contract.rs` and `tests/cli_contract.rs`:

- Navigation: `--help` at every level lists every flag in §3.1; `sync --help` and `ical --help` describe the vdirsyncer boundary explicitly.
- Happy path: scripted first-run — `sync doctor` (offline, fails cleanly with no config), `sync init-config`, `sync map`, `sync run --no-transport --dry-run`, `sync run --no-transport` — against a pre-populated mirror; assert exit codes 78, 0, 0, 0, 0 and stable output.
- Error recovery: each row of §3.6 is asserted for exit code, error `code`, and the presence of the named recovery command in the message.
- `--json --no-input`: exactly one JSON object on stdout for every command; progress lines only on stderr; `serde_json` parses stdout in full; unknown-field-free against the golden schema.
- `--no-input` never blocks: every command is run with stdin closed and a 30-second test timeout; a hang is a failure.
- `--no-color` / `NO_COLOR` / `TERM=dumb` / non-TTY: assert zero ANSI bytes and byte-equality with the colored output after ANSI stripping.
- `--progress plain`: assert no `\r`, no ANSI, monotonic append-only lines, and at least one line per phase.
- Width: render the plan table at 40, 80, and 200 columns; assert UID, digest, revision, action, outcome, and recovery command are never truncated.
- Terminal-injection: an item whose `SUMMARY` contains `\x1b]0;pwned\x07` renders escaped; assert the raw escape byte never reaches stdout.
- Confirmation: `--deletions propagate` without `--yes`/`--confirm-deletions` under `--no-input` fails 64 with zero writes; `Ctrl-C`-equivalent EOF at a prompt exits `input_cancelled` with zero writes.

### 5.4 Visual / manual verification

- **Theme variants.** Light and dark terminal themes with default color, `--no-color`, `NO_COLOR`, and `TERM=dumb`. Verify conflict, held-deletion, and tombstone rows are unambiguous in all four, since each is a word first.
- **Text size extremes.** Largest and smallest terminal font at a fixed pixel window, which yields the narrowest and widest column counts; verify the <40-column labeled-record degradation and the ≥200-column table both stay readable.
- **Screen size extremes.** 40, 80, 120, and 200 columns; a 10-row-tall terminal during a long run to confirm the in-place progress line does not scroll away the phase context.
- **Empty vs. populated.** No collections mapped; one collection with zero items; a collection with 10,000 items; a run with 1 conflict; a run with 200 conflicts (verify the footer is still actionable and the list is paginated, not a wall of text); a tombstone list with mixed origins.
- **Screen reader.** Drive a full `sync run --progress plain` and a `sync conflicts show` under a screen reader; verify reading order, that the three-way diff's `base:`/`local:`/`mirror:` prefixes are announced before values, and that `(absent)` is spoken rather than implied by an empty cell.
- **Redaction inspection.** Manually review every diagnostic produced by a deliberately broken iCloud config; confirm no URL retains userinfo or query and no credential appears.

**Required gates** (extending `README.md`): `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --all-targets --all-features`; the opt-in PostgreSQL integration suite; the golden `.ics` and JSON contract suites; the network-denial suite; the secret scanner; and the clean-machine packaging smoke from feature H. A green happy path never overrides a failed preservation, conflict, recovery, or secret gate.

---

## 6. Compliance & Safety Gate

### 6.1 Sensitive data classification

- [ ] No sensitive data involvement
- [x] **Handles sensitive data.** Event titles, descriptions, locations, URLs, attachments, organizer/attendee email addresses and display names, alarm content, and — at the boundary — an iCloud account identifier and app-specific password. Protection measures: the credential is never stored by `mg-calr` and never read on the common path (an external `password_command` is handed to vdirsyncer as an indirection, not resolved by `mg-calr`); a literal secret in config is a hard configuration error; every URL passes through a redactor before any output, log, or error; captured subprocess stderr is redacted before display and never written to a file; the credential type has `Debug`/`Display` impls that print `<redacted>` and is zeroized after any `--probe-secret` read; event content lives only in the user's local PostgreSQL database and the user-owned vdir mirror, whose files are created mode `0600` in a directory that is refused if it is a symlink; diagnostics report residual property names, lengths, and hashes but never values unless `--show-values` is passed; logs contain typed IDs, counts, digests, and error codes only.
- [x] **Uses synthetic/test data only until compliance gate clears.** The entire fixture corpus is authored, not captured; integration tests require an explicitly opted-in disposable database; no test contacts a real iCloud account.

Purge removes mutable content when retention permits, but PostgreSQL backups and immutable provenance rows may retain data per feature G3. No claim of secure erasure is made.

### 6.2 Asset provenance

- [x] **No third-party assets.**
- [ ] Uses third-party assets

The synthetic `.ics` corpus is authored for this repository. Rust crates and the external `vdirsyncer` program are dependencies rather than bundled assets; each must clear license, maintenance, and supply-chain review before adoption, and vdirsyncer is invoked as a user-installed system program, never vendored or redistributed.

### 6.3 Language / claims audit

- [ ] Makes claims not supported by evidence — **no.** §7 states exactly what exists today; every capability in §§3–5 is marked target state.
- [ ] Promises capabilities not yet built — **no.** Help text must not mention `sync`, `ical`, iCloud, or vdirsyncer until the corresponding slice passes its gates; the `sync-transport` feature gate makes the unbuilt half physically absent from the binary rather than advertised.
- [ ] Uses language restricted by domain regulations — **no.** No health, financial, or legal claim is made. User-visible text must not say "synced", "backed up", or "safe" as a standalone assurance; it reports what was pulled, pushed, held, or conflicted, with counts.

Additionally binding: no output may claim an item is "resolved" when it was only classified, or "deleted" when it was held; the words `HELD` and `TOMBSTONED` exist precisely to prevent that.

### 6.4 Regulatory alignment

Lens 3 is fully in scope for this feature; nothing here is N/A.

**I1 — Lossless iCalendar (target 3).** Supported mapping is enumerated in §4.2 (`SupportedProperties`) and covers `UID`, `DTSTART`/`DTEND`/`DURATION`, `SUMMARY`, `DESCRIPTION`, `LOCATION`, `URL`, `STATUS`, `TRANSP`, `CATEGORIES`, `CLASS`, `PRIORITY`, `SEQUENCE`, `CREATED`, `LAST-MODIFIED`, `DTSTAMP`, `RRULE`, `RDATE`, `EXDATE`, `RECURRENCE-ID`, `ORGANIZER`, `ATTENDEE`, `VALARM`, and `VTIMEZONE`. **Unknown-property preservation** is the `Residual` store: every unmapped content line and sub-component is retained with exact unfolded bytes, original name casing, parameter order and quoting, source ordinal, component path, fold offsets, and SHA-256, and no code path deletes an entry. Golden tests (§5.2.1) assert byte identity for unedited items and canonical content-line multiset equality after edits; property tests (§5.2.2) assert `decode→encode→decode` fixity over generated documents with random unknown properties; the malformed corpus (§5.2.3) proves edge and malformed fixtures quarantine rather than drop.

**I2 — Sync authority (target 3).** PostgreSQL is the sole authority (§4.4); the vdir mirror is an explicitly derived durable cache and can be rebuilt from the database and the ledger. **Three-way fingerprints** are base/local/mirror with separate semantic and byte digests (§4.2), stored in `sync_items` and never advanced except in `finalizing` for known-good outcomes. **Interruption recovery** is the write-ahead `sync_run_steps` journal with digest-compared idempotent resume, atomic mirror writes, and a single-active-run constraint (§4.4), proven by the kill-injection matrix in §5.2.5. **Explicit orchestration** is the six-phase state machine in §3.2 with a printable dry-run plan and a plan digest.

**I3 — Conflict / deletion (target 3).** Classification is a pure total function with an exhaustive fixture matrix (§5.1); a conflict **stops that item**, leaves both the database row and the mirror file byte-unchanged, and records all three digests. Both sides are preserved permanently in `sync_item_bytes`. Resolution is deterministic in both senses that matter: the same three inputs always classify identically, and a recorded resolution replays identically — but a winner is never selected by the tool. There is no `Auto` resolution variant, no `policy` configuration key, and no API accepting "remote wins". **Tombstones are separated from soft deletes** by construction: `events.deleted_at` (trash, restorable, no sync meaning), `events.remote_tombstoned_at` (remote absence), and `sync_tombstones` (the synchronization fact with origin, held bytes, and retention) are three independent things with an invariant (§4.2.4) that no path sets one from another. Delete/restore round trips are fixtured in §5.2.6 for remote delete, propagated local trash, and delete/update conflict.

**I4 — Scope / network (target 3).** Network access exists only inside `src/sync/transport.rs`, only during `sync discover`, `sync doctor --probe-transport`, and the two transport phases of an explicit `sync run`, and even then `mg-calr` opens no socket — it spawns vdirsyncer. Database access remains confined to explicit database-related commands as `docs/ARCHITECTURE.md` already requires. CalDAV Scheduling (RFC 6638 — invitations, RSVP, free-busy) is deferred to branch I3 and is not implemented; organizer/attendee data is preserved as data only, and `PARTSTAT` is never written locally. §5.2.8 proves the boundary five ways: dependency denylist, source-location contract test, empty-network-namespace execution of every non-sync command, a default test profile that does not even link a network-capable transport, and a spawn-counting double asserting zero spawns on non-sync paths.

**Auto-fail rules, each addressed by name.**

- *Loss of unsupported iCalendar properties on round trip* — the `Residual` store, its no-delete invariant, quarantine-instead-of-drop, `residual_regression` conflict detection, and golden/property/malformed suites (I1 above).
- *Automatic conflict overwrite* — no auto-resolution variant exists in the type system, config, or CLI; conflicts stop the item and require an explicit `--take`/`--keep-both`/`--merge-file` with revision and mirror-digest preconditions.
- *Silent event/todo loss* — nothing is unlinked or deleted: mirror deletions move to a holding area, database losers move to `sync_item_bytes`, tombstones hold restorable bytes, and only an explicit post-retention `sync tombstones purge --yes` removes anything.
- *Unconfirmed overwrite* — every write to an existing event goes through feature B's revision-checked `patch_event`; every mirror write is preceded by a digest precondition; deletion propagation requires interactive confirmation or `--confirm-deletions N` matching the plan exactly.
- *UID instability* — foreign UIDs are stored verbatim and never regenerated; `--keep-both` mints a new UID for the new event rather than reassigning; purged UIDs stay reserved; `uid_is_stored_verbatim` and the case-insensitive-filesystem filename derivation prevent accidental collapse.
- *Plaintext credentials or secret logging* — literal secret keys in config are a hard error; the credential is an external command indirection `mg-calr` normally never resolves; redaction is applied to every URL, error, log, and captured subprocess stderr; the secret-boundary test asserts a sentinel appears in no artifact.
- *Network access outside explicit synchronization* — the five-way proof in I4 above.
- *Recurrence/exception corruption, timezone/DST drift* — this feature never reinterprets recurrence or temporal semantics; it carries `RRULE`/`RDATE`/`EXDATE`/`RECURRENCE-ID`/`VTIMEZONE` through the codec and applies changes only through feature B's validated application boundary, which owns those invariants. An item whose recurrence cannot be represented is quarantined and reported, never approximated.
- *Duplicate reminder delivery* — `VALARM` is preserved as data; this feature creates no delivery state and holds no grant to write `reminder_deliveries`. Feature E owns delivery.
- *Non-idempotent scans* — `sync run --dry-run` is read-only and repeatable; a completed `sync run` re-run with no changes performs zero writes and reports `unchanged`; resume is digest-compared and idempotent.

**Other lenses.** T1: immutable `EventId`, verbatim `RfcUid`, and the tombstone/conflict identity triple with fixtures. T2: temporal semantics are carried, not reinterpreted; `VTIMEZONE` is preserved and all-day items stay date-valued. T3: journal-ordered writes, transactional ledger updates, single-active-run and one-open-conflict constraints, and the kill-injection matrix. T4: three distinct deletion concepts with restore/purge eligibility and immutable provenance. T5: no delivery state is created. C1: guided defaults and an actionable recovery line on every error. C2: three versioned JSON schemas with golden contracts and `--no-input` everywhere. C3: XDG roots reused from feature A, `CLI > env > TOML > default`, redaction, and a rejected-legacy-key policy. C4: word-first states, `NO_COLOR`, width degradation, and `--progress plain`. C5: a non-mutating offline `sync doctor` with stable check IDs, exact administrator commands, and no sudo or secret leakage. O1–O4: the secret boundary, the unprivileged role with no DDL, typed errors with stable codes and exits, the resume runbook, and the CI gate list in §5.4.

---

## 7. Gap Analysis vs. Current State

### 7.1 What exists today

**Implemented.**

- `src/interop.rs` implements the `mg.interop/1` JSON snapshot: `export_snapshot` reads calendars, events, projects, tags, and todos under one `IsolationLevel::RepeatableRead` transaction via `storage::export_snapshot_sources`, emits deterministically sorted `records`/`links`, derives `created_at` from the maximum `observed_at` rather than the wall clock, and computes a SHA-256 `source_revision` over a canonical identity object. CLI: `interop export --json` (`--json` is mandatory; omitting it exits 65).
- mg-todo projection import: `TodoProjectionSnapshot::{parse, validate, store, load, agenda_todos_at, revision}` with size/record/link caps, canonical `global_id` derivation, lifecycle validation, relationship kind/direction checks, cycle detection, freshness limits, and stale/conflict detection against the existing file. `store` is crash-safe: `ProjectionLock` advisory locking, symlink refusal, `O_NOFOLLOW`, mode-`0600` `O_EXCL` temp file, `write_all`, `sync_all`, `rename`, parent-directory `fsync`. CLI: `interop import-todo --input --store`, which never opens an mg-todo database.
- Calendar/event JSON interchange: `storage::export_events`/`import_events` and `EventExport::parse` with full domain re-validation, plus `todo export`/`todo import`. `import_events` is insert-only and returns `ImportConflict` on any pre-existing id or RFC UID.
- Stable identity: `RfcUid::for_event` derives `{event-uuid}@mg-calr.local` once from the immutable `EventId` and is never regenerated on edit; `RfcUid::new` rejects empty or whitespace-bearing UIDs.
- Contract tests: `tests/interop_contract.rs` (44 lines) asserts the `--json` gate, single repeatable-read transaction, deterministic identity digest, absence of `Utc::now()` in the snapshot, no invented relationship creation times, and the `purged_absence` diagnostic. `tests/todo_projection_contract.rs` (318 lines) covers the projection contract.
- URL redaction at the configuration boundary: `ConnectionSettings::safe_summary` renders a database URL as `PostgreSQL URL (…; credentials redacted)`.

**Prototyped.**

- The interop `Lifecycle` vocabulary (`active`/`trashed`/`deleted`/`tombstoned`/`archived`, plus `purged`) and its validation exist as a projection vocabulary only. `export_snapshot` maps `event.remote_tombstoned_at` to `tombstoned_at`, but nothing consumes the vocabulary for reconciliation.

**Absent.**

- Any iCalendar codec. There is no `.ics` parser, serializer, lexer, or fixture, and `Cargo.toml` contains no iCalendar crate. `ical` is not a command.
- Unknown-property preservation. `migrations/0001_foundation.sql` defines `events.extension_properties jsonb`, but `src/storage.rs` uses it to hold **known** metadata — `categories`, `alarms`, `organizer`, `attendees` — deserialized into a fixed `ExtensionProperties` struct. Unknown keys are silently discarded by that struct. The column name is misleading and there is no residual store.
- Organizer/attendee parameter preservation. `EventMetadata::organizer: Option<String>` and `attendees: Vec<String>` are plain strings with no parameter carriage.
- The vdir mirror, the sync ledger (`sync_collections`, `sync_items`, `sync_item_bytes`, `sync_runs`, `sync_run_steps`, `sync_conflicts`, `sync_tombstones`), three-way fingerprints, classification, reconciliation, resume, and every `sync` command.
- Any transport. There is no process spawn anywhere in `src/` (`std::process` appears only for `ExitCode` in `main.rs` and `process::id()` in the temp-file name) and no HTTP/TLS/socket dependency in `Cargo.toml`. There is also no test that asserts this stays true.
- Sync tombstones. `events.remote_tombstoned_at` exists in the schema and in `Event`, but `Event::new` always sets it to `None` and no code path ever writes a non-null value; it is pure scaffolding.
- Credential handling of any kind: no `[sync]` config section, no `password_command`, no secret type, no redactor beyond `safe_summary`, no secret-boundary test.

**Planned.** All of F1–F11 as specified above, in the three rollout gates of §4.6.

**Gated.** F5 discovery and F12 transport probes are gated behind the `sync-transport` Cargo feature and the vdirsyncer runtime prerequisite. CalDAV Scheduling (RFC 6638) is gated to branch I3 and out of scope here.

### 7.2 Delta to spec

**New files / modules.** `src/fsutil.rs` (atomic write, advisory lock, symlink refusal, extracted from `src/interop.rs` and shared); `src/interop/ical/{mod,lexer,model,decode,encode,fingerprint}.rs`; `src/vdir.rs`; `src/sync/{mod,classify,reconcile,transport,secret}.rs`; `src/application/sync.rs`; `src/cli/{sync,ical}.rs`; `contracts/{mg-ical-v1,mg-sync-v1,mg-interop-v1}.schema.json` plus golden fixtures; `tests/fixtures/ical/*.ics`; new test files `tests/ical_roundtrip.rs`, `tests/ical_property.rs`, `tests/sync_classify.rs`, `tests/sync_recovery.rs`, `tests/sync_conflict.rs`, `tests/sync_tombstone.rs`, `tests/no_network_contract.rs`, `tests/secret_boundary.rs`.

**Modified files.** `src/interop.rs` → `src/interop/mod.rs`, contents otherwise unchanged so existing contract tests keep passing. `src/domain.rs`: replace `EventMetadata::{organizer, attendees}` strings with parameter-preserving `CalendarUser` values and add a residual handle. `src/storage.rs`: stop using `extension_properties` as a metadata bag; read/write `typed_metadata`, `event_ical_residual`, and `event_calendar_users`; add ledger repositories. `src/config.rs`: add the `[sync]` and `[sync.remote.*]` tables, the `password_command` argv type, and hard rejection of literal secret keys. `src/lib.rs`: add the sync/codec error variants with their codes and exit codes. `src/main.rs`: dispatch `ical` and `sync`. `Cargo.toml`: the crates in §4.5 and the `sync-transport` feature. `README.md` and `docs/ARCHITECTURE.md`: document the vdirsyncer boundary, the mirror, and the new connectivity rule. `tests/repository_contract.rs`: update the column list for the rename.

**Migrations.** One new `migrations/0006_sync.sql` per §4.2, including the `extension_properties` → `typed_metadata` rename and a data migration of `categories`/`alarms` into `typed_metadata` and of `organizer`/`attendees` into `event_calendar_users`. Append-only, idempotent, applied by the existing embedded runner.

**New dependencies.** An RFC 5545 parsing foundation (or the hand-written lexer fallback), `zeroize`, `base64`, and the external `vdirsyncer` runtime program. No HTTP/TLS/socket crate.

### 7.3 Estimated scope

**XL.** This is the largest single feature in the tree. It contains a byte-preserving RFC 5545 codec with a lossless residual model, a durable filesystem mirror with crash-safe write ordering, a seven-table ledger with a write-ahead journal and resume, a pure three-way classifier with an exhaustive fixture matrix, a conflict store and explicit resolution workflow, a three-way tombstone model with restore round trips, a process-spawn transport boundary with a five-way non-network proof, and a credential-free secret indirection with an artifact scanner. Deliver it as three dependency-ordered slices matching the §4.6 gates: (1) codec plus `ical` commands, which is independently useful and independently testable; (2) mirror plus offline reconciliation, conflicts, and tombstones (`sync run --no-transport`); (3) transport, discovery, and the iCloud diagnostics. No slice may be declared complete on a happy path alone.

### 7.4 Blocking dependencies

- **A1–A5** (implemented): XDG configuration, envelopes, exit codes, migrations, and audit. The `[sync]` config tables and the new error codes extend A's contracts and must not fork them.
- **B1–B5** must land before slice 2: reconciliation writes exclusively through feature B's revision-checked `create_event`/`patch_event` with `ExpectedRevision` and `OperationId`, and depends on B's semantic fingerprint seam (`b-event-calendar-core.md` §4.5.2) and its `extension_collision` behavior. Without that boundary this feature would have to write events directly, which is prohibited.
- **B6–B7** (recurrence and exceptions) block lossless handling of recurring series with per-occurrence overrides. Until they land, a recurring master with `RECURRENCE-ID` overrides is carried verbatim in the residual and marked `blocked` by `sync doctor` rather than reconciled — an honest gate, not a silent approximation.
- **Spike Q1** (codec selection) blocks slice 1 implementation, not this contract.
- **G3** (backup/restore) should land before the first real-account synchronization is advertised; slice 3's release note must not claim safety it cannot demonstrate.
- **H1–H2, H6** package `mg-calr` with the optional vdirsyncer dependency and document iCloud app-specific password setup.
- **E** is not a dependency and is not depended upon: this feature creates no delivery state.
- **Branch I3** (CalDAV Scheduling) is explicitly out of scope; nothing here may be built in a way that presumes it.

---

## 8. Open Questions

- **Q1:** Which RFC 5545 crate — if any — preserves unknown properties, parameter order, name casing, quoting, and fold offsets byte-exactly, and behaves predictably on malformed input? If none passes the §5.2 golden and property suites, is the hand-written content-line lexer fallback accepted as the shipping path? — **blocks:** §4.5 dependency selection and slice 1 implementation, not the contracts in §§3–4.
- **Q2:** What exactly belongs in the versioned volatile-property set used by `semantic_fingerprint`? `DTSTAMP`, `LAST-MODIFIED`, and `PRODID` are clear. Is a bare `SEQUENCE` increment volatile, and which `X-APPLE-*` properties does iCloud rewrite on every server-side touch? Getting this wrong produces either false conflicts or missed changes. — **blocks:** conflict-rate tuning in slice 2; the classifier's structure is already fixed.
- **Q3:** Should `event_ical_residual.source_bytes` (a full second copy of every foreign item) be mandatory, per-collection configurable, or dropped in favor of reconstructing verbatim output from `entries` plus `fold_offsets`? This is a storage-versus-guarantee trade: mandatory `source_bytes` makes byte-identical re-emission trivially provable. — **blocks:** the §4.7 storage budget; recommended default is mandatory until the reconstruction path is proven byte-exact.
- **Q4:** Default tombstone `retention_until` — 90 days is proposed. Should purge after retention be manual only (current spec) or offered as an explicit `sync tombstones purge --expired --yes` batch? — **blocks:** nothing; the retention column and manual purge are already binding.
- **Q5:** Should `--deletions propagate` be expressible in configuration at all, or remain per-invocation only? A config default would be convenient for a systemd timer but weakens the unconfirmed-overwrite boundary. Current spec: per-invocation only, and a timer must pass the flag and the count explicitly. — **blocks:** the H-owned systemd timer unit's default arguments.
- **Q6:** When vdirsyncer is absent, should `sync run` fail (`transport_unavailable`, 69) or silently degrade to `--no-transport`? Current spec: fail, because a "successful" run that touched no remote is exactly the kind of quiet lie this feature exists to prevent. — **blocks:** nothing; confirm the choice.

Resolved by this spec and not open: PostgreSQL as sole authority; vdirsyncer as the sole network transport; conflicts never auto-resolved; tombstones separate from soft deletes; nothing unlinked; credentials never stored or read on the common path; and exit 75 as the conflict class.
