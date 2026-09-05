# Scorecard: Reminder System

**Feature ID:** e-reminder-system
**Spec file:** docs/specs/e-reminder-system.md
**Reviewer agent:** blind verification agent
**Date:** 2026-08-30
**Spec iteration reviewed:** 2
**Graded against commit:** `6e855f9`

---

## Verdict: PASS

**Summary:** The strongest quality is T5: duplicate prevention is a *mechanism*, not an assertion —
a DB-level four-field unique claim key, a claim committed before any side effect, a pre-call fence/lease
re-check that refuses the side effect itself, fence-guarded dependent writes, and a two-class
`BackendError` taxonomy in which the only retryable class is one that provably rendered nothing.
The most critical gap is a factual defect in §4.2: the backfill bullet copies a
`reminder_deliveries.transport` column into `audit_log`, and no such column exists in any migration
(`transport` is a hard-coded Rust `&'static str` at `src/application.rs:776`), so migration 7 as written
would fail on every database. Fix that clause and the ledger design stands.

---

## Lens 1 — Temporal and Data Integrity (weight: 35%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| T1 Identity | 3 | §4.2 `DeliveryKey` is a four-field total key with "no default constructor, so a delivery cannot be written without full identity"; typed UUIDv7 `DeliveryId`/`ReminderId` verified at `src/domain.rs:112-113`. §3.3 makes short IDs explicitly non-authoritative: "A short ID is a display convenience only and never carries authority", with `delivery_selector_ambiguous` (65) printing full UUIDs and mutating nothing "before any transaction opens". Tombstone identity via `revoked` (invariant 6); conflict identity via `delivery_state_conflict` / `reminder_claim_fence_stale`. Fixtures: §5.1 `delivery_key_is_total_and_ordered`, `short_id_prefix_resolution_is_unambiguous`; §5.2(10) golden DTOs. | — |
| T2 Temporal correctness | 3 | §4.2 invariant 2: `scheduled_for` is UTC derived from civil intent, "so a TZ change or DST transition cannot shift a stored trigger". DND quiet hours evaluated in a stored IANA zone with B4 gap/fold policy (§3.2 E6). Vectors: §5.1 `dnd_window_evaluation_is_zone_correct` (DST gap *and* fold), `catch_up_classification_boundaries` (half-open); §5.0 rows 8 (suspend across two due instants) and 17 (clock moved back 2 h). §4.5 detects wall-clock jumps with `TFD_TIMER_CANCEL_ON_SET` on `CLOCK_REALTIME` and suspend by the `CLOCK_BOOTTIME`−`CLOCK_MONOTONIC` delta rather than by polling. Recurring-exception vectors are explicitly gated on B6/B7 in §7.4 rather than assumed. | — |
| T3 Transaction integrity | 3 | §3.2 step 5 is a single atomic CAS with the isolation level *named and reasoned*: READ COMMITTED, "the loser's `UPDATE` re-evaluates its predicate against the winner's committed row and returns zero rows", plus a bounded-retry path for SQLSTATE `40001` under a stricter session default and the rule "A serialization failure is **never** treated as licence to present." DB-level enforcement, not app-level: `reminder_deliveries_claim_key` unique index, `CHECK (state IN …)`, `CHECK (claim_fence >= 0)`. §5.2(3) injects faults after claim commit, backend call, presented write, digest insert, and audit insert; §5.2(4) runs 8 concurrent dispatchers over 500 due rows asserting exactly 500 presentations; §5.1 adds property tests over crash points and reconciliation races. | — |
| T4 Deletion/audit | 2 | Strong in design: invariant 6 retains `revoked` rows ("Deliveries are never deleted by the scanner; purge is a G-owned operation"); invariant 9 gives a total restore-revival predicate; §4.2 converts the migration-1 `ON DELETE CASCADE` (verified at `migrations/0001_foundation.sql:77`) to nullable `SET NULL` so "the ledger must outlive the definition"; every transition writes `audit_log` (table verified, `0001_foundation.sql:88`). **Defect:** the provenance clause "The legacy migration-1 `transport` text … is copied into the `audit_log` before/after JSON" names a column that does not exist — `grep -rn transport migrations/ src/` returns only `src/application.rs:776` and `src/storage.rs:1989`, both Rust. That SQL step would abort migration 7. §4.2 also says "Change the migration-1 foreign key … to nullable" without spelling out the required `ALTER COLUMN reminder_id DROP NOT NULL` (the column is `NOT NULL` today). | Delete the `transport` clause from the §4.2 backfill bullet, or replace it with the real provenance source (there is no delivery-row transport value to preserve). Add the explicit `DROP NOT NULL` on `reminder_deliveries.reminder_id`. |
| T5 Reminder idempotency | 3 | See the adversarial walk below. Anchor 3 met literally: durable unique claim state **plus** a 32-row retry/crash/sleep/DND matrix (§5.0) that asserts two *separate* counters — `present()` calls and bubbles rendered — with the binding rule "nothing in this matrix may ever raise the second above 1". Rows 3, 5, 6a, 21, 23, 25 cover the crash/unknown-outcome/fence cases individually. | — (two residuals recorded as Priority 2/3) |

**Lens average:** 2.80
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

### T5 adversarial walk (anchor criterion)

**Is the flush discriminator observable at every listed error?** Mostly, but **§4.3 states one rule and its
table applies another.** The stated discriminator is "was any byte of the `Notify` method call flushed to the
bus socket?" Two rows contradict it: `ServiceUnknown`/`NameHasNoOwner` → `NotSent(NoNameOwner)` and bus-policy
`AccessDenied` → `NotSent(PolicyDenied)`. In both, the `Notify` message *was* serialized, assigned a serial,
written to the bus socket, and read by the bus daemon — which then replied with an error. Under the literal
flush rule these are `UnknownOutcome`; the table classifies them `NotSent` using a different (unstated) rule
in its "Why" column — *did the message reach a notification server*. The two rules are not the same bit.
**This is a precision defect, not a safety defect:** the bus daemon is authoritative that it did not route,
so no bubble rendered, and the disagreement resolves conservatively either way (the literal rule yields more
recorded misses, never a duplicate). The spec should say plainly that the classifier is two questions —
*refused locally* and *refused by the bus before routing* are both `NotSent`; everything at or past routing
is `UnknownOutcome`.

**Any path where a notification renders but classifies as `NotSent`?** I could not construct one.
`Marshal`, `Throttled`, `TransportForbidden`, `NoBus`, `ConnectFailed`, `HandshakeFailed` are all strictly
pre-write. `WriteFailedBeforeFlush` requires "**zero** bytes of this call written", and where the client
library cannot establish that, §4.3 forces `UnknownOutcome`. The `log` backend's `NotSent` classification is
guarded by "**only when** the write is refused before any byte reaches the file"; a short or interrupted write
is `UnknownOutcome`. The bus-rejection rows are safe for the reason above.

**Does the fence stop a stalled scanner that resumes after reclamation?** Yes, at both points, given the
specified ordering. The reconciler writes its terminal state and `claim_fence = claim_fence + 1` in **one**
statement (§4.6), so (i) a stalled owner that resumes *before* `present()` hits §3.2 step 6's pre-call re-read,
sees a moved fence, and "`present()` is not called at all" (§5.0 row 25 asserts the backend records zero calls);
(ii) an owner that resumes *after* `present()` finds its `AND claim_owner=$owner AND claim_fence=$fence`
predicate false and returns `reminder_claim_fence_stale` (§5.0 row 23, §5.2(11c)). Under the shipped default
`recovery.represent_unconfirmed = never`, the residual TOCTOU window between the pre-call re-read and the
call itself still yields exactly one bubble, because `unconfirmed_lost` is never re-presented.

**Does "unclassifiable defaults to `UnknownOutcome`" hold everywhere?** Yes. §4.3: "When the client library
cannot answer the question, the adapter classifies `UnknownOutcome`." §5.1
`dbus_failure_modes_map_to_the_documented_class`: "An unclassifiable failure defaults to `UnknownOutcome`."
The enum has no `Other`, and §5.1 `backend_error_classification_gates_retry` requires the transition planner
to match "with **no wildcard arm**, so a variant added later fails to compile until it is classified". I found
no fallback defaulting the other way; every `NotSent` classification in the table and in both non-D-Bus
backends is affirmatively guarded rather than assumed.

**The one hole I did find.** §4.6 says proof of death "is **any one** of (a)–(d)". Proof (b) —
`heartbeat_at` older than `2 × lease` — is a liveness *heuristic*, not proof, and OR-ing it with the
dispositive checks (c) boot-id mismatch and (d) `/proc/<pid>` absent-or-start-time-mismatch means (b) can
declare a process dead that (d) would show alive. Suspend is the concrete trigger: the daemon suspends
between claim and `present()`, wall clock advances hours, `heartbeat_at` is stale on resume, and (b) fires
against a live owner. Under the default `never` this still produces one bubble. Under
`recovery.represent_unconfirmed = once` it is the spec's only reachable duplicate: the reconciled row is
re-presented for a bubble that then actually rendered. §4.6 names exactly this hazard in prose — "the
reconciler's terminal write plus `recovery.represent_unconfirmed = once` would then show a second bubble" —
and builds proof-of-death to prevent it, but the OR leaves (b) as the weak link, and §5.1
`lease_expiry_alone_never_reconciles` confirms the OR semantics ("each of proofs (a)–(d) **independently**
unlocks the transition") without testing (b)-fires-while-alive. Not an auto-fail — unreachable in the
shipped default configuration, which Q2 locks — but it should be closed (Priority 2 below).

---

## Lens 2 — CLI Usability and Automation (weight: 25%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| C1 Human workflow | 3 | §3.1 defines nine command views with navigation and layout; §3.6's 18-row table gives every trigger a Recovery column and a data-loss column, not just an error string. §3.2 "Guided and non-interactive parity" makes every bubble action a flag-complete command, and §3.7 contract-tests that equivalence ("not documented aspiration"). §3.4 specifies `Ctrl-C` → 130 "before any transaction opens", `SIGTERM` graceful shutdown, `SIGHUP` reload without dropping claims. Verb-noun `remind <verb>` matches the Clap style already in `src/main.rs`. | — |
| C2 Automation | 3 | §4.3 pins the existing `schema_version: 1` envelope (verified `src/lib.rs:146-157`), deterministic ordering by `(scheduled_for, schedule_ref, occurrence_key)`, `--no-input` that "never reads stdin", and a complete code→exit table. Anchor-3 items are all three present and executable: §5.2(10) pins the DTO, five success envelopes, and **one error envelope for every code** as byte-for-byte golden fixtures; §4.4/§4.3 state an additive-field compatibility policy gated on D9; §3.3 guarantees selectors "never mutate ambiguously" with §5.3(2a) asserting the ledger is byte-identical after an ambiguous `remind snooze`. | — |
| C3 Configuration | 3 | §4.4 gives the complete 19-key `[reminders]` table with type, default, and bounds, and closes it: "This is the complete list; nothing in E reads a setting absent from it." Distinct XDG roots verified at `src/config.rs:54-59`; redaction reuses `ConnectionSettings::safe_summary` (verified `src/config.rs:113`). Anchor 3 met on all three: purity test `reminder_config_resolution_is_pure` ("touches no clock, filesystem, or socket", byte-identical across 1,000 iterations), redaction (§6.1), and a stated unknown-key / deprecated-alias / removal compatibility policy. Cross-field validation is real, not decorative: `lease` "**must exceed `backend_reply_timeout`**". | — |
| C4 Output/accessibility | 3 | §3.3 fixes column order and requires words over glyphs (`DEFERRED (dnd_quiet_hours)`); §3.7 covers screen readers, custom-action CLI equivalents, text scaling, color independence, and focus order. §3.4 degrades at 80 and 50 columns with "identity, state, reason, and time never truncate". §5.3(5) asserts 40/80/200 columns with `--no-color` and `NO_COLOR=1` and "no ANSI bytes appear"; §5.4 adds screen-reader, RTL, and combining-character verification. Quickshell public-interface behavior is addressed as deferred JSON consumers (§3.1, §7.5). | — |
| C5 Diagnostics | 2 | `remind doctor` is non-mutating and proven so by schema snapshot equality (§5.3(6)); `remind install-units` "prints unit text to stdout; `--write` required" and never invokes systemctl or sudo; check names are stable and specific (`deliveries.orphan_channel`, `claims.expired_unproven`, `scanner.stopped`); a prerequisite matrix exists (§4.6 PostgreSQL 18, systemd ≥ 249, fdo Notifications 1.2 with 1.3 `Inhibited` optional) and §3.6 prints the exact `busctl --user list` diagnostic. **Gap against "stable machine output":** §5.2(10) pins golden envelopes for `remind.scan`, `.list`, `.show`, `.catch-up`, and `.dnd` — `remind doctor` is **not** in that list, so the doctor check matrix has named checks but no pinned machine schema. | Add the `remind doctor` / `doctor --component reminders` envelope to the §5.2(10) golden-fixture set so the check matrix is contract-stable, not merely named. |

**Lens average:** 2.80
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Lens 3 — Standards Interoperability and Sync (weight: 25%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| I1 Lossless iCalendar | 2 | Not a hollow N/A. §6.4 I1 states E "is strictly read-only over definitions" and enumerates the five tables it writes, and backs it with two independent proofs: §5.2(8) hashes `events.extension_properties` (column verified at `migrations/0001_foundation.sql:28`), alarm rows, and todo payloads before and after a full scan/dispatch/catch-up cycle requiring byte equality; §5.2(7) proves the database role **cannot** write `events`/`todos`/`reminders`/`extension_properties` "at the database, not only in code". Codec ownership is explicitly deferred to F1 with no architecture that precludes it. Anchor 3 (golden/property tests over malformed/edge iCalendar fixtures) is correctly out of scope — E parses no iCalendar — so the literal anchor caps at 2. | — (deferral is correct; anchor 3 is F1's to earn) |
| I2 Sync authority | 2 | §4.4 and §6.4 I2: "PostgreSQL is the single delivery authority"; the `mg.interop/1` projection is "a read-only *schedule source* and is explicitly **not** a delivery authority"; the daemon "never writes it, never writes a vdir mirror, and never creates a second store of delivery state". Fails closed on stale/conflicting projections with the existing codes (verified `projection_stale`/`projection_conflict`, `src/lib.rs:83-84`), and §4.4 refuses "deliver now and record later" outright. §5.2(6) tests the source boundary with distinct `schedule_ref` namespaces. Anchor 3 (three-way fingerprints, explicit orchestration) belongs to F8–F10 and is deferred by name; there is no vdir mirror here because E owns none. | — |
| I3 Conflict/deletion | 3 | Both halves of the round trip are specified and fixtured. Delete: invariant 6 transitions vanished schedules to `revoked` with retained provenance, and the FK change means "a deleted-and-recreated definition cannot resurrect an already-presented occurrence". Restore: invariant 9 gives a *total*, single-statement predicate (`state='revoked'` ∧ `presented_at IS NULL` ∧ `attempts = 0` ∧ `scheduled_for > now()`) that "cannot revive a row that ever presented or was ever claimed" and bumps the fence. Tombstones are separated by kind (`revoked` / `expired` / `unconfirmed_lost` / `failed`, each with a reason). Fixtures: §5.0 rows 26–28 and §5.2(12) (trash/restore three times, assert one presentation and a complete `audit_log` chain). Sync-conflict resolution is explicitly F8–F10's. | — |
| I4 Scope/network | 3 | Explicitly refuses N/A and does the work. §6.4 I4: "The only sockets opened in any path are the PostgreSQL Unix socket (peer auth) and the D-Bus session Unix socket" — the spec states outright that D-Bus over a Unix socket is not network access and then constrains it: a `DBUS_SESSION_BUS_ADDRESS` whose transport is not `unix:` is refused with `reminder_backend_transport_forbidden` **before connecting** (§3.6, §4.3 `NotSent(TransportForbidden)`). Both anchor-3 proof layers exist: adapter test §5.1 `bus_address_transport_guard` (`tcp:` and `autolaunch:` refused before any connection attempt) and command-wide observation §5.2(9), "run every reminder command and one full daemon cycle under a socket-observing harness; assert only the PostgreSQL and D-Bus Unix sockets are opened, and zero AF_INET/AF_INET6 sockets or DNS lookups". §4.5 forbids linking any HTTP/TLS/DNS client and §4.7 budgets network payload at "Exactly zero"; icons and sounds are theme *names*, never fetched. | — |

**Lens average:** 2.50
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s
**Auto-fail triggered:** No

---

## Lens 4 — Operational Security and Reliability (weight: 15%)

| Criterion | Score (0–3) | Evidence from spec | Remediation needed |
|---|---|---|---|
| O1 Credentials | 3 | §6.4 O1 and §6.1: no credential is stored, prompted, or logged; database URLs and bus addresses redacted through the existing `ConnectionSettings::safe_summary` discipline (verified `src/config.rs:113`). §3.6 closes the error channel specifically: "No error message contains a database URL, a bus address with credentials, or reminder body text beyond a truncated title." Sanitized artifacts: logs "record IDs, states, and reasons but never bodies"; `log_backend_path` is confined under `XDG_STATE_HOME` and created `0600`; `notification.privacy = generic` renders "Reminder — open mg-calr" for shared/locked sessions. The secret boundary is executable, not aspirational: §5.2(10) pins every error envelope byte-for-byte and §5.5 lists a secret scan as a release gate. | — |
| O2 Least privilege | 3 | §4.6 ships a systemd **user** unit under `~/.config/systemd/user/`, and `remind install-units` "never runs `systemctl` and never uses sudo"; §5.3(6) asserts it writes nothing without `--write` and that doctor "prints administrator commands without executing them or invoking sudo". Role boundaries are executable: §5.2(7) asserts the reminder role can write the five ledger tables and **cannot** write `events`/`todos`/`reminders`/`extension_properties`. §7.5 forbids system units, sudo, root, package-manager invocation, and PID-file singletons; the singleton is a PostgreSQL advisory lock instead, so "a killed process releases it automatically when its connection closes". §5.5 requires a clean-machine unit install/enable rehearsal. | — |
| O3 Failure contracts | 2 | Typed and stable: §4.3 extends the verified `code()`/`exit_code()` tables (`src/lib.rs:48,105`) with `AppError::Reminder(ReminderError)` and a full code→exit mapping; §4.2 invariant 4 is an explicit directed state machine tested by §5.1 `state_machine_rejects_illegal_transitions`; §5.2(3) is the fault-injection suite and §4.6 the recovery runbook. **Contradiction:** §3.6 twice names `remind scan --represent-failed` as the recovery action for a `failed` delivery, but invariant 4 declares `failed` **terminal for that row** and "Terminal rows are never re-presented". The flag appears in no view table (§3.1), no use-case signature (§4.3), no state edge (§4.2), and no matrix row (§5.0) — an undefined command that mutates a state the spec calls terminal. | Either define `--represent-failed` fully — its predicate (minimally `state='failed' ∧ presented_at IS NULL`), its state edge in invariant 4, its use case in §4.3, and a §5.0 row proving it cannot double-present — or delete it from §3.6 and make the recovery text "inspect and act on the item directly". |
| O4 Verification | 3 | §5.0 is a binding 32-row gate with an unusual and correct discipline: two counters per claim key, `present()` calls versus bubbles rendered, "and they are not the same counter". §5.1 names ~22 unit tests plus property tests with the global invariant stated precisely rather than gestured at. §5.2 gives 13 integration tests on the existing opt-in harness — verified real: `tests/postgres_integration.rs:9-21` enforces `MG_CALR_RUN_DATABASE_TESTS`, `MG_CALR_TEST_DATABASE_URL`, and refuses any database not named `mg_calr_test`. §5.3 adds 8 process-level tests via `assert_cmd` (present in dev-dependencies). §5.5 lists the real gate commands plus migration, secret-scan, privilege-matrix, network-denial, and clean-machine gates, with the rule "A green happy path never overrides a failed idempotency, privilege, or network gate." Only the package gate from anchor 3 is absent (packaging is out of repo scope). | — |

**Lens average:** 2.75
**Lens pass:** Yes — avg ≥ 2.0, zero 1s, no 0s

---

## Auto-fail rule walk (each rule individually)

| Rule | Result | Mechanism that delivers it (not assertion) |
|---|---|---|
| Silent event/todo loss | Pass | E writes only `reminder_deliveries`, `reminder_digests`, `reminder_dnd_windows`, `reminder_scanner_runs`, `audit_log` (§6.4 I1); §5.2(7) enforces this at the database role, so a code regression still cannot write `events`/`todos`. |
| Unconfirmed overwrite | Pass | Invariant 7: every claim-dependent write carries `AND claim_owner=$owner AND claim_fence=$fence`; a superseded owner "matches zero rows and raises `reminder_claim_fence_stale`". §5.2(11c) tests the resurrected owner's write against real PostgreSQL. |
| UID instability | Pass | Immutable typed UUIDv7 (`src/domain.rs:112-113`); §3.3 keeps short IDs display-only and JSON always emits the full UUID in `id`. |
| Recurrence/exception corruption | Pass | E is read-only over schedules (§4.4); recurring event occurrence keys are gated on B6/B7 in §7.4 rather than improvised. |
| Timezone/DST drift | Pass | Invariant 2 (absolute UTC from civil intent, never host-local); DND in a stored IANA zone with B4 gap/fold; §5.1 `dnd_window_evaluation_is_zone_correct`; §5.0 rows 8 and 17. |
| **Duplicate reminder delivery** | Pass | Layered and each layer is a real mechanism: (1) DB unique index `reminder_deliveries_claim_key`; (2) claim committed *before* any backend call (§3.2 step 5, invariant 1: "There is no in-memory-only claim"); (3) pre-call fence/lease re-check that refuses the side effect (§3.2 step 6, §5.0 row 25 asserts zero backend calls); (4) fence-guarded writes (invariant 7); (5) `UnknownOutcome` is never retried and "There is no code path from an unknown outcome to a second `present()` call for the same key" (invariant 8); (6) monotonic `attempts` cap; (7) reconciliation refuses to act without proof of death (§4.6). Verified by §5.0 rows 2, 3, 5, 6a, 21, 23, 25, 29, 30. **Residual:** proof (b) OR-ing (see T5 walk) is reachable only under the non-default `recovery.represent_unconfirmed = once`; the shipped default is `never`, so this does not trigger the rule — but it is Priority 2. |
| **Non-idempotent scans** | Pass | §3.2 step 2: materialization is `INSERT … ON CONFLICT (claim key) DO NOTHING` and "never presents, never claims, and never mutates a row that already exists in any state". The single exception is stated, bounded, and idempotent — invariant 9's revival is one statement whose `DO UPDATE … WHERE` clause excludes any row that ever presented (`presented_at IS NULL`) or was ever claimed (`attempts = 0`) and any past instant. Verified by §5.0 rows 1, 16, 27 and §5.1 `revoked_revival_predicate_is_total`. Suppressed schedules "produce **no** row at all". |
| Plaintext credentials / secret logging | Pass | §6.1, §3.6 error-content rule, §5.5 secret-scan gate, `0600` log file under `XDG_STATE_HOME`. |
| Network outside explicit sync | Pass | §6.4 I4 plus §5.1 `bus_address_transport_guard` and §5.2(9) socket observation across every command and a full daemon cycle. D-Bus-over-Unix is named as non-network *and* constrained: non-`unix:` transports refused before connecting. |
| Automatic conflict overwrite | Pass | Every mutation is a CAS; ambiguous selectors mutate nothing (§3.3, §5.3(2a)); `--rechannel` refuses any row whose target key is occupied and any row that ever presented, and "Nothing re-channels automatically" (§4.4). |
| Loss of unsupported iCalendar properties on round trip | Pass | §5.2(8) byte-equality hash fixture over `extension_properties` and alarm rows across a full cycle, backed by the §5.2(7) role denial. |

**Any auto-fail triggered:** No

---

## Feasibility Check

Verified against working tree at `6e855f9`.

| Check | Status | Notes |
|---|---|---|
| Types/models exist or are clearly specified | ✓ | `ReminderId`/`DeliveryId` UUIDv7 at `src/domain.rs:112-113`; `TodoReminder` at `src/domain/todo.rs:525`; `RepositoryFuture` at `src/application.rs:19` (the alias §4.3 says it follows); `Envelope`/`ErrorEnvelope` at `src/lib.rs:146,166`. New `DeliveryKey`/`Claim`/`BackendError` types are fully specified in §4.2/§4.3. |
| API/interface changes are feasible with current architecture | ✓ | `Command::Doctor` (`src/main.rs:50`) and `TodoCommand::ScanReminders` (`src/main.rs:285`) exist as §3.1/§7.2 claim; new `Command::Remind` fits the existing Clap derive style. §4.1's authority boundaries work with the current flat package. |
| Views/screens fit current navigation pattern | ✓ | CLI-only; `--no-color`/`NO_COLOR` already parsed at `src/main.rs:1160` (bound as `_color_disabled`, i.e. parsed but not yet applied — §3.7's "already wired" is a mild overstatement, not a spec defect). |
| Dependencies are available and version-compatible | ✓ (caveat) | **No D-Bus crate is in `Cargo.toml` today** — deps are chrono, chrono-tz, sha2, clap, serde, serde_json, thiserror, tokio (`macros`,`rt-multi-thread`), tokio-postgres, toml, uuid, fs2, libc, rustix (`fs`). §4.5 handles this honestly: the client is declared as a **new** crate, confined to `src/notify/freedesktop.rs` so "`domain`/`application`/`storage` never link it", and gated behind the Q1 evidence spike; `unsafe_code = "forbid"` is real (`[lints.rust]`) and Q1 explicitly asks whether the client interacts cleanly with it. `rustix` gaining `time` and `tokio` gaining `signal`+`net` are feature-flag additions on crates already present at compatible versions. |
| Platform/renderer requirements are realistic | ✓ | Arch/Hyprland/systemd-user/freedesktop daemon; §4.6 pins systemd ≥ 249 and probes rather than assumes capabilities; a machine with no session bus is a supported `--backend log` configuration. |
| Test strategy is executable with current infrastructure | ✓ (caveat) | Opt-in harness verified (`tests/postgres_integration.rs:9-21`); `assert_cmd`/`predicates`/`tempfile` in dev-dependencies; `tests/cli_contract.rs::reminder_scan_contract_is_explicitly_dry_run_capable` and `tests/todo_core.rs::reminders_validate_due_offsets_and_deduplicate` exist as §7.1 claims. New infrastructure required and not costed in detail: a stub D-Bus service on a private bus (§5.3), a socket-observing harness (§5.2(9)), and a systemd user manager in CI or a container (§5.3(8)). §7.3 flags this as the reason scope is "L, bordering XL". |
| Performance budget is realistic for target hardware | ✓ | §4.7 targets are modest and index-backed (`reminder_deliveries_due` partial index); the replaced behavior is real — `src/storage.rs` calls `expand_due_instances(NaiveDate::MIN, through)` on every scan, exactly as §4.7 states. |
| No undeclared dependency on unbuilt features | ✓ | §7.4 declares A1–A5, B8, B4, B6/B7, C8, D9, G3/G5/G6 and the Q1 spike, with an explicit fallback (singleton/non-recurring event reminders until B6/B7 lands; `--source todo-legacy` gated). |

**Migration version collision:** none. Files `0001`–`0006` exist; `src/storage.rs:44-75` registers six migrations ending at `Migration { version: 6, name: "repair_todo_recurrence" }`. The spec proposes `migrations/0007_reminder_delivery_ledger.sql` at version 7 and states the collision reasoning correctly in §4.1, §4.2, and §7.1.

**`tests/migration_contract.rs` delta:** the spec's §5.2(1)/§7.2 claim is **exactly correct**. The file today asserts `MIGRATIONS.len() == 6` (line 9), `MIGRATIONS[5].version == 6` and `MIGRATIONS[5].sql == REPAIR_TODO_RECURRENCE_MIGRATION` (in `recurrence_history_is_preserved_and_append_only_repair_converts_legacy_text_json`), and `migration_versions_are_strictly_increasing_and_unique` — all as described, and the last stays passing under an appended version 7. One item the spec does not mention but which is compatible: the same file's `reminder_migration_bridges_delivery_identity_without_external_transport` asserts `FOUNDATION_MIGRATION.contains("UNIQUE (reminder_id, scheduled_for)")`. Because §7.2 edits no existing migration file, that text assertion still passes; §5.2(1)'s "the old constraint is gone" is a live-database assertion, not a SQL-text one. Worth stating so an implementer does not try to satisfy both in the same place.

**Feasibility break found:** §4.2's backfill bullet copies a `reminder_deliveries.transport` value into `audit_log`. No such column exists in any migration — `grep -rn transport migrations/ src/` returns only `src/application.rs:776` (`pub transport: &'static str`) and `src/storage.rs:1989` (`transport: "none"`), both Rust struct fields on the `ReminderDelivery` output DTO. Migration 7 SQL referencing that column would abort.

**Feasibility verdict:** Feasible with caveats
**Caveats:** (1) the nonexistent `transport` column in the §4.2 backfill; (2) the D-Bus crate is not yet in the tree and its selection is Q1-gated; (3) three new test harnesses (stub bus, socket observer, systemd-in-CI) are required and only scoped, not designed.

---

## Composite Score

| Lens | Average | Weight | Weighted |
|---|---|---|---|
| Temporal and Data Integrity | 2.80 | 35% | 0.980 |
| CLI Usability and Automation | 2.80 | 25% | 0.700 |
| Standards Interoperability and Sync | 2.50 | 25% | 0.625 |
| Operational Security and Reliability | 2.75 | 15% | 0.4125 |
| **Composite** | | | **2.72** |

**Pass conditions (from criteria.md):**
- [x] Composite ≥ 2.0 — 2.72
- [x] All lens averages ≥ 2.0 — 2.80 / 2.80 / 2.50 / 2.75
- [x] No criterion scores 0
- [x] No more than two criteria at 1 per lens — zero 1s in any lens
- [x] All auto-fail rules pass — all eleven walked individually above
- [x] Feasibility ≠ Infeasible — Feasible with caveats

**All conditions met:** Yes → PASS

---

## Remediation Brief (non-blocking — spec passes)

### Must fix before implementation (correctness of the written artifact)

1. **§4.2, backfill bullet — remove the `transport` provenance clause.** There is no
   `reminder_deliveries.transport` column in `0001_foundation.sql` or any later migration; `transport` is a
   hard-coded Rust `&'static str` at `src/application.rs:776`, emitted by `src/storage.rs:1989`, and never
   persisted. The migration step as written would fail with `column "transport" does not exist`. Delete the
   clause; the `channel = 'freedesktop'` backfill decision it supports is independently sound and needs no
   provenance copy.
2. **§4.2, FK bullet — spell out `ALTER TABLE reminder_deliveries ALTER COLUMN reminder_id DROP NOT NULL`.**
   The column is `NOT NULL` today (`migrations/0001_foundation.sql:77`); "change the foreign key … to
   nullable `ON DELETE SET NULL`" requires dropping the constraint explicitly, and §5.2(2) asserts
   `reminder_id IS NULL` after a definition delete.
3. **§3.6 / §4.2 invariant 4 — resolve `remind scan --represent-failed`.** It is offered twice as a recovery
   action for a state invariant 4 declares terminal, and it is defined nowhere (no §3.1 view, no §4.3 use
   case, no state edge, no §5.0 row). Either define it fully with a predicate (`state='failed' ∧
   presented_at IS NULL`), a state edge, and a matrix row proving it cannot double-present, or remove it and
   make the recovery text "inspect and act on the item directly".

### Priority 2 — Should fix for quality

4. **§4.6 — tighten proof of death (b).** Change "any one of (a)–(d)" so that (b) (`heartbeat_at` older than
   `2 × lease`) counts as proof only in conjunction with (d) (`/proc/<pid>` absent or start-time mismatch).
   As written, a suspended-but-alive owner is declared dead by (b) while (d) would show it alive; combined
   with `recovery.represent_unconfirmed = once` that is the spec's one reachable duplicate path. Add a §5.1
   case asserting that a live process with stale heartbeats is **not** reconciled, and a §5.0 row for
   suspend-across-a-claim under `once`.
5. **§4.3 — state the classifier as two questions, not one bit.** "Was any byte of the `Notify` call flushed
   to the bus socket?" contradicts the table's own `NotSent(NoNameOwner)` and `NotSent(PolicyDenied)` rows,
   where the message *was* flushed and the bus daemon replied with an error. Restate as: refused locally →
   `NotSent`; refused by the bus daemon before routing (bus-originated `ServiceUnknown`/`NameHasNoOwner`/
   `AccessDenied`) → `NotSent`; at or past routing, or unanswerable → `UnknownOutcome`. Both readings are
   duplicate-safe today, so this is precision, not a defect in the guarantee — but an implementer applying
   the literal one-bit rule will classify two documented rows differently from the table.
6. **§5.2(10) — add the `remind doctor` envelope to the golden fixtures.** The doctor check names are stable
   and specific but its machine output is unpinned, which is the gap holding C5 at 2.
7. **§5.2(1) — note the `FOUNDATION_MIGRATION` text assertion.** `tests/migration_contract.rs` asserts the
   *string* `"UNIQUE (reminder_id, scheduled_for)"` is still present in `FOUNDATION_MIGRATION`. Since
   migration 7 replaces the constraint in the live database and edits no existing file, that assertion stays
   passing — say so, so an implementer does not delete it while satisfying "the old constraint is gone".

### Priority 3 — Consider for excellence

8. §3.2 step 1 says `pg_try_advisory_lock` follows "the same advisory-lock discipline `src/storage.rs::migrate`
   already uses"; `migrate` actually uses `pg_advisory_xact_lock` (`src/storage.rs:673`). The daemon needs a
   session-scoped lock, not a transaction-scoped one — worth naming the difference so §4.4's "a killed process
   releases it automatically when its connection closes" is unambiguous.
9. §3.7 says `--no-color`/`NO_COLOR` is "already wired in `src/main.rs`"; it is parsed into
   `let _color_disabled` (`src/main.rs:1160`) and not yet consumed. Fine as a statement of intent, but §5.3(5)
   is the test that will actually have to force the wiring.
10. §4.7's `remind list` p95 target rests on a 100,000-delivery synthetic fixture that is not listed among the
    §5.2 or §5.4 fixtures — add it, or cite where it is generated.
