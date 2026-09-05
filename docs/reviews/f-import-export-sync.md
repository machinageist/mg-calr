# Scorecard: Import, Export, and Synchronization

**Feature ID:** f-import-export-sync
**Spec file:** docs/specs/f-import-export-sync.md
**Reviewer agent:** Verification agent (blind review)
**Date:** 2026-08-30
**Spec iteration reviewed:** 1
**Graded against:** `git rev-parse --short HEAD` = `6e855f9`, with uncommitted working-tree
modifications to `Cargo.toml`, `src/{application,interop,lib,main,storage,tui}.rs`,
`tests/{cli_contract,migration_contract,todo_core,todo_projection_contract}.rs`, and an
untracked `migrations/0006_repair_todo_recurrence.sql`. Findings below were verified against
the working tree as it stands, not against `HEAD` alone.

---

## Verdict: PASS

**Summary:** This is the strongest interoperability spec in the tree on its anchor lens: the
`Residual` model (§4.2) with ordinals, fold offsets, raw unfolded bytes and a no-delete
invariant, backed by golden/property/malformed suites (§5.2.1–3), is a real
unknown-property-preservation design rather than a supported-subset mapping, and the
five-way non-network proof (§5.2.8) and secret-boundary sentinel test (§5.2.9) are
executable as written. The most critical defect is not conceptual but concrete: the
proposed `migrations/0006_sync.sql` collides with the existing
`migrations/0006_repair_todo_recurrence.sql` (registered at `version: 6` in
`src/storage.rs:71`, asserted by `tests/migration_contract.rs`), so the migration as
numbered cannot be applied at all. The second is that `--take local`/`--take mirror`/
`--merge-file` converge but `--keep-both` (§3.2) leaves the original item's base
un-advanced with local ≠ mirror, so a "resolved" conflict re-detects on every subsequent
run — which violates the spec's own §6.3 rule against calling an item resolved when it was
only classified.

---

## Lens 1 — Temporal and Data Integrity (weight: 35%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| T1 Identity | 3 | §4.2 invariant 2 layers `EventId` (UUIDv7, internal) / `RfcUid` (interchange) / `(collection_id, rfc_uid)` (mirror) and forbids regeneration; §3.1 forbids positional RFC UIDs because they are foreign-controlled; §4.6 derives vdir filenames from a UID hash so a case-only difference cannot collapse on a case-insensitive FS; tombstone identity is `UNIQUE (collection_id, rfc_uid, deleted_at)` and conflict identity is `sync_conflicts_one_open`; fixtured by `uid_is_stored_verbatim` (§5.1) and the `--keep-both` new-UID assertion (§5.2.7). Verified `RfcUid::for_event` and its no-regeneration comment at `src/domain.rs:120-141`. | §4.2 invariant 2 says a foreign UID is stored verbatim "including … any character RFC 5545 permits", but `RfcUid::new` (`src/domain.rs:129-135`) rejects any UID containing whitespace, and §7.2's `src/domain.rs` change list does not include relaxing it. Either state the whitespace restriction as an accepted deviation or list the constructor change. |
| T2 Temporal correctness | 2 | §6.4 "T2" and the auto-fail walk state the feature carries rather than reinterprets temporal semantics; `VTIMEZONE`/`RRULE`/`RDATE`/`EXDATE`/`RECURRENCE-ID` are in the §6.4 supported list and a `VTIMEZONE` fixture and an all-day fixture appear in §5.2.1; §7.4 gates recurring masters with `RECURRENCE-ID` overrides behind B6–B7 with a `blocked` doctor row rather than approximating. That meets the level-2 anchor by deferring authority to feature B, which owns the IANA-zone column (`events.timezone`, `migrations/0001_foundation.sql`). | Level-3 anchor is unmet: no DST/fold/gap vectors anywhere in §5.1–5.3, and no specified behavior for a `DTSTART;TZID=` whose `VTIMEZONE` carries a non-IANA or Apple-custom `TZID` — §3.6 has no error row for an unmappable `TZID` and §5.1 has no `TZID` test. Add a `tzid_unmappable` row to §3.6 (quarantine, not approximate) and DST-boundary/fold fixtures to §5.2.1. |
| T3 Transaction integrity | 2 | Design itself is level-3: §4.2 invariant 6 routes every event write through a revision-checked `patch_event`/`create_event`; DB constraints carry real invariants (`sync_runs_one_active`, `sync_conflicts_one_open`, `event_one_organizer`, `CHECK (base_semantic_fp IS NULL OR base_bytes_digest IS NOT NULL)`, `CHECK ((resolved_at IS NULL) = (resolution IS NULL))`); §4.4 fixes the three-commit write-ahead ordering and atomic mirror writes; §5.2.5 injects `SIGKILL` at six points plus every phase transition and §5.2.12 covers two-process concurrency. | **Blocking defect (dock reason):** the vehicle for all of the above is `migrations/0006_sync.sql` (§4.2, §3.2 step 1, §5.2.11, §7.2), but version 6 is taken by `migrations/0006_repair_todo_recurrence.sql`, registered at `src/storage.rs:71` as `version: 6` and pinned by `tests/migration_contract.rs` (`MIGRATIONS.len() == 6`, `MIGRATIONS[5].version == 6`). Renumber to `0007_sync.sql`, rename check ID text in §3.2 step 1, and add `tests/migration_contract.rs` to §7.2's modified-files list. Separately: the raw SQL is not self-idempotent (bare `CREATE TABLE`, bare `ALTER TABLE … RENAME COLUMN`); §5.2.11's "apply twice" only holds because `storage::migrate` is version-and-checksum gated — say so, or use `IF NOT EXISTS` forms. |
| T4 Deletion/audit | 3 | §4.2 invariant 4 separates three concepts by construction — `events.deleted_at` (trash, no sync meaning), `events.remote_tombstoned_at` (remote absence), `sync_tombstones` (the sync fact with `origin`, held bytes, `retention_until`) — with an explicit no-cross-write rule fixtured by `tombstone_and_soft_delete_are_independent` (§5.1). Invariant 5 unlinks nothing: mirror deletes move to `.mg-calr-holding/`, losers to `sync_item_bytes`. Restore/purge eligibility is gated by `--if-digest`, `retention_until`, and `retention_bounds_are_enforced` (§5.1); provenance is immutable via `resolved_by_operation`, `origin`, and permanent conflict rows. §5.2.6 fixtures all three delete/restore round trips. | §5.2.6(a) says restore "clears the tombstone" while §4.2 invariant 4 says restoring a tombstone never clears trash and the schema offers `acknowledged_at`; state which column restore writes. |
| T5 Reminder idempotency | 2 | Scoped out with a real, specific justification rather than an empty N/A: §6.4's auto-fail walk states `VALARM` is preserved as data, "this feature creates no delivery state and holds no grant to write `reminder_deliveries`", and feature E owns delivery; §7.4 explicitly records that E is neither a dependency nor a dependent. Verified that alarms currently live in the `extension_properties` JSON bag (`src/storage.rs:2635-2643`), not in `reminders`, so the claim is consistent with the code. | No test enforces the grant: §5.1–5.3 contain no assertion that any `ical import` or `sync run` path writes zero rows to `reminders`/`reminder_deliveries`. Add that assertion in the style of the existing source-grep contract tests (`tests/interop_contract.rs`), which would lift this toward the durable-claim anchor. |

**Lens average:** 2.40
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Lens 2 — CLI Usability and Automation (weight: 25%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| C1 Human workflow | 2 | §3.2's primary flow is genuinely guided (doctor → init-config → discover → map → dry-run → run), every §3.6 row carries a named recovery path, and §5.3 asserts the recovery command's literal presence in each message. Defaults are safe: `--on-existing stop`, `DeletionPolicy::Hold`, `--deletions hold`. | Falls short of the benchmark-speed anchor on the highest-friction path. §3.1's grammar makes `--if-revision N --if-mirror-digest SHA256` **mandatory** on every `sync conflicts resolve`, and §3.1 states `--yes` "never waives a revision or digest check" — so a user must hand-copy a digest per conflict even interactively, and there is no batch or per-collection resolution verb. §5.4 explicitly contemplates "a run with 200 conflicts", which this grammar makes impractical. Specify that the interactive path pre-fills both preconditions from the `conflicts show` it just rendered, and add a bounded batch form. |
| C2 Automation | 3 | Level-3 anchor met item by item: three versioned schemas (`mg.interop/1`, `mg.ical/1`, `mg.sync/1`) printable via `interop schema` (§3.1, §4.3); an explicit additive-compatibility policy ("adding an optional field is allowed; removing, retyping, or narrowing an enum requires a version bump"); golden files under `contracts/` asserted byte-for-byte; `--json` emits exactly one stdout envelope with progress relegated to stderr as NDJSON (§3.3); §3.1 forbids positional RFC UIDs so selectors cannot mutate ambiguously. Envelope shape matches the real `Envelope`/`ErrorEnvelope` in `src/lib.rs:164-176`, and the §3.1 exit-code table matches `AppError::exit_code()` (`src/lib.rs:105-142`) exactly — 64/65/66/69/70/74/75/78. | — |
| C3 Configuration | 2 | XDG roots are inherited from feature A and verified real (`ConfigPaths::from_env`, `src/config.rs:38-62`, honoring `XDG_CONFIG_HOME`/`DATA`/`STATE`/`CACHE`); §7.2 adds `[sync]`/`[sync.remote.*]`, an argv-typed `password_command`, and hard rejection of literal secret keys; §6.4 asserts `CLI > env > TOML > default` and redaction. | Two concrete gaps against the level-3 anchor. (1) `vdir_root` is checked by `sync doctor` (§3.2 step 1, `vdir.root_writable`) and is the mirror's home, but the spec never states its **default path or which XDG root owns it** — §4.4 calls the mirror "a derived, durable cache", which points at `XDG_CACHE_HOME` while §1.2 and invariant 1 call it durable, which points at `XDG_DATA_HOME`. Pick one and state it. (2) No pure resolution test is enumerated for the new `[sync]` tables; §5.1's config tests cover only secret rejection, and `tests/config_contract.rs` is not in §7.2's modified list. |
| C4 Output/accessibility | 3 | §3.7 is the strongest accessibility section in the tree and is backed by executable assertions: every state is a word first (`CONFLICT`/`HELD`/`TOMBSTONED`/`PUSHED`…) with color only re-emphasizing; §5.3 asserts zero ANSI bytes and byte-equality after ANSI stripping across `--no-color`/`NO_COLOR`/`TERM=dumb`/non-TTY; width degradation at ≥100 / <100 / <40 columns with identity, digests, revisions and recovery commands never truncated, tested at 40/80/200 (§5.3); `--width`/`COLUMNS` override for large-font users; `--progress plain` guarantees append-only output with no `\r` and a line at least every 100 items or 5 s; untrusted `SUMMARY` control characters are escaped with a test asserting the raw escape byte never reaches stdout; §5.4 drives a real screen reader over `sync run` and `conflicts show`. Global `--json`/`--no-input`/`--no-color`/`--database-url` verified present at `src/main.rs:30-40`. | — |
| C5 Diagnostics | 3 | §3.3 specifies a 14-row prerequisite matrix with stable, versioned check IDs (`config.no_literal_secret`, `secret.command_executable`, `vdir.no_symlink`, `transport.vdirsyncer_version`, `ledger.consistent`, `run.no_interrupted`, …), a `pass`/`fail`/`blocked` tri-state, and an indented `recovery:` line containing an exact command. §3.2 step 1 binds doctor to be entirely non-mutating and socket-free by default, with network probing behind explicit `--probe-transport`; `sync init-config` prints and writes only under `--write PATH --yes` and refuses to overwrite. No sudo anywhere; `secret.command_exit` reports "exit status and byte length only". | — |

**Lens average:** 2.60
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Lens 3 — Standards Interoperability and Sync (weight: 25%)

Nothing in this lens is N/A; each criterion is graded on the level-3 anchor.

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| I1 Lossless iCalendar | 3 | This is unknown-property **preservation**, not a supported subset. §4.2's `ResidualEntry` carries `raw_unfolded` bytes with original name casing, parameter order, quoting and value encoding, plus `ordinal`, `component_path`, `fold_offsets` and `sha256`; `IcalItem` additionally keeps `source_bytes` verbatim and a separate `CalendarUser` type preserving ORGANIZER/ATTENDEE parameters in source order. §4.2 invariant 3 forbids any code path from deleting an entry, makes quarantine a state change only, and turns a shrinking residual into a `residual_regression` conflict rather than a silent drop. The level-3 anchor's "golden/property tests across malformed/edge fixtures" is met literally: §5.2.1 golden round trip over ~30 fixtures (BOM, CRLF-less, odd fold offsets, unknown sub-component, `VALARM` with `ATTACH`, `VTIMEZONE`, malformed-but-parseable) asserting byte identity; §5.2.2 a `decode→encode→decode` fixed-point property test asserting residual multiset preservation and no panics; §5.2.3 a fuzz-seeded malformed corpus asserting typed error or quarantine with no neighbor loss and no partial state; and eleven codec unit tests in §5.1 including `unfold_refold_is_byte_identical` with a fold inside a multi-byte grapheme. §7.1's account of the current gap is accurate — `ExtensionProperties` (`src/storage.rs:2635-2643`) is a fixed struct over `categories`/`alarms`/`organizer`/`attendees` that silently discards unknown keys, and `Cargo.toml` contains no iCalendar crate. | Two holes to close before implementation, neither defeating the anchor. (1) `--fidelity verbatim|canonical` (§3.1) is never defined; for a spec whose central claim is losslessness, `canonical` must be stated to re-emit every residual line (canonical folding/ordering only) and never to drop one. (2) §5.2.1's corpus includes "a VTODO" and asserts byte identity for it, but `event_ical_residual` is keyed `event_id uuid PRIMARY KEY REFERENCES events(id)` and the §6.4 supported list is event-shaped — there is no todo residual table and no VTODO mapping, so that assertion cannot pass as specified. Either add the todo residual carriage or drop VTODO from the corpus and say VTODO import is out of scope. |
| I2 Sync authority | 3 | Level-3 anchor met on all three clauses. **Three-way fingerprints:** §4.2's `Fingerprints` carries two independent digests per side — `semantic` over a versioned `mg.icalfp/1` canonical encoding excluding a declared volatile set, and `byte` over exact stored bytes — assembled into `ThreeWay { base, local, mirror }` and persisted as `base_semantic_fp`/`base_byte_fp`/`base_bytes_digest` in `sync_items`, with the base advanced only in `finalizing` and only for known-good outcomes (§4.4). **Interruption recovery:** §4.4's three-commit ordering (journal row with `intended_digest` → act → mark applied) makes both crash windows recoverable, the crash-after-act window being detected by digest equality and completed idempotently; `sync_runs_one_active` prevents racing; §5.2.5 injects `SIGKILL` at six points plus every phase transition and asserts the resumed final state equals the uninterrupted state exactly, for both directions. **Explicit orchestration:** the six-phase machine in §3.2 writes each transition before doing work and exposes a printable dry-run plan with a `plan_digest` and a `mirror_digest_mismatch` precondition. Authority is unambiguous (§4.2 invariant 1, §4.4) and §5.2.13 fixtures it: a hand-edited mirror file changes no DB row until classified. The atomic-write recipe matches what the repo already proves — `O_EXCL` + `Mode::RUSR|WUSR` + `NOFOLLOW` at `src/interop.rs:1122-1128`, `sync_all`/`renameat` at 1150-1153. | §3.2 step 2 has `sync init-config` emit the user's vdirsyncer config, but the spec never constrains `conflict_resolution` in that generated block, and §5.2.10 only asserts the block's credential shape. vdirsyncer configured `conflict_resolution = "remote wins"` would auto-resolve behind the ledger. Emit `conflict_resolution = "error"` and add a `transport.no_auto_conflict_resolution` doctor check. |
| I3 Conflict/deletion | 2 | Level-2 anchor fully met and then some: §3.2's conflict branch stops the item, leaves both the DB row and the mirror file **byte-unchanged**, writes all three digests, excludes the item from both fast-forward directions, never pushes it, and lets the run continue for unrelated items with exit 75. §4.2's `Classification` is total with `Conflict(ConflictKind)` as the catch-all and an explicit statement that no variant, parameter or configuration value resolves a conflict automatically; §4.3 removes the possibility structurally — no `Auto` variant on `Resolution`, no `policy` field on `SyncRequest`, no defaulting constructor for `ExpectedRevision`/`ExpectedMirrorDigest`. Tombstone separation is genuine (three independent concepts, §4.2 invariant 4) and delete/restore round trips are fixtured three ways in §5.2.6. | **Dock reason — resolution does not converge in one of four modes.** §3.2 specifies `--keep-both` as: write the mirror side as a new event with a new `EventId`/`RfcUid`, "and leaves the local event untouched". Nothing advances the original `sync_item`'s base, so by §4.4's own base-advancement rule the original still has local ≠ base, mirror ≠ base and local ≠ mirror — the next `sync run` re-detects the identical conflict forever, while the conflict row is marked resolved. §5.2.7 asserts only that a new EventId/UID was minted; it never asserts the original item converges. This also collides with §6.3's binding rule that no output may claim an item is "resolved" when it was only classified. Specify the original item's post-`keep-both` disposition (most likely: base advances to the mirror side, which the new event now owns, and the local event becomes `CreateMirrorFromLocal` under a fresh UID) and add the convergence assertion — "a second `sync run` immediately after any resolution reports zero conflicts and zero writes" — to §5.2.7 for all four modes. |
| I4 Scope/network | 3 | Level-3 anchor ("adapter/command tests prove no network in all non-sync paths") is met redundantly. §4.1 makes `src/sync/transport.rs` the sole process/network chokepoint with exactly two `Transport` impls, one of which (`NoTransport`) returns `TransportDisabled` for every call; §4.4 constructs `SyncSession` only for `sync` subcommands. §5.2.8 proves the boundary five ways: (a) a transitive dependency denylist for HTTP/TLS/DNS/socket crates; (b) a source-contract test that `std::process::Command` and `std::net` appear only in the transport module; (c) every non-sync command executed inside `unshare -rn` and required to succeed including DB access over the Unix socket, with a recorded skip reason where unavailable; (d) a transport-free default test profile; (e) a spawn-counting `Transport` double asserting zero spawns for `sync run --no-transport`, `sync status`, `sync verify`, `sync conflicts *`, `sync tombstones *`, `ical *`, `interop *`. **Verified against the real tree:** `Cargo.toml` contains no HTTP/TLS/CalDAV/iCalendar crate (only chrono, chrono-tz, sha2, clap, serde, serde_json, thiserror, tokio, tokio-postgres, toml, uuid, fs2, libc, rustix), so §4.5's "Cargo.toml currently contains none" is exactly right; and `std::process` appears in `src/` only as `ExitCode` in `main.rs:4` and `process::id()` in `interop.rs:1209`, so §7.1's claim and test (b)'s narrow predicate are both correct and immediately writable. §6.4 correctly defers RFC 6638 scheduling to branch I3 and forbids local `PARTSTAT` writes. Database confinement matches `docs/ARCHITECTURE.md:23`. | Leg (d) is not executable as written: §4.6 makes `sync-transport` a **default** Cargo feature, and §5.4's required gate is `cargo test --workspace --all-targets --all-features`, which enables it — Cargo has no per-test-profile feature disablement. Either make `sync-transport` non-default and add an explicit `--features sync-transport` gate, or replace (d) with a `--no-default-features` gate run. Legs (b), (c) and (e) carry the guarantee regardless. |

**Lens average:** 2.75
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s
**Auto-fail triggered:** No — full walk below

---

## Lens 4 — Operational Security and Reliability (weight: 15%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| O1 Credentials | 3 | The external secret-command indirection is real, not decorative. §3.2 step 2 has `init-config` print the *same* `["command", …]` argv indirection into both the `mg-calr` TOML and the vdirsyncer `password.fetch` block, so `mg-calr` hands the fetch over rather than resolving it; §6.1 states the credential is never stored and never read on the common path, with `zeroize` (§4.5) confined to the optional `--probe-secret` read. Redaction is specified at every diagnostic emission point, not just one: the pre-spawn target print in §3.2 step 3 (`https://caldav.icloud.com/… as u***@example.com`), captured subprocess stderr in §3.6 (`transport_failed`), the blanket §3.6 closing rule ("No error message ever contains a credential, a URL with userinfo, a query string, a raw `.ics` payload…"), a `Redactor` injected into `SyncSession` (§4.4), and `sync_collections.remote_name` schema-commented "config key only; never a URL". Secret-boundary tests exist and are specific: §5.1 asserts literal `password`/`pass`/`token`/`app_password`/`secret` keys are rejected with the value never reaching the error string, that a shell string is rejected in favor of argv so no shell is invoked, that `Debug`/`Display` both yield `<redacted>`, and a property test that the redactor strips userinfo, query and fragment; §5.2.9 runs a sentinel through `--probe-secret`, `discover` and a failing run and asserts it appears in **no** stdout byte, stderr byte, JSON envelope, DB row, ledger column, mirror file, temp file, holding file or panic message; §5.2.10 asserts no credential on the child's command line or environment. Today's baseline is only `ConnectionSettings::safe_summary` (`src/config.rs:113`), which §7.1 reports accurately. | §5.2.9's "the repository's secret scanner" does not exist — there is no CI config, script, or scanner in the tree — and it is absent from §7.2's new-files list and §7.4's dependencies. Name it as a deliverable or bind it to feature H. |
| O2 Least privilege | 2 | §4.3 "Auth / permissions" is explicit that no account, token, or HTTP authorization exists in `mg-calr`, that the existing unprivileged peer-auth role is reused, and that remote authorization is entirely vdirsyncer's against a credential `mg-calr` never reads on the common path. §3.2 step 1 and §2's administrator story bind `sync doctor` to run without sudo and without mutation, and §3.3 gives every failing check an exact non-privileged recovery command. | Self-contradictory on DDL, and clean-machine recovery is not executable here. §4.3 states the role needs "`SELECT`/`INSERT`/`UPDATE` on the tables in §4.2 and no DDL, role, or superuser privilege", but §7.2 has the same role apply a new migration through the existing embedded runner, which issues `CREATE TABLE`/`ALTER TABLE`. State the DDL grant explicitly (or split a migrator role from the runtime role), and note that the level-3 "clean-machine recovery" evidence is delegated to feature H's packaging smoke (§5.4) rather than demonstrated here. |
| O3 Failure contracts | 3 | §3.6 is a 26-row error table with a typed code, an exit code, a presentation rule, a recovery path, and an explicit data-loss-risk column that reads "none" on every row because each failure is designed to be non-destructive. §4.3 binds every one of those codes to a `thiserror` variant wired into the existing `AppError::code()`/`exit_code()` rather than forking a new scheme, and requires codec errors to carry component path and content-line ordinal but never the value. The chosen codes are consistent with the real `src/lib.rs:105-142` mapping, and §3.1 justifies 75 for conflicts specifically so a timer can distinguish a halted run from a transport outage (69). Atomicity is spelled out in §4.4 and fault/concurrency coverage is executable (§5.2.5 kill-injection matrix, §5.2.12 two-process races on both the run lock and the vdir advisory lock). The recovery runbook is effectively enforced: §5.3 asserts every §3.6 row for exit code, error code, and the literal presence of its named recovery command. | §3.1 assigns exit 66 to "selector not found (unknown conflict ID)" but §3.6 has no corresponding row, so §5.3's "each row of §3.6" leaves 66 untested. Add `conflict_not_found`/`tombstone_not_found` rows. |
| O4 Verification | 3 | Level-3 gate list is present nearly verbatim and correctly extends the repository's real conventions. §5.1's unit tests all carry setup/assert/edge triples; §5.2 specifies thirteen integration suites including golden round trip, property, malformed corpus, a 12-case three-way matrix, kill injection, delete/restore, resolution, the five-way network proof, secret boundary, transport isolation with a stub `vdirsyncer`, migration idempotence with a rename-preservation assertion, concurrency, and authority; §5.3 adds process-level `assert_cmd` E2E covering help completeness, first-run happy path with expected exit codes, per-row error recovery, single-envelope JSON, a `--no-input` never-blocks suite with a 30-second hang-is-failure timeout, ANSI byte-equality, progress-surface shape, width, terminal injection, and confirmation-with-zero-writes. §5.4's required gates and the opt-in `MG_CALR_RUN_DATABASE_TESTS=1` / `mg_calr_test` convention match `README.md:50-64` exactly, and the closing rule — "A green happy path never overrides a failed preservation, conflict, recovery, or secret gate" — is the right ordering. | Test-infrastructure dependencies are under-declared: §5.2.2 requires a property-testing crate, but neither §4.5's new-crate list nor §7.2's new-dependency list names one and `Cargo.toml` dev-dependencies are only `assert_cmd`, `predicates`, `tempfile`. Add `proptest` (or equivalent) to both lists. |

**Lens average:** 2.75
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Auto-Fail Walk (each rule individually)

| Rule | Verdict | Basis |
|---|---|---|
| Loss of unsupported iCalendar properties on round trip | **Pass** | §4.2 `Residual` with `raw_unfolded`/`fold_offsets`/`ordinal`/`component_path`; invariant 3 forbids deletion by any code path, makes quarantine a state change, and turns a shrinking residual into a `residual_regression` conflict; §3.6 routes `residual_unrepresentable` to quarantine, never drop; §5.2.1–3 golden, property and malformed suites enforce it. Residual carriage extends to JSON via `mg.ical/1` base64 so the JSON path is "exactly as lossless as `.ics`". |
| Automatic conflict overwrite | **Pass** | §4.2: "There is no variant, parameter, or configuration value that resolves a `Conflict` automatically." §4.3 removes it from the type system (no `Auto` on `Resolution`, no `policy` on `SyncRequest`). §5.1's `no_input_yields_automatic_resolution` is a source-grep contract test that no `Classification` consumer maps `Conflict` to a write. `Converged` fires only on exact semantic equality. The one residual exposure — a user's vdirsyncer `conflict_resolution` setting — is outside `mg-calr`'s classifier and any resulting mirror change still passes through the three-way classifier; recorded as I2 remediation, not an auto-fail. |
| Silent event/todo loss | **Pass** | §4.2 invariant 5: nothing is unlinked. Mirror deletions move to `.mg-calr-holding/<digest>.ics`; conflict losers and held bytes go to the content-addressed append-only `sync_item_bytes`; only `sync tombstones purge --yes` after `retention_until` removes anything, and it records the removal. `import_events` remains insert-only and `--on-existing` defaults to `stop`, so import is never a blind upsert (§3.2). |
| Unconfirmed overwrite | **Pass** | §4.2 invariant 6 admits no repository method that writes an event without a revision predicate; resolution requires both `--if-revision` and `--if-mirror-digest` with a mismatch failing before any write (§3.2); `--yes` "confirms only the fully resolved target already printed by dry validation … never waives a revision or digest check" (§3.1); deletion propagation requires interactive confirmation or `--confirm-deletions N` matching the plan exactly, with `deletion_count_mismatch` failing with no write. |
| UID instability | **Pass** | §4.2 invariant 2: foreign UIDs stored verbatim, never regenerated, `RfcUid::for_event` restricted to self-created events, purged UIDs permanently reserved; `--keep-both` mints a new UID rather than reassigning; §4.6 derives vdir filenames from a UID hash so case-insensitive filesystems cannot collapse two UIDs. Fixtured by `uid_is_stored_verbatim`. |
| Recurrence/exception corruption | **Pass** | §6.4: the feature never reinterprets recurrence; `RRULE`/`RDATE`/`EXDATE`/`RECURRENCE-ID`/`VTIMEZONE` are carried and changes applied only through feature B's validated boundary. §7.4 gates recurring masters with overrides behind B6–B7 with a `blocked` doctor row — "an honest gate, not a silent approximation" — and an unrepresentable recurrence is quarantined, never approximated. |
| Timezone/DST drift | **Pass** (with T2 dock) | Nothing reinterprets temporal values; `VTIMEZONE` is preserved verbatim and all-day items stay date-valued. No drift is introduced. The missing DST/fold/gap vectors and unspecified non-IANA `TZID` behavior are scored against T2, not treated as an auto-fail. |
| Duplicate reminder delivery | **Pass** | §6.4: `VALARM` is preserved as data; the feature creates no delivery state and "holds no grant to write `reminder_deliveries`"; feature E owns delivery and §7.4 records E as neither dependency nor dependent. Consistent with the current code, where alarms live in the metadata JSON bag rather than `reminders`. |
| Non-idempotent scans | **Pass** | §6.4: `sync run --dry-run` is read-only and repeatable; a completed run re-executed with no changes performs zero writes and reports `unchanged`; resume is digest-compared and idempotent (§4.4). |
| Plaintext credentials or secret logging | **Pass** | Literal secret keys in config are a hard `secret_in_config` (78) error before any other work, reported by key name only; the credential is an external argv command indirection `mg-calr` normally never resolves; `Debug`/`Display` yield `<redacted>`; `zeroize` on the probe path; redaction applied to every URL, error, log and captured subprocess stderr; §5.2.9's sentinel must appear in no artifact of any kind. |
| Network access outside explicit synchronization | **Pass** | Single `transport.rs` chokepoint with a `NoTransport` implementation; network confined to `sync discover`, `sync doctor --probe-transport`, and the two transport phases of an explicit `sync run`; even then `mg-calr` opens no socket, it spawns a process. Verified against the tree: no network crate in `Cargo.toml`, and no `std::process::Command` or `std::net` anywhere in `src/`. Five-way proof in §5.2.8. Database access stays confined to explicit database commands per `docs/ARCHITECTURE.md:23`. |

**Auto-fail triggered:** No.

---

## Feasibility Check

Verified against the working tree at `6e855f9` plus uncommitted changes.

| Check | Status | Notes |
|---|---|---|
| Types/models exist or are clearly specified | ✓ | All new types are given concrete Rust definitions (§4.2) or SQL DDL. Existing anchors confirmed: `EventMetadata` (`src/domain.rs:282-293`) with the plain-string `organizer`/`attendees` §7.1 correctly calls out; `ExtensionProperties` (`src/storage.rs:2635-2643`) confirmed to be a fixed struct that silently drops unknown keys, exactly as §7.1 states; `events.extension_properties`, `remote_tombstoned_at` and `version` all exist in `migrations/0001_foundation.sql` + `0005_event_lifecycle.sql`. One unstated conflict: `RfcUid::new` (`src/domain.rs:129-135`) rejects whitespace, contradicting §4.2's "any character RFC 5545 permits". |
| API/interface changes are feasible with current architecture | ✓ | The §3.1 exit-code table is an exact match for the live `AppError::exit_code()` (`src/lib.rs:105-142`), and the §4.3 envelope example matches `Envelope`/`ErrorEnvelope` (`src/lib.rs:164-176`). Global `--json`/`--no-input`/`--no-color`/`--database-url` already exist (`src/main.rs:30-40`). The `src/interop.rs` → `src/interop/mod.rs` split is safe because `tests/interop_contract.rs` reads `include_str!("../src/interop.rs")` — §7.2 already commits to keeping contents unchanged, but that test's path must be updated with the split. |
| Views/screens fit current navigation pattern | ✓ | Terminal-only; §3.4's claim that `src/tui.rs` is a read-only bounded shell gaining no sync commands is accurate (`src/tui.rs` exposes only Up/Down/Refresh/Quit over `AgendaOutput`). |
| Dependencies are available and version-compatible | ✓ | Edition 2024 / MSRV 1.85 in `Cargo.toml` match §4.6 exactly. §4.5's core claim — that no HTTP/TLS/CalDAV/DNS/socket crate is present today — is verified true, and reused crates (`sha2`, `serde`, `serde_json`, `fs2`, `rustix`, `libc`, `tokio-postgres`, `uuid`, `chrono`, `chrono-tz`) are all present. `zeroize` and `base64` are new but uncontroversial. |
| Platform/renderer requirements are realistic | ✗ | §4.6 asserts "the existing `sync_parent_directory` already handles the non-Unix fallback." No such function exists, and there is **no** non-Unix fallback: `SecureProjectionDirectory`'s entire impl block is `#[cfg(unix)]` (`src/interop.rs:1056`), as is `io_error` (`:1155`). §4.1 likewise names `unique_temp_path`, `sync_parent_directory` and `reject_symlink` as existing primitives; the real names are `unique_temp_name` (`:1202`), the directory handle's `sync()` (`:1150`), and symlink refusal folded into `open`/`open_with`/`entry_exists` via `OFlags::NOFOLLOW` and `AtFlags::SYMLINK_NOFOLLOW`. The substance of the reuse claim holds; the naming and the portability claim do not. |
| Test strategy is executable with current infrastructure | ✗ | Three concrete blockers: (a) no property-testing crate is declared anywhere though §5.2.2 requires one; (b) §5.2.9's "repository's secret scanner" does not exist and is undeclared — there is no CI config or script in the tree at all; (c) §5.2.8(d)'s transport-free default test profile contradicts §4.6's default-on `sync-transport` feature and §5.4's `--all-features` gate. `unshare -rn` availability is already handled by the spec's recorded-skip clause. Everything else (`assert_cmd`, `tempfile`, `predicates`, the opt-in `MG_CALR_RUN_DATABASE_TESTS=1` / `mg_calr_test` convention, source-grep contract tests in the style of `tests/interop_contract.rs`) is directly executable today. |
| Performance budget is realistic for target hardware | ✓ | §4.7's numbers are stated against a documented baseline and a 10,000-event synthetic corpus, are dominated by SHA-256 over mirror bytes, and are honest about what is not measurable ("Network payload … `mg-calr` neither measures nor budgets it, and must not claim to"). Pre-allocation caps (1 MiB/item, 256 MiB/collection) produce `ical_item_too_large` rather than OOM. |
| No undeclared dependency on unbuilt features | ✓ | §7.4 declares every one: A1–A5 (verified implemented), B1–B5 as a hard gate for slice 2 with `patch_event`/`ExpectedRevision`/`OperationId` correctly identified as not yet existing (the tree has `edit_event(id, expected_version, …)` only), B6–B7 for recurrence, G3 for backup, H1–H2/H6 for packaging, spike Q1 for codec selection, and branch I3 as explicitly out of scope. |

**Feasibility verdict:** Feasible with caveats
**Caveats:** One blocking-but-mechanical defect (migration numbered 0006 when 6 is taken — renumber to 0007 and update `tests/migration_contract.rs`); three test-infrastructure declarations missing; one false claim about existing non-Unix filesystem support; three primitive names in §4.1/§4.6 that do not match the real functions.

---

## Composite Score

| Lens | Average | Weight | Weighted |
|---|---|---|---|
| Temporal and Data Integrity | 2.40 | 35% | 0.840 |
| CLI Usability and Automation | 2.60 | 25% | 0.650 |
| Standards Interoperability and Sync | 2.75 | 25% | 0.688 |
| Operational Security and Reliability | 2.75 | 15% | 0.413 |
| **Composite** | | | **2.59** |

**Pass conditions (from criteria.md):**
- [x] Composite ≥ 2.0 — 2.59
- [x] All lens averages ≥ 2.0 — 2.40 / 2.60 / 2.75 / 2.75
- [x] No criterion scores 0
- [x] No more than two criteria at 1 per lens — zero 1s in any lens
- [x] All auto-fail rules pass — eleven-rule walk above, none triggered
- [x] Feasibility ≠ Infeasible — Feasible with caveats

**All conditions met:** Yes → PASS

---

## Required Corrections

The verdict is PASS; none of the items below reverse it. P1 items are nonetheless blocking
for implementation — an agent picking this spec up would produce wrong code without them.

### Priority 1 — Must fix before implementation

1. **Renumber the migration.** Replace every occurrence of `0006_sync.sql` / `0006_sync`
   with `0007_sync.sql` / `0007_sync`, in §4.2 (the DDL block header), §3.2 step 1 (doctor
   check `ledger.migration_applied` recovery text), §5.2.11, and §7.2. Version 6 is
   occupied by `migrations/0006_repair_todo_recurrence.sql`, registered at
   `src/storage.rs:71` as `version: 6` with `REPAIR_TODO_RECURRENCE_MIGRATION`. Add
   `tests/migration_contract.rs` to §7.2's modified-files list: it asserts
   `MIGRATIONS.len() == 6` and `MIGRATIONS[5].version == 6`, and both must become 7 / index 6.
   Also add a `MIGRATIONS` entry constant name (e.g. `SYNC_MIGRATION`) matching the pattern
   at `src/storage.rs:29-35`.
2. **Specify the post-`--keep-both` state of the original item.** §3.2's resolution branch
   says `--keep-both` writes the mirror side as a new event and "leaves the local event
   untouched", but never says what happens to the original `sync_item`'s base. Under §4.4's
   base-advancement rule the original still satisfies local ≠ base, mirror ≠ base,
   local ≠ mirror, so the next `sync run` re-raises the same conflict indefinitely while
   `sync_conflicts.resolved_at` is set — which §6.3 forbids ("no output may claim an item is
   'resolved' when it was only classified"). State the disposition explicitly, and add to
   §5.2.7 the assertion that a second `sync run` immediately after any of the four
   resolutions reports zero conflicts and performs zero writes.
3. **Define `--fidelity canonical`.** §3.1 and §4.3's `encode_ics(item, fidelity)` accept
   `verbatim|canonical`, but no section says what `canonical` does. In a spec whose central
   guarantee is losslessness, state that `canonical` re-emits every residual entry in
   ordinal order with canonical folding and normalized line endings, changes no property
   value or parameter, and drops nothing — and add a §5.2.1 assertion that
   `canonical` output decodes back to an identical residual multiset.
4. **Resolve the VTODO contradiction.** §5.2.1's corpus includes a VTODO and asserts byte
   identity, but `event_ical_residual` is keyed `event_id uuid PRIMARY KEY REFERENCES
   events(id)`, `event_calendar_users` is event-keyed, and §6.4's supported-property list is
   event-shaped. Either add todo-side residual carriage to §4.2 and a VTODO mapping to
   §6.4's I1 list, or remove VTODO from the corpus and state in §4.1 that `.ics` VTODO
   import/export is out of scope for v1 with `interop import-todo` remaining the todo path.
5. **Fix leg (d) of the network proof.** §5.2.8(d) requires a default test profile that does
   not link a network-capable transport, but §4.6 makes `sync-transport` a default Cargo
   feature and §5.4's required gate is `--all-features`; Cargo cannot disable a default
   feature per test profile. Make `sync-transport` non-default with an explicit
   `--features sync-transport` gate added to §5.4, or replace (d) with a
   `cargo test --no-default-features` gate. State which.

### Priority 2 — Should fix for quality

6. **Constrain the generated vdirsyncer config.** §3.2 step 2 emits the user's vdirsyncer
   block. Require it to set `conflict_resolution = "error"`, and add a doctor check ID
   `transport.no_auto_conflict_resolution` (§3.3) that fails when the user's vdirsyncer
   config sets any auto-resolution. Extend §5.2.10 to assert the emitted block contains no
   auto-resolution key.
7. **Declare the missing test dependencies.** Add a property-testing crate to §4.5 and §7.2
   (§5.2.2 cannot be written without one), and either name the secret scanner of §5.2.9 as a
   §7.2 deliverable or bind it to feature H in §7.4. Note the repository currently has no CI
   configuration at all.
8. **Correct the three primitive names and the portability claim.** §4.1 should read
   `unique_temp_name` (`src/interop.rs:1202`), the `SecureProjectionDirectory::sync()`
   directory-fsync helper (`:1150`), and `OFlags::NOFOLLOW` / `AtFlags::SYMLINK_NOFOLLOW`
   symlink refusal inside `open`/`open_with`/`entry_exists` — there is no `reject_symlink`
   function. Delete §4.6's claim that "the existing `sync_parent_directory` already handles
   the non-Unix fallback": the whole impl block is `#[cfg(unix)]` and no fallback exists.
   State Unix-only as a platform requirement instead.
9. **Give `vdir_root` a default and an XDG home.** §3.2 step 1 checks it and §4.4 calls the
   mirror "a derived, durable cache" while §1.2 and invariant 1 call it durable. Pick
   `XDG_DATA_HOME/mg-calr/vdir` or `XDG_STATE_HOME/...` and state it in §4.6 or §7.2's
   `src/config.rs` changes, then add a pure resolution test for the `[sync]` tables to §5.1
   and `tests/config_contract.rs` to §7.2.
10. **Reduce conflict-resolution friction.** §3.1 makes `--if-revision` and
    `--if-mirror-digest` mandatory with `--yes` unable to waive them. Specify that the
    interactive path pre-fills both from the `conflicts show` render it just produced (the
    precondition remains enforced against the live state), and add a bounded batch form so
    §5.4's own 200-conflict scenario is workable.
11. **Reconcile the DDL privilege claim.** §4.3 says the role needs "no DDL" while §7.2 has
    that role apply a migration. State the DDL grant, or split migrator from runtime role.
12. **Fix the `RfcUid` contradiction.** §4.2 invariant 2 promises verbatim storage of "any
    character RFC 5545 permits"; `RfcUid::new` (`src/domain.rs:129-135`) rejects whitespace.
    Add the constructor change to §7.2's `src/domain.rs` list, or narrow the invariant.

### Priority 3 — Consider for excellence

13. Add T2 depth: DST-boundary, fold and gap fixtures to §5.2.1, and a `tzid_unmappable`
    row to §3.6 specifying quarantine for a non-IANA or Apple-custom `TZID`.
14. Add `conflict_not_found` / `tombstone_not_found` rows to §3.6 so §3.1's exit code 66 is
    covered by §5.3's per-row assertion sweep.
15. Add a source-grep contract test asserting no `ical`/`sync` path writes `reminders` or
    `reminder_deliveries`, enforcing §6.4's "holds no grant" claim (T5).
16. §6.4 says `SupportedProperties` is "enumerated in §4.2", but §4.2 only declares the
    field; the enumeration lives in §6.4. Move it into §4.2 or fix the cross-reference.
17. Note in §7.2 that `tests/interop_contract.rs` uses
    `include_str!("../src/interop.rs")` and must be repointed when that file becomes
    `src/interop/mod.rs`.
18. §5.2.6(a) says restore "clears the tombstone" while §4.2 invariant 4 forbids one
    lifecycle concept from clearing another; state whether restore sets `acknowledged_at` or
    deletes the row.
