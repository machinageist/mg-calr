# Spec: Reminder System

**Feature ID:** e-reminder-system
**Parent feature:** root (E1–E8)
**Spec author agent:** Hermes Agent
**Date:** 2026-08-30 (iteration 2; iteration 1 dated 2026-08-29)
**Iteration:** 2

---

## 1. Purpose

### 1.1 One-sentence job

Turn stored event and todo reminder schedules into notifications that reach the user on their Arch/Hyprland workstation exactly once per scheduled occurrence, survive crashes, suspend, restarts, and quiet hours, and remain inspectable and actionable from the keyboard.

### 1.2 Why it matters

`mg-calr` is a local daily-driver calendar. B8 and C8 store *when* a reminder should fire but deliberately never deliver anything; `migrations/0001_foundation.sql` scaffolds `reminders`/`reminder_deliveries` and `src/storage.rs::scan_reminders` records candidates with `transport: "none"`. Without E, every schedule the user enters is silently inert, which is the worst possible failure for a reminder product. E is also the only feature in the tree that performs a side effect the database cannot roll back: once a notification is on screen it cannot be un-shown. A duplicate reminder trains the user to ignore the product; a missed reminder that nobody can find afterwards makes it untrustworthy. This feature therefore exists to make one durable, auditable claim per presentation and to make every failure visible instead of silent.

### 1.3 Success signal

Against a disposable PostgreSQL database and a recording null backend, the binding matrix in §5.0 runs: overlapping scans, killed scanners at every window, suspend/resume jumps, DND windows, snooze double-taps, and catch-up after long downtime produce **exactly one presentation per `(schedule_ref, occurrence_key, scheduled_for, channel)`**, zero duplicates, and a ledger in which every non-presented due delivery has an explicit terminal state and reason discoverable by `mg-calr remind list`.

### 1.4 Feature coverage

| Sub-feature | Binding outcome in this spec |
|---|---|
| E1 | Durable schedule→delivery ledger with a unique claim key, lease, state machine, and immutable provenance (§4.2) |
| E2 | Two-phase idempotent scanner: bounded materialization + atomic compare-and-set claim (§3.2, §4.3) |
| E3 | Action service that maps backend actions/signals back to ledger transitions with token idempotency (§3.2, §4.3) |
| E4 | Snooze as an explicit successor delivery row; dismiss as a terminal CAS (§3.2) |
| E5 | Catch-up policy after downtime: individual, folded digest, or expired — never a burst of duplicates (§3.2) |
| E6 | DND deferral that claims and defers rather than drops, with automatic re-eligibility (§3.2, §4.2) |
| E7 | `NotificationBackend` trait with freedesktop D-Bus, log, and null adapters (§4.3) |
| E8 | systemd user service (`Type=notify`) plus fallback timer, restart policy, and crash-recovery semantics (§4.6, §3.2) |

---

## 2. User Stories

> As a keyboard-first user, I want a reminder I scheduled to appear as a desktop notification at the right moment with snooze and dismiss actions, so that I can trust `mg-calr` instead of a second reminder app.

> As a user who suspends their laptop nightly, I want reminders that came due while the machine was asleep to be reconciled into one honest catch-up summary rather than fifteen stacked bubbles, so that resuming is not punishing.

> As a user in a meeting, I want do-not-disturb to defer reminders and then deliver them once when the window ends, so that quiet hours never mean lost reminders.

> As a user who has already seen a reminder, I want a restart of the scanner, a database reconnect, or a second scan to never show it to me again, so that repetition never becomes noise.

> As an accessibility user, I want notification text that is complete and self-describing without color, icon, or position, and a `mg-calr remind list` view that reads linearly in a screen reader, so that I never have to interpret a colored dot.

> As an operator of my own machine, I want a systemd user unit with a bounded restart policy, a non-mutating `remind doctor`, and printed (never executed) setup commands, so that the daemon is diagnosable without sudo.

> As an automation author, I want `remind scan`, `remind list`, and the action commands to emit the versioned JSON envelope and stable exit codes with `--no-input`, so that scripts and future Quickshell/TUI clients never scrape prose or touch PostgreSQL directly.

---

## 3. UX Specification

### 3.1 Screen / view inventory

This is a CLI plus background-daemon feature. It introduces no graphical screens of its own; it introduces terminal command views and notification surfaces rendered by the user's existing notification daemon.

**Command views** (all new; existing `mg-calr todo scan-reminders` is retained as a deprecated alias):

| View | Navigation | Status | Layout |
|---|---|---|---|
| Scan report | `mg-calr remind scan` | New | Line-oriented planned/materialized rows, or one JSON envelope |
| Delivery list | `mg-calr remind list` | New | Chronological table: state, when, source, title, reason |
| Delivery detail | `mg-calr remind show DELIVERY_ID` | New | Labeled record: identity, key, state history, attempts, backend handle |
| Snooze / dismiss result | `mg-calr remind snooze|dismiss` | New | One confirmation line or JSON envelope |
| DND control | `mg-calr remind dnd status|on|off` | New | Current window, source, and next re-evaluation instant |
| Catch-up report | `mg-calr remind catch-up` | New | Classified rows: presented / folded / expired |
| Reminder doctor | `mg-calr remind doctor` (and `mg-calr doctor --component reminders`) | New; extends existing `Command::Doctor` in `src/main.rs` | Non-mutating stable check matrix |
| Unit templates | `mg-calr remind install-units` | New | Prints unit text to stdout; `--write` required to touch `$XDG_CONFIG_HOME/systemd/user/` |
| Scanner foreground log | `mg-calr remind run --foreground` | New | Structured line log; `--log-format json` for machine consumption |

**Notification surfaces** (rendered by the freedesktop backend; layout in §3.3):

| Surface | Trigger | Urgency | Actions |
|---|---|---|---|
| Single reminder | one due delivery presented | `normal` (byte 1) by default | `snooze` (default 10m), `snooze-long`, `dismiss` |
| Catch-up digest | ≥2 missed deliveries folded by E5 | `normal` | `list`, `dismiss` |
| DND release summary | deferred deliveries released at window end and folded | `normal` | `list`, `dismiss` |
| Backend/daemon health | scanner cannot reach PostgreSQL or the bus after its retry budget | `critical` (byte 2), never auto-expiring | `dismiss` |

There is no full-screen TUI, panel, popover, or Quickshell surface in this feature; I1/I2 remain deferred consumers of the public JSON contracts.

### 3.2 Interaction flows

#### E2 — the two-phase scan (primary flow)

1. **Acquire singleton.** `remind run` takes `pg_try_advisory_lock` on a fixed reminder-scanner key (the same advisory-lock discipline `src/storage.rs::migrate` already uses). A second scanner exits 0 with `scanner_already_running` rather than racing; `remind scan` uses the shared lock only for its own materialize transaction.
2. **Materialize (idempotent, bounded).** For the window `[now - catch_up.expire_after, now + materialize_horizon]` (default 24 h forward), read reminder schedules from the configured sources (§4.4), compute every trigger instant, and `INSERT … ON CONFLICT (schedule_ref, occurrence_key, scheduled_for, channel) DO NOTHING` a `pending` ledger row per trigger. Materialization never presents, never claims, and never mutates a row that already exists in any state. Suppressed schedules (completed, trashed, blocked, cancelled event, stale projection) produce **no** row at all.
3. **Dispatch tick.** Query `state = 'pending' AND scheduled_for <= now AND (next_attempt_at IS NULL OR next_attempt_at <= now) AND channel = $active_channel` ordered by `(scheduled_for, schedule_ref, occurrence_key)`. The `channel` clause is what keeps a row bound to an adapter that is not running from being selected by the one that is (§4.4, *Channel binding and re-channelling*); such rows are reported by `remind doctor` as `deliveries.orphan_channel` and labeled in `remind list`, never silently dropped and never presented by the wrong backend. The same tick also runs the fenced lease sweep of §4.6. The tick fires on the earlier of the configured poll interval (default 20 s) and a `timerfd` armed at the next `scheduled_for`.
4. **Evaluate DND (E6).** If a DND window covers `now` and the delivery's urgency does not bypass it, transition `pending → deferred` with `deferred_reason` and `deferred_until = window end` in one statement. No presentation occurs and no row is dropped.
5. **Claim (the load-bearing step).** A single atomic statement claims the row and mints a fencing token:
   `UPDATE reminder_deliveries SET state='claimed', claim_owner=$owner, claim_fence=claim_fence+1, claimed_at=now(), claim_expires_at=now()+$lease, attempts=attempts+1 WHERE id=$1 AND state='pending' AND scheduled_for<=$now RETURNING id, claim_fence, …`. Zero rows returned means another scanner or an earlier tick already owns it; the dispatcher moves on without presenting. The claim is **committed before any backend call**, and the returned `claim_fence` is the token every later write for this attempt must carry (§4.2 invariant 7).
   *Isolation.* The claim statement runs at **READ COMMITTED**, the PostgreSQL default and the level the rest of `src/storage.rs` already uses. At that level the loser's `UPDATE` re-evaluates its predicate against the winner's committed row and returns zero rows — exactly the loser semantics this step depends on. E never requires REPEATABLE READ or SERIALIZABLE; under a stricter session default the loser raises a serialization failure (SQLSTATE `40001`) instead, and the repository must then retry the claim statement a bounded number of times and map exhaustion to `delivery_state_conflict` (exit 75). A serialization failure is **never** treated as licence to present.
6. **Present, then classify the outcome by retry safety.** Immediately before the call, the dispatcher re-reads `claim_owner`, `claim_fence`, and `claim_expires_at` for the row. **If the fence no longer equals the token step 5 minted, or the lease has already expired, the side effect is refused:** `present()` is not called at all, the dispatcher records `reminder_claim_fence_stale` (exit 75 for the CLI path) and moves on. Otherwise it calls `NotificationBackend::present`, and every write below carries `WHERE id=$1 AND state='claimed' AND claim_owner=$owner AND claim_fence=$fence`, so a superseded owner's write matches zero rows instead of landing.

   The outcome is one of exactly three cases — there is no "transient error" bucket:

   - **Success.** Record `state='presented'`, `presented_at`, and the backend handle returned by the call.
   - **`BackendError::NotSent`.** The notification server **provably rendered nothing**: not one byte of the `Notify` method call was flushed to the bus socket (§4.3 gives the exhaustive freedesktop/D-Bus mapping). *This is the only class that may return the row to `pending`*: record `state='pending'`, `next_attempt_at = now + backoff(attempts)`, clear the claim, and let a later tick re-claim it, up to `max_attempts` (default 3). On exhaustion record `state='failed'` with the typed reason.
   - **`BackendError::UnknownOutcome`.** The call was written and the result is unknowable from this side — the bubble may already be on screen. **This class is never retried.** It takes the same path as a crash between claim and present: `state='unconfirmed_lost'`, `terminal_reason='backend_unknown_outcome'`, governed by `recovery.represent_unconfirmed` exactly as the branch below, counted by `remind doctor`, and listed by `remind list --state unconfirmed_lost`. The honest failure direction is a **recorded miss**, never a second bubble.

   Because `NotSent` is by definition a call that rendered nothing, **at most one `present()` call per claim key can ever have reached the notification server**, no matter how many attempts the row records. A failed delivery is never silently retried forever and never presented twice.
7. **Await actions.** The daemon subscribes to backend events and applies E3/E4 transitions.

Branch — **crash between claim and presented** (E8): the row is `claimed` with an expired lease and no `presented_at`. It is *unconfirmed*. It is moved out of `claimed` **only by the fenced reconciliation in §4.6** — that is, only once its owner is *proven* not live; an expired lease alone is never sufficient, because a live-but-slow owner that is about to present must not have its row taken from underneath it. Once reconciled, the default `recovery.represent_unconfirmed = never` transitions it to `unconfirmed_lost`: it is **not** presented again, it is counted by `remind doctor`, and it appears in `remind list --state unconfirmed_lost`. Setting `recovery.represent_unconfirmed = once` re-presents it exactly once with `body` prefixed `Recovered:` and a `x-mg-calr-recovered` hint; the durable `attempts` counter makes a third presentation impossible. Silent re-presentation is prohibited in both modes. A `BackendError::UnknownOutcome` from step 6 enters this same branch directly, without waiting for lease expiry, because its owner is alive and already knows the outcome is unknowable.

#### E4 — snooze and dismiss

1. The user activates `snooze` on the bubble, or runs `mg-calr remind snooze DELIVERY_ID --for 15m` (or `--until`).
2. In one transaction: CAS the source row `WHERE id=$1 AND state IN ('presented','deferred','unconfirmed_lost')` to `state='snoozed'`, `snoozed_until=$target`; then `INSERT … ON CONFLICT DO NOTHING` a successor row with the same `schedule_ref`/`occurrence_key`, `scheduled_for = $target`, `state='pending'`, `supersedes = $1`. The unique key differs only in `scheduled_for`, so history is preserved and the successor cannot collide with an existing plan.
3. A second `ActionInvoked` for the same delivery (double click, replayed signal, CLI retry) finds the CAS predicate false and returns `already_snoozed` with the existing successor — no second successor, no second bubble.
4. `dismiss` is the same CAS to the terminal `dismissed` state and closes the bubble via `CloseNotification`. `NotificationClosed` reason 2 (dismissed by user) maps to `dismissed`; reason 1 (expired) leaves `presented` so the delivery still shows in `remind list`; reason 3 (closed by our own call) is ignored as self-caused.
5. Snooze targets are bounded: minimum 1 minute, maximum 7 days, and a snooze target beyond the item's own next occurrence warns in guided mode and is accepted only with an explicit `--until`.

#### E5 — catch-up after downtime

On start, on resume, and whenever the dispatcher observes a wall-clock jump, classify every `pending`/`deferred`-released row with `scheduled_for < now` by age in one transaction:

| Age of `scheduled_for` | Action | Presentation |
|---|---|---|
| ≤ `catch_up.present_within` (default 1 h) | claim and present individually, in `scheduled_for` order, rate-limited to `catch_up.max_burst` (default 5) | one bubble each |
| > `present_within` and ≤ `catch_up.fold_within` (default 24 h) | claim all, set `state='folded'`, `folded_into = <digest id>`, insert one `reminder_digests` row in the *same* transaction | exactly one digest bubble |
| > `fold_within` | set `state='expired'`, `expired_reason='beyond_catch_up_window'` | none; visible in `remind list` and doctor |

The digest row carries its own unique key `(kind, window_start, window_end)`, so a crash during digest presentation cannot produce a second digest; an unconfirmed digest follows the same `recovery.represent_unconfirmed` rule. Beyond `max_burst`, the remaining `present_within` rows fold into the digest rather than queue a burst.

#### E6 — do-not-disturb

DND is active when any of these hold, evaluated in this order and reported by source: (a) an explicit window in `reminder_dnd_windows` created by `remind dnd on [--for|--until]`; (b) a configured quiet-hours rule evaluated in the configured IANA zone with the same gap/fold policy as B4; (c) *optionally*, when `dnd.follow_backend_inhibit = true` and the backend advertises it, the `Inhibited` property of `org.freedesktop.Notifications`. Source (c) is best-effort: an unavailable property is `unknown`, never an implicit "do disturb" or an error. Deferred rows become eligible again automatically when `deferred_until <= now`; the release path folds ≥2 released rows into one DND release summary. `dnd.bypass_urgency` (default `none`) may be set to `critical` so that only explicitly critical reminders pierce DND.

#### E3 — action service

The action service is the daemon's event loop over `BackendEvent` values plus the CLI action commands; both call the same application use cases, so a headless user and a bubble-clicking user take identical code paths. Every backend action carries `action_token = delivery_id` in the action key (`snooze:<uuid>`), so a replayed or out-of-order signal is idempotent by construction and a signal for an unknown/foreign token is logged and dropped. No action shells out, opens a URL, launches a browser, or executes any external program.

#### Guided and non-interactive parity

Every action available on a notification is available as a flag-complete command. `--no-input` never reads stdin: `remind snooze` without `--for`/`--until` fails `input_required` (exit 64) instead of prompting. `--json` puts exactly one envelope on stdout; prompts and logs go to stderr.

### 3.3 Layout descriptions

**Notification anatomy** (freedesktop backend). `app_name` is `mg-calr`; `app_icon` is empty by default (`notification.icon` may set a themed name, never a fetched or embedded image); `summary` is `<Title> — <relative time>` truncated at 120 graphemes; `body` is a self-contained plain-text block:

```text
Standup
Tomorrow's agenda review
Starts 09:00 (in 10 minutes) · America/Los_Angeles
Calendar: Work · reminder 10m before
```

Body lines are ordered: item title, optional one-line detail, absolute time + relative time + IANA zone, then source context. Every state word is spelled out. Actions are supplied as `["snooze:<id>","Snooze 10m","snooze-long:<id>","Snooze 1h","dismiss:<id>","Dismiss"]`; `expire_timeout` is `-1` (server default) for normal urgency and `0` (persist) for critical. Hints set `urgency`, `category` (`x-mg-calr.reminder`), `desktop-entry`, and a stack tag so a re-presented recovery replaces rather than stacks. `replaces_id` reuses the stored backend handle when a delivery is updated. **Neither is a duplicate-suppression mechanism.** Stack-tag support is daemon-specific, is not advertised by `GetCapabilities`, and is absent on some servers, so it is a cosmetic nicety only; `replaces_id` requires a handle, and a `Notify` that fails returns none. All duplicate prevention therefore lives in the ledger (§4.2) and the retry-safety taxonomy (§4.3), never in the backend, and correctness is unchanged on a daemon that ignores both hints.

**`remind list`** columns in fixed order: `SHORT-ID`, `STATE`, `SCHEDULED` (local ISO + zone), `SOURCE` (`event`/`todo`), `TITLE`, `REASON`. State and reason are always words (`DEFERRED (dnd_quiet_hours)`, `EXPIRED (beyond_catch_up_window)`), never a color or glyph alone. A row whose `channel` has no backend bound in the running configuration is labeled `channel=<name> (no backend bound)` so an undispatchable row is never silently invisible. Data comes from one ledger query joined to the schedule projection; the renderer performs no second business query.

**Selectors.** `DELIVERY_ID` in `remind show`, `remind snooze`, and `remind dismiss` accepts either a full `DeliveryId` UUID or the `SHORT-ID` shown in the list — the first 8 hex characters of the UUID, and any longer prefix. Resolution is exact-or-typed-error: a prefix matching exactly one delivery resolves; a prefix matching two or more returns `delivery_selector_ambiguous` (exit 65) and prints every candidate's **full** UUID plus its state and `scheduled_for`; a prefix matching none returns `delivery_not_found` (exit 66). **No command ever picks a match, and no command mutates on an ambiguous selector** — the ambiguity is detected before any transaction opens. A short ID is a display convenience only and never carries authority: JSON always emits the full UUID in `id`, `short_id` is additive, and every scripted caller is expected to use `id` (§6.4 T1).

**Empty states.** `remind list` with no rows prints `No reminder deliveries match.` and JSON returns `"deliveries": []`. `remind scan` that materializes nothing prints `No reminders due in the scan window.` `remind dnd status` with no window prints `Do not disturb: off. Next quiet-hours window: none configured.`

### 3.4 Input & gestures

All input is keyboard, argv, and stdin. Notification actions are activated with the pointer or the notification daemon's own keybindings (mako `mako-ctl`, dunst `context`, swaync's panel); `mg-calr` neither requires nor assumes a pointer, because every action has an equivalent command. There is no stylus, controller, voice, camera, or touch input. Repeated flags preserve sets, `--` ends option parsing, and `Ctrl-C` during a guided prompt exits 130 before any transaction opens. `SIGTERM` to the daemon triggers graceful shutdown: stop accepting ticks, finish or release in-flight claims, release the advisory lock, exit 0. `SIGHUP` re-reads configuration without dropping claims. Responsive behavior is terminal-width based: rows wrap at word boundaries below 80 columns and degrade to labeled records below 50; identity, state, reason, and time never truncate. JSON is width independent.

### 3.5 Transitions & animation

`mg-calr` renders no animation. Bubble entry/exit animation, stacking, and timeout behavior belong entirely to the user's notification daemon and are not overridden; the only motion-relevant choices we make are `expire_timeout` and whether a re-presentation replaces (`replaces_id`) or stacks — it always replaces. Reduced-motion preferences are honored by construction because we emit no animation and never poll a redrawing terminal UI; `remind run --foreground` appends log lines and never repaints. Sound is off by default; `notification.sound_name` may set a freedesktop `sound-name` hint, and the daemon owns whether to play it.

### 3.6 Error states

| Trigger | Presentation | Recovery | Data-loss risk |
|---|---|---|---|
| Session bus unavailable / `DBUS_SESSION_BUS_ADDRESS` unset | daemon: `reminder_backend_unavailable`, exit 69 after retry budget; CLI: same typed error | start the session, or run with `--backend log` | none; nothing is claimed while the backend is known-down |
| `DBUS_SESSION_BUS_ADDRESS` names a `tcp:` transport | `reminder_backend_transport_forbidden`, exit 69, refuse to connect | use the Unix socket bus | none |
| No notification server owns `org.freedesktop.Notifications` | `reminder_backend_unavailable` with the exact `busctl --user list` diagnostic | install/start a daemon (mako, dunst, swaync) | none |
| PostgreSQL unreachable mid-run | daemon logs `database_unavailable`, holds no claim, retries with capped backoff; systemd restarts after the budget | start PostgreSQL; claims resume from the ledger | none; claims are committed, not buffered |
| Another scanner holds the advisory lock | `scanner_already_running`, exit 0 for `remind run`, exit 75 for `remind scan --exclusive` | inspect `systemctl --user status mg-calr-remind` | none |
| Claim CAS returns zero rows | dispatcher skips silently; `remind snooze` returns `delivery_state_conflict`, exit 75 | re-read with `remind show` | none; no overwrite |
| Delivery ID unknown | `delivery_not_found`, exit 66 | `remind list` | none |
| Snooze target out of bounds | `snooze_target_invalid` with the accepted range, exit 65 | pass a bounded `--for`/`--until` | none |
| Todo projection missing/stale/conflicting | existing `projection_missing`/`projection_stale`/`projection_conflict` codes (`src/lib.rs`), exit 74/65 | run `mg-calr interop import-todo` | none; **fails closed — no todo reminder is presented from a stale projection** |
| Backend presentation error, attempts exhausted (reachable from `NotSent` only) | ledger `failed` with reason; one critical health bubble at most per hour | `remind list --state failed`, then `remind scan --represent-failed` | none; schedules are untouched |
| Unconfirmed claim after crash | `unconfirmed_lost` in list and doctor; no re-presentation by default | inspect and act on the item directly, or opt into `recovery.represent_unconfirmed = once` | notification not shown; the ledger row and reason are durable |
| Wall clock moved backwards | dispatcher re-derives the plan from the ledger; a `scheduled_for` now in the future returns to waiting | none required | none; `scheduled_for` is absolute UTC |
| Backend reply lost or timed out **after** the `Notify` call was written (`BackendError::UnknownOutcome`) | ledger `unconfirmed_lost` with `terminal_reason='backend_unknown_outcome'`; counted by doctor; **no retry is attempted** | `remind list --state unconfirmed_lost`, act on the item directly, or opt into `recovery.represent_unconfirmed = once` | the notification may or may not have been shown; the row and reason are durable. **Never a duplicate** |
| Backend provably sent nothing (`BackendError::NotSent`) | retried under `backoff(attempts)` up to `max_attempts`, then `failed` with the typed reason | `remind list --state failed`, then `remind scan --represent-failed` | none; nothing was rendered by any failed attempt |
| Claim fence superseded (the row was reconciled under its owner) | `reminder_claim_fence_stale`, exit 75; the side effect is refused **before** `present()` is called and the owner's write matches zero rows | `remind show DELIVERY_ID` for the terminal state and reason | none; no overwrite and no second bubble |
| Lease expired but the owner cannot be proven dead | no transition at all; the row stays `claimed` and `remind doctor` reports `claims.expired_unproven` | `remind doctor`; stop the stale scanner, or wait for its heartbeat to lapse | none; the reconciler refuses to act rather than risk a duplicate |
| Short-ID selector matches more than one delivery | `delivery_selector_ambiguous`, exit 65, listing every candidate's full UUID | re-run with a longer prefix or the full `DELIVERY_ID` | none; nothing is mutated and no match is picked silently |
| Configured backend changed, leaving non-terminal rows at the previous channel | daemon start reports `deliveries.orphan_channel = N`; rows are re-channelled by the fenced `UPDATE` in §4.4 or reported if the target key is occupied | `remind doctor`, then `remind scan --rechannel` | none; terminal rows keep their original channel forever and are never re-materialized |

Errors reuse the A4 envelope: `{schema_version:1, ok:false, error:{code, message}}` on stderr with the exit codes already encoded by `AppError::exit_code` in `src/lib.rs`. No error message contains a database URL, a bus address with credentials, or reminder body text beyond a truncated title.

### 3.7 Accessibility

- **Screen readers.** Notification body text is complete and linear: title, detail, absolute time, relative time, zone, source. Nothing depends on the bubble's position, color, icon, or urgency styling; urgency is additionally stated in the body for `critical`. Action labels are full words (`Snooze 10 minutes`, `Dismiss`), never icons, and are exposed to AT via the notification daemon's own accessibility surface.
- **Custom actions.** Every notification action has a named CLI equivalent (`remind snooze`, `remind dismiss`, `remind list`), so a user whose notification daemon exposes no accessible actions retains full control from the terminal. That equivalence is contract-tested, not documented aspiration.
- **Text scaling / dynamic type.** Bubble typography and scaling belong to the notification daemon and the compositor; `mg-calr` emits plain text with no embedded markup beyond what `GetCapabilities` advertises (`body-markup` is used only when advertised, and the plain-text form is always the fallback rendered by `--backend log`).
- **Color independence.** Terminal output honors `--no-color` and `NO_COLOR` (already wired in `src/main.rs`); every state, reason, and urgency is a word in both human and JSON output. `remind list` is readable and unambiguous with all ANSI stripped.
- **Focus order and keyboard navigability.** Guided prompts follow the documented order (delivery selector → duration → confirmation); `--no-input` removes prompts entirely. The daemon never steals focus, never opens a window, and never blocks the compositor.

---

## 4. Implementation Specification

### 4.1 Architecture placement

Building on the implemented flat package (`src/domain.rs`, `src/application.rs`, `src/storage.rs`, `src/interop.rs`, `src/main.rs`):

- `src/domain/reminder.rs` — `DeliveryKey`, `DeliveryState`, `SnoozeTarget`, `DndWindow`, `CatchUpClass`, and the **pure** planner. No SQL, D-Bus, clock, or CLI knowledge; the clock and TZDB are injected.
- `src/application/reminder.rs` — `ReminderUseCases<R, B, C>` over a delivery repository `R`, a `NotificationBackend` `B`, and a `Clock` `C`; owns transaction boundaries, claim/CAS ordering, catch-up classification, and DND evaluation.
- `src/notify/mod.rs` — the `NotificationBackend` trait, `PresentationRequest`, `BackendEvent`, `BackendCapabilities`, `BackendError`.
- `src/notify/freedesktop.rs` — session-bus adapter (`org.freedesktop.Notifications`).
- `src/notify/log.rs`, `src/notify/null.rs` — headless JSON-lines backend and recording test backend.
- `src/storage.rs` (or `src/storage/reminder.rs` if the module is split) — `PostgresDeliveryRepository`, advisory lock helpers, and the claim/CAS statements. `due_reminders` and `scan_reminders` are refactored into the new source/ledger boundary rather than duplicated.
- `src/daemon.rs` — the `remind run` loop: timerfd/poll scheduling, suspend detection, `sd_notify` handshake, signal handling.
- `src/main.rs` — a new `Command::Remind(RemindArgs)` with the subcommands in §3.1, matching the existing Clap style; `TodoCommand::ScanReminders` becomes a deprecated alias.
- `migrations/0007_reminder_delivery_ledger.sql` — appended after the existing `0006_repair_todo_recurrence.sql` (the current highest migration; `0005_event_lifecycle.sql` is no longer the tail), registered as `Migration { version: 7, name: "reminder_delivery_ledger" }` in `storage::MIGRATIONS` with a matching `REMINDER_DELIVERY_LEDGER_MIGRATION` `include_str!` constant. **Version 6 is already taken by `repair_todo_recurrence`; 7 is the next free version** (§7.1, §7.2).
- `tests/reminder_ledger.rs`, `tests/reminder_daemon.rs`, `tests/reminder_cli.rs`, and fixtures under `tests/fixtures/reminders/`.

Authority boundaries are binding even if the package stays flat: `domain` is pure, `application` owns claims and transactions, `notify` performs the only side effect, `storage` owns SQL, `main` renders.

### 4.2 Data model

```rust
/// The durable claim key. Every field is required; there is no default
/// constructor, so a delivery cannot be written without full identity.
pub struct DeliveryKey {
    /// Interop-style global reference: `mg-calr:reminder:<uuid>` or
    /// `mg-todo:reminder:<local-id>` (same grammar as `interop::Snapshot` global IDs).
    pub schedule_ref: ScheduleRef,
    /// `singleton` for a non-recurring item, otherwise the B7/C4 occurrence identity.
    pub occurrence_key: OccurrenceKey,
    /// Absolute trigger instant in UTC; derived from civil intent, never from host-local time.
    pub scheduled_for: DateTime<Utc>,
    /// Presentation channel; one ledger row per channel.
    pub channel: DeliveryChannel,
}

pub enum DeliveryState {
    Pending, Claimed, Presented, Deferred, Snoozed, Dismissed,
    Folded, Expired, Failed, Revoked, UnconfirmedLost,
}

pub struct Delivery {
    pub id: DeliveryId,          // existing domain::DeliveryId (UUIDv7)
    pub key: DeliveryKey,
    pub state: DeliveryState,
    pub attempts: u16,
    pub claim: Option<Claim>,    // owner + fence + claimed_at + claim_expires_at
    pub presented_at: Option<DateTime<Utc>>,
    pub deferred: Option<Deferral>,   // reason + deferred_until
    pub snoozed_until: Option<DateTime<Utc>>,
    pub supersedes: Option<DeliveryId>,
    pub folded_into: Option<DigestId>,
    pub backend_handle: Option<BackendHandle>, // opaque u32 for freedesktop
    pub reason: Option<TerminalReason>,
}

/// The fencing token. `fence` is a per-row monotonic counter, never reset and
/// never reused, so it totally orders every claim the row has ever had.
pub struct Claim {
    pub owner: OwnerId,          // boot id + PID + process start time
    pub fence: u64,              // `claim_fence` after this claim's increment
    pub claimed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
```

`migrations/0007_reminder_delivery_ledger.sql` — **version 7**, because `migrations/0006_repair_todo_recurrence.sql` already occupies version 6 (append-only; idempotent in the style of `0004`/`0005`/`0006`):

- `ALTER TABLE reminder_deliveries ADD COLUMN IF NOT EXISTS schedule_ref text`, `occurrence_key text`, `channel text`, `state text`, `attempts integer NOT NULL DEFAULT 0`, `claim_owner text`, `claim_fence bigint NOT NULL DEFAULT 0`, `claim_expires_at timestamptz`, `next_attempt_at timestamptz`, `deferred_until timestamptz`, `supersedes uuid`, `folded_into uuid`, `backend_handle bigint`, `terminal_reason text`, `updated_at timestamptz`. `claim_fence` is the monotonic fencing token of invariant 7; it is `NOT NULL`, starts at 0, is only ever incremented, and carries `CHECK (claim_fence >= 0)`.
- Backfill from the columns migration 1 already provides: `schedule_ref = 'mg-calr:reminder:' || reminder_id::text`, `occurrence_key = 'singleton'`, and `state` derived from the existing `dismissed_at`/`delivered_at`/`snoozed_until`/`claimed_at` (`dismissed_at IS NOT NULL → 'dismissed'`; else `snoozed_until IS NOT NULL → 'snoozed'`; else `delivered_at IS NOT NULL → 'presented'`; else `claimed_at IS NOT NULL → 'unconfirmed_lost'` with `terminal_reason='backfilled_unconfirmed'`; else `'pending'`), then `SET NOT NULL` and add `CHECK (state IN (…))`.
- **Backfilled `channel` (explicit decision).** Legacy rows are backfilled to `channel = 'freedesktop'`, the shipped default presentation channel — **not** to a sentinel `'none'`. The reason is a duplicate hazard, not tidiness: `channel` is part of the claim key, so backfilling a *presented* legacy row to a non-presentable sentinel would leave the `freedesktop` key free and let materialization insert a fresh `pending` row for an occurrence the user has already seen. Anchoring the legacy row on the real channel keeps the key occupied and makes re-materialization impossible. The legacy migration-1 `transport` text (literally `'none'` in every row `scan_reminders` ever wrote) is copied into the `audit_log` before/after JSON for provenance and is never used as a channel value. `CHECK (channel IN ('freedesktop','log','null'))` — there is no `'none'` channel in the schema at all.
- **Backfilled `pending` rows in the past are expired, not delivered.** In the same migration, every backfilled row still `pending` with `scheduled_for < <migration timestamp>` becomes `state='expired'`, `terminal_reason='backfilled_before_delivery_existed'`. No delivery mechanism existed when those rows were recorded (`transport: "none"`), so presenting them on upgrade would be a burst of historical bubbles; expiring them is honest, keeps the key occupied against re-materialization, and leaves every row visible in `remind list --state expired`. Backfilled rows whose `scheduled_for` is still in the future stay `pending` and deliver normally on the first tick after upgrade.
- Replace the migration-1 constraint `UNIQUE (reminder_id, scheduled_for)` with `CREATE UNIQUE INDEX reminder_deliveries_claim_key ON reminder_deliveries (schedule_ref, occurrence_key, scheduled_for, channel)`. This is the auto-fail-critical constraint and matches the `(reminder_definition_id, occurrence_or_instance_identity, scheduled_instant, delivery_channel)` contract published by `gauntlet-output/specs/d-views-query-output.md`.
- Change the migration-1 foreign key `reminder_deliveries.reminder_id … ON DELETE CASCADE` to nullable `ON DELETE SET NULL`. **Rationale (a real defect in the current schema):** cascade deletes destroy delivery provenance when a reminder definition is removed, which both loses audit history and lets a re-created identical definition present an already-delivered occurrence again. The ledger must outlive the definition.
- `CREATE INDEX reminder_deliveries_due ON reminder_deliveries (state, scheduled_for) WHERE state IN ('pending','deferred','claimed')` — the dispatcher's only hot query.
- `CREATE TABLE reminder_digests (id uuid PRIMARY KEY, kind text NOT NULL, window_start timestamptz NOT NULL, window_end timestamptz NOT NULL, delivery_count integer NOT NULL, state text NOT NULL, presented_at timestamptz, created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP, UNIQUE (kind, window_start, window_end))`.
- `CREATE TABLE reminder_dnd_windows (id uuid PRIMARY KEY, starts_at timestamptz NOT NULL, ends_at timestamptz, source text NOT NULL CHECK (source IN ('manual','quiet_hours')), reason text, created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP, CHECK (ends_at IS NULL OR ends_at > starts_at))`.
- `CREATE TABLE reminder_scanner_runs (id uuid PRIMARY KEY, owner text NOT NULL, started_at timestamptz NOT NULL, heartbeat_at timestamptz NOT NULL, stopped_at timestamptz, stop_reason text)` — the durable evidence that supports crash detection and `remind doctor`.
- Every state transition writes an `audit_log` row (table already exists in migration 1) with `entity_type='reminder_delivery'`, before/after JSON, and the transaction UUID, so the ledger is immutable provenance rather than a mutable scratchpad.

Binding invariants:

1. A presentation is attempted **only** for a row whose `claimed` state is already committed. There is no in-memory-only claim.
2. `scheduled_for` is UTC and derived from the owning feature's civil intent (B4 zone/fold policy, C8 `todo.date_reminder_time`). The daemon never uses host-local time to decide when something is due, so a TZ change or DST transition cannot shift a stored trigger.
3. The claim key is the unique index above. Two scanners, two ticks, a replayed action, and a re-run `remind scan` cannot create a second row for the same key.
4. States are a directed machine: `pending → {claimed, deferred, expired, revoked}`; `claimed → {presented, pending(retry — **`BackendError::NotSent` only**, invariant 8), failed, unconfirmed_lost}`; `presented → {snoozed, dismissed}`; `deferred → pending`; `revoked → pending` **only** under the restore-revival predicate of invariant 9; `snoozed`, `dismissed`, `expired`, `folded`, `failed`, `unconfirmed_lost` are terminal for that row, and `revoked` is terminal for presentation with invariant 9 as its single, stated exception. Terminal rows are never re-presented; a snooze produces a *new* row instead.
5. `attempts` is monotonic per row and capped; it is the durable proof that bounds re-presentation.
6. A schedule that disappears (event trashed/purged, todo completed/trashed/blocked, projection revision drops it) transitions its non-terminal deliveries to `revoked`, closes any live bubble, and **retains** the row. Deliveries are never deleted by the scanner; purge is a G-owned operation.
7. **Fencing.** Every claim mints a token: the claim CAS sets `claim_owner = $owner` and `claim_fence = claim_fence + 1` in one statement and returns the new value (§3.2 step 5). Every write that depends on that claim — `claimed → presented`, `claimed → failed`, `claimed → pending`, the owner's own `claimed → unconfirmed_lost`, the presented-handle write, and the release performed on graceful shutdown — carries `AND claim_owner = $owner AND claim_fence = $fence`. A superseded owner's write therefore matches zero rows and raises `reminder_claim_fence_stale` (exit 75) instead of landing. **The side effect itself is refused when the token is stale:** §3.2 step 6 re-checks fence and lease immediately before calling the backend and does not call it at all if either has moved on. The reconciler bumps the fence in the same statement that writes its terminal state (§4.6), so reconciliation and the original owner can never both win.
8. **Retry class.** Returning a `claimed` row to `pending` is legal for exactly one cause: `BackendError::NotSent` (§4.3), which by definition means the notification server rendered nothing. Every other failure — including every `UnknownOutcome` — is terminal (`unconfirmed_lost` or `failed`) and is counted as a *missed* notification. **There is no code path from an unknown outcome to a second `present()` call for the same key**, and `attempts` counts `NotSent` retries only.
9. **Restore revival (the one mutation materialization may perform).** A schedule that is trashed and then restored before its trigger must still fire exactly once, so `revoked` is the single state materialization may revive, under a total predicate: the schedule reappears with the identical `DeliveryKey`, `scheduled_for > now()`, the existing row is `revoked`, its `presented_at IS NULL`, and its `attempts = 0`. The revival is materialization's own conflict path, one statement — `INSERT … ON CONFLICT (schedule_ref, occurrence_key, scheduled_for, channel) DO UPDATE SET state='pending', terminal_reason=NULL, claim_owner=NULL, claim_fence=reminder_deliveries.claim_fence+1, updated_at=now() WHERE reminder_deliveries.state='revoked' AND reminder_deliveries.presented_at IS NULL AND reminder_deliveries.attempts=0 AND EXCLUDED.scheduled_for > now()` — so it is idempotent across repeated scans, cannot revive a row that ever presented or was ever claimed, cannot revive any other state, and bumps the fence so no in-flight owner can write over the revived row. A row restored *after* its `scheduled_for` has passed is not revived; it stays `revoked`, and E5 governs anything newly materialized. Both transitions write `audit_log` rows, so the trash → restore round trip is visible as provenance rather than inferred. For every state other than `revoked`, §3.2 step 2's "never mutates a row that already exists in any state" rule stands unchanged.
10. **Digest claim key.** `reminder_digests` carries `UNIQUE (kind, window_start, window_end)` and is inserted with `ON CONFLICT DO NOTHING … RETURNING`; a catch-up run that loses the race adopts the winner's `id` for its `folded_into` links rather than inserting a second digest. Two concurrent catch-up runs therefore fold into exactly one digest and present exactly one digest bubble.

### 4.3 API contracts

**Application use cases** (local Rust, no network endpoints; futures follow the existing `RepositoryFuture<'a, T, E>` alias in `src/application.rs`):

```rust
pub async fn materialize(&self, window: ScanWindow)          -> Result<MaterializeReport, ReminderError>;
pub async fn dispatch_due(&self, now: DateTime<Utc>)         -> Result<DispatchReport, ReminderError>;
pub async fn catch_up(&self, now: DateTime<Utc>)             -> Result<CatchUpReport, ReminderError>;
pub async fn snooze(&self, id: DeliveryId, target: SnoozeTarget) -> Result<DeliveryView, ReminderError>;
pub async fn dismiss(&self, id: DeliveryId)                  -> Result<DeliveryView, ReminderError>;
pub async fn set_dnd(&self, window: DndRequest)              -> Result<DndStatus, ReminderError>;
pub async fn list_deliveries(&self, filter: DeliveryFilter)  -> Result<Vec<DeliveryView>, ReminderError>;
pub async fn revoke_orphans(&self, now: DateTime<Utc>)       -> Result<RevokeReport, ReminderError>;
```

**Schedule sources** — the read side that keeps E honest about the mg-todo extraction described in `README.md`:

```rust
pub trait ReminderScheduleSource {
    type Error;
    /// Every trigger in the window, with stable `schedule_ref`/`occurrence_key`,
    /// already filtered by the owning feature's suppression rules.
    fn schedules(&self, window: ScanWindow) -> RepositoryFuture<'_, Vec<ScheduledReminder>, Self::Error>;
}
```

Implementations: `PostgresEventSchedules` (B8 `reminders` rows joined to live events — authoritative), `ProjectionTodoSchedules` (read-only over the validated `interop::TodoProjectionSnapshot`, reusing its freshness/conflict errors), and `LegacyPostgresTodoSchedules` (today's `storage::scan_reminders` path, gated behind `--source todo-legacy` and removed when the mg-todo migration completes).

**E7 backend abstraction:**

```rust
pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, BackendError>> + Send + 'a>>;

pub trait NotificationBackend: Send + Sync {
    fn name(&self) -> &'static str;                       // "freedesktop" | "log" | "null"
    fn capabilities(&self) -> BackendFuture<'_, BackendCapabilities>;
    fn present(&self, request: PresentationRequest) -> BackendFuture<'_, BackendHandle>;
    fn close(&self, handle: BackendHandle) -> BackendFuture<'_, ()>;
    /// Cold stream of action/close events; `None` for backends without a return channel.
    fn events(&self) -> Option<BackendEventStream>;
}

pub enum BackendEvent {
    ActionInvoked { handle: BackendHandle, action: ActionToken },
    Closed { handle: BackendHandle, reason: CloseReason }, // Expired | DismissedByUser | ClosedByCall | Undefined
    BackendLost,
}
```

**`BackendError` — the retry-safety taxonomy (auto-fail critical).** Because a presentation is the one side effect the database cannot roll back, `BackendError` is not a flat list of causes. It is a two-class enum whose *outer* class is the retry decision and whose inner cause is diagnostics only:

```rust
/// Every backend failure is exactly one of these two classes. There is no
/// third "transient" class, no `Other`, and no default arm anywhere that
/// matches on it, so a variant added later cannot silently become retryable.
pub enum BackendError {
    /// The notification server **provably rendered nothing**: not one byte of
    /// the method call was flushed to the bus socket. Safe to return the row
    /// to `pending` and re-claim (§4.2 invariant 8).
    NotSent(NotSentCause),
    /// The request was written and the result is unknowable from this side.
    /// The bubble may already be on screen. MUST NOT be retried; follows the
    /// `unconfirmed_lost` path (§3.2 step 6).
    UnknownOutcome(UnknownOutcomeCause),
}
```

The discriminating question is one bit: **was any byte of the `Notify` method call flushed to the bus socket?** The adapter answers it from the D-Bus client's own connection state — a call that has been assigned a serial and handed to the transport is `UnknownOutcome` from that instant forward; everything strictly before it is `NotSent`. When the client library cannot answer the question, the adapter classifies `UnknownOutcome`. **The classifier always fails toward a recorded miss, never toward a duplicate.**

| Real freedesktop / D-Bus failure mode | Class | Why |
|---|---|---|
| `DBUS_SESSION_BUS_ADDRESS` unset and `$XDG_RUNTIME_DIR/bus` absent | `NotSent(NoBus)` | no connection exists |
| `connect(2)` on the bus socket returns `ENOENT`, `ECONNREFUSED`, or `EACCES` | `NotSent(ConnectFailed)` | no connection exists |
| SASL `EXTERNAL` authentication or the `Hello` handshake fails | `NotSent(HandshakeFailed)` | the connection is not usable; no call was written |
| Address transport is not `unix:` (`tcp:`, `autolaunch:`) | `NotSent(TransportForbidden)` | refused locally, before connecting (§6.4 I4) |
| `org.freedesktop.DBus.Error.ServiceUnknown` / `NameHasNoOwner` | `NotSent(NoNameOwner)` | the **bus** rejects routing; the message body never reaches a server |
| `org.freedesktop.DBus.Error.AccessDenied` from bus policy | `NotSent(PolicyDenied)` | rejected by the bus daemon before routing |
| Marshalling failure, or the message exceeds the bus maximum message size | `NotSent(Marshal)` | rejected locally before any byte is written |
| `max_in_flight` back-pressure refuses the call before it is queued | `NotSent(Throttled)` | never handed to the transport |
| `write(2)` on the bus socket returns `EPIPE`/`ECONNRESET` with **zero** bytes of this call written | `NotSent(WriteFailedBeforeFlush)` | the serial was never flushed |
| Reply timeout expires (`reminders.backend_reply_timeout`, default 25 s) | `UnknownOutcome(ReplyTimeout)` | the server had the call; the bubble may be on screen |
| `org.freedesktop.DBus.Error.NoReply` or `…Error.Timeout` / `TimedOut` | `UnknownOutcome(NoReply)` | the call was routed; only the reply is missing |
| `EPIPE`/`ECONNRESET`/`Disconnected` on a call already flushed | `UnknownOutcome(ConnectionLostInFlight)` | partial or complete write already left this process |
| `NameOwnerChanged` shows the server exited with our call in flight | `UnknownOutcome(ServerVanishedInFlight)` | it may have rendered before exiting |
| `org.freedesktop.DBus.Error.LimitsExceeded` / `NoMemory` raised after routing | `UnknownOutcome(ServerResourceError)` | raised downstream of delivery |
| A reply arrives but is unreadable: wrong signature, non-`u` return, truncated message | `UnknownOutcome(UnreadableReply)` | the bubble exists; we merely cannot learn its id |
| Our own cancellation of an in-flight call (`SIGTERM` during `Notify`, shutdown, in-flight timeout) | `UnknownOutcome(Cancelled)` | cancelling the future does not un-render a bubble |

The `log` backend classifies a failed append as `NotSent(WriteFailedBeforeFlush)` only when the write is refused before any byte reaches the file and the file is `fsync`ed before `present()` returns; a short or interrupted write is `UnknownOutcome(UnreadableReply)`. The `null` backend takes an injected class so §5.0 can drive both branches deterministically.

`PresentationRequest` is a value type (`summary`, `body`, `actions`, `urgency`, `category`, `expire_timeout`, `replaces`, `stack_tag`) with no D-Bus types in its signature, so `null`/`log` backends are total implementations and `domain`/`application` never depend on `zbus`.

**Concrete freedesktop adapter.** Bus name `org.freedesktop.Notifications`, path `/org/freedesktop/Notifications`, interface `org.freedesktop.Notifications`. Calls `Notify(app_name:s, replaces_id:u, app_icon:s, summary:s, body:s, actions:as, hints:a{sv}, expire_timeout:i) -> u`, `CloseNotification(u)`, `GetCapabilities() -> as`, `GetServerInformation() -> (ssss)`. Subscribes to `ActionInvoked(u,s)` and `NotificationClosed(u,u)`. Urgency is the `urgency` byte hint (0 low / 1 normal / 2 critical). Capabilities are probed once at start and cached: absent `actions` downgrades to a body that names the CLI equivalents; absent `body-markup` forces plain text. The adapter connects **only** to the session bus reached through `DBUS_SESSION_BUS_ADDRESS` or `$XDG_RUNTIME_DIR/bus`, and it refuses any address whose transport is not `unix:` (§6.4 I4).

**`null` and `log` backends.** `NullBackend` records every `PresentationRequest` into an in-memory vector and returns synthetic handles; it is the backend for every unit and integration test and lets the §5.0 matrix assert exact presentation counts. `LogBackend` appends one JSON object per presentation to `$XDG_STATE_HOME/mg-calr/reminders.log` and is the supported headless/TTY mode.

**JSON contracts.** Success uses the existing envelope (`Envelope::success` in `src/lib.rs`): `{"schema_version":1,"command":"remind.scan","ok":true,"data":{…}}`. A delivery object is:

```json
{"id":"<uuid>","short_id":"<prefix>","schedule_ref":"mg-calr:reminder:<uuid>","occurrence_key":"singleton","scheduled_for":"2026-11-01T16:30:00Z","channel":"freedesktop","state":"deferred","reason":"dnd_quiet_hours","deferred_until":"2026-11-01T15:00:00Z","attempts":0,"source":{"kind":"event","id":"<uuid>","title":"Standup"},"supersedes":null,"folded_into":null}
```

Arrays are ordered by `(scheduled_for, schedule_ref, occurrence_key)`. `id` is always the full UUID and is the only identity a script may depend on; `short_id` is an additive display convenience and is never accepted as an identity in JSON input. New optional fields may be added within schema version 1; removal or meaning change requires the D9 compatibility decision. The DTO above and one envelope per command are pinned as **golden fixtures** under `tests/fixtures/reminders/golden/` and asserted byte-for-byte by §5.2(10), so any field addition, removal, rename, or reordering fails the suite until the fixture is updated in the same commit.

**Errors, codes, and exits.** A new `AppError::Reminder(ReminderError)` extends the existing `code()`/`exit_code()` tables in `src/lib.rs`: `reminder_backend_unavailable`/`reminder_backend_transport_forbidden` → 69; `delivery_not_found` → 66; `delivery_state_conflict`/`scanner_already_running` (exclusive mode)/`reminder_claim_fence_stale` → 75; `snooze_target_invalid`/`dnd_window_invalid`/`delivery_selector_ambiguous` → 65; `input_required` → 64; `reminder_config_invalid` → 78. `reminder_claim_fence_stale` is the typed refusal of a superseded owner's write (§4.2 invariant 7) and `delivery_selector_ambiguous` is the typed refusal of an ambiguous short-ID prefix (§3.3); neither ever degrades into a silent success. Projection errors keep their existing codes and exits. The daemon exits 0 on `SIGTERM`, 69 on an exhausted backend/database budget, and 78 on invalid configuration.

**Auth, pagination, rate limiting.** No authentication exists: this is a local process using the unprivileged peer-auth PostgreSQL role, and the session bus authenticates by socket peer credentials. `remind list` paginates with `--limit` (default 100, max 1000) and an opaque stable cursor. Presentation is rate limited by `catch_up.max_burst` and by at most one health bubble per hour — a rate limit on *output*, never on claiming.

### 4.4 State management

PostgreSQL is the single delivery authority. The daemon is **stateless across restarts**: every claim, defer, snooze, and terminal outcome is a committed row before it matters, and restart re-derives all work from the ledger. In-memory state is limited to the cached backend capabilities, the current backend handle map, and the timer — all safely reconstructible.

Ownership: `ReminderUseCases` owns mutation; `PostgresDeliveryRepository` owns SQL; the daemon loop owns only scheduling and signal handling; renderers are read-only. The scanner singleton is enforced by a PostgreSQL advisory lock rather than a PID file, so a killed process releases it automatically when its connection closes.

Local vs. shared boundaries: reminder **schedules** are owned elsewhere (B8 in PostgreSQL; C8 increasingly through the validated `mg.interop/1` projection file), and E treats both as read-only inputs — it never writes a todo, an event, an alarm definition, or the projection store. Delivery **state** is owned exclusively by E; B and D are contractually forbidden from writing it, and `gauntlet-output/specs/d-views-query-output.md` already binds D to an observational read.

Offline/draft persistence: there are no drafts. A guided snooze that is cancelled writes nothing. If PostgreSQL is unavailable, the daemon performs **no** presentation — it will not "deliver now and record later", because an unrecorded presentation is exactly the duplicate that the auto-fail rule forbids.

**Configuration surface.** Every setting this feature introduces lives under `[reminders]` in the XDG TOML and resolves through the existing CLI > env > TOML > default precedence with the distinct XDG config/data/state roots already implemented in `src/config.rs`. This is the complete list; nothing in E reads a setting absent from it.

| Key | Type | Default | Bounds / validation |
|---|---|---|---|
| `backend` | enum `freedesktop` \| `log` \| `null` | `freedesktop` | unknown value → `reminder_config_invalid`, exit 78 |
| `poll_interval` | duration | `20s` | 1s–300s |
| `materialize_horizon` | duration | `24h` | 1h–30d |
| `lease` | duration | `120s` | 30s–15m; **must exceed `backend_reply_timeout`**, else `reminder_config_invalid` |
| `backend_reply_timeout` | duration | `25s` | 5s–60s; the boundary that produces `UnknownOutcome(ReplyTimeout)` |
| `max_attempts` | u16 | `3` | 1–10; counts `BackendError::NotSent` retries only |
| `max_in_flight` | u16 | `64` | 1–512 |
| `recovery.represent_unconfirmed` | enum `never` \| `once` | `never` | see Q2 |
| `catch_up.present_within` | duration | `1h` | 0s–24h |
| `catch_up.fold_within` | duration | `24h` | ≥ `present_within`, ≤ 30d |
| `catch_up.expire_after` | duration | `24h` | ≥ `fold_within`, ≤ 30d |
| `catch_up.max_burst` | u16 | `5` | 1–50 |
| `dnd.follow_backend_inhibit` | bool | `false` | see Q4 |
| `dnd.bypass_urgency` | enum `none` \| `critical` | `none` | — |
| `dnd.quiet_hours` | list of `{ start, end, zone }` | empty | `zone` must resolve in the TZDB; evaluated with B4 gap/fold policy |
| `notification.icon` | string | empty | a **theme icon name** only; a path, URL, or `file:` value is rejected |
| `notification.sound_name` | string | empty | a freedesktop sound **name** only; never a path or URL |
| `notification.privacy` | enum `full` \| `title_only` \| `generic` | `full` | — |
| `log_backend_path` | path | `$XDG_STATE_HOME/mg-calr/reminders.log` | must resolve under `XDG_STATE_HOME`; created `0600` |

Resolution is a pure function of (argv, env map, parsed TOML) — no clock, no filesystem, no bus — and is proved so by `reminder_config_resolution_is_pure` in §5.1. Out-of-bounds and unparsable values are `reminder_config_invalid` (exit 78) at load, before any connection is opened.

**Config compatibility policy.** An unknown key under `[reminders]` is reported once on stderr and ignored, so a newer configuration file never prevents an older binary from starting. A renamed key keeps its previous name as a deprecated alias for one minor series with a stderr deprecation line naming the replacement; the alias and the new name may not both be set (`reminder_config_invalid`). Removing a key or changing the meaning of an existing one requires the D9 compatibility decision, the same gate that governs the JSON contract.

**Channel binding and re-channelling.** `channel` is part of the claim key, and v1 binds exactly one presentable channel at a time: materialization stamps rows with the configured `backend`, and the dispatch predicate in §3.2 step 3 additionally requires `channel = $active_channel`, so a row belonging to another channel is never selected and never presented by the wrong adapter. Changing `reminders.backend` between runs is therefore a configuration change with ledger consequences, and it is handled explicitly rather than silently: at start the daemon counts non-terminal rows at other channels and reports `deliveries.orphan_channel = N` in `remind doctor`. `remind scan --rechannel` moves them with one fenced statement — `UPDATE … SET channel=$new, claim_fence=claim_fence+1 WHERE state IN ('pending','deferred') AND presented_at IS NULL AND attempts=0 AND NOT EXISTS (<target key>)`. It is an `UPDATE` of the same row, never an insert, so no occurrence gains a second row; any row that ever presented keeps its original channel **forever** and is never moved and never re-materialized; any row whose target key is already occupied is left in place and reported. Nothing re-channels automatically.

### 4.5 Dependencies

- **New crate:** a pure-Rust async D-Bus client (`zbus`, evaluated in Q1) behind `src/notify/freedesktop.rs` only, so `domain`/`application`/`storage` never link it and the `log`/`null` backends work without it. License, maintenance, and lockfile audit apply as for every dependency; `unsafe_code = "forbid"` in `Cargo.toml` stays.
- **Existing crate, new feature:** `rustix` (already a dependency, `fs` feature) gains `time` for `timerfd` with `TFD_TIMER_CANCEL_ON_SET` on `CLOCK_REALTIME` — the mechanism that detects a wall-clock jump precisely instead of by polling. Suspend is detected by the growing delta between `CLOCK_BOOTTIME` and `CLOCK_MONOTONIC`.
- **Existing crate, new features:** `tokio` gains `signal` (SIGTERM/SIGHUP) and `net` (`UnixDatagram` for `sd_notify`). No `libsystemd` dependency: `READY=1`, `WATCHDOG=1`, and `STOPPING=1` are datagrams written to `$NOTIFY_SOCKET`.
- **Assets:** none. No icon files, sounds, or fonts are bundled; icon and sound are optional *names* resolved by the user's theme.
- **Infrastructure:** migration **7** (`0007_reminder_delivery_ledger.sql`) on the existing local database; two new systemd **user** units; no server, container, CDN, or third-party service.
- **Explicitly not added:** any HTTP/TLS/DNS client, push service, e-mail/SMS transport, CalDAV or vdirsyncer dependency, or Quickshell/TUI runtime.

### 4.6 Platform-specific considerations

First-class target is Arch Linux + Hyprland with a systemd user manager and a freedesktop notification daemon (mako, dunst, or swaync). Capability differences are probed, never assumed: `GetCapabilities` decides whether actions, markup, and persistence are available, and `GetServerInformation` is recorded in `remind doctor` output so a bug report identifies the daemon.

**E8 — systemd integration** (`remind install-units` prints these; it never runs `systemctl` and never uses sudo):

```ini
# ~/.config/systemd/user/mg-calr-remind.service
[Unit]
Description=mg-calr reminder scanner
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=notify
NotifyAccess=main
ExecStart=/usr/bin/mg-calr remind run --log-format json
Restart=on-failure
RestartSec=5
StartLimitIntervalSec=300
StartLimitBurst=5
WatchdogSec=60
TimeoutStopSec=20
Slice=session.slice

[Install]
WantedBy=graphical-session.target
```

```ini
# ~/.config/systemd/user/mg-calr-remind-scan.timer  (headless / no-daemon fallback)
[Unit]
Description=Periodic mg-calr reminder scan

[Timer]
OnBootSec=2min
OnUnitActiveSec=5min
AccuracySec=30s
Persistent=true

[Install]
WantedBy=timers.target
```

The timer's `mg-calr-remind-scan.service` is `Type=oneshot` running `mg-calr remind scan --dispatch --backend log`. **The long-running service and the timer share the same ledger and the same advisory lock, so running both is safe** — the timer path simply finds nothing to claim while the daemon is healthy. `Persistent=true` gives the timer its own catch-up after downtime, and E5 classification then decides individual/folded/expired.

**Crash recovery semantics.** `Restart=on-failure` with a burst limit means a crash-looping daemon stops rather than flapping; `remind doctor` reports `scanner.stopped` with the last `reminder_scanner_runs` row. On every start the daemon: (1) writes a `reminder_scanner_runs` row with a new owner id (boot id + PID + process start time) and begins heartbeating `heartbeat_at` every `lease / 4`; (2) runs the **fenced claim reconciliation** below; (3) runs catch-up; (4) sends `READY=1`. `WatchdogSec` keepalives come from the tick loop, so a wedged dispatcher is restarted by systemd rather than sitting silently.

**Fenced claim reconciliation (never on lease expiry alone).** A `claimed` row is moved out of `claimed` by anyone other than its own owner **only when that owner is provably not live**. An expired lease is a *precondition*, never the proof: a live-but-slow owner one instruction away from `present()` must not have its row taken from underneath it, because the reconciler's terminal write plus `recovery.represent_unconfirmed = once` would then show a second bubble for a bubble that already rendered. Proof of death is any one of:

- (a) the owner's `reminder_scanner_runs` row has a non-null `stopped_at` — it shut down and said so;
- (b) the owner's `heartbeat_at` is older than `2 × lease`, i.e. at least eight consecutive heartbeats were missed at the `lease / 4` cadence;
- (c) the owner id embeds a boot id different from the current `/proc/sys/kernel/random/boot_id` — the process provably cannot exist on this boot;
- (d) the owner id embeds this boot's id and either `/proc/<pid>` is absent or the start time in `/proc/<pid>/stat` differs from the one embedded in the owner id (PID reuse is therefore not mistaken for liveness).

If the lease has expired and none of (a)–(d) holds, **the row is not moved.** It stays `claimed`, `remind doctor` counts it as `claims.expired_unproven`, and `remind list --state claimed` shows it with its owner and lease. Refusing to act is the correct behaviour here: a stuck row is a visible, recoverable miss, while a wrong reconciliation is an invisible duplicate.

When proof exists, reconciliation is one statement that both writes the terminal state and **bumps the fence**: `UPDATE reminder_deliveries SET state='unconfirmed_lost', terminal_reason='claim_reconciled_owner_dead', claim_owner=NULL, claim_fence=claim_fence+1 WHERE id=$1 AND state='claimed' AND claim_owner=$dead_owner AND claim_fence=$observed_fence`. Because the fence moves, a stalled-then-resumed process from that owner finds its own `claimed → presented` / `→ failed` / `→ pending` predicate false, matches zero rows, and returns `reminder_claim_fence_stale` (§4.2 invariant 7) instead of landing a write after its lease was reclaimed. And because §3.2 step 6 re-checks the fence *before* calling the backend, the far commoner case — a stalled owner that has not yet presented — never performs the side effect at all.

**Sweeping expired leases while the daemon runs.** Reconciliation is not start-up-only. Every dispatch tick runs the identical statement over rows with `state='claimed' AND claim_expires_at <= now()`, so a row claimed by a oneshot `remind scan --dispatch` that was killed mid-flight is swept within one lease period **without waiting for a daemon restart**. When no daemon runs at all, the `mg-calr-remind-scan.timer` path performs the same sweep on each activation. Both paths call the same repository method with the same proof-of-death predicate; there is no second implementation to drift.

Version compatibility: PostgreSQL 18 target (as elsewhere in the project), systemd ≥ 249 for `Type=notify` user units, and freedesktop Notifications spec 1.2 with 1.3 `Inhibited` treated as optional. A machine with no session bus is a supported configuration through `--backend log` — the product degrades to a recorded, inspectable ledger rather than failing.

### 4.7 Performance budget

- **Memory.** Resident daemon target < 32 MiB with one PostgreSQL connection, one bus connection, and a bounded in-flight map (`max_in_flight`, default 64). Materialization streams and never holds more than one horizon of schedules.
- **CPU.** Idle CPU must be effectively zero: the loop blocks on `timerfd`/signals rather than spinning, and the default 20 s poll is a safety net, not the primary wake. One dispatch tick with no due rows is a single index-only scan on `reminder_deliveries_due`.
- **Latency.** Presentation within 5 s of `scheduled_for` at p95 on a healthy session; catch-up classification of 1,000 missed rows in < 2 s; `remind list` first page < 150 ms p95 on the 100,000-delivery synthetic fixture, with `EXPLAIN` showing no sequential scan.
- **Network payload.** Exactly zero. The two sockets opened are the PostgreSQL Unix socket and the D-Bus session Unix socket.
- **Storage.** One row per `(schedule_ref, occurrence_key, scheduled_for, channel)` plus one audit row per transition; a year of five daily reminders is ~1,800 rows plus history. Retention/pruning is a G3 operation, not a scanner side effect.
- **Startup.** `remind run` reaches `READY=1` in < 500 ms excluding database connect; the CLI action commands add no startup cost beyond the existing binary. The current `scan_reminders` implementation expands todo occurrences from `NaiveDate::MIN` on every scan and is explicitly replaced by the bounded horizon above.

---

## 5. Test Specification

### 5.0 Binding idempotency matrix (T5)

Every row runs against a disposable PostgreSQL database with `NullBackend`, an injected clock, and an injected `BackendError` class. Two counters are asserted per claim key, and they are not the same counter: **`present()` calls** (how many times the adapter was invoked) and **bubbles rendered** (how many times the recording backend actually produced a notification). A `NotSent` retry may raise the first; **nothing in this matrix may ever raise the second above 1.** This matrix is a release gate; weakening any row fails the spec.

| # | Scenario | Required result |
|---|---|---|
| 1 | Run `remind scan` three times at the same instant | identical planned key set; ledger row count unchanged after runs 2–3 |
| 2 | Two scanners dispatch the same due row concurrently | exactly one claim CAS returns a row; exactly one `present()`; the loser records no state change |
| 3 | Kill the process **after** claim commit, **before** `present()` | on restart the row is `unconfirmed_lost`; `present()` total stays 0; doctor counts it |
| 4 | Same as #3 with `recovery.represent_unconfirmed = once` | exactly one `present()` total, flagged `recovered`; a second restart presents nothing |
| 5 | Kill **after** `present()` returns, **before** the presented write | on restart the row is `unconfirmed_lost` (not re-presented by default); `present()` total stays 1 |
| 6 | Backend returns a **definitely-not-sent** error twice (`NotSent(NoNameOwner)`, then `NotSent(WriteFailedBeforeFlush)`), then succeeds | `present()` called 3 times and **the two failing calls provably rendered nothing**; exactly one success, `attempts = 3`, final state `presented`; **bubbles on screen for the key = 1** |
| 6a | Backend returns an **unknown-outcome** error: the stub server renders the bubble, then the reply is dropped so the call hits `backend_reply_timeout` | **`present()` total stays 1**; no retry is attempted and no `claimed → pending` transition occurs; row is `unconfirmed_lost` with `terminal_reason='backend_unknown_outcome'`; default config presents nothing further; doctor counts it and `remind list --state unconfirmed_lost` shows it |
| 7 | Backend fails past `max_attempts` | state `failed` with reason; no further attempts on later ticks; at most one health bubble per hour |
| 8 | Suspend across two due instants, then resume | wall-clock jump detected; both rows classified by E5; no duplicate for either key |
| 9 | Downtime of 6 h with 20 missed rows, `present_within=1h`, `fold_within=24h` | ≤5 individual presentations, remainder `folded` into exactly one digest, digest `present()` called once |
| 10 | Crash during digest presentation, then restart | at most one digest presentation total; folded rows are not re-presented individually |
| 11 | Downtime of 3 days | all rows `expired` with reason; zero presentations; every row listed |
| 12 | DND active when the row comes due | state `deferred`, `present()` count 0; at `deferred_until` exactly one presentation (or one release summary) |
| 13 | DND toggled on/off repeatedly during one tick window | no duplicate presentation; final state is a single deterministic outcome |
| 14 | Two `ActionInvoked` snooze signals for one delivery | one successor row; second returns `already_snoozed`; two successors are impossible by the unique index |
| 15 | Snooze, then the snoozed occurrence comes due again naturally | successor and natural row have different `scheduled_for`, so both exist; each presents once |
| 16 | Dismiss, then a later scan and restart | no re-materialization of the dismissed key; `present()` total stays 1 |
| 17 | Clock moved backwards 2 h between ticks | rows already presented stay terminal; nothing is re-presented |
| 18 | Schedule deleted (event trashed) between claim and present | presentation is skipped, row `revoked`, live bubble closed |
| 19 | Todo projection replaced mid-run with a stale/conflicting snapshot | todo deliveries fail closed with the existing projection codes; event deliveries continue unaffected |
| 20 | Database connection dropped mid-dispatch | no presentation without a committed claim; on reconnect the claim state decides, never memory |
| 21 | `UnknownOutcome` where the reply is lost **and** the server in fact rendered nothing | `present()` total stays 1; row is `unconfirmed_lost`, **not** `presented`; the outcome is a recorded miss and no path converts it into a retry — the spec accepts a miss here and never a duplicate |
| 22 | Owner exceeds its lease but is alive, and its `present()` then succeeds | exactly one presentation; row ends `presented`; the reconciler made **no** terminal write because death could not be proven; doctor reported `claims.expired_unproven = 1` while it was overdue |
| 23 | Owner exceeds its lease, is proven dead (`heartbeat_at` older than `2 × lease`), is reconciled — then its stalled thread resumes and attempts the presented write | the write matches zero rows because the fence moved; it returns `reminder_claim_fence_stale`; the row stays `unconfirmed_lost`; `present()` total stays 1 |
| 24 | Oneshot `remind scan --dispatch` killed after claim commit while the daemon keeps running | the row is swept to `unconfirmed_lost` within one lease period **without any restart**; `present()` total stays 0 |
| 25 | Stalled owner resumes *before* calling `present()` after its fence moved | `present()` is never called (§3.2 step 6 refuses the side effect on a stale fence); `present()` total stays 0 for that owner |
| 26 | Item trashed, then restored before `scheduled_for` | the `revoked` row is revived to `pending` under the §4.2(9) predicate; **exactly one** presentation; no second row exists for the key; both transitions appear in `audit_log` |
| 27 | Item trashed, then restored **after** `scheduled_for` has passed, and trashed/restored twice more | no revival in any cycle; the row stays `revoked`; zero presentations; the revival statement is idempotent across repeated scans |
| 28 | A `revoked` row that had already presented before revocation is restored | no revival (`presented_at IS NOT NULL` fails the predicate); `present()` total stays 1 |
| 29 | Two concurrent catch-up runs fold the same window | exactly one `reminder_digests` row for `(kind, window_start, window_end)`; the loser adopts the winner's id for its `folded_into` links; digest `present()` called once |
| 30 | Migration-1 rows backfilled by migration 7: one `delivered_at`-set row and one past-dated bare row | the delivered row is `presented` at `channel='freedesktop'` and re-materialization of its key is impossible; the past-dated row is `expired` with `terminal_reason='backfilled_before_delivery_existed'`; `present()` total 0 for both; no upgrade burst |
| 31 | A future-dated backfilled row after upgrade | presented exactly once on the first tick after its `scheduled_for` |
| 32 | Dispatcher configured for `log` while `freedesktop` rows exist | the `freedesktop` rows are never selected by the dispatch query and never presented by the `log` adapter; doctor reports `deliveries.orphan_channel`; `remind scan --rechannel` moves only non-terminal, never-presented rows and creates no second row |

### 5.1 Unit tests

- `delivery_key_is_total_and_ordered`: the four-field key round-trips, orders deterministically, and has no defaulted constructor.
- `planner_is_pure_and_repeatable`: the same schedules + clock produce byte-identical plans across 1,000 iterations.
- `planner_omits_suppressed_schedules`: completed, trashed, blocked (C8), and cancelled-event schedules yield no key at all.
- `state_machine_rejects_illegal_transitions`: every non-edge in §4.2(4) is a typed error, and terminal states accept nothing.
- `snooze_target_bounds`: <1 min and >7 days rejected; `--for`/`--until` equivalence proved.
- `catch_up_classification_boundaries`: exactly-at `present_within` and `fold_within` boundaries classify deterministically (half-open).
- `dnd_window_evaluation_is_zone_correct`: quiet hours across a DST gap and fold resolve with the B4 policy and never with host-local time.
- `dnd_precedence`: manual window > quiet hours > optional backend inhibit; an unavailable inhibit probe yields `unknown`, not "disturb".
- `backoff_is_bounded_and_monotonic`: attempt delays are capped and never zero.
- `presentation_request_is_plain_text`: no ANSI, no markup unless the capability is advertised, body self-contained, action labels are words.
- `close_reason_mapping`: reasons 1/2/3/4 map to `presented`/`dismissed`/ignored/`presented` respectively.
- `action_token_parsing_is_strict`: unknown, foreign, and malformed tokens are dropped without a state change.
- `bus_address_transport_guard`: `tcp:` and `autolaunch:` addresses are refused before any connection attempt.
- `backend_error_classification_gates_retry`: the transition planner is driven with **every** `BackendError` value; assert that `NotSent(_)` is the only class that can yield `claimed → pending`, and that no `UnknownOutcome(_)` value can yield a retry, a second `present()`, or a `presented` write. The planner matches on the enum with **no wildcard arm**, so a variant added later fails to compile until it is classified — the classification cannot rot silently.
- `dbus_failure_modes_map_to_the_documented_class`: table-driven over the §4.3 mapping table; every named freedesktop/D-Bus failure maps to the stated class, and specifically "no reply for a serial that was already flushed" maps to `UnknownOutcome`, never `NotSent`. An unclassifiable failure defaults to `UnknownOutcome`.
- `fence_predicate_refuses_superseded_writes`: a `claimed → presented` write carrying a stale `claim_fence` matches nothing and returns `reminder_claim_fence_stale`; the same write carrying the current fence lands. Fences are monotonic per row and never reused.
- `side_effect_refused_on_stale_fence_or_lease`: the dispatcher's pre-call check returns "refuse" — and the backend records **zero** calls — when the fence has moved or the lease has expired, proving the refusal happens before the side effect rather than after it.
- `lease_expiry_alone_never_reconciles`: an expired lease with no proof of death yields no transition and a `claims.expired_unproven` count; each of proofs (a)–(d) in §4.6 independently unlocks the transition; a reused PID with a mismatched start time counts as *alive*, not dead.
- `revoked_revival_predicate_is_total`: only `revoked` + `presented_at IS NULL` + `attempts = 0` + `scheduled_for > now` revives; every other state, a past instant, and a previously presented or previously claimed row do not; running the revival twice is a no-op.
- `digest_claim_key_is_unique_per_window`: two concurrent catch-up plans for the same `(kind, window_start, window_end)` produce exactly one insert; the loser adopts the winner's id and never inserts a second digest.
- `reminder_config_resolution_is_pure`: CLI > env > TOML > default resolves for `backend`, `recovery.represent_unconfirmed`, `lease`, `backend_reply_timeout`, and all four `catch_up` keys; bounds violations and `lease <= backend_reply_timeout` are `reminder_config_invalid`; an unknown key warns and is ignored; a deprecated alias set together with its replacement is an error. The function touches no clock, filesystem, or socket, and the same inputs give byte-identical output across 1,000 iterations.
- `short_id_prefix_resolution_is_unambiguous`: a prefix matching one delivery resolves; a prefix matching two returns `delivery_selector_ambiguous` with both full UUIDs and mutates nothing; a prefix matching none returns `delivery_not_found`; no code path picks a candidate.
- `channel_predicate_excludes_foreign_rows`: the dispatch query built for `channel='log'` never selects a `freedesktop` row, and the re-channel statement refuses any row that ever presented.
- Property tests generate schedule sets, clock jumps, crash points, **backend error classes**, reconciliation races, and DND windows, then assert the global invariant, stated precisely: **successful presentations per key ≤ 1**; **total `present()` calls per key ≤ 1 for every error class other than `NotSent`**; and on `NotSent` runs, every extra `present()` call is one the backend provably did not render, so bubbles-on-screen per key remains ≤ 1 in all cases. No non-terminal row is lost: every key ends `presented`, `dismissed`, `snoozed` (with a successor), `folded`, `expired`, `failed`, `revoked`, or `unconfirmed_lost`, each with a reason.

### 5.2 Integration tests

Using the existing opt-in harness (`MG_CALR_RUN_DATABASE_TESTS=1`, `MG_CALR_TEST_DATABASE_URL` containing `mg_calr_test`, as `tests/postgres_integration.rs` already enforces):

1. Apply migration **7** (`0007_reminder_delivery_ledger.sql`) twice; assert the claim unique index exists, the old `(reminder_id, scheduled_for)` constraint is gone, `claim_fence` is `NOT NULL DEFAULT 0`, and no delivery row was destroyed. Assert the backfill for synthetic migration-1 rows: every row lands at `channel='freedesktop'` (there is no `'none'` channel and the `CHECK` rejects one), a bare `claimed_at` maps to `unconfirmed_lost`/`backfilled_unconfirmed`, and a still-`pending` row dated before the migration maps to `expired`/`backfilled_before_delivery_existed` while a future-dated one stays `pending`. **Migration-ledger delta:** `tests/migration_contract.rs` today asserts `MIGRATIONS.len() == 6`, `MIGRATIONS[5].version == 6`, and `MIGRATIONS[5].sql == REPAIR_TODO_RECURRENCE_MIGRATION`; this feature updates those to `len() == 7` and adds `MIGRATIONS[6].version == 7` / `MIGRATIONS[6].sql == REMINDER_DELIVERY_LEDGER_MIGRATION`, leaving the existing `migration_versions_are_strictly_increasing_and_unique` assertion passing unchanged (§7.2).
2. Prove the FK change: deleting a reminder definition leaves its delivery rows with `reminder_id IS NULL` and full `schedule_ref` history.
3. Run the entire §5.0 matrix against real PostgreSQL with fault injection after each of: claim commit, backend call, presented write, digest insert, audit insert.
4. Run 8 concurrent dispatchers over 500 due rows; assert exactly 500 presentations and 500 claim rows.
5. Advisory-lock singleton: a second `remind run` exits without claiming; killing the first releases the lock automatically.
6. Source boundary: event schedules from PostgreSQL and todo schedules from a validated projection materialize into the same ledger with distinct `schedule_ref` namespaces; a stale projection blocks only todo rows.
7. Capability assertion: the reminder role can `INSERT`/`UPDATE` the ledger, digests, DND, and audit tables and **cannot** write `events`, `todos`, `reminders`, or `extension_properties`; attempted writes fail at the database, not only in code.
8. Preservation fixture: hash `events.extension_properties`, alarm rows, and todo payloads before and after a full scan/dispatch/catch-up cycle and require byte equality (I1).
9. Network denial: run every reminder command and one full daemon cycle under a socket-observing harness; assert only the PostgreSQL and D-Bus Unix sockets are opened, and zero AF_INET/AF_INET6 sockets or DNS lookups (I4).
10. **Golden JSON contracts.** `tests/fixtures/reminders/golden/` pins the §4.3 delivery DTO, the success envelope for `remind.scan`, `remind.list`, `remind.show`, `remind.catch-up`, and `remind.dnd`, and one error envelope for **every** code in the §4.3 table. The test serializes against a frozen clock and fixed UUIDv7 seeds and compares bytes; any field addition, removal, rename, or reordering fails until the fixture is updated in the same commit — the reviewable signal D9 needs to adopt these DTOs.
11. **Fenced reconciliation against real PostgreSQL.** (a) A `claimed` row whose owner is alive-but-past-lease survives a reconciliation pass untouched and is counted as `claims.expired_unproven`. (b) A `claimed` row whose owner is proven dead is reconciled and its fence is bumped in the same statement. (c) The resurrected owner's `presented` write is rejected with `reminder_claim_fence_stale` and changes nothing. (d) A oneshot dispatcher killed after claim is swept by the running daemon's tick within one lease period, with no restart.
12. **Restore round trip.** Trash an item, assert its future delivery is `revoked`; restore it, run `remind scan`, assert the same row (same `id`, same key) is `pending` again and presents exactly once; repeat trash/restore three times and assert one presentation total and a complete `audit_log` chain. Restore after `scheduled_for` has passed and assert no revival and zero presentations.
13. **Channel binding.** Materialize under `backend=freedesktop`, restart under `backend=log`, and assert the `freedesktop` rows are never presented, doctor reports `deliveries.orphan_channel`, `remind scan --rechannel` moves only non-terminal never-presented rows, and the total row count per `(schedule_ref, occurrence_key, scheduled_for)` never increases.

### 5.3 UI / E2E tests

Process-level tests with `assert_cmd` (matching `tests/cli_contract.rs` conventions) plus a stub D-Bus service on a private bus:

1. `remind --help`, and each subcommand's help, lists every flag; `todo scan-reminders` still works and prints its deprecation line.
2. `remind scan --json --no-input` emits exactly one envelope on stdout, byte-identical to the §5.2(10) golden fixture; a forced error emits exactly one error envelope on stderr, byte-identical to that code's golden fixture, with the documented exit code.
2a. Selector behaviour end to end: `remind show <8-char short id>` resolves; with two deliveries sharing that prefix it exits 65 with `delivery_selector_ambiguous`, prints both full UUIDs, and `remind snooze` on the same ambiguous prefix exits 65 **and leaves the ledger byte-identical** (snapshot before/after).
3. Against the stub bus: a due reminder produces one `Notify` call with the expected `summary`/`body`/`actions`/`urgency`; emitting `ActionInvoked("snooze:<id>")` twice produces one successor row; `NotificationClosed(id, 2)` marks `dismissed`.
4. Missing bus: `remind run --backend freedesktop` exits 69 with actionable text and presents nothing; `--backend log` succeeds and writes JSON lines to the state dir.
5. `remind list` and `remind show` render every state and reason as words at 40, 80, and 200 columns with `--no-color` and with `NO_COLOR=1`; no ANSI bytes appear.
6. `remind install-units` prints both units to stdout and writes nothing without `--write`; `remind doctor` is non-mutating (schema snapshot equal before/after) and prints administrator commands without executing them or invoking sudo.
7. `remind snooze` without `--for`/`--until` under `--no-input` exits 64 and never blocks on stdin (test asserts closed stdin cannot hang).
8. Full daemon E2E under a systemd user manager in CI-or-container: start, verify `READY=1`, deliver, `systemctl --user kill -s SIGKILL`, restart, and assert the §5.0 rows 3/5 outcomes.

### 5.4 Visual / manual verification

- Verify bubble rendering on mako, dunst, and swaync: action labels, urgency styling, persistence of `critical`, and stacking behavior of a recovered re-presentation.
- Verify light and dark terminal themes for `remind list`/`remind show` with color on, `--no-color`, and `NO_COLOR`; confirm no state is conveyed by color alone.
- Verify long Unicode titles, combining characters, RTL text, and a 4,000-character body: truncation happens at grapheme boundaries and never mid-escape.
- Verify empty vs. populated states: no deliveries, only deferred, only expired, a large catch-up digest.
- Verify screen-reader output of `remind list` and of a bubble (via the notification daemon's accessibility path), including the CLI-equivalent hint when the daemon lacks `actions`.
- Verify terminal extremes (40 and 200 columns) and a headless TTY session with `--backend log`.

### 5.5 Required quality gates

```text
cargo fmt --all -- --check
TMPDIR=/dev/shm cargo clippy --workspace --all-targets --all-features -- -D warnings
TMPDIR=/dev/shm cargo test --workspace --all-targets --all-features
```

Plus: the §5.0 matrix on disposable PostgreSQL, the role-privilege matrix, the network-denial test, the migration forward/idempotency test, the preservation hash fixture, secret scan, and the clean-machine unit install/enable rehearsal. A green happy path never overrides a failed idempotency, privilege, or network gate.

---

## 6. Compliance & Safety Gate

### 6.1 Sensitive data classification

- [ ] No sensitive data involvement
- [x] **Handles sensitive data** — reminder text derives from event titles/locations and todo titles, and a notification renders that text on a screen that may be shared, projected, or locked. Protections: content stays in the local PostgreSQL authority and the user's own session bus; `notification.privacy = full | title_only | generic` (default `full`, with `generic` rendering `Reminder — open mg-calr` for locked/shared sessions); bodies are truncated and control characters are stripped before presentation; logs record IDs, states, and reasons but never bodies; the `log` backend writes to `$XDG_STATE_HOME` with `0600`; database URLs and bus addresses are redacted using the existing `ConnectionSettings::safe_summary` discipline.
- [x] **Uses synthetic/test data only until compliance gate clears** — every fixture, matrix row, and benchmark uses synthetic reminders.

### 6.2 Asset provenance

- [x] **No third-party assets** — no icons, sounds, fonts, models, or datasets are bundled. Optional `icon`/`sound-name` hints are *names* resolved from the user's installed theme; `mg-calr` ships and fetches nothing.
- [ ] Uses third-party assets

New crate code (the D-Bus client) is a dependency, not an asset, and passes the repository-wide license/maintenance/lockfile audit before release. No `LICENSE` decision is unblocked or bypassed by this feature.

### 6.3 Language / claims audit

- [ ] Makes claims not supported by evidence — **no.** §7 states plainly that no delivery exists today and that `transport: "none"` is the current literal behavior.
- [ ] Promises capabilities not yet built — **no.** Until the §5.0 matrix passes, CLI help and output say "delivery recorded" / "presentation attempted", never "delivered exactly once". The word *exactly-once* is used in this spec for the **claim** (which is transactional) and as *presentation intent* for the side effect, and the honest failure direction (a missed, recorded notification) is stated in §3.2, §4.3, and §3.6 rather than hidden. The `BackendError` taxonomy makes that direction structural rather than aspirational: an unknown outcome is recorded as a miss because no retry path exists for it.
- [ ] Uses language restricted by domain regulations — **no.** Reminders carry no medical, legal, financial, or safety-critical claim; help text must not describe `mg-calr` as suitable for medication, alarm, or life-safety reminders, because best-effort desktop notification is not an alarm guarantee.

### 6.4 Regulatory alignment

**Lens 3 — Standards Interoperability and Sync (walked by name):**

- **I1 Lossless iCalendar — addressed, not deferred wholesale.** Reminder definitions may originate from `VALARM` data whose unknown properties B5/F1 must preserve. E is strictly read-only over definitions: it writes `reminder_deliveries`, `reminder_digests`, `reminder_dnd_windows`, `reminder_scanner_runs`, and `audit_log`, and has no statement that touches `events`, `todos`, `reminders`, or `extension_properties`. §5.2(8) hashes the opaque property store and alarm rows before and after a full cycle and requires byte equality, and §5.2(7) proves the database role cannot write them even if code regressed. Codec/round-trip ownership remains F1; E claims no iCalendar parsing or serialization.
- **I2 Sync authority — addressed.** PostgreSQL is the sole delivery authority. The `mg.interop/1` projection file is a read-only *schedule source* and is explicitly **not** a delivery authority; the daemon never writes it, never writes a vdir mirror, and never creates a second store of delivery state. A stale or conflicting projection fails closed with the existing `projection_stale`/`projection_conflict` codes rather than presenting from a divergent copy.
- **I3 Conflict/deletion — addressed.** A schedule that vanishes produces `revoked` deliveries with retained provenance rather than deleted rows; the ledger outlives the definition after the FK change in §4.2, so a deleted-and-recreated definition cannot resurrect an already-presented occurrence. E never resolves a sync conflict, never picks a winner, and never overwrites remote state; three-way resolution and tombstone round trips remain F8–F10. Delivery rows are never hard-deleted by the scanner; purge/retention is G3. The **restore** half of the round trip is specified as well as the delete half: §4.2 invariant 9 revives a `revoked` row — and only a `revoked` row that never presented and was never claimed, whose trigger is still in the future — through materialization's own conflict path, so an item trashed and restored before its reminder is due still fires exactly once, while a restore after the instant has passed leaves the tombstone intact. §5.0 rows 26–28 and §5.2(12) are the round-trip fixtures.
- **I4 Scope/network — addressed and never N/A.** A notification daemon must make no network call, and this one does not. The only sockets opened in any path are the PostgreSQL Unix socket (peer auth) and the D-Bus session Unix socket; a `DBUS_SESSION_BUS_ADDRESS` whose transport is not `unix:` is refused with `reminder_backend_transport_forbidden` before connecting. No HTTP/DNS/TLS client is linked, no icon or sound is fetched, no URL from event data is opened or executed, and no push, e-mail, or SMS transport exists. §5.1 (`bus_address_transport_guard`) and §5.2(9) prove this by adapter test and by socket observation across every command and a full daemon cycle.

**Lens 1 — Temporal and Data Integrity:** **T1** identity uses the existing typed `ReminderId`/`DeliveryId` UUIDv7 values plus an interop-grammar `schedule_ref` and an occurrence key, all immutable; selectors (short IDs) never carry authority. **T2** `scheduled_for` is absolute UTC derived from the owning feature's civil intent with B4 gap/fold policy; DND quiet hours are evaluated in a stored IANA zone; host-local time never decides due-ness. **T3** every claim and transition is a single transactional CAS with audit rows, and §5.2(3) injects faults at every stage. **T4** deliveries are never destructively deleted; `revoked`/`expired`/`unconfirmed_lost` are distinct, reasoned, auditable states, and the cascade defect in migration 1 is fixed. **T5** is the spec's center: a durable unique claim key, claim-before-side-effect ordering, a monotonic per-row fencing token that refuses both a superseded owner's side effect and its write, a two-class `BackendError` taxonomy in which only a *provably-not-sent* failure may ever retry, a bounded attempt counter, and the §5.0 retry/crash/sleep/DND/catch-up matrix (rows 1–32) proving exactly-once presentation intent — with a **recorded miss** as the only honest failure direction and a duplicate as an outcome no path can reach.

**Lens 2 — CLI Usability and Automation:** **C1** guided defaults with actionable recovery text for every failure. **C2** the versioned envelope, deterministic ordering, `--no-input`, stable exit codes, golden JSON fixtures itemized as an executable test in §5.2(10) and §5.3(2), and selectors that resolve exactly or fail typed (§3.3, `delivery_selector_ambiguous`) — never a silent pick and never a mutation on an ambiguous match. **C3** all new settings live under `[reminders]` in the XDG TOML with the existing CLI > env > TOML > default precedence and distinct XDG config/data/state roots, consolidated in the §4.4 key/type/default/bounds table, proved pure by `reminder_config_resolution_is_pure` (§5.1), and governed by a stated unknown-key/rename/removal compatibility policy; the `log` backend writes only under `XDG_STATE_HOME`. **C4** `--no-color`/`NO_COLOR`, word-based states, chronological ordering, and width degradation. **C5** `remind doctor` is non-mutating, emits a stable machine check matrix, and prints administrator commands without executing them or invoking sudo.

**Lens 4 — Operational Security and Reliability:** **O1** no credential is stored, prompted, or logged; database URLs and bus addresses are redacted. **O2** the daemon runs as an unprivileged systemd *user* unit with a least-privilege database role and no DDL, no sudo, and no system-unit installation. **O3** typed errors, stable codes/exits, atomic mutation, a bounded restart policy, and the recovery semantics in §4.6. **O4** unit, property, contract, disposable-integration, fault-injection, privilege-matrix, network-denial, and E2E gates.

Any design permitting duplicate reminder delivery, a non-idempotent scan, a presentation without a committed claim, network access from a non-sync path, secret logging, or destruction of delivery provenance is an automatic failure.

### 6.5 Security controls

- The daemon is unprivileged, has no setuid path, spawns no child process, and executes nothing from event/todo content — no `xdg-open`, no URL handler, no shell interpolation.
- Notification text is treated as untrusted data: control characters and terminal escape sequences are stripped before rendering to a terminal and before `Notify`, and markup is emitted only when `GetCapabilities` advertises `body-markup`.
- All SQL is parameterized; state names and channels come from enums, never user text.
- Action tokens are opaque UUIDs validated against the ledger; a forged or foreign token cannot transition another user's row (the bus is per-session anyway).
- Resource caps: bounded horizon, `max_in_flight`, `max_burst`, body/summary truncation, and a capped attempt counter prevent memory, CPU, and notification-flood exhaustion from malformed or adversarial local data.

---

## 7. Gap Analysis vs. Current State

### 7.1 What exists today

- **Implemented — schema scaffolding.** `migrations/0001_foundation.sql` creates `reminders` (exactly one of `event_id`/`todo_id`, exactly one of `offset_seconds`/`absolute_at`) and `reminder_deliveries` with `UNIQUE (reminder_id, scheduled_for)` plus unused `claimed_at`, `delivered_at`, `dismissed_at`, `snoozed_until`, and `deferred_reason` columns, and an `audit_log` table. The FK is `ON DELETE CASCADE`, which currently destroys delivery provenance with its definition.
- **Implemented — todo reminder definitions.** `migrations/0004_todo_reminders.sql` creates `todo_reminders (todo_id, minutes_before, repeatable)` with a 1–10,080-minute check and index, adds `reminders.repeatable`, performs a deterministic de-duplication of legacy duplicate reminder identities *and their deliveries* (keeping the lowest id), and adds the partial unique index `reminders_todo_schedule_unique (todo_id, offset_seconds, repeatable)`. It provides **definition** uniqueness and a delivery-identity bridge; it provides no delivery state machine, channel, occurrence key, lease, or transport.
- **Implemented — migration ledger.** `src/storage.rs` registers six migrations in `MIGRATIONS`, `0001_foundation` through `0006_repair_todo_recurrence`, each with an `include_str!` constant; `tests/migration_contract.rs` asserts `MIGRATIONS.len() == 6`, `MIGRATIONS[5].version == 6`, `MIGRATIONS[5].sql == REPAIR_TODO_RECURRENCE_MIGRATION`, and that versions are strictly increasing and unique. **Version 6 is therefore already taken**, and this feature's migration is version **7** (`0007_reminder_delivery_ledger.sql`); §7.2 carries the corresponding test delta.
- **Implemented — identity and contracts.** `src/domain.rs` defines `ReminderId` and `DeliveryId` (UUIDv7, typed, non-substitutable). `src/lib.rs` defines the versioned success/error envelopes, `reminder_invalid` code, and exit-code table. `src/domain/todo.rs` defines `TodoReminder` with offset validation and duplicate rejection (`tests/todo_core.rs::reminders_validate_due_offsets_and_deduplicate`).
- **Prototyped — scan only.** `src/storage.rs::scan_reminders` expands todo occurrences, filters completed/trashed/blocked todos, sorts candidates deterministically, upserts `reminders` rows, and inserts `reminder_deliveries` with `ON CONFLICT (reminder_id, scheduled_for) DO NOTHING`, returning `status` of `recorded` / `already_recorded` / `would_record` and a hard-coded `transport: "none"`. `--dry-run` rolls back. `src/storage.rs::due_reminders` returns due todo reminders with a hard-coded 09:00 default for date-only todos. Exposed as `mg-calr todo scan-reminders --at --dry-run` (`src/main.rs`), asserted only at the help level by `tests/cli_contract.rs::reminder_scan_contract_is_explicitly_dry_run_capable`.
- **Absent — everything that delivers.** No notification backend, no D-Bus code, no daemon, no claim/lease/state machine, no channel or occurrence key in the ledger, no snooze/dismiss/DND/catch-up, no action service, no systemd units, no scanner singleton, no crash recovery, no event-sourced schedules (`scan_reminders` reads todos only), and no reminder tests beyond the help assertion. `README.md` lists "Reminder delivery/service actions" under remaining scope.
- **Gated — todo authority.** `README.md` and `src/interop.rs` record that todo ownership is moving to a separate `mg-todo` application: agenda reads come from a validated `mg.interop/1` projection, while the legacy todo tables remain "for migration compatibility" and are no longer an agenda read authority. E must therefore treat the projection as the forward todo schedule source and the legacy tables as a gated migration path.
- **Planned — sibling contracts.** `gauntlet-output/specs/b-event-calendar-core.md` (B8 reminder definitions, `ReminderInstanceKey`, and an explicit prohibition on B claiming/delivering), `gauntlet-output/specs/c-todo-core.md` (`eligible_todo_reminders`, suppression for blocked/completed/trashed), and `gauntlet-output/specs/d-views-query-output.md` (observational `--has-reminder` gated on E's durable uniqueness contract).

### 7.2 Delta to spec

- **New files:** `src/domain/reminder.rs`, `src/application/reminder.rs`, `src/notify/{mod,freedesktop,log,null}.rs`, `src/daemon.rs`, `migrations/0007_reminder_delivery_ledger.sql`, `tests/reminder_ledger.rs`, `tests/reminder_daemon.rs`, `tests/reminder_cli.rs`, `tests/fixtures/reminders/*`, and two systemd user unit templates emitted by `remind install-units`.
- **Modified files:** `src/main.rs` (new `Command::Remind`, deprecate `TodoCommand::ScanReminders`, extend doctor); `src/lib.rs` (`AppError::Reminder`, new codes/exits); `src/storage.rs` (delivery repository, advisory lock, fenced reconciliation, add `REMINDER_DELIVERY_LEDGER_MIGRATION` and register `Migration { version: 7, name: "reminder_delivery_ledger" }` as the seventh `MIGRATIONS` entry, refactor `scan_reminders`/`due_reminders` behind `ReminderScheduleSource`); `tests/migration_contract.rs` (`MIGRATIONS.len()` 6 → 7 and the new `MIGRATIONS[6]` version/SQL assertions; the existing `MIGRATIONS[5]` repair-migration assertions stay as they are because version 6 is untouched); `src/application.rs` (source/backend traits, `ReminderUseCases`); `src/config.rs` + `config/example.toml` (`[reminders]` section); `src/interop.rs` (expose a read-only todo schedule view); `README.md`/`docs/ARCHITECTURE.md` (daemon and socket boundary).
- **Migrations:** version **7** — `migrations/0007_reminder_delivery_ledger.sql`, appended after `0006_repair_todo_recurrence.sql`, which already holds version 6 — adds `schedule_ref`/`occurrence_key`/`channel`/`state`/`claim_fence`/lease/attempt/linkage columns with the backfill rules in §4.2, replaces the claim unique constraint, converts the cascade FK to `SET NULL`, adds the dispatch index, and creates `reminder_digests`, `reminder_dnd_windows`, and `reminder_scanner_runs`. No existing migration file is edited; version 6 keeps its current identity and its contract tests.
- **New dependencies:** one pure-Rust D-Bus client; `rustix/time`; `tokio` `signal` + `net`. No network client.
- **Behavior removed:** the literal `transport: "none"` claim and the unbounded `NaiveDate::MIN` occurrence expansion in `scan_reminders`.

### 7.3 Estimated scope

**L**, bordering XL. The domain and ledger work is contained and highly testable, but the feature spans a schema migration with backfill, a new side-effecting adapter and its trait boundary, a long-running daemon with signal/timer/suspend handling, systemd integration, and a 32-row crash/concurrency matrix that requires fault-injection harnesses. It is smaller than B or C because it introduces no new user-authored domain data and no recurrence algebra, but it carries the project's only irreversible side effect. Deliver in four TDD slices: (1) ledger + migration + pure planner + null backend; (2) claim/dispatch/action service + CLI; (3) DND, snooze, catch-up, digests; (4) freedesktop backend, daemon, systemd units, crash-recovery E2E.

### 7.4 Blocking dependencies

- **A1–A5** must remain stable: configuration precedence, migration runner and advisory-lock discipline, typed IDs, error/JSON envelopes, and audit transaction semantics.
- **B8** must supply immutable event reminder definitions and, for recurring events, a stable `ReminderInstanceKey`/occurrence identity. Until B6/B7 recurrence lands, E delivers singleton and non-recurring event reminders; recurring event occurrence keys are gated on that work.
- **C8** must supply `eligible_todo_reminders`-equivalent suppression (completed/trashed/blocked) and `todo.date_reminder_time` semantics. During the mg-todo extraction, the `mg.interop/1` projection must expose reminder schedules with stable local IDs and revisions; until it does, the legacy source stays gated behind `--source todo-legacy`.
- **B4 temporal policy** (IANA zone, gap/fold) blocks correct trigger derivation and DND quiet-hours evaluation.
- **Backend evidence spike (Q1)** must select the D-Bus client and prove action/close signal handling on mako, dunst, and swaync before the freedesktop backend leaves the gate.
- **G3/G5** own delivery-ledger retention/pruning and migration rollback policy; **G6** owns broader crash-recovery runbooks. E guarantees its own atomic writes and restart semantics regardless.
- **D9** owns suite-wide JSON compatibility; E must publish additive version-1 delivery DTOs that D9 can adopt.

### 7.5 Explicit non-goals

- No alarm-clock guarantee, wake-from-suspend scheduling (`RTC_WAKEALARM`), or life-safety/medication reminder claim.
- No e-mail, SMS, push, webhook, Matrix, or any network transport; no remote or multi-device delivery, and no reminder state sync.
- No iCalendar parsing/serialization, `VALARM` authoring, CalDAV scheduling, or vdirsyncer interaction.
- No editing of reminder definitions: `remind` never creates, changes, or deletes a schedule (that is B8/C8), and never completes or trashes an item from a notification action in v1.
- No Quickshell pill/card, TUI panel, tray icon, or graphical settings surface; those remain I1/I2 consumers of the JSON contracts.
- No system-level systemd units, sudo, root, or package-manager invocation; no PID-file singleton.
- No arbitrary user-defined notification templates or scripting hooks in v1, and no execution of any external command from a reminder.

---

## 8. Open Questions

- **Q1:** Which pure-Rust D-Bus client (`zbus` vs. alternatives) passes an evidence spike for async session-bus calls, `ActionInvoked`/`NotificationClosed` subscription, reconnect after a notification-daemon restart, license/maintenance review, and clean interaction with `unsafe_code = "forbid"` in this crate? — blocks: `src/notify/freedesktop.rs` dependency selection only; the trait boundary in §4.3 is already binding.
- **Q2:** Should `recovery.represent_unconfirmed` default to `never` (at-most-once; a crashed presentation is recorded but never re-shown) as specified, or to `once` with an explicit `Recovered:` label? The spec locks `never` because duplicate delivery is an auto-fail and a missed delivery is recorded and discoverable — confirm this is the intended tradeoff. — blocks: default configuration value, not the mechanism.
- **Q3:** What are the user's preferred catch-up defaults (`present_within = 1h`, `fold_within = 24h`, `expire_after = 24h`, `max_burst = 5`)? — blocks: shipped defaults only; the classification model and its idempotency are fixed.
- **Q4:** Should `dnd.follow_backend_inhibit` be enabled by default where the notification daemon implements the 1.3 `Inhibited` property, given that mako and dunst express DND through their own mechanisms rather than that property? — blocks: E6 default; manual windows and configured quiet hours are authoritative regardless.
- **Q5:** When does the `mg-todo` extraction finish, i.e. when can `LegacyPostgresTodoSchedules` and `todo scan-reminders` be removed rather than merely deprecated? — blocks: removal timing and the deprecation notice wording, not the source abstraction.
- **Q6:** Should a delivery whose item was completed or trashed *after* presentation but *before* the user acted have its bubble closed automatically (current spec: yes, `revoked` + `CloseNotification`), or should it remain visible for context? — blocks: §3.2 revocation UX detail only.
- **Q7:** Are the fencing/lease defaults right for this workstation — `lease = 120s`, `backend_reply_timeout = 25s`, heartbeat at `lease / 4`, and proof-of-death at `2 × lease` of missed heartbeats? A shorter lease sweeps a killed oneshot sooner; a longer one is more forgiving of a slow notification daemon. The **mechanism** is fixed by §4.2 invariant 7 and §4.6 regardless — an expired lease alone never reconciles — so this blocks shipped defaults only, not correctness.
- **Q8:** Should a `BackendError::UnknownOutcome` be surfaced to the user immediately (a one-line stderr note from `remind run --foreground`, or a single low-urgency "a reminder may have been missed" bubble at most once per hour), or left to `remind list`/`remind doctor` as the spec currently has it? Presenting anything here risks becoming the duplicate we just eliminated, so the spec's default is silence-plus-ledger. — blocks: §3.6 presentation detail only.
