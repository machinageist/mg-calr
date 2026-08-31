# Scorecard: Safety and Operations

**Feature ID:** g-safety-operations
**Spec file:** gauntlet-output/specs/g-safety-operations.md
**Reviewer agent:** Verification agent G (Spec Gauntlet), blind review
**Date:** 2026-08-30
**Spec iteration reviewed:** 1
**Graded against commit:** `6e855f9` (working tree dirty: `Cargo.toml`, `src/{application,interop,lib,main,storage,tui}.rs`, `tests/*` modified; `migrations/0006_repair_todo_recurrence.sql` and `tests/projection_agenda_contract.rs` untracked)

---

## Verdict: PASS

**Summary:** This spec is the strongest defender of the destructive-operation
auto-fail rules in the set: dry-run is structurally mandatory rather than a flag
(§3.1 rule 1), `--no-input` provably refuses instead of assuming consent (§3.1
rule 3, §5.0 refuse-not-assume, §5.3 piped-stdin vector), undo is scoped to one
recorded transaction with no blanket-rollback path (§3.2 G2.7, §5.0 "undo is not
rollback"), and `doctor` is non-mutating by type, by policy, and by test (§3.2
G4.6, §4.2 invariant 8, §7.5). Its most critical defect is that
`migrations/0006_safety_operations.sql` (§4.1, §4.2, §7.2) collides with the
already-registered version 6 — this would trigger `MigrationDrift` on every
install and break an existing contract test — and two of its own §5.0 schema
vectors contradict each other about whether the binary embeds version 6. Fix the
migration number and the two vectors before any implementation begins.

---

## Lens 1 — Temporal and Data Integrity (weight: 35%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| T1 Identity | 3 | §4.2 `PlanItem{entity_type, entity_id: Uuid, expected_revision}` and `bulk_plan_items UNIQUE (plan_id, entity_type, entity_id)`; §3.1 last grammar rule separates selectors from identity absolutely — "Selectors resolve to a frozen set at plan time. Apply re-resolves and compares; it never re-runs the filter to pick up new matches"; §3.2 G2.5 "never rewrites a UID"; §6.4 I3 "undo never clears a remote tombstone and never resurrects a purged identity"; §3.2 G3.4 invariant suite asserts `rfc_uid` uniqueness on every verify and restore (verified against `migrations/0001_foundation.sql:15` `rfc_uid text NOT NULL UNIQUE`); §5.0 "plan drift" and "targeted undo" fixtures assert IDs and both revisions. | — |
| T2 Temporal correctness | 2 | §6.4 T2 "G performs no temporal arithmetic on user data … all G timestamps (`created_at`, `expires_at`, manifest time) are UTC `timestamptz`" and §4.2 SQL uses `timestamptz` throughout; §3.3 sample diff distinguishes `timed 2026-09-01T15:00Z` from `all-day 2026-09-03`; §4.6 "durations, timestamps, and counts are locale-independent". Falls short of 3: no DST/fold/gap vector anywhere in §5.0–§5.4. Worse, §3.1 lists `bulk move` as a verb but `[mutation flags for the verb]` is never expanded, so the one bulk verb that could shift times has no stated temporal contract; §3.1 `history list [--since TS] [--until TS]` leaves `TS` timezone interpretation unspecified. | Define `bulk move`'s mutation flags in §3.1 and state explicitly whether it can alter start/end/timezone; if it can, add a DST-gap and a DST-fold acceptance vector to §5.0. Specify `--since`/`--until` timestamp interpretation. |
| T3 Transaction integrity | 2 | Design is 3-level: §3.2 G1.9 default `--chunk-size 0` commits the whole plan in one serializable transaction; §4.2 CHECK constraints (`octet_length(fingerprint)=32`, `CHECK (status <> 'applied' OR transaction_id IS NOT NULL)`, `CHECK (expires_at > created_at)`); §4.4 plan-keyed transaction-scoped advisory lock; §5.2 tests 3/4/7 and §5.0 "partial run visibility"/"crashed migrate"/"restore atomicity" are real fault and concurrency tests; §5.1 property tests forbid a lower revision or a deleted audit row. **Docked to 2 for a concrete blocking defect**: §4.1/§4.2/§7.2 propose `migrations/0006_safety_operations.sql` at version 6, but version 6 is already registered (`src/storage.rs` `MIGRATIONS[5] = {version: 6, name: "repair_todo_recurrence"}`, `migrations/0006_repair_todo_recurrence.sql`). `storage::migrate` compares the recorded name against the embedded name and returns `StorageError::MigrationDrift` on mismatch, so this spec's migration would fail on every existing database, and `tests/migration_contract.rs` (`assert_eq!(MIGRATIONS.len(), 6)`, `assert_eq!(MIGRATIONS[5].version, 6)`) would fail to compile against the new list. Compounding this, §5.0's "older binary vs newer schema" vector says "binary embeds 6" while the very next row, "unknown recorded version", says "ledger has version 6 that the binary does not embed" — mutually contradictory, and the second is false at HEAD. Two binding acceptance vectors for the G5 guard are unrunnable as written. | Renumber the migration to a version above 6 **that is unique across the whole spec set** — siblings A, E, and F each independently propose a version 6, so "greater than 5" is insufficient; the gauntlet must assign disjoint numbers. Update §4.1, §4.2, §7.2, and §5.2 test 1. Rewrite the two §5.0 schema vectors so they use one consistent embedded version and a recorded version that genuinely exceeds it. |
| T4 Deletion/audit | 3 | §3.2 G2.4 gives a complete per-entity eligibility matrix (`eligible` / `undo_stale` naming the intervening transaction / `undo_irreversible` / `unsupported`) with whole-plan refusal unless `--skip-ineligible`, which itself prints exactly what it skips; §4.2 invariant 4 "audit rows are append-only and are never deleted, rewritten, or renumbered"; §3.2 G2.5 undo "increments revisions and writes new audit rows referencing `undoes_transaction_id`… never lowers a revision"; §3.1 separates `trash`/`restore`/`purge` as distinct verbs with `purge` requiring both a typed phrase and `--acknowledge-irreversible`; §6.4 T4 "archives carry the audit history so provenance survives a restore"; §5.1 `undo_eligibility_matrix`. §6.3 honestly disclaims secure erasure. | — |
| T5 Reminder idempotency | 2 | §6.4 T5 "G never claims, delivers, or dedupes a reminder"; §4.2 invariant 5 forbids G from writing reminder tables directly; §3.2 G3.4's invariant suite asserts "reminder single-target, delivery `(reminder_id, scheduled_for)` uniqueness" on every verify and restore (both verified real: `0001_foundation.sql:71` and `:85`). Falls short of 3 and carries a G-owned hole the spec never names: `backup restore` reinstates `reminder_deliveries` wholesale from an archive, so restoring a pre-delivery archive resets `delivered_at` and the next `scan-reminders` will re-present already-delivered reminders. Restore is G's operation, and §3.6's restore rows all say "Data-loss risk: none" without mentioning delivery-state regression. | Add a restore consequence note to §3.2 G3.5 and a §3.6 row stating that a restore rewinds reminder delivery state and may re-present reminders whose deliveries post-date the archive; either warn in `restore` output or add an acceptance vector fixing the expected behavior. |

**Lens average:** 2.4
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Lens 2 — CLI Usability and Automation (weight: 25%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| C1 Human workflow | 2 | Guided defaults and actionable recovery are unambiguous: §3.3's plan footer prints the exact apply command with plan ID and fingerprint; §3.6's 24-row table gives every error a named recovery path; §3.2 G6.4 prints exact `recovery resume`/`abandon`/`undo --transaction` commands; §3.1 "Every prompt has a corresponding flag; every flag is documented in `--help` with an example", enforced by §5.3. Falls short of 3 on a real internal contradiction: §3.1's grammar renders `backup prune [--keep N] [--keep-days D] [--yes] [--no-input]` and `recovery clean [--older-than D] [--yes] [--no-input]` as **single-shot** commands, while §3.2 G3.6 ("dry-run-first like a bulk plan") and §4.3 (`plan_prune`/`apply_prune`, `plan_recovery_clean`/`apply_recovery_clean`) describe two-phase plan→apply. An implementer reading §3.1 alone builds a one-shot `prune --yes --no-input` that deletes archives the user never saw listed. §3.1 also offers no affordance for reaching a just-created plan (no `--plan last`, no completion story), so the safest path is also the most clipboard-dependent. | Reconcile §3.1's grammar with §3.2/§4.3: give `backup prune` and `recovery clean` explicit `plan`/`apply` subcommands taking a plan ID and fingerprint, exactly as `bulk` does, or state in §3.1 why these two are exempt from the two-phase rule. |
| C2 Automation | 3 | §3.1 "`--json` emits exactly one foundation envelope (`{schema_version, command, ok, data}`) on stdout; prompts and progress go to stderr" — matches the real `Envelope` in `src/lib.rs`; §4.3 gives a concrete, well-formed doctor payload at `schema_version: 1`; §5.3 states an explicit compatibility policy — "Golden JSON contracts for doctor, plan, verify report, and recovery status; adding an optional field passes, renaming or removing one fails"; §5.1 `doctor_registry_is_stable` asserts a golden check-ID list "so a rename is a deliberate breaking change"; §3.1's frozen-set rule and §3.2 G1.7's re-read-and-compare make selectors provably non-ambiguous. Minor unaddressed hole: `--operation-id UUID` appears in §3.1 and in `apply_bulk(…, OperationId)` (§4.3) but its idempotency semantics are never defined, even though §3.2 G6.5 makes it the authority for uncertain commits. | Specify `--operation-id` semantics in §3.2 G1: whether re-supplying it on retry is a dedupe key, and what a repeat with the same ID against an applied plan returns. |
| C3 Configuration | 2 | Meets 2 cleanly: §3.2 G3.1 resolves the backup root `--output-dir` → `[backup].dir` → `$XDG_STATE_HOME/mg-calr/backups`, using the correct XDG root for mutable state (matching `src/config.rs`'s real `state_dir`); §7.2 adds `[backup]` (`dir`, `keep_count`, `keep_days`) and plan expiry to `src/config.rs` and `config/example.toml`. Redaction is complete and tested (§3.2 G3.2 `config.toml.redacted`, §3.3 "never a URL", §5.1 `redaction_of_connection_summary`). Migration/compatibility policy is excellent (§3.2 G5, §4.6's `EMBEDDED_SCHEMA_VERSION`/`MIN_SUPPORTED_SCHEMA_VERSION` window). Falls short of 3 on the anchor's first named element: **no pure resolution test** for the new configuration surface appears in §5.1, §5.2, or §5.3, although `tests/config_contract.rs` already exists as its home. §6.4 also claims "the existing CLI > env > TOML > default precedence" while §3.2 G3.1's concrete chain for the backup root has no env layer — `docs/ARCHITECTURE.md` documents `DATABASE_URL` as the real env step, so the new setting silently drops one tier. | Add a pure resolution unit test to §5.1 covering `--output-dir` > env > `[backup].dir` > `$XDG_STATE_HOME` and plan-expiry precedence; name the env variable for the backup root in §3.2 G3.1 or state in §6.4 that the backup root deliberately has no env tier. |
| C4 Output/accessibility | 3 | §3.7 makes color strictly redundant — "Every status is a word first: `PASS`, `WARN`, `FAIL`, `SKIPPED`, `N/A`… The `+ - ~` markers are redundant decoration. Removing color removes nothing" — with `--no-color`, `NO_COLOR`, and non-TTY plainness all asserted by contract tests for zero escape bytes; §3.4 specifies `COLUMNS` handling with a hard rule that "identity, revision, action word, recurrence scope, and irreversibility notices are never truncated"; §5.3 asserts this at 40 and 200 columns; §3.5 removes animation entirely (no spinner, no redrawn line, `--progress never`) so reduced-motion is satisfied by construction; §3.2 G2.1 and §3.3 fix chronological and section ordering; §5.4 includes a screen-reader read-through and a light/dark comparison. The Quickshell public-interface element is handled by explicit, justified deferral in §7.5 with the consumed contract named ("later clients consume the public JSON contracts"). | — |
| C5 Diagnostics | 3 | **Non-mutating doctor verified in depth, no repair path found anywhere.** §3.2 G4.1 "connects once, issues `SET TRANSACTION READ ONLY`… never calls the ledger-creating path, never applies a migration, and never writes a file"; §3.2 G4.6 "**Doctor never repairs.** Each remediation names a separate explicit command"; §7.5 forecloses the usual escape hatches — "No repair inside `doctor`, ever — including a 'safe' fix, a cache rebuild, or ledger creation"; §4.2 invariant 8 and §4.1's `ReadOnlyConnection` newtype make it a type-level property, not a convention. Enforcement is tested, not asserted: §5.0 "doctor is inert" requires a byte-identical `pg_dump` content checksum and that `mg_calr_schema_migrations` remains absent; §5.2 tests 8–9 repeat it as a regression guard on today's correct behavior (verified: `storage::doctor` is a direct alias for `migration_status`, which never calls `ensure_migration_table`). Stable machine output (§4.3 payload, §5.1 golden check-ID list), prerequisite matrix (§3.3 `check_id \| proves \| owner \| severity`, §4.3 `prerequisites` array), and no sudo or secret leakage (§3.2 G4.3, §5.0 secret containment) are all present. | — |

**Lens average:** 2.6
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Lens 3 — Standards Interoperability and Sync (weight: 25%)

I1–I3 are **not** marked N/A by this spec; §6.4 claims each "applies, addressed" with substantive content, so each is graded on its merits rather than as a deferral.

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| I1 Lossless iCalendar | 2 | §6.4 I1 argues preservation structurally rather than by codec ownership: bulk and undo mutate only through the owning aggregate's use case, "no bulk flag can clear or rewrite an extension envelope", and the backup content fingerprint plus invariant suite cover the extension columns "so a restore that dropped them fails verification". §5.0's "opaque bytes through bulk" vector asserts "envelope raw bytes, order, parameters, and SHA-256 are identical" after apply — a real golden fixture grounded in real schema (`events.extension_properties jsonb`, `0001_foundation.sql:28`). Falls short of 3: there is exactly one such fixture, over one envelope, on one verb (`retag`). No malformed or edge fixture set exists, and §5.1's property tests cover plan/apply/undo revisions, not generated extension payloads. | Extend §5.0's opaque-bytes vector to a matrix over every mutating bulk verb (`edit`, `move`, `retag`, `complete`, `trash`, `restore`) plus undo, and add a property test to §5.1 over generated extension-property payloads including malformed and adversarially-ordered inputs. |
| I2 Sync authority | 3 | §4.4 and §6.4 I2 keep authority singular and say so explicitly: "G introduces no third authority"; PostgreSQL remains sole authority for calendars and events; the validated projection file remains the read-only todo authority with a hard `projection_read_only` refusal naming the owning `mg-todo` command (§3.2 G1.3); "A backup archive is an inert copy, never a second authority". Interruption recovery is the strongest content in the spec — §3.2 G6 in full, plus §3.2 G3.3's `.part` discipline (a crash leaves a directory that "`backup list` never shows and `restore` never accepts") and G3.5's `pg_restore --single-transaction --exit-on-error`. Orchestration is explicit everywhere: two-phase plan→apply, mandatory explicit `--target-database-url` with no default, "It never runs `database migrate` implicitly", and "Retention never runs automatically as a side effect of another command". Three-way fingerprints and the vdir mirror are F's and are deferred by name (§4.5 "F's fingerprint discipline is consumed, not duplicated"; §6.4 I3 "F retains ownership of three-way reconciliation"), so they are not credited to G but their absence is correctly scoped. | — |
| I3 Conflict/deletion | 2 | §6.4 I3 is a genuine no-winner policy: "G never resolves a conflict by picking a winner"; `plan_stale` stops the operation and "preserves both the current row and the untouched plan for re-inspection" (§3.2 G1.7, §3.6 "none; zero writes", plan remains `open` per §5.0); `undo_stale` reports and names the intervening transaction rather than overwriting; restore is "a whole-database operation that the user explicitly targets, never a silent per-item merge". Refusal is deterministic and fixture-backed for both conflict paths (§5.0 "plan drift", "targeted undo"). Falls short of 3 on the anchor's second named element: **no delete/restore round-trip fixture**. §3.1 exposes `trash` and `restore` as bulk verbs, but §5.0 has no vector exercising bulk trash → bulk restore, and nothing anywhere asserts that local trash and remote tombstone stay distinct across such a round trip — the very distinction §6.4 I3 claims to preserve. | Add a §5.0 vector: bulk-trash a mixed set including a remotely-tombstoned event and a recurring master, then bulk-restore it, asserting `deleted_at` clears while `remote_tombstoned_at` is untouched, revisions increment, and the RRULE/exception state round-trips unchanged. |
| I4 Scope/network | 3 | §6.4 I4 is unambiguous and correctly refuses the N/A escape ("applies, never N/A"): "No command in G opens a network socket"; `pg_dump`/`pg_restore` "inherit that same connection target — there is no remote fetch, no upload, no telemetry, no update check, and no CalDAV or scheduling traffic"; "`doctor` executes only `pg_dump --version` / `pg_restore --version` locally and performs no DNS resolution". This is proved, not asserted: §5.2 test 10 and §5.5's release gate both require a test that "every G command opens a connection only when invoked and performs no DNS lookup or socket operation other than the configured PostgreSQL socket/URL", and §4.7 states "Network payload is exactly zero for every command in this feature". §4.5 adds "no daemon, no network endpoint". Consistent with the real boundary in `docs/ARCHITECTURE.md` ("`version` and `config paths` read environment/config only… There is no sync or other network client"). | — |

**Lens average:** 2.5
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s
**Auto-fail triggered:** No — see the per-rule walk below.

### Auto-fail walk (every rule individually)

| Rule | Verdict | Basis |
|---|---|---|
| Silent event/todo loss | **Pass** | No mutation path exists without a printed plan: §3.1 rule 1 makes dry-run the only first phase with no `--apply` shortcut on the verb; §4.2 invariant 2 "There is no unconditional bulk update path and no `Option<ExpectedRevision>` anywhere in the bulk API"; §3.2 G1.9 "**Partial application is always recorded and reported; it is never silent**", backed by `bulk_runs`/`bulk_run_chunks` rows and §5.0 "partial run visibility". `recovery clean` is confined to an allowlist of mg-calr's own artifact patterns inside resolved roots (§3.2 G6.3, §5.0 "clean is narrow"). `backup prune` "never removes the newest archive, never removes an archive that fails to parse… never removes anything outside the resolved backup root". |
| Unconfirmed overwrite | **Pass** | §3.1 rule 3 — "**`--no-input` refuses; it never assumes yes**" — returning `confirmation_required` (exit 64) with zero writes, and §3.1 rule 2 prevents `--yes` from ever standing in for `--acknowledge-irreversible` or `--acknowledge-overwrite`. §3.4 closes the usual leak: "Prompts read the controlling terminal, never stdin, so a piped stdin cannot be mistaken for consent; under `--no-input` no terminal is opened at all", asserted by §5.3's `echo y \| … --no-input` vector. `backup restore` refuses a populated target without `--acknowledge-overwrite` (§3.2 G3.5). §4.3 makes it type-enforced: `Confirmation` has no default constructor and the acknowledgements are separate types "so a compile error — not a code review — prevents a destructive path from taking a plain `bool`". §6.5 forecloses the escape hatch: "there is no `--force` that bypasses a fingerprint, a revision, or an integrity check". *Noted but not qualifying:* the §3.1-vs-§3.2 grammar contradiction on `prune`/`clean` (see C1) creates a one-shot deletion path for archives and temp artifacts, not for events or todos. |
| UID instability | **Pass** | §3.2 G2.5 undo "never rewrites a UID"; §6.4 T1 "plans reference immutable UUIDs and revisions… restore reproduces identity exactly"; §3.2 G3.4's invariant suite asserts `rfc_uid` uniqueness on every verify and restore. |
| Recurrence/exception corruption | **Pass** | §3.2 G1.4 refuses any plan touching a recurring master or occurrence exception without explicit `--scope` (`recurrence_scope_required`), and **no plan is stored** on refusal; §4.2 invariant 5 forbids G from issuing `UPDATE`/`DELETE` against `events`, `todos`, exception, or reminder tables at all, enforced by "a repository capability test" — "This is what makes recurrence/exception corruption unreachable from G"; §5.0 "recurrence guard" asserts RRULE, EXDATE, and every exception row unchanged; §3.3's diff surfaces recurrence explicitly (`RECURRING — scope=series, 12 occurrences, 2 exceptions`) rather than hiding it in a count. |
| Timezone/DST drift | **Pass** | §6.4 T2 "G performs no temporal arithmetic on user data; it moves whole aggregates through their owning use cases"; all G-owned timestamps are UTC `timestamptz` (§4.2). See T2 for the residual gap (undefined `bulk move` flags), which is a specification hole, not a drift-permitting design. |
| Duplicate reminder delivery | **Pass** | §6.4 T5 "G never claims, delivers, or dedupes a reminder… a bulk operation that changes a reminder definition goes through E's owning use case and produces no delivery"; the `(reminder_id, scheduled_for)` unique constraint is asserted by the verify and restore invariant suites (§3.2 G3.4). See T5 for the unaddressed restore-rewinds-delivery-state consequence — a documentation and warning gap, not a design that permits duplicates in normal operation. |
| Non-idempotent scans | **Pass** | §3.6 "migrate is idempotent"; §5.2 test 1 applies migration 0006 twice asserting idempotence and untouched existing rows; §7.5 forbids background or scheduled anything. |
| Plaintext credentials / secret logging | **Pass** | §3.2 G3.2 writes `config.toml.redacted` with any connection credential replaced by `REDACTED`; §3.3 "Redacted connection summary at the top; never a URL"; §3.6 "Diagnostics never echo event titles, descriptions, locations, attendee URIs, or connection credentials"; §6.1 collects nothing beyond the database, config, and projection — "no `~/.pgpass`, no shell history, no OS credentials"; §6.5 minimizes the subprocess environment and keeps logs to IDs, counts, and codes. Tested by §5.0 secret containment, §5.1 `redaction_of_connection_summary`, §5.3's full-session secret scan, and a §5.5 release gate. |
| Network access outside explicit sync | **Pass** | See I4 — zero sockets outside the configured PostgreSQL connection, with a dedicated contract test and a release gate. |
| Automatic conflict overwrite | **Pass** | Every divergence stops rather than resolves: `plan_stale` (§3.2 G1.7) and `undo_stale` (§3.2 G2.4) both write nothing and report. §3.1 rule 2: `--yes` "never resolves an ambiguous selector, never waives a fingerprint check". |
| Loss of unsupported iCalendar properties on round trip | **Pass** | §5.0 "opaque bytes through bulk" asserts byte, order, parameter, and hash identity; §6.4 I1 adds that the backup content fingerprint and invariant suite cover the extension columns "so a restore that dropped them fails verification". Coverage breadth is the I1 deduction, not an auto-fail. |

---

## Lens 4 — Operational Security and Reliability (weight: 15%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| O1 Credentials | 3 | Redaction is specified at every egress and then tested at the boundary. §3.2 G3.2 `config.toml.redacted`; §3.3 "never a URL"; §3.6's closing line covers diagnostics; §6.1 bounds collection to the database, config, and projection only; §6.5 "the environment passed to `pg_dump`/`pg_restore` is minimized and carries no credential the parent did not already resolve" and logs carry "never a connection string, never a credential". The 3-anchor's "secret-boundary tests and sanitized diagnostics/artifacts" is met literally: §5.0's secret-containment vector requires the fixture credential `sup3rsecret` to appear "in no stdout, stderr, JSON field, manifest, or archive file" — the artifact surface, not just the console — reinforced by §5.1, §5.3's full-session grep, and a §5.5 release gate. | — |
| O2 Least privilege | 3 | **No sudo automation anywhere; privileged steps are printed only.** §3.2 G4.3 is explicit — "Administrator remediations are **printed, never executed**: `sudo -u postgres createuser --login \"$USER\"` appears as text with the standing note that mg-calr never invokes sudo" — and §4.5 confirms the only thing doctor executes is `--version` output. §3.2 G3.4 keeps the rule absolute — "**mg-calr never creates or drops a database on its own**" — and Q3 defends holding that line rather than exploiting a `CREATEDB` role. §4.3's Auth paragraph: "every G operation runs as the existing unprivileged peer role… mg-calr requests no new privilege and never escalates". §7.5 and §6.5 restate it as a non-goal ("never creates or drops a role or database, and never edits `pg_hba.conf`"; "no `--force`"). The 3-anchor's executable clean-machine recovery is a real gate in §5.5: a clean-Arch container run that "proves `doctor` fails read-only with the prerequisite matrix, applies the printed administrator steps out of band, migrates unprivileged, and completes a backup → verify → restore round trip", with §5.4 manually confirming admin commands "read as instructions, not as something that already ran". Role boundaries surface in the machine contract via §4.3's `prerequisites[].owner: "administrator"`. | — |
| O3 Failure contracts | 3 | §4.3's error-code list maps to exit classes that match the real `src/lib.rs::exit_code` exactly (64 required input, 65 invalid, 66 not found, 69 unavailable, 70 serialization, 74 local I/O, 75 conflict, 78 config — all verified present). §3.6 tabulates 24 error conditions with trigger, code, exit, recovery path, and an explicit data-loss-risk column. Atomicity is stated per path: one serializable transaction by default, `pg_restore --single-transaction --exit-on-error` "so any failure or crash leaves the target unchanged", and `--plan-out` through the same temp+fsync+rename discipline "so a crash cannot leave a truncated plan file that looks valid" (§4.4). Fault and concurrency tests are concrete (§5.2 tests 3, 4, 7; §5.0 "restore atomicity" under a fault-injection build, "crashed migrate", "partial run visibility"), and §3.2 G6 plus `recovery status`'s printed commands constitute the recovery runbook. Q6 flags the one unresolved code choice honestly. | — |
| O4 Verification | 3 | §5.5's gate commands match the repo's real conventions (`cargo fmt --all -- --check`; `TMPDIR=/dev/shm cargo clippy --workspace --all-targets --all-features -- -D warnings`), and §5.2's opt-in preamble reproduces the repository's actual integration guard exactly — `MG_CALR_RUN_DATABASE_TESTS=1` plus `MG_CALR_TEST_DATABASE_URL` containing `mg_calr_test`, ignored by default (verified against `tests/postgres_integration.rs:9–21`). Coverage spans all four anchor tiers: unit and property (§5.1, including compile-level tests that `Confirmation` cannot be built from `bool`), opt-in isolated integration (§5.2, ten scenarios), process-level E2E with golden JSON contracts (§5.3), and release gates for clean-machine packaging, migration, fault injection, secret scan, and zero-non-PostgreSQL-network (§5.5). §5.0's fifteen binding acceptance vectors state exact expected exit codes and byte-identity conditions rather than prose. §5.5 closes with the right precedence rule: "A green happy path never overrides a failed safety, migration, privilege, secret, or recovery gate." | — |

**Lens average:** 3.0
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Feasibility Check

Verified against `src/storage.rs` (embedded `MIGRATIONS`), `src/application.rs`, `src/main.rs`, `src/config.rs`, `src/lib.rs`, `src/domain.rs`, `src/interop.rs`, `migrations/*.sql`, `tests/migration_contract.rs`, `tests/postgres_integration.rs`, `tests/config_contract.rs`, `Cargo.toml`, and `docs/ARCHITECTURE.md` at `6e855f9`.

| Check | Status | Notes |
|---|---|---|
| Types/models exist or are clearly specified | ✓ | `audit_log` really has `transaction_id`, `before_state`, `after_state`, `occurred_at` (`0001_foundation.sql:88–98`), so §4.2's `ADD COLUMN IF NOT EXISTS undoes_transaction_id / before_revision / after_revision` and the `(transaction_id, occurred_at)` index are all applicable. `events.extension_properties jsonb` exists, grounding the opaque-bytes vector. Optimistic `version` columns exist for events and todos. **One inconsistency:** §4.2's structs use `OffsetDateTime` (the `time` crate) while the repo uses `chrono` and §4.5 asserts "No new Rust crates are required" — `time` is not in `Cargo.toml`. Should be `DateTime<Utc>`. |
| API/interface changes are feasible with current architecture | ✓ | The application-layer surface in §4.3 fits the existing `src/application.rs` use-case style. **Caveat:** §4.1 claims it is "following the module boundaries in `docs/ARCHITECTURE.md`", but that document defines only flat `src/domain.rs`, `src/config.rs`, `src/storage.rs`, `src/main.rs` and assigns "human/JSON rendering" to `main.rs`. The spec's `src/cli/*` and `src/render/*` are a restructuring, not a conformance. Mechanically fine in Rust 2018+ module style (`src/application.rs` can coexist with `src/application/bulk.rs`), but §4.1 mischaracterizes it and §7.2 does not list `docs/ARCHITECTURE.md`'s boundary section as needing revision. |
| Views/screens fit current navigation pattern | ✓ | Terminal-only; §3.1's grammar composes with the existing global `--json`, `--no-input`, `--no-color`, `--database-url` flags, which are real (`src/main.rs:34` and dispatch sites). `doctor` genuinely exists today as a `migration_status` alias, matching the "Modified" marking. |
| Dependencies are available and version-compatible | ✓ | `sha2 0.10`, `fs2 0.4.3`, `rustix 1.1` (with `fs`), `serde_json`, `uuid` (v7), `chrono`, `tokio-postgres`, `thiserror` are all present in `Cargo.toml`; `tempfile 3` is a dev-dependency exactly as §4.5 describes. `pg_dump`/`pg_restore` are correctly treated as external binaries that are detected and version-checked rather than assumed. |
| Platform/renderer requirements are realistic | ✓ | Arch + PostgreSQL 18 matches the repo's peer-auth socket default (`/run/postgresql`, `config/example.toml`). §4.6's directory-fsync fallback aligns with the real `rustix`-based `openat`/`renameat`/`sync_all` discipline in `src/interop.rs`. |
| Test strategy is executable with current infrastructure | ✓ | §5.2's opt-in guards are the repo's actual convention, verbatim. §5.3's `assert_cmd` matches the existing `tests/cli_contract.rs` pattern. **Caveat:** §5.0's "recurrence guard" vector asserts "EXDATE, and every exception row unchanged" and §3.2 G3.4 asserts "no orphaned exceptions", but **no occurrence-exception table exists anywhere in `migrations/`** — a grep for "exception" across `migrations/` and `src/domain.rs` returns nothing. Those assertions are unrunnable until B ships exception storage, and §7.1's "Absent" list does not disclose this. |
| Performance budget is realistic for target hardware | ✓ | §4.7's numbers are defensible: a ≤40-check bounded registry, `--limit` 5000 / hard cap 50000 at ~200 bytes per item, 1 MiB streaming checksums, and one indexed single-row compatibility probe. It correctly declines to promise a latency for `backup create`, reporting bytes and duration instead. |
| No undeclared dependency on unbuilt features | ✗ | Mostly excellent — §7.4 names A5 audit, B/C/E aggregate APIs, and the G3-before-G5-rollback-honesty ordering. But two gaps: (a) §3.2 G6.5 makes "B's `mutation status OPERATION_ID`" the authority for uncertain commits, and §7.1 correctly lists `mutation status` as absent, yet §7.4 declares "G-a blocks nothing and is blocked by nothing" — G6 is in G-a, so this is a self-contradiction; (b) occurrence-exception storage (above) is an undeclared prerequisite for the recurrence acceptance vectors. |

**Feasibility verdict:** Feasible with caveats

**Caveats:**
1. **Blocking, must fix before implementation — migration version collision.** §4.1/§4.2/§7.2 propose `migrations/0006_safety_operations.sql`. Version 6 is already registered: `src/storage.rs` `MIGRATIONS[5] = {version: 6, name: "repair_todo_recurrence"}` with `migrations/0006_repair_todo_recurrence.sql` present in the working tree. `storage::migrate` reads `SELECT name, checksum FROM mg_calr_schema_migrations WHERE version = $1` and returns `StorageError::MigrationDrift` on a name mismatch, so this migration would hard-fail on every existing database. `tests/migration_contract.rs` would also break — it asserts `MIGRATIONS.len() == 6` and `MIGRATIONS[5].version == 6` bound to `REPAIR_TODO_RECURRENCE_MIGRATION`. **For the record: sibling specs A, E, and F each independently propose a version 6 as well, so this spec's number must be unique across the entire spec set, not merely greater than 5.** The gauntlet needs a single disjoint assignment across A/E/F/G before any of them is implemented.
2. **Internal contradiction in the §5.0 schema vectors.** "older binary vs newer schema" states "binary embeds 6"; the immediately following "unknown recorded version" states "ledger has version 6 that the binary does not embed". These contradict each other, and the second is false at HEAD. Both vectors test the G5 guard and both are unrunnable as written. The underlying §7.1 mechanism claim — that `migration_status` iterates only the embedded `MIGRATIONS` slice and therefore silently ignores a higher recorded version — is **verified correct**; only the version numbers are wrong.
3. **Stale line references (low severity, working tree is dirty).** `src/main.rs` init guidance is at ~1227, not ~1211; `src/storage.rs` `scan_reminders` `dry_run` is at ~1894, not ~1829; `src/interop.rs` atomic-write discipline is at ~561–563 and ~1059–1176, not ~434–500 and ~955–1030. §7.1's "migrations 0001–0005" and §3.2 G5.6's "migrations 0002–0005" both predate 0006. All mechanism claims behind these references were verified accurate; only the coordinates drifted.
4. **§7.1 accuracy spot-checks all passed otherwise.** Verified true: `audit_log` exists but no code reads or writes it (grep over `src/` returns nothing); `doctor` is a literal alias for `migration_status` and hardcodes `database_reachable: true` / `administrator_guidance: Vec::new()` (`src/main.rs:1208–1210`); `init` prints the sudo examples with the never-invokes-sudo note; `todo purge --yes` is indeed the only destructive-confirmation gate in the product (`event cancel`, `event restore`, `todo trash`, `todo restore` are all reversible, version-locked operations); `--dry-run` exists only on `todo scan-reminders`; `migrate` uses `pg_advisory_xact_lock`; exit codes and the `{schema_version, command, ok, data}` envelope match `src/lib.rs` exactly.
5. **`time` vs `chrono`** in §4.2 (see the types row) — a one-line correction that also removes a contradiction with §4.5's no-new-crates claim.

---

## Composite Score

| Lens | Average | Weight | Weighted |
|---|---|---|---|
| Temporal and Data Integrity | 2.40 | 35% | 0.840 |
| CLI Usability and Automation | 2.60 | 25% | 0.650 |
| Standards Interoperability and Sync | 2.50 | 25% | 0.625 |
| Operational Security and Reliability | 3.00 | 15% | 0.450 |
| **Composite** | | | **2.57** |

**Pass conditions (from criteria.md):**
- [x] Composite ≥ 2.0 — 2.57
- [x] All lens averages ≥ 2.0 — 2.40 / 2.60 / 2.50 / 3.00
- [x] No criterion scores 0
- [x] No more than two criteria at 1 per lens — zero 1s in any lens
- [x] All auto-fail rules pass — all eleven walked individually above
- [x] Feasibility ≠ Infeasible — Feasible with caveats

**All conditions met:** Yes → PASS

---

## Required Corrections Before Implementation

The verdict is PASS, but caveat 1 is a hard blocker for any agent that tries to
build from this spec. These are ordered by whether an implementer can proceed
without them.

### Priority 1 — Blocking for implementation (do not start without these)

1. **Renumber the migration.** Replace every reference to
   `migrations/0006_safety_operations.sql` (§4.1 last bullet, §4.2 SQL block
   heading, §7.2 "Migrations/schema", §5.2 test 1) with a version strictly
   greater than 6 **and unique across the A/E/F/G spec set** — all four
   currently claim version 6. Coordinate a single disjoint assignment at the
   gauntlet level; do not resolve it independently inside this spec. Version 6
   is held by `repair_todo_recurrence` in `src/storage.rs` and asserted by
   `tests/migration_contract.rs`, and reusing it produces
   `StorageError::MigrationDrift` on every existing database.
2. **Fix the two contradictory §5.0 schema vectors.** "older binary vs newer
   schema" and "unknown recorded version" disagree about whether the binary
   embeds version 6. Rewrite both against one consistent
   `EMBEDDED_SCHEMA_VERSION` that reflects the renumbered migration, with a
   recorded version that genuinely exceeds it. Also update §4.3's doctor JSON
   example ("database schema 99, binary embeds 6").
3. **Reconcile §3.1's grammar with §3.2/§4.3 for `backup prune` and
   `recovery clean`.** §3.1 renders both as single-shot `--yes` commands while
   §3.2 G3.6/G6.3 and §4.3's `plan_prune`/`apply_prune` and
   `plan_recovery_clean`/`apply_recovery_clean` describe two-phase plan→apply.
   Give both explicit `plan`/`apply` subcommands taking a plan ID and
   fingerprint, matching `bulk`, or state in §3.1 why they are exempt. As
   written, an implementer following §3.1 ships a deletion path that never
   shows the user the list.
4. **Resolve the G-a dependency self-contradiction.** §7.4 says "G-a blocks
   nothing and is blocked by nothing", but §3.2 G6.5 makes B's
   `mutation status OPERATION_ID` the authority for uncertain commits, and §7.1
   lists `mutation status` as absent. Either add it to §7.4 as a G6 blocker or
   specify a G-owned fallback for uncertain-commit resolution that does not
   require B.
5. **Disclose the missing occurrence-exception storage in §7.1 "Absent".** No
   exception table exists in any migration, so §5.0's "recurrence guard"
   (EXDATE and exception rows) and §3.2 G3.4's "no orphaned exceptions"
   invariant are unrunnable today. Add it to §7.4 as a B-owned prerequisite for
   the recurrence acceptance vectors.

### Priority 2 — Should fix for quality

6. **Define `bulk move`'s mutation flags** (§3.1 `[mutation flags for the verb]`
   is never expanded for any verb). State explicitly whether `move` can alter
   start/end/timezone; if it can, add DST-gap and DST-fold vectors to §5.0.
   Specify timezone interpretation for `history --since`/`--until`. (T2)
7. **Document the restore/reminder-delivery interaction** (T5). Add a §3.6 row
   and a §3.2 G3.5 note that restoring an archive rewinds
   `reminder_deliveries` and may re-present reminders whose deliveries
   post-date the archive; the current §3.6 restore rows all read "Data-loss
   risk: none" without qualification.
8. **Add a pure resolution test for the new configuration surface** (C3) to
   §5.1 — `--output-dir` > env > `[backup].dir` > `$XDG_STATE_HOME`, plus plan
   expiry — and either name the env variable in §3.2 G3.1 or state in §6.4 that
   the backup root deliberately has no env tier, since §6.4 currently claims
   the full "CLI > env > TOML > default" chain that §3.2 G3.1 does not
   implement.
9. **Specify `--operation-id` semantics** (C2). It appears in §3.1's grammar
   and in `apply_bulk(…, OperationId)` and is load-bearing for §3.2 G6.5, but
   its idempotency behavior on retry is never defined.
10. **Correct `OffsetDateTime` → `DateTime<Utc>`** in §4.2's `BulkPlan`, which
    currently contradicts §4.5's "No new Rust crates are required".
11. **Correct §4.1's architecture claim.** It says it follows
    `docs/ARCHITECTURE.md`'s module boundaries while proposing `src/cli/*` and
    `src/render/*`, which that document does not define (it assigns rendering
    to `main.rs`). Either describe it as a restructuring or add
    `docs/ARCHITECTURE.md`'s boundary section to §7.2's modified-files list.

### Priority 3 — Consider for excellence (2 → 3)

12. **I1:** extend the opaque-bytes vector from one verb to a matrix over every
    mutating bulk verb plus undo, and add a §5.1 property test over generated
    extension-property payloads including malformed and reordered inputs.
13. **I3:** add a bulk trash → bulk restore round-trip vector asserting
    `deleted_at` clears while `remote_tombstoned_at` is untouched, revisions
    increment, and RRULE/exception state survives.
14. **C1:** add an affordance for reaching a just-created plan (a
    `bulk plan list --latest`, a `--plan last` alias, or shell completion) so
    the safest path is not the most clipboard-dependent one.
15. **Refresh stale line references** in §7.1 (`src/main.rs` ~1227,
    `src/storage.rs` ~1894, `src/interop.rs` ~561 and ~1059) and update
    "migrations 0001–0005" / "0002–0005" to include 0006. Note that
    `storage::migrate` already contains a version-3 in-place
    `ALTER COLUMN … TYPE jsonb` rewrite, which slightly qualifies §3.2 G5.6's
    claim that existing migrations contain "no destructive rewrite outside an
    explicit gate".
16. **`--plan-out FILE`** (§4.4) does not say whether it refuses an existing
    path; the atomic rename would overwrite silently. Low stakes (a derived
    artifact), but the spec's own standard argues for refusing.
