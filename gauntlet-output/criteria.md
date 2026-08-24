# mg-calr — Binding Quality Criteria

**Generated:** 2026-08-23
**Evidence:** accepted product interview and `mg-calr-gauntlet-plan.md`
**Criteria version:** 1

## Scoring contract

Each criterion is scored 0–3 using its explicit anchors below. A spec passes only when no criterion is 0, every lens average is at least 2.0, no lens has more than two scores of 1, and no auto-fail applies. Weights sum to 100%.

## Auto-fail rules

Any design permitting: silent event/todo loss; unconfirmed overwrite; UID instability; recurrence/exception corruption; timezone/DST drift; duplicate reminder delivery; non-idempotent scans; plaintext credentials or secret logging; network access outside explicit synchronization (database access is only allowed for explicit database-related commands); automatic conflict overwrite; or loss of unsupported iCalendar properties on round trip.

## Lens 1 — Temporal and Data Integrity (35%)

Standards: RFC 5545 semantics, PostgreSQL transactional integrity, immutable UUID identity.

| Criterion | 0 — Missing | 1 — Inadequate | 2 — Acceptable | 3 — Excellent |
|---|---|---|---|---|
| T1 Identity | No stable identity | IDs are mutable/ambiguous | Immutable typed UUID authority; selectors separate | Plus explicit RFC UID/tombstone/conflict identity invariants and fixtures |
| T2 Temporal correctness | Time/all-day unspecified | Host-local timestamps or DST gaps | IANA zone for timed items; explicit all-day bounds | Plus DST/fold/gap and recurring-exception vectors with transactional semantics |
| T3 Transaction integrity | Partial writes possible | Application-only checks | Transactional writes and DB constraints for local invariants | Plus crash/concurrency recovery and property/contract tests |
| T4 Deletion/audit | Destructive or unaudited | Soft delete without reconciliation | Soft delete and audit/tombstone distinctions specified | Plus safe restore/purge/undo eligibility and immutable provenance |
| T5 Reminder idempotency | Duplicates possible | Best-effort dedupe | Durable unique claim/delivery state | Plus retry/crash/sleep/DND test matrix proving exactly-once presentation intent |

## Lens 2 — CLI Usability and Automation (25%)

Standards: CLI Guidelines, GNU/POSIX conventions where applicable, XDG Base Directory Specification. Benchmarks: khal, Taskwarrior, calcurse.

| Criterion | 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| C1 Human workflow | No usable flow | Opaque flags/errors | Guided defaults and actionable recovery | Keyboard-first flow matches benchmark speed without terse grammar |
| C2 Automation | Human-only output | Unstable/ad hoc JSON | Versioned deterministic JSON and noninteractive controls | Golden contracts, documented compatibility, selectors never mutate ambiguously |
| C3 Configuration | Hard-coded paths/secrets | Partial XDG/precedence | Distinct XDG roots and CLI > env > TOML > default | Pure resolution tests, redaction, migration/compatibility policy |
| C4 Output/accessibility | Color-dependent or unreadable | Partial no-color | `--no-color`/`NO_COLOR`, chronological semantics, clear text errors | Width/focus/Quickshell public-interface behavior and accessibility fixtures |
| C5 Diagnostics | Silent or mutating setup | Generic failure | Non-mutating doctor/init with actionable administrator steps | Stable machine output, prerequisite matrix, no sudo/secret leakage |

## Lens 3 — Standards Interoperability and Sync (25%)

Standards: RFC 5545, RFC 4791; RFC 6638 only for deferred scheduling. Benchmark: Apple Calendar plus khal/vdirsyncer transport separation.

| Criterion | 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| I1 Lossless iCalendar | Properties dropped | Supported subset only | Supported mapping plus unknown-property preservation | Golden/property tests preserve byte-relevant semantics across malformed/edge fixtures |
| I2 Sync authority | Multiple authorities | Implicit precedence | PostgreSQL authority and durable vdir mirror | Three-way fingerprints, interruption recovery, explicit orchestration |
| I3 Conflict/deletion | Overwrites either side | Warns after loss | Stops item, preserves both, separates tombstones | Deterministic resolution and delete/restore round-trip fixtures |
| I4 Scope/network | Hidden background access | Boundary unclear | Network only during explicit sync; scheduling deferred | Adapter/command tests prove no network in all non-sync paths |

Foundation specs may mark I1–I3 N/A only with explicit deferral and architecture that does not preclude them; I4 always applies.

## Lens 4 — Operational Security and Reliability (15%)

Standards: least privilege, peer auth, secret minimization, conventional Rust errors/testing.

| Criterion | 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| O1 Credentials | Plaintext/logged | Redaction incomplete | External secret fetch later; URLs redacted now | Secret-boundary tests and sanitized diagnostics/artifacts |
| O2 Least privilege | Root/sudo automation | Privilege expectations vague | Unprivileged migrations; admin commands printed only | Role boundaries and clean-machine recovery are executable/documented |
| O3 Failure contracts | Panic/data loss | String-only nondeterminism | Typed errors, stable codes/exits, atomic mutation | Fault/concurrency tests and recovery runbook |
| O4 Verification | No tests | Happy-path only | TDD unit/contract tests; opt-in isolated integration | CI clean-machine, migration, secret scan, package and synthetic E2E gates |

## Competitive baseline

| Product | Match | Improve |
|---|---|---|
| Apple Calendar | Mature event/calendar/recurrence semantics | CLI control, portability, open interchange |
| khal + vdirsyncer | Standards-first transport separation and speed | Recurrence/timezone mutation fidelity |
| Taskwarrior | Filters, JSON, recurrence, dependencies | Guided discoverability and lower grammar complexity |
| calcurse | Offline integrated agenda/todos/notifications | Non-monolithic public interfaces and stable JSON |

## Summary

| Lens | Criteria | Weight | Auto-fail relevance |
|---|---:|---:|---|
| Temporal and Data Integrity | 5 | 35% | data/UID/time/reminder rules |
| CLI Usability and Automation | 5 | 25% | deterministic, accessible, non-mutating contracts |
| Standards Interoperability and Sync | 4 | 25% | lossless/conflict/network rules |
| Operational Security and Reliability | 4 | 15% | credentials/privilege/failure rules |
