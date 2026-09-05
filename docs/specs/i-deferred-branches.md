# Spec: Later Branches — Approved but Deferred (TUI, Quickshell, CalDAV Scheduling, Broader Packaging)

**Feature ID:** i-deferred-branches **Parent feature:** root **Spec author agent:** Spec agent (branch I) **Date:** 2026-08-29 **Iteration:** 1

> **Status: deferred by schedule, not by uncertainty.** Nothing in branch I is next up. No
> I-branch code may be written until its blocking dependencies in §7.4 land. This document
> exists so that the deferral is a calendar decision rather than an unknown, and so that
> branches A–H can be reviewed against a written interface contract they are forbidden to
> break. Where this spec says "the client does X", read it as "when I is scheduled, the
> client will do X"; §7.1 records what actually exists today, which is very little. >
> **Disambiguation.** The feature sub-IDs in this branch (I1 TUI, I2 Quickshell, I3 CalDAV
> Scheduling, I4 broader packaging) collide numerically with the Lens 3 criteria in
> `docs/specs/QUALITY-CRITERIA.md` (I1 Lossless iCalendar, I2 Sync authority, I3
> Conflict/deletion, I4 Scope/network). Throughout this spec, criteria are always written
> **"Lens-3 I*n*"** and features are always written **"feature I*n*"**. §6.4 uses the Lens-3
> form exclusively.

---

## 1. Purpose

### 1.1 One-sentence job

Let the same local calendar data be operated from a full-screen terminal workspace and glanced at from the Hyprland bar, and hold open a credible path to CalDAV Scheduling and non-Arch Linux distribution — all of it through mg-calr's public command and JSON interfaces, so that no client ever becomes a second authority over PostgreSQL.

### 1.2 Why it matters

`mg-calr` is a daily-driver tool on one workstation. A CLI answers "what is happening?" only when the user types; a bar pill answers it continuously, and a full-screen TUI is where an hour of triage actually happens. Those two clients are the reason the D9 JSON contract, the A4 error envelope, and the C/E identity rules were specified as *public* rather than internal. If they are left unwritten, the pressure during I implementation is to reach past the application layer straight into SQL, which would silently create a second authority, a second recurrence interpretation, and a second deletion policy — exactly the failure modes Lens 1 and Lens 3 exist to prevent. Feature I3 matters for the same structural reason: organizer/attendee data is already flowing through import today, and if the scheduling *target* is not written down, the preservation rules that make it reachable later get quietly dropped now. Feature I4 matters because the H branch's Arch package is a single-distro solution to a problem (a reproducible, license-clean, non-privileged install) that recurs identically on Debian, Fedora, and Nix.

### 1.3 Success signal

Two observable outcomes, one available before I is scheduled and one after:

- **Available now, and the point of writing this early:** `contracts/client-interface-v1.json` plus `tests/client_interface_contract.rs` are checked in and green, and the mg-calr test suite fails if any branch A–H change removes, renames, or alters the meaning of a `stable` interface entry. The deferral is proven safe because the boundary is executable.
- **After I is scheduled:** on the reference workstation, `mg-calr tui` drives an eight-hour triage session — navigate, filter, edit, complete, snooze, undo — with keypress-to-repaint p95 ≤16 ms and zero SQL statements originating in `src/tui/`; concurrently the Quickshell pill renders the next item within 60 s of any change; and a byte-level dump of the disposable database before and after every non-mutating client path is identical.

---

## 2. User Stories

> As a keyboard-first user, I want a full-screen terminal workspace with a period navigator, an
> agenda list, and a detail pane, so that I can triage a week without retyping range flags for
> every question.

> As a Hyprland user, I want a bar pill showing my next event with its relative time and an
> expandable card, so that I know what is next without switching workspaces or opening a terminal.

> As a user whose laptop lid was shut for two days, I want the pill to show a labeled stale state
> with its age rather than a confidently wrong agenda, so that I never act on data that is not
> current.

> As a screen-reader user, I want a TUI mode that emits append-only announced lines instead of
> repainting a full frame, and a Quickshell card whose every row and action has an accessible
> name, so that neither client is a pointer-only or sighted-only path to my calendar.

> As a user in a shared office or on a screen share, I want to blank event titles in the bar with
> one keybind and have the pill never render on the lock screen, so that a glanceable widget is
> not a disclosure channel.

> As someone invited to a meeting from Apple Calendar, I want my `ATTENDEE` line with its
> `PARTSTAT`, `RSVP`, `SCHEDULE-AGENT`, and `X-` parameters to survive import and re-export
> byte-for-byte even though `mg-calr` cannot yet reply, so that scheduling can be added later
> without having destroyed the data it needs.

> As a Debian, Fedora, or NixOS user, I want the same unprivileged, XDG-clean install that Arch
> users get, so that trying `mg-calr` does not require building from a git checkout by hand.

> As a security-conscious operator, I want to be able to prove that neither client can open a
> network socket or reach PostgreSQL directly, so that adding a desktop widget does not widen the
> application's attack surface.

---

## 3. UX Specification

Feature I1 and feature I2 carry all of this section's substance. Feature I3 has no user surface until it is implemented and is marked as such per subsection. Feature I4 has no user surface at all beyond an install transcript, which H already specifies.

### 3.1 Screen / view inventory

**Feature I1 — full-screen TUI.** One process, one alternate-screen application reached by `mg-calr tui`. All panes and overlays below are **new**; `mg-calr tui` itself is a modification of the prototyped line shell described in §7.1.

| Surface | Reached by | New / modified | Layout pattern |
|---|---|---|---|
| Workspace root | `mg-calr tui` | modified (line shell exists) | full-screen alternate buffer, three-pane split |
| Navigator pane | focus `1` or `Tab` | new | left rail, 24 cols: mini month grid + scope selector |
| Agenda pane | focus `2`, default focus | new | center list, flexible width, day-sectioned rows |
| Detail pane | focus `3` or `Enter` on a row | new | right column, 40 cols: labeled field list |
| Status bar | always visible | new | bottom line: pane name, counts, range, freshness, message |
| Command line | `:` | new | bottom line, replaces status while active |
| Search overlay | `/` | new | bottom line input + live-narrowed agenda pane |
| Filter sheet | `F` | new | centered modal, 60×18, typed filter fields |
| Help overlay | `?` | new | full-screen scrollable key grammar, grouped by pane |
| Confirm modal | any destructive key | new | centered modal, 50×7, explicit `y` required |
| Edit form | `e` | new | centered modal, field-per-line, tab-ordered |
| Chooser modal | ambiguous selector | new | reuses D10 candidate contract, numbered list |
| Line-mode fallback | `mg-calr tui --line` | modified (this is today's shell) | non-raw, append-only, `j`/`k`/`r`/`q` |
| Screen-reader mode | `--screen-reader` | new | non-alternate-screen, append-only announcements |

**Feature I2 — Quickshell.** Three surfaces, all **new**, all QML under `integrations/quickshell/`, all rendered by the user's existing shell.

| Surface | Reached by | Layout pattern |
|---|---|---|
| Agenda pill | always present in the bar | single-row bar module, max 320 px, glyph + time + title + badge |
| Expanded card | click or `Super+C` on the pill | popover panel anchored to the pill, 420×520 max, scrollable |
| Action toast | after any card action | shell's existing toast surface, ≤2 lines, auto-dismiss 4 s |

The pill has five mutually exclusive states, each with a distinct glyph **and** a distinct text token: `ok`, `empty`, `stale`, `error`, `unconfigured`. No state is distinguished by color alone.

**Feature I3 — CalDAV Scheduling.** N/A — no screen, modal, pane, widget, or prompt exists or is authorized. The only user-visible artifact before implementation is the fact that preserved `ORGANIZER`/`ATTENDEE` lines render as read-only text in the TUI detail pane and the Quickshell card, labeled `attendees (read-only; scheduling not implemented)`. That label is mandatory: it is the §6.3 guard against implying an RSVP capability that does not exist.

**Feature I4 — broader packaging.** N/A — no application screen. Install transcripts belong to each distribution's package manager and are not authored by `mg-calr`; the post-install message text is H's, reused verbatim so that all four packaging targets say the same thing.

### 3.2 Interaction flows

**I1 primary flow — open, triage, act.**

1. `mg-calr tui` parses argv, resolves configuration, and validates terminal capability *before* any database connection (capability ladder in §3.4). Failure here exits without connecting.
2. Enter raw mode, enable the alternate screen, install a panic hook and a `SIGTERM`/`SIGHUP` handler that restore the terminal before propagating. Terminal restoration is unconditional: any exit path, including a panic, leaves the terminal in its entry state.
3. Issue exactly one D `execute_view` call for the initial range (today, in the effective zone). Render the first frame. The status bar shows `range`, `total`, and the D `snapshot_token` short form.
4. Keys are dispatched to the focused pane, then to the global map if unhandled. Navigation, selection, scrolling, and pane focus are pure in-memory operations over the already-materialized `QueryResult`; **they never re-query.**
5. A range change, filter change, search commit, or explicit `r` issues one new `execute_view`. While it is in flight the status bar reads `refreshing…` and the previous frame stays on screen, dimmed only in the status bar's freshness field — never in the data rows, which would encode state in color.
6. `e`, `x`, `X`, `D`, `s`, `A` open a confirm modal or an edit form, then call the corresponding application use case exactly as the CLI does. On success the TUI re-queries and reports the audit record ID in the status bar. On failure it renders the typed A4 error code and message in the status bar and changes nothing.
7. `q` from the root, or `:q`, restores the terminal and exits 0. `Esc` never quits: it dismisses the topmost overlay, and at the root it clears the active search.

**Branch — mutation under `--read-only`.** With `--read-only` (or `tui.read_only = true`), every mutating key is inert and reports `read-only session` in the status bar. This is the mode recommended for a shared or recorded screen.

**Branch — destructive action.** `D` (trash) opens a confirm modal naming the item's kind, state, title, and complete identity token, requiring a literal `y`. `n`, `Esc`, and `Ctrl-C` cancel. The underlying operation is G's soft delete; the status bar afterward names the `u` key and the undo token. There is no hard delete key in the TUI: purge remains a deliberate CLI command.

**Branch — external change during the session.** The TUI holds no long transaction and no subscription. Its data is a snapshot with an age. If a mutation's optimistic version check fails because another process changed the row, the use case returns the typed conflict error; the TUI re-queries, re-selects by full UUID, and reports `item changed since it was loaded — re-check before retrying`. It never retries automatically and never overwrites.

**I2 primary flow — glance, expand, act.**

1. At shell startup the widget runs `mg-calr contract describe --json` once. If `interface_version.major` is not the version the widget was written against, the pill enters `unconfigured` with the text `calendar interface v<n> unsupported` and makes no further calls.
2. On its poll timer the widget spawns `mg-calr agenda --json --from … --to … --no-input --no-color`, reads one JSON object from stdout, and reads the exit code.
3. Exit 0 → parse, render `ok` (or `empty` when `items` is `[]`), store the frame and its `generated_at`. Nonzero → render `error` with the A4 `code`, keep the last good frame **only** for the card's stale banner, and never present it as the pill's `ok` state.
4. Clicking the pill (or `Super+C`) opens the card, which immediately requests a fresh frame at the card interval and renders day sections plus a reminder section.
5. A card action spawns one short-lived mutating command (§4.3), shows a toast with the result, then forces one refresh. Actions are serialized: at most one mutating child at a time, and the action button is disabled while a child is running so a double-click cannot double-submit.
6. Closing the card returns the widget to the pill interval. Hiding the bar, locking the session, or a compositor idle signal suspends polling entirely.

**Branch — mg-calr missing or database down.** `command not found` and `database_unavailable` (exit 69) are distinct pill errors with distinct text. Neither is retried faster than the normal interval; there is no retry storm.

**Branch — ambiguous or stale selector on an action.** The widget always sends the full canonical UUID taken from the JSON `id`, never `short_id`. If the item disappeared between frames the action returns `selector_not_found` (exit 66); the toast says so and the card refreshes. No chooser ever opens in the widget: it always passes `--no-input`.

**I3.** N/A — deferred; no flow exists. The *forbidden* flows are specified instead, and are binding on branches A–H: importing a VEVENT carrying `METHOD:REQUEST` must not send anything; saving or editing an event with attendees must not send anything; and no timer, hook, or widget path may reach a scheduling outbox. §6.4 records the tests.

**I4.** N/A — install flows are `apt install`, `dnf install`, `nix profile install`, and `pacman -U`, each owned by its package manager. The only mg-calr-authored step is the same post-install message H already specifies.

### 3.3 Layout descriptions

**I1 workspace, ≥120 columns.** Top to bottom, leading to trailing:

```
┌ Navigator (24) ┬ Agenda (flex) ─────────────────────┬ Detail (40) ────┐
│ ‹ Nov 2026  ›  │ Sunday 2026-11-01                  │ Standup         │
│ Su Mo Tu We .. │  > [E] [ACTIVE] 01:30–01:45 -07:00 │ kind    event   │
│  1  2  3  4 .. │        America/Los_Angeles fold=0  │ state   ACTIVE  │
│  E  T  B  . .. │        Synthetic fold fixture      │ calendar Work   │
│                │        event:3f2a…                 │ start   01:30   │
│ scope: [week]  │    [T] [ACTIVE] DUE 09:00 -08:00   │ zone    America…│
│ kinds: E T     │        Synthetic due task          │ uid     fold-f… │
│ filters: 2     │        todo:91cc…                  │ attendees (read-│
│                │                                    │ only; scheduling│
│                │                                    │ not implemented)│
├────────────────┴────────────────────────────────────┴─────────────────┤
│ agenda 2/12 · 2026-11-01/2026-11-08 · fresh 3s · snap 7c1e · ready     │
└───────────────────────────────────────────────────────────────────────┘
```

- **Navigator pane.** Component types: a mini month grid (same Sunday-first, `. E T B` marker contract as D4) and a static scope/filter summary. Data source: the same `QueryResult` the agenda pane renders — the grid is derived, never separately queried, so grid and list cannot disagree. Empty month: grid still renders with all-`.` cells; there is no empty state.
- **Agenda pane.** A day-sectioned list. Each row reproduces the D §3.3 human row contract verbatim — kind token, state token, time token, offset + IANA zone + fold, title, complete identity token — because a TUI row that carried less information than a piped CLI row would be a regression, not a richer view. Data source: `QueryResult.items`. Selection is by full UUID plus occurrence identity, not by index, so a refresh that reorders rows keeps the same item selected. Empty state, centered in the pane: `No events or due todos.` on line one and the normalized range on line two, with `press F to change filters · r to refresh` beneath.
- **Detail pane.** A labeled two-column field list for the selected item: kind, state, calendar or project, start/end or due with zone and fold, RRULE summary (text, not re-parsed — supplied by B6), reminder definitions with delivery state, tags, notes, and — read-only — organizer and attendees with their preserved parameters. It does **not** render the opaque residual property store: displaying arbitrary preserved bytes in a terminal is both a control-sequence hazard and a privacy leak; a count and a digest are shown instead, with `mg-calr event show --raw` named as the deliberate way to see them. Empty state (no selection): `Select an item to see its detail.`
- **Status bar.** Fixed single line, always present, never color-only: focused pane name, `selected/total`, normalized range, data freshness in seconds, `snapshot_token` short form, and the most recent message or typed error code. This line is the TUI's equivalent of D's `RESULT` summary and is mandatory at every width.

**I1 at 80–119 columns:** the detail pane becomes an overlay opened by `Enter` rather than a column. At 60–79: the navigator collapses to a one-line header (`‹ 2026-11 › week · E T · 2 filters`) and the agenda occupies the full width with secondary context on indented continuation lines. At 40–59: titles are grapheme-ellipsized first; kind, state, time, offset, fold, identity token, and the status bar remain verbatim.

**I2 pill.** One row, leading to trailing: state glyph, relative time (`in 12m`, `now`, `2h ago`), ellipsized title, and a count badge (`+3`) when more items fall in the window. Maximum 320 px; the title is the only element allowed to shrink. Data source: the last successful `agenda --json` frame. Empty state: neutral glyph plus `No agenda`. Stale state: `!` glyph plus `stale 2h` — the age, not the item, is what the user needs. Error state: `×` glyph plus a short code-derived phrase (`calendar unavailable`), never a raw error string, never a path or URL.

**I2 card.** Top to bottom: header (date, IANA zone, `N events · M todos`), freshness line (`updated 14s ago · snapshot 7c1e`), day sections with the same row semantics as the TUI, a reminder section listing pending and snoozed deliveries with their scheduled time, an error banner slot, and a footer with the action row plus `Open in terminal` (which spawns the user's terminal running `mg-calr tui`, an explicit user action, never automatic). Empty state, centered: `No events or due todos in this window.` plus the normalized range. Stale state: a full-width banner above the list reading `Showing data from HH:MM (2h old) — last refresh failed: <code>`, with the list rendered at reduced emphasis *and* every row prefixed with a `stale` text token, because reduced emphasis is a color signal and cannot carry the meaning alone.

**I3 / I4.** N/A — no layout. When feature I3 lands, its inbox surface will be specified in its own spec; nothing here reserves screen space for it.

### 3.4 Input & gestures

**I1 key grammar.** The grammar is modal only in the sense that a pane owns its own keys; there is no vim-style operator-pending state and no multi-key chords beyond `g g`. Every binding is listed in the `?` overlay, grouped by the same pane headings.

| Scope | Key | Action |
|---|---|---|
| Global | `Tab` / `Shift-Tab` | focus next / previous pane |
| Global | `1` `2` `3` | focus navigator / agenda / detail directly |
| Global | `?` | help overlay; `?` or `Esc` closes |
| Global | `:` | command line |
| Global | `/` | search; `Enter` commits, `Esc` cancels and restores the prior result |
| Global | `F` | filter sheet |
| Global | `r` | explicit refresh (one new query) |
| Global | `u` | undo the last undoable mutation from this session (G2) |
| Global | `Ctrl-L` | full redraw |
| Global | `q` | quit from the root; inert while an overlay is open |
| Global | `Esc` | dismiss topmost overlay, else clear search; never quits |
| Navigator | `h` / `l` | previous / next period |
| Navigator | `[` / `]` | previous / next month |
| Navigator | `{` / `}` | previous / next year |
| Navigator | `t` | jump to today |
| Navigator | `d` `w` `m` | scope day / week / month |
| Agenda | `j` / `k`, `↓` / `↑` | move selection |
| Agenda | `g g` / `G` | first / last item |
| Agenda | `Ctrl-D` / `Ctrl-U` | half page down / up |
| Agenda | `n` / `p` | next / previous day section |
| Agenda | `Enter` | focus or open detail |
| Agenda | `e` | edit form |
| Agenda | `x` | complete todo (no-op with a message on an event) |
| Agenda | `X` | cancel event (confirm) |
| Agenda | `D` | trash (confirm; soft delete, undoable) |
| Agenda | `s` / `A` | snooze / dismiss the selected item's due reminder |
| Detail | `j` / `k` | scroll |
| Detail | `y` | print the complete identity token to the status bar for manual copy |
| Command | `:q` `:refresh` `:goto DATE` `:day` `:week` `:month` `:kind` `:filter` `:help` | typed equivalents of the above |

Command-line and search input support standard line editing plus bracketed paste. Bracketed paste is enabled **only** while a text input is focused, so a paste can never be interpreted as a destructive key sequence. Numeric prefixes, macros, and registers are deliberately absent: the C1 criterion rewards benchmark-speed keyboard flow *without* terse grammar.

**Mouse:** never required and off by default (`tui.mouse = false`). Enabling mouse reporting breaks a terminal's native text selection, which is a real accessibility and workflow regression; when enabled it adds only click-to-select and wheel-scroll, and every one of those actions has a key equivalent.

**Specialized input:** N/A — no stylus, controller, camera, or voice input. Voice and switch access reach both clients through the operating system's terminal and compositor accessibility stack, which is why every action has a key binding and an accessible name rather than a gesture.

**Terminal capability ladder (I1), evaluated before connecting:**

1. **Raw mode.** stdin and stdout must both be a TTY and the terminal must accept raw mode with resize events. `TERM=dumb`, `TERM` unset, or a non-TTY fails with `tui_unavailable` (exit 69) naming `mg-calr tui --line` and `mg-calr agenda --json`. There is no silent degradation.
2. **Size.** Width <40 → `terminal_too_narrow` (exit 64). Height <12 → `terminal_too_short` (exit 64). Otherwise the width tiers of §3.3 and: height 12–23 drops the navigator's grid to a header line; ≥24 renders the full layout.
3. **Color.** truecolor (`COLORTERM` in `truecolor,24bit`) → 256 (`TERM` contains `256color`) → 16 → none. `--no-color`, `NO_COLOR` (presence wins), and `TERM=dumb` force none. Because every state carries a text token, tier none loses no information.
4. **Glyphs.** UTF-8 locale → box drawing and marker glyphs; otherwise, or with `--ascii`, an ASCII-only border and marker set.
5. **Resize.** `SIGWINCH` re-lays out from the same `QueryResult`; a resize never re-queries and never changes selection.

**I2 input.** The pill is activatable by click and by the shell's keybind (`Super+C` in the reference configuration, user-rebindable). The card is fully keyboard operable: `Tab` moves between the list and the action row, `↑`/`↓` and `j`/`k` move the row selection, `Enter` opens detail, `c` completes, `s` snoozes, `Esc` closes and returns focus to the compositor's prior window. **Responsive behavior:** the card is capped at 420×520 logical pixels and scrolls internally; on a display whose scale factor makes 520 exceed 60% of the output height, the card switches to a full-height side panel. The pill never exceeds 320 px and never reflows the bar.

**I3 / I4.** N/A — no input surface.

### 3.5 Transitions & animation

**I1.** No animation of any kind. Frames are diffed and repainted; there is no fade, slide, spinner, or progress bar. An in-flight query is communicated by the word `refreshing…` in the status bar, not by a moving element. Overlays appear and disappear on a single frame. Consequently reduced-motion behavior is **identical** to default behavior and no alternative rendering path exists — this is a deliberate design choice, not an omission. `--screen-reader` additionally disables the alternate screen and full-frame repaint entirely (§3.7).

**I2.** The card open/close uses the shell's existing panel transition, capped at 120 ms, and is suppressed to an instant show/hide when the compositor or the shell reports a reduced-motion preference, or when `card.animate = false`. The pill itself never animates: no pulsing, no blinking, no marquee scroll of a long title, and no attention-grabbing motion when a reminder fires — a firing reminder is E's notification, and duplicating it as pill motion would be a second delivery surface with its own idempotency problem. The badge count changes without transition.

**I3 / I4.** N/A — no rendering surface.

### 3.6 Error states

| Feature | Trigger | Presentation | Recovery | Data loss |
|---|---|---|---|---|
| I1 | not a TTY / `TERM=dumb` / raw mode refused | typed `tui_unavailable`, exit 69, on stderr before any connection — a modal is impossible with no TUI | use `--line`, or `agenda --json` | none |
| I1 | width <40 or height <12 | `terminal_too_narrow` / `terminal_too_short`, exit 64, stderr | resize, or use line mode | none |
| I1 | database unreachable at startup | typed `database_unavailable`, exit 69, stderr, URL redacted; the TUI never starts on empty data | fix the local server, retry | none |
| I1 | database fails mid-session on refresh | status-bar error line with the code; last good frame retained and its freshness age keeps counting up | `r` to retry, `q` to leave | none — read path |
| I1 | mutation rejected (validation, conflict, ambiguity) | status-bar typed code + message; the edit form stays open with the entered values | correct and resubmit, or `Esc` | none — transaction aborted |
| I1 | optimistic version conflict | confirm modal replaced by a message naming the item and telling the user to re-check; **no overwrite offered** | re-query, inspect, act again | none |
| I1 | panic in rendering | panic hook restores the terminal, then the normal Rust panic path; exit 101 | report defect | none — no write is in flight during render |
| I1 | `Ctrl-C` | restore terminal, exit 130; any open transaction rolls back | rerun | none |
| I2 | `mg-calr` not on `PATH` | pill `error` + `calendar not installed`; card banner names the expected command | install the package | none |
| I2 | nonzero exit from a read command | pill `error` + short phrase from the A4 code; card banner shows code and message | fix per the code; next tick retries | none |
| I2 | frame older than `stale_after` (default 300 s) | pill `stale` + age; card stale banner + per-row `stale` token | refresh, or fix the underlying error | none |
| I2 | unknown `interface_version.major` | pill `unconfigured`; **all further calls suppressed** | update the widget or mg-calr | none — no call is made |
| I2 | malformed or over-size JSON on stdout | treated as an error frame; the payload is discarded and never partially parsed; size cap 4 MiB | report defect | none |
| I2 | action returns `selector_not_found` (66) | toast `item no longer exists`; card force-refreshes | retry against the new list | none |
| I2 | action returns `selector_ambiguous` (66) | toast `ambiguous — resolve in the terminal`; **widget never chooses** | act in the TUI or CLI | none |
| I2 | a mutating child exceeds 10 s | child killed with `SIGTERM` then `SIGKILL`; toast `action timed out — check state in the terminal`; **not retried** | verify with `mg-calr` and act | none — the use case is transactional; a killed client cannot half-commit |
| I3 | any pre-implementation path that would send a scheduling message | hard typed error `scheduling_not_implemented`, exit 78; nothing sent | none required; the operation is not offered | none |
| I4 | distro package with an unresolved `LICENSE` | build/publish gate fails, per H's `license-gate` | resolve H Q1 | none |

Every error path in both clients is non-mutating unless the user explicitly invoked a mutating action, and no error message may contain a database URL, connection string, SQL, credential, residual property value, or unescaped user text.

### 3.7 Accessibility

**I1.**

- **Screen reader.** `--screen-reader`, `MG_CALR_SCREEN_READER=1`, or `tui.screen_reader = true` disables the alternate screen and frame diffing and switches to append-only announcement lines. Every state change emits exactly one line: `agenda: item 3 of 12 selected: [E] [ACTIVE] 09:00–09:30 -08:00 America/Los_Angeles, Standup, event:3f2a…`; a focus change emits `focus: detail pane`; an overlay emits its title and item count on open and `closed: filter sheet` on close. Full-frame repaint is what makes a conventional TUI unusable with a screen reader, so this mode does not exist as a courtesy — it is the supported nonvisual path, and it is exercised by a PTY test.
- **Labels, hints, traits.** Every pane announces its name and its item count on focus. Every row announces kind, state, time with zone, title, and identity. Every modal announces its title, its required key (`press y to confirm, n or Esc to cancel`), and its default. No control is identified by position alone.
- **Custom actions.** Complex interactions — snooze with a duration, filter with several fields — are decomposed into single-key actions with spoken confirmations rather than requiring a pointer-driven composite gesture. The chooser reuses D10's numbered-candidate contract, which is already specified as screen-reader safe.
- **Text scaling.** Terminal font size is the user's; the TUI reads only cell counts. Because the width and height ladders degrade to 40×12, a user at a very large font on a 1080p display still gets a fully functional layout. Nothing is laid out in pixels.
- **Color independence.** Kind, state, blocked, completed, cancelled, stale, focus, selection, and current date all have text or symbol carriers. Focus is `>` plus the word `selected` in screen-reader mode. Color, when present, is redundant. `NO_COLOR` presence wins over config.
- **Focus order and keyboard navigability.** Focus order equals visual order: navigator → agenda → detail → (overlay when open). Focus is never trapped except deliberately inside a modal, which always has a documented dismissal key. There is no action reachable only by mouse.
- **Control-sequence safety.** All user text — titles, notes, locations, attendee display names, and every preserved parameter value — is escaped before rendering. ESC, CSI, OSC, C0/C1 controls, and bidi overrides cannot alter terminal structure. Width is computed in grapheme display cells.

**I2.**

- Every pill and card element exposes an accessible name and role through the shell's accessibility surface: the pill is a button named `Calendar: <state>, <next item>, <relative time>`; each card row is a list item whose name is the same semantic sequence the TUI announces; each action is a button with an explicit verb name (`Complete "Standup"`, not `✓`).
- Every state — including `stale` and `error` — is announced as text; the reduced-emphasis styling of a stale list is redundant with a per-row `stale` token.
- The card honors the shell's font scale; at large scales rows wrap rather than truncate, and the identity token and time token are never the elements that get dropped.
- The card is fully keyboard operable and does not steal focus when it opens on a timer — it only takes focus on explicit user activation.
- **Privacy is an accessibility-adjacent requirement here.** `pill.privacy` accepts `titles` (default), `counts` (renders `3 items` with no titles), or `hidden`. A shell keybind toggles to `counts` instantly. The pill and card must be excluded from lock-screen and screenshot layers by the shell's layer rules; the widget must not render on a locked session.

**I3 / I4.** N/A — no interactive surface. The read-only attendee label required in §3.1 must be part of the announced text, not a visual-only affordance, so that a nonvisual user is not left to infer that RSVP is unavailable.

---

## 4. Implementation Specification

### 4.1 Architecture placement

Target placement, given today's flat `src/*.rs` layout and D/E/F's already-specified moves:

- `src/tui/mod.rs` — application root, event loop, terminal lifecycle (raw mode, alternate screen, panic/signal restoration).
- `src/tui/state.rs` — pure workspace state: focus, selection (by full identity, not index), scope, filters, overlay stack, message log. No I/O, no clock, no SQL.
- `src/tui/keymap.rs` — the §3.4 grammar as data, so the `?` overlay and the man page render from one table.
- `src/tui/render/{agenda,navigator,detail,status,overlay}.rs` — pure functions from `(&QueryResult, &TuiState, Caps)` to a frame. No repository access.
- `src/tui/ports.rs` — the *only* way `src/tui/` reaches data: `AgendaQueryPort`, `MutationPort`, `ReminderActionPort` trait objects, implemented in `src/application/` and injected at `src/main.rs`. `src/tui/` may not `use crate::storage`, may not name `tokio_postgres`, and may not contain a SQL string literal.
- `src/tui/line.rs` — today's prototyped line shell, preserved verbatim as the `--line` fallback.
- `src/cli/watch.rs` — the optional `mg-calr watch` streaming command (feature I2).
- `src/cli/contract.rs` — `mg-calr contract describe`, printing `include_str!` of the manifest.
- `contracts/client-interface-v1.json` + `contracts/client-interface-v1.lock` — the public client interface manifest and its frozen digest.
- `integrations/quickshell/` — `CalendarPill.qml`, `CalendarCard.qml`, `CalendarService.qml`, `README.md`. First-party QML, vendored in this repository so mg-calr's own test suite can assert what it does and does not do. Not installed by the Arch package by default; installed under `/usr/share/mg-calr/quickshell/` as documentation-grade example integration.
- `src/schedule/` — **does not exist and must not be created** before feature I3 is scheduled.
- `packaging/{arch,debian,fedora,nix}/` — H owns `arch/`; feature I4 adds the other three.

Layering rules, binding on branches A–H: `tui` depends on `application` and `domain` only. `application` never depends on `tui`. `storage` never emits display strings. The Quickshell integration is a separate process tree and depends on nothing but the `mg-calr` executable on `PATH`.

### 4.2 Data model

**Feature I1 — no database change.** The TUI adds no table, column, index, or migration. Its state is process-local:

```rust
/// Complete workspace state for one TUI session. Pure: no clock, no I/O, no SQL.
pub struct TuiState {
    /// Focused pane; also determines which keymap layer receives a key first.
    focus: Pane,
    /// Selection by complete projection identity, never by row index, so a
    /// refresh that reorders or inserts rows keeps the same item selected.
    selected: Option<ProjectionVersion>,
    /// Scroll offset in rows, clamped on every relayout.
    offset: usize,
    /// Requested period and scope; a change here is the only thing besides `r`
    /// and a committed filter/search that may trigger a new query.
    scope: Scope,
    filters: ViewFilters,
    search: Option<SearchTerm>,
    /// Modal stack; `Esc` pops exactly one.
    overlays: Vec<Overlay>,
    /// Most recent message or typed error code shown in the status bar.
    message: StatusMessage,
    /// True only while a query is in flight; drives the word "refreshing…".
    refreshing: bool,
    /// Undo token from the last mutation this session, for the `u` key.
    last_undo: Option<UndoToken>,
}
```

Optional non-authoritative UI preferences persist to `$XDG_STATE_HOME/mg-calr/tui-state.toml` (last scope, last filters, pane widths). This file is explicitly **not** a data cache: it holds no event, todo, title, identity, or timestamp of user content. A corrupt or unreadable file is ignored with a status-bar note and defaults are used — it can never fail a session or become an authority.

**Feature I2 — no database change.** The wire types are the frame envelope in §4.3. The widget's only persistence is the shell's own `localStorage`-equivalent holding the last frame for the stale banner and the user's `pill.privacy` choice. Storing the last frame is permitted **only** because §3.3 requires it to be rendered as labeled stale data with an age; presenting it as current is forbidden and is asserted by a widget test.

**Feature I3 — deferred; sketched so that F's schema is forward-compatible.** No migration is authorized now. When scheduling lands it will need, at minimum, a `scheduling_messages` ledger (`id uuid PK`, `event_id uuid`, `direction text CHECK (direction IN ('inbound','outbound'))`, `method text`, `sequence int`, `state text`, `raw_bytes bytea`, `source_byte_fp bytea`, `created_at timestamptz`, `UNIQUE (event_id, direction, method, sequence)`), and a `schedule_status` carrier per calendar user. What is binding **now** is that F's `event_calendar_users` table (§4.2 of `specs/f-import-export-sync.md`) must preserve, verbatim and in order, every parameter of `ORGANIZER` and `ATTENDEE` — including `PARTSTAT`, `RSVP`, `ROLE`, `CUTYPE`, `MEMBER`, `DELEGATED-TO`, `DELEGATED-FROM`, `SENT-BY`, `CN`, `DIR`, `LANGUAGE`, every `X-` parameter, and the three RFC 6638 parameters `SCHEDULE-AGENT`, `SCHEDULE-STATUS`, and `SCHEDULE-FORCE-SEND` — plus `raw_line` bytes. `METHOD`, `REQUEST-STATUS`, and whole `VFREEBUSY` components must land in F's residual store with their ordinal, folding, and digest. **mg-calr must never write a `PARTSTAT` locally, and must never synthesize an `ORGANIZER`, before feature I3 is implemented and explicitly invoked.**

**Feature I4 — no data model.** N/A.

### 4.3 API contracts

This subsection is the load-bearing part of the branch. The claim "I consumes stable application/JSON interfaces and never reads PostgreSQL directly" is made concrete here as a manifest, a stability guarantee, and a test.

**The client interface surface (`contracts/client-interface-v1.json`).** One JSON document, compiled into the binary with `include_str!` and printed verbatim by `mg-calr contract describe --json`. Each entry:

```json
{
  "id": "agenda.read",
  "argv": ["agenda", "--json", "--from", "<DATE>", "--to", "<DATE>",
           "--timezone", "<IANA>", "--no-input", "--no-color"],
  "stability": "stable",
  "mutating": false,
  "network": false,
  "schema_ref": "contracts/view-result-v1.schema.json",
  "exit_codes": [0, 64, 65, 66, 69, 70],
  "since": "0.1.0",
  "deprecated_since": null
}
```

The v1 surface is exactly:

| id | argv shape | mutating | Purpose |
|---|---|---|---|
| `contract.describe` | `contract describe --json` | no | interface version + capability list; the widget's first call |
| `agenda.read` | `agenda --json --from --to --timezone --no-input --no-color` | no | D9 agenda projection (pill and card) |
| `search.read` | `search <QUERY> --json --from --to --no-input` | no | TUI search overlay |
| `event.show` | `event show <UUID> --json --no-input` | no | detail pane / card detail |
| `todo.show` | `todo show <UUID> --json --no-input` | no | detail pane / card detail |
| `remind.list` | `remind list --json --state pending,snoozed --no-input` | no | card reminder section |
| `agenda.watch` | `watch agenda --json-lines --from --to --interval <D>` | no | optional streaming (§below) |
| `todo.complete` | `todo complete <UUID> --json --no-input` | **yes** | card and TUI action |
| `event.cancel` | `event cancel <UUID> --json --no-input` | **yes** | TUI action |
| `event.trash` | `event trash <UUID> --json --no-input` | **yes** | TUI action (soft delete, undoable) |
| `remind.snooze` | `remind snooze <UUID> --for <DURATION> --json --no-input` | **yes** | card and TUI action |
| `remind.dismiss` | `remind dismiss <UUID> --json --no-input` | **yes** | card and TUI action |
| `undo.apply` | `undo <TOKEN> --json --no-input` | **yes** | TUI `u` key |

Every entry takes and returns only the A4 envelope: `{"schema_version":1,"command":…,"ok":true, "data":{…}}` on stdout, `{"schema_version":1,"ok":false,"error":{"code":…,"message":…}}` on stderr, and an exit code from the `contracts/error-v1.json` table. Every entry passes `--no-input`, so no client can ever be blocked on a prompt or served a chooser. Clients always send the full canonical UUID from `data.items[].id`; `short_id` is presentation-only and passing it is a client defect.

**Stability guarantee.** Within `schema_version: 1`, an entry marked `stable` guarantees: the argv shape accepts the same arguments; the JSON field names, types, enum values, null-vs-absent policy, and documented field order do not change; and each listed exit code keeps its meaning. Additions are permitted — new optional JSON fields, new flags with defaults that preserve current behavior, new entries. Removing an entry or a field, narrowing an enum, changing a type or meaning, or repointing an error code requires `schema_version: 2` **and** a deprecation window of at least one minor release during which the old entry keeps working with `deprecated_since` set and `mg-calr doctor --check clients` emits a warning row. An entry marked `provisional` (the `agenda.watch` stream starts here) carries no such guarantee and must be advertised as such by `contract describe`; a client must treat a provisional entry as optional.

**Streaming contract (`agenda.watch`, provisional).** `mg-calr watch agenda --json-lines` writes newline-delimited JSON to stdout, one object per line, flushed per frame:

```json
{"schema_version":1,"stream":"agenda","stream_version":1,"seq":7,
 "emitted_at":"2026-11-01T08:00:12Z","reason":"changed",
 "content_digest":"<64 hex>","data":{ /* identical to agenda.read data */ }}
```

Rules: `seq` is monotonic from 0 within a process. The first frame is always `reason:"initial"`. Subsequent data frames are emitted only when `content_digest` changes. A `reason:"heartbeat"` frame with `data:null` is emitted every `heartbeat` seconds (default 60) so a client can distinguish "nothing changed" from "the producer died". `SIGTERM`/`SIGINT` emits a final `reason:"shutdown"` frame and exits 0; `EPIPE` on stdout exits 0 silently without a retry loop. `--interval` below 5 s is rejected with exit 64. The command's only I/O is the configured local PostgreSQL Unix socket, the todo projection file, and stdout: **it opens no listening socket, owns no D-Bus name, and is not a daemon** — its lifetime is the parent client's, and the client, not mg-calr, supervises it. Change detection is a cheap digest query on the same interval; no trigger, `LISTEN/NOTIFY` channel, or background writer is introduced. (A `LISTEN/NOTIFY` optimization would require an emitter and is explicitly out of scope; it is Q4 in §8.)

**Application interface consumed by the in-process TUI.** The TUI never sees a repository:

```rust
/// The only data surface `src/tui/` may reference. Implemented in
/// `src/application/`; injected at the composition root in `src/main.rs`.
#[async_trait]
pub trait AgendaQueryPort {
    async fn view(&self, query: ViewQuery) -> Result<QueryResult, AppError>;
    async fn detail(&self, id: EntityId) -> Result<DetailResult, AppError>;
}

/// Mutations reach exactly the same use cases the CLI calls, so a TUI edit and a
/// CLI edit share one transaction, one audit record, and one undo token.
#[async_trait]
pub trait MutationPort {
    async fn apply(&self, intent: MutationIntent) -> Result<MutationOutcome, AppError>;
}
```

`MutationIntent` is a closed enum whose variants correspond one-to-one with the mutating manifest entries. It carries the expected `source_revision` for the optimistic check, so a TUI mutation is structurally incapable of an unconditional overwrite. Auth: none beyond the invoking user's own PostgreSQL peer identity — no client is given a connection string, and none is issued a token. Rate limiting: N/A for in-process calls; the widget self-limits by interval and by the one-mutating-child-at-a-time rule in §3.2. Pagination: N/A — clients hold a whole bounded result, per D's snapshot-bound pagination rule.

**Feature I3 API.** N/A — deferred, no endpoint, no function, no flag. When it lands it must be reachable only from explicit `mg-calr schedule …` / `mg-calr sync …` invocations behind the same `sync-transport` feature flag F defines, and must appear in the manifest with `"network": true` — the only entries that ever may. A local, network-free `mg-calr freebusy --json --from --to` that computes busy intervals from the local database and can emit a `VFREEBUSY` component is *separable* from I3 and may land earlier under F's codec; it is named here so that "free-busy" is not assumed to require a network.

**Feature I4 API.** N/A — packaging exposes no runtime interface.

### 4.4 State management

PostgreSQL remains the single authority (Lens-3 I2). Neither client owns durable domain state.

- **Ownership.** The TUI session owns `TuiState` for one process lifetime; it is rebuilt from scratch on every launch. `QueryResult` is immutable and replaced wholesale on refresh. The Quickshell widget owns one last-frame value and one privacy preference.
- **New state container.** `TuiState` is the only new container, injected nowhere: it is created in `src/tui/mod.rs` and passed by `&mut` to the pure key handler and by `&` to the pure renderers. There is no global, no singleton, no lazily initialized cache, and no state that outlives the process besides the non-authoritative UI preferences file.
- **Local vs. server-synced boundary.** Everything a client displays came from one `execute_view` snapshot with a `snapshot_token` and a `generated_at`. Nothing is merged across snapshots. Selection survives a refresh by re-locating the same complete identity; if that identity is gone, the selection clears and the status bar says so rather than silently sliding to a neighbor.
- **Offline / draft persistence.** There is none, deliberately. An in-progress TUI edit form is ephemeral: cancelling, quitting, crashing, or losing the database discards it, exactly as B specifies for CLI prompts. A queued-offline mutation would be a second authority with its own conflict policy and is forbidden. The Quickshell widget likewise queues nothing: a failed action is reported and dropped, never retried in the background.
- **Freshness.** Every rendered surface carries an age. A frame older than `stale_after` is labeled `stale` in both clients. Stale data is never presented as current — that is the concrete client-side expression of the "no silent loss / no unconfirmed overwrite" auto-fail posture.

### 4.5 Dependencies

**Feature I1 (deferred additions, subject to license and maintenance review):** `ratatui` and `crossterm` (both MIT) for terminal rendering and raw-mode/event handling, plus the grapheme/display-width crate D8 already selects — reused, not duplicated. `unsafe_code = "forbid"` in `Cargo.toml` stays; a dependency requiring its relaxation is rejected. No new database, HTTP, TLS, DNS, D-Bus, or async runtime dependency: the TUI runs on the existing `tokio` runtime.

**Feature I2:** **zero new Rust dependencies.** `watch` uses `tokio` timers and `serde_json`, already present. The QML side depends on the user's existing Quickshell and Qt; those are runtime prerequisites of the shell, not of `mg-calr`, and the Arch package lists them at most as `optdepends`.

**Feature I3 (deferred):** would introduce mg-calr's **first** HTTP/TLS client, which is a significant architectural change — F's design deliberately spawns `vdirsyncer` and opens no socket itself, and `vdirsyncer` does not implement RFC 6638. Any such crate must be feature-gated behind `sync-transport`, added as an explicit allowlist entry in the CI dependency check H §5.2 job 9 already runs, and reviewed for TLS configuration, certificate validation, and redirect policy. It is Q1 in §8 and is not decided here.

**Feature I4:** build-time only — `cargo-deb` or hand-written `debhelper` rules, `rust2rpm` or a hand-written `.spec`, and a Nix flake using `rustPlatform.buildRustPackage`. No runtime dependency is added by any of them.

**Assets:** none bundled. The Quickshell widget uses the shell's existing theme font and its already-present icon glyph set; no font, image, or icon file is added to this repository. The TUI ships no asset at all.

**Infrastructure:** none. No CDN, no service, no daemon, no systemd unit added by branch I. E's reminder user unit remains E's and remains opt-in.

### 4.6 Platform-specific considerations

- **Compositor / display server.** Feature I2 targets Wayland under Hyprland with Quickshell as the shell. It uses `wlr-layer-shell` positioning through Quickshell's own abstractions; it does not talk to Hyprland IPC except to read the reduced-motion and idle/lock signals the shell already exposes. X11 is untested and unclaimed. A different Wayland shell (waybar, ags) could consume the same JSON contract — that is the point of specifying an IPC surface rather than a widget API — but only the Quickshell integration is first-party.
- **Terminal emulators.** Feature I1 targets foot, kitty, alacritty, and `xterm-256color`-class emulators under a UTF-8 locale. The capability ladder in §3.4 makes every other terminal either work in a reduced tier or fail with a typed error; there is no untested middle.
- **Version compatibility.** Rust edition 2024 with `rust-version = "1.85"`. PostgreSQL compatibility is A2's, unchanged — branch I adds no SQL. Quickshell's API is fast-moving, so the widget records the Quickshell and Qt versions it was developed against in `integrations/quickshell/README.md` and the widget checks `contract describe` before rendering so an mg-calr/widget version skew degrades to `unconfigured` rather than misrendering.
- **Feature flags and rollout.** `mg-calr tui` ships the `--line` fallback in the same release as the full-screen mode, so a regression has an immediate escape hatch. `watch` ships as `provisional` in the manifest. Feature I3, if ever built, is behind `sync-transport` and default off. The Quickshell integration installs as example integration under `/usr/share/mg-calr/`, never auto-enabled in anyone's shell configuration.
- **Distribution matrix (feature I4).** The concrete widening requirements, beyond H's Arch baseline:

| Target | Mechanism | Real requirements beyond H |
|---|---|---|
| Debian/Ubuntu | `.deb` via `cargo-deb` or `debhelper` | `debian/{control,rules,changelog,copyright}`; DEP-5 copyright covering every vendored crate; `lintian` clean; **rustc ≥ 1.85 for edition 2024** — Debian 12 ships 1.63, so trixie/backports or a rustup toolchain is required, and official archive inclusion additionally requires every dependency packaged in Debian, which this dependency graph does not satisfy. **v1 target is an unofficial `.deb`, not archive inclusion**, stated honestly in `docs/PACKAGING.md`. |
| Fedora/RHEL | `.spec` via `rust2rpm` | bundled `Provides:` for the crate graph, `%license` (gates on H Q1), `%check` running `cargo test` without a database, `rpmlint` clean; Copr for distribution, not Fedora proper, until the packaging guidelines' crate-packaging rules are met |
| Nix / NixOS | flake, `rustPlatform.buildRustPackage` | `cargoHash` pinned and CI-refreshed; a Home Manager module exposing `programs.mg-calr.{enable,settings}` that writes the XDG TOML — **and nothing else**; the module may not create a database, run a migration, or enable a systemd unit |
| Arch | H's PKGBUILD | unchanged; remains the reference |

Cross-cutting rules binding on all four: the same file layout H fixes (`/usr/bin`, completions, man1/man5, `/usr/share/doc/mg-calr/`, **no `/etc`**); no maintainer script may run `sudo`, `systemctl`, `initdb`, `createuser`, `createdb`, `psql`, or `mg-calr database migrate` — the post-install message tells the user to run migrations themselves, unprivileged; no package enables a unit; and every target's `license` field gates on H Q1 being resolved. A single `tests/packaging_contract.rs` assertion set is extended to scan all four packaging directories for the forbidden verbs, so widening the distro list cannot quietly widen the privilege surface.

### 4.7 Performance budget

Measured on the documented reference workstation with warm caches, against D's 100,000-item synthetic corpus.

**Feature I1.**
- Startup to first frame ≤300 ms total, of which the D query is ≤150 ms; terminal setup ≤10 ms.
- Keypress to repaint p95 ≤16 ms and max ≤33 ms for a 500-item result, because navigation never re-queries. A key that does re-query (`r`, scope change, committed filter/search) is bounded by D's budgets and shows `refreshing…` within one frame.
- `SIGWINCH` relayout ≤16 ms; it re-renders from the held result and never re-queries.
- Resident memory ≤64 MiB at the 5,000-item maximum, ≤24 MiB at the 500-item default.
- **Idle CPU is 0%**: the loop blocks on the terminal event stream. The single exception is an optional one-per-minute tick to refresh relative-time strings, which repaints only the affected cells and is disabled in screen-reader mode.
- No network payload, no persistent storage beyond the ≤4 KiB preferences file.

**Feature I2.**
- Pill poll default 60 s; card-open poll 15 s; both suspended when the bar is hidden, the session is locked, or the compositor reports idle. Never below 5 s.
- One `agenda --json` process per tick, ≤60 ms warm wall time, ≤2 processes ever concurrent, never more than one *mutating* child at a time.
- JSON payload ≤120 KiB for a 7-day window; the widget rejects >4 MiB outright.
- `watch` idle resident memory ≤24 MiB, one PostgreSQL connection, digest query ≤20 ms, at most one frame per interval plus one heartbeat per `heartbeat` period.
- Widget memory in the shell process ≤16 MiB including the last-frame cache; the cache is capped at one frame.
- No network payload at all.

**Feature I3.** N/A until implemented. When specified, its budget must be expressed per *explicit invocation*, never as a background rate, because a background rate would presuppose exactly the periodic network access §6.4 forbids. The separable local free-busy computation is bounded by D's query budgets.

**Feature I4.** Build-time only: ≤10 min clean container build per distro target on CI; artifact ≤6 MiB per package; no runtime, memory, or startup impact — the binary is identical.

---

## 5. Test Specification

### 5.1 Unit tests

- `tui_state_selection_survives_refresh_reorder` — setup: a `QueryResult` with items A,B,C, B selected; apply a refresh whose result is C,B,A. Assert selection is still B by identity. Edge: the selected identity is absent in the new result → selection clears and a message is set; it must **not** slide to a neighbor.
- `tui_state_navigation_never_requests_a_query` — drive every navigation, focus, scroll, and overlay key through the state machine with a port that panics on `view()`. Assert no call. Edge: `Ctrl-D` at the last page.
- `tui_esc_pops_one_overlay_and_never_quits` — push help, filter, confirm; three `Esc` presses pop three overlays and `should_quit()` is still false. Edge: `Esc` at the root clears search only.
- `tui_capability_ladder_is_total_and_ordered` — table-driven over (`is_tty`, `TERM`, `COLORTERM`, width, height, locale): assert exactly one outcome per row and that every failure is a typed code, never a degraded render. Edge: width 39, height 11, `TERM=dumb` on a TTY.
- `tui_width_tiers_preserve_mandatory_tokens` — render one row at 40, 59, 60, 79, 80, 119, 120 columns; assert kind, state, time, offset, zone, fold, and identity token appear verbatim at every width and only the title is ellipsized.
- `tui_escapes_terminal_control_sequences` — titles containing ESC, CSI, OSC 8, bidi overrides, and a NUL render as escaped text; assert the frame contains no raw control byte and the column count is unchanged.
- `tui_mutation_intent_always_carries_expected_revision` — every `MutationIntent` variant, by construction, requires a `source_revision`; a compile-fail test (`trybuild`) asserts an unconditional variant cannot be constructed.
- `keymap_table_has_no_duplicate_binding_per_scope` — assert the §3.4 table is injective within each scope and that every entry has help text.
- `watch_frame_is_emitted_only_on_digest_change` — feed a fake port a repeated identical result; assert exactly one `initial` frame plus heartbeats, and one `changed` frame when the digest moves.
- `watch_rejects_interval_below_floor` — `--interval 1s` exits 64 before connecting.
- `pill_state_selection_is_total` — the five pill states are chosen by a pure function of (exit code, item count, frame age); assert every input class maps to exactly one state and that a nonzero exit can never yield `ok`.
- `stale_frame_is_never_rendered_as_current` — a frame older than `stale_after` yields `stale` with an age, in both pill and card, with a per-row token. Edge: age exactly at the threshold.

### 5.2 Integration tests

- `client_interface_manifest_entries_all_exist` — parse every `argv` template in `contracts/client-interface-v1.json` against the real Clap command tree. A manifest entry naming a nonexistent command or flag fails the build. **This is the test that makes A–H unable to break the boundary silently.**
- `client_interface_stable_entries_are_frozen` — a checked-in `contracts/client-interface-v1.lock` digest over the `stable` subset. Any change fails until the lock is regenerated with a note; a *removal* within `schema_version: 1` fails unconditionally.
- `client_interface_outputs_are_schema_valid` — run every non-mutating entry against a disposable database seeded with the D fixture corpus and validate stdout against its `schema_ref`.
- `client_interface_non_mutating_entries_change_nothing` — dump the disposable database before and after running every `mutating:false` entry; assert byte equality including audit tables.
- `client_interface_makes_no_network` — run every entry inside `unshare -rn` with `/run/postgresql` bind-mounted. All must succeed. Any entry with `"network": true` (none in v1) would be excluded by an explicit allowlist, not by omission.
- `quickshell_sources_contain_no_database_access` — scan `integrations/quickshell/**` for `postgres`, `postgresql://`, `libpq`, `psql`, `PGHOST`, `PGPASSWORD`, `PGSERVICE`, `.pgpass`, `DATABASE_URL`, Qt's `LocalStorage`/`QSqlDatabase` imports, and SQL keyword sequences. Any hit fails. **This is the executable form of "never reads PostgreSQL directly."**
- `quickshell_sources_only_invoke_manifest_commands` — extract every `mg-calr` argv literal from the QML/JS and assert each matches a `stable` manifest entry. An ad-hoc flag fails the suite.
- `tui_module_has_no_storage_dependency` — scan `src/tui/**` for `crate::storage`, `tokio_postgres`, and SQL keyword sequences; assert none. Complemented by `tui_runs_entirely_against_a_fake_port`, which drives a full scripted session — navigate, filter, search, edit, complete, undo, quit — against an in-memory `AgendaQueryPort`/`MutationPort` with no database process running at all.
- `tui_mutations_produce_the_same_audit_record_as_the_cli` — perform the same edit through the CLI and through the TUI port against a disposable database; assert the resulting rows and audit records are identical apart from the audit's recorded interface field.
- `tui_restores_the_terminal_on_every_exit_path` — PTY test covering clean quit, `Ctrl-C`, `SIGTERM`, `SIGHUP`, and an induced panic; assert the terminal is out of raw mode and off the alternate screen in all five.
- `watch_shutdown_frame_is_last_and_epipe_is_quiet` — send `SIGTERM`; assert a final `reason:"shutdown"` frame then exit 0. Separately close the read end; assert exit 0, no stderr spew, no retry loop.
- `scheduling_is_not_implemented_and_sends_nothing` — import the Apple `.ics` invitation corpus (`METHOD:REQUEST`, `ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE`, `SCHEDULE-AGENT=SERVER`), re-export, and assert byte identity of every calendar-user line and residual entry; assert no `PARTSTAT` was written locally, no `ORGANIZER` synthesized, and — running inside `unshare -rn` — that nothing attempted a connection. Assert `mg-calr schedule …` does not exist as a command.
- `packaging_scriptlets_are_unprivileged_in_every_target` — extend `tests/packaging_contract.rs` to scan `packaging/{arch,debian,fedora,nix}` for `sudo`, `systemctl`, `initdb`, `createuser`, `createdb`, `psql`, and `database migrate`; assert no target installs under `/etc` and no target enables a unit.

### 5.3 UI / E2E tests

- **TUI PTY suite** (`expectrl`-driven, deterministic 100×30 and 45×14 PTYs, fixed clock, fixed zone `America/Los_Angeles`, DST fold fixture): open → navigate to the fold pair → confirm both 01:30 rows render with distinct offsets and `fold=0`/`fold=1` → `/` search → commit → `F` filter → complete a todo → observe the status bar undo token → `u` → verify the item is back → `q`. Frames are captured and compared against goldens with only the freshness field and snapshot token substituted.
- **TUI error-recovery E2E:** start with the database up, stop it mid-session, press `r`; assert the last good frame stays, the status bar shows `database_unavailable`, the freshness age climbs, and no panic. Restart, press `r`; assert recovery.
- **TUI screen-reader E2E:** run with `--screen-reader` under a PTY; assert append-only output, one announcement line per state change, no alternate-screen escape sequence emitted at all, and that the announced text for each row contains kind, state, time, zone, title, and identity.
- **Quickshell widget E2E** (headless Quickshell in a nested compositor, `mg-calr` replaced by a scripted stub that returns canned envelopes and exit codes): assert each of the five pill states renders with its glyph *and* text; assert an `error` frame never replaces `ok` content with unlabeled stale data; assert the card is reachable and fully operable by keyboard; assert an action disables its button until the child exits; assert a 10 s timeout kills the child and toasts; assert unknown `interface_version.major` suppresses all further calls.
- **Widget privacy E2E:** toggle `pill.privacy` to `counts` and assert no title text appears in the rendered scene graph; assert the widget renders nothing when the shell reports a locked session.

### 5.4 Visual / manual verification

- **Theme variants:** TUI at truecolor, 256-color, 16-color, and `NO_COLOR` on both a light and a dark terminal profile — confirm every state is legible and that removing color removes no information. Quickshell pill and card against the light and dark ends of the workstation's theme set, confirming the card's stale and error banners meet contrast in both.
- **Text size extremes:** terminal font at the smallest and largest practical sizes (yielding roughly 200×60 and 45×14 cells) — confirm the width/height ladder produces the intended tiers. Quickshell at 100% and 200% font scale — confirm card rows wrap rather than dropping the time or identity token.
- **Screen size extremes:** 1080p single output and a 4K output at scale 2 — confirm the pill never reflows the bar and the card converts to a side panel where specified.
- **Empty vs. populated:** empty day, single item, 500 items, and the 5,000-item maximum in both clients; plus the stale, error, and `unconfigured` states, which must each be reachable by deliberate manual setup (stop the database; rename the binary; bump the interface version).
- **Fold and gap days** rendered manually in both clients on 2026-11-01 and 2026-03-08 to confirm the offset/fold disambiguators are visible without color and without truncation.

---

## 6. Compliance & Safety Gate

### 6.1 Sensitive data classification

- [ ] No sensitive data involvement
- [x] **Handles sensitive data.** Event titles, descriptions, locations, URLs, todo notes, projects, tags, reminder content, and preserved organizer/attendee display names and mail addresses. Branch I is unusual because it puts that data on a *persistently visible desktop surface*, which is a disclosure channel the CLI does not have. Protections: `pill.privacy` (`titles` | `counts` | `hidden`) with a one-key shell toggle; mandatory exclusion from lock-screen and screenshot layers; no rendering on a locked session; the TUI's `--read-only` mode for shared or recorded screens; the detail pane and card render a *count and digest* of preserved residual properties rather than their bytes; all user text is escaped before terminal or QML rendering; and no client is ever handed a connection string, a credential, or a `DATABASE_URL`. Client-side logs contain typed IDs, counts, error codes, and digests only — never titles, never URLs, never SQL. The widget's last-frame cache lives in the shell's per-user storage, holds exactly one frame, and is cleared on `pill.privacy = hidden`.
- [x] **Uses synthetic/test data only until compliance gate clears** — every fixture, golden, PTY transcript, and widget stub uses the synthetic corpus and `example.invalid` addresses.

### 6.2 Asset provenance

- [x] **No third-party assets.** No font, icon, image, model, sound, or data file is added by branch I. The Quickshell widget uses the shell's already-installed theme font and glyph set by reference; the TUI ships no asset. QML sources under `integrations/quickshell/` are first-party.
- [ ] Uses third-party assets

Third-party *code* dependencies are `ratatui` and `crossterm` (both MIT, feature I1) and, for feature I4, build-time packaging tooling. A feature I3 HTTP/TLS crate would be a new dependency requiring license, maintenance, and security review before acceptance — it is unresolved (Q1) and nothing here presumes it.

### 6.3 Language / claims audit

- [x] **Makes claims not supported by evidence? No.** §7.1 states the true implementation state with the required vocabulary, and the banner at the top of this document states plainly that branch I is deferred. This spec claims no working TUI pane, no widget, no scheduling, and no non-Arch package.
- [x] **Promises capabilities not yet built? No — and this is enforced in shipped text.** `mg-calr tui --help` must describe only the line-oriented shell until the full-screen slice lands. `mg-calr contract describe` must list only entries that actually exist and must mark the stream `provisional`. `mg-calr --help` must not mention Quickshell, scheduling, RSVP, free-busy, or non-Arch packages. The TUI detail pane and the Quickshell card must label attendees `read-only; scheduling not implemented`. A test asserts help text contains none of the forbidden terms while the corresponding feature is absent.
- [x] **Uses language restricted by domain regulations? No.** `mg-calr` is a personal calendar tool; no health, financial, or legal claim is made. In particular, nothing in either client may describe a reminder as guaranteed, assured, or medically reliable — E owns delivery semantics and neither client presents or re-presents a notification.

### 6.4 Regulatory alignment

Walked by name against `docs/specs/QUALITY-CRITERIA.md` Lens 3. (Lens-3 numbering, not feature numbering — see the disambiguation note at the top.)

**Lens-3 I1 — Lossless iCalendar. Addressed at the client boundary; codec remains F's.** Neither client parses, serializes, normalizes, indexes, or logs iCalendar. Feature I1 reads only typed fields plus identity through `AgendaQueryPort`, and its mutations go through the same application use cases the CLI uses, which carry F's residual store and `event_calendar_users` rows unchanged. The binding test is `tui_mutations_produce_the_same_audit_record_as_the_cli` extended with a residual-digest assertion: run a full scripted TUI editing session over the synthetic `.ics` corpus and require every residual entry's `sha256`, ordinal, folding metadata, and calendar-user `raw_line` to be byte-identical before and after. Feature I3 is where losslessness matters most and it is **deferred**: the architecture note is that deferral is only safe because the RFC 6638 carriers — `SCHEDULE-AGENT`, `SCHEDULE-STATUS`, `SCHEDULE-FORCE-SEND`, `PARTSTAT`, `RSVP`, `DELEGATED-TO`/`-FROM`, `SENT-BY`, `MEMBER`, plus `METHOD`, `REQUEST-STATUS`, and whole `VFREEBUSY` components — are preserved verbatim *now* by F (§4.2 above makes that a binding requirement on F, not an aspiration). `scheduling_is_not_implemented_and_sends_nothing` proves the round trip today, so scheduling can be added later without having destroyed its inputs.

**Lens-3 I2 — Sync authority. Confirmed; this is the spine of the branch.** PostgreSQL remains the single authority. Branch I introduces **no** second store, no daemon, no cache of domain data, no local index, and no client-side database access of any kind. The complete durable footprint added by branch I is a ≤4 KiB TUI preferences file containing no user content and one last-frame value in the shell's per-user storage that exists solely so the card can render a *labeled stale banner with an age*; presenting it as current is forbidden and tested (`stale_frame_is_never_rendered_as_current`). The boundary is executable, not aspirational: `contracts/client-interface-v1.json` enumerates the surface, `client_interface_manifest_entries_all_exist` and `client_interface_stable_entries_are_frozen` make an A–H change that breaks it a build failure, `quickshell_sources_contain_no_database_access` and `quickshell_sources_only_invoke_manifest_commands` prove the out-of-process client cannot reach PostgreSQL, and `tui_module_has_no_storage_dependency` plus `tui_runs_entirely_against_a_fake_port` prove the in-process client cannot either. Every snapshot carries D's `snapshot_token` and `source_revision` through to both clients, so fingerprint identity is preserved across the boundary rather than recomputed.

**Lens-3 I3 — Conflict/deletion. Addressed for what branch I does; resolution stays F/G's.** Both clients render only D's authoritative-live tri-state; tombstones and unresolved conflict branches are never displayed as live and are never selectable, because selection resolves through the same D selector contract. The TUI's only deletion key is G's soft delete, it is confirm-gated, it reports an undo token, and there is no hard-delete or purge key — purge stays a deliberate CLI command. Mutations carry an expected `source_revision` by construction (§4.3), so a client cannot express an unconditional overwrite; an optimistic conflict surfaces as a message telling the user to re-check and offers **no** "overwrite anyway" affordance. The widget never resolves anything: an ambiguous selector produces `ambiguous — resolve in the terminal`, never a choice. Deferred piece: feature I3's scheduling inbox will eventually receive replies that could collide with local edits; the binding architectural note is that a reply landing on a locally-modified event must halt with F's `sync_conflict`, preserve both sides, and never auto-apply a `PARTSTAT` — written down now so that the deferred design cannot later take the easy path.

**Lens-3 I4 — Scope/network. Always applies; confirmed and test-enforced; never N/A.** Feature I1 opens no socket of any kind: `Cargo.lock` contains no HTTP, TLS, or DNS crate today, feature I1 adds none, and `src/tui/` has no transport dependency to misuse. Feature I2 opens no socket either — the widget's entire capability is spawning short-lived `mg-calr` child processes, and every manifest entry is marked `"network": false`; `watch` is a child process with a database connection and a stdout pipe, not a server: it listens on nothing, owns no D-Bus name, and dies with its parent. Database access remains confined to explicit database-related commands exactly as `docs/ARCHITECTURE.md` requires. Feature I3 is the only place network access could ever enter, and it is **deferred** with these binding constraints written down in advance: scheduling network I/O may exist only inside explicit `mg-calr schedule …` / `mg-calr sync …` invocations behind the `sync-transport` feature flag; it may never be triggered by an event save, an import, a timer, a systemd unit, a reminder scan, the TUI, or the widget; there is no auto-RSVP and no auto-send; and free-busy has a separable *local, network-free* form so that wanting free-busy does not force a transport. Proofs: `client_interface_makes_no_network` runs every v1 entry inside `unshare -rn`; `scheduling_is_not_implemented_and_sends_nothing` runs the invitation corpus in the same namespace; H §5.2 job 9's dependency denylist keeps a network client from appearing without an explicit allowlist entry; and `quickshell_sources_only_invoke_manifest_commands` prevents the widget from inventing a call that could reach one.

**Other lenses, briefly.** T1: clients pass full canonical UUIDs and never mint, mutate, or shorten identity — `short_id` is presentation-only, so UID instability is structurally impossible here. T2: both clients render offset, IANA zone, and fold on every timed row and all-day items keep civil bounds; the fold/gap fixtures are in the PTY and widget suites. T3: no client owns a transaction; every mutation is one application use case, atomic, with an expected revision. T4: soft delete plus undo token in the TUI, no purge key, audit parity with the CLI. T5: neither client presents, re-presents, claims, or acknowledges a reminder delivery — they display E's ledger state and invoke E's snooze/dismiss use cases, so no second delivery surface and no duplicate presentation is introduced (the pill is explicitly forbidden from animating on a firing reminder for exactly this reason). C1–C5: keyboard-first grammar with a discoverable `?` overlay and no terse modifier syntax; a versioned frozen client manifest with goldens; XDG-only configuration with the CLI > env > TOML > default precedence and no secret; color-independent semantics, `NO_COLOR`, width ladder, and the Quickshell public-interface behavior C4 names explicitly; and `doctor --check clients` as a non-mutating prerequisite matrix. O1: no client receives or logs a credential or URL. O2: nothing in branch I requires privilege, and the packaging rules in §4.6 forbid privileged maintainer scripts across all four distro targets. O3: typed errors with the A4 codes and exits everywhere, terminal restoration on every exit path including panic, and no partial commit reachable by killing a client. O4: the test matrix in §5.

---

## 7. Gap Analysis vs. Current State

### 7.1 What exists today

Verified against the working tree at commit `6e855f9` on 2026-08-29.

- **`src/tui.rs` — prototyped.** 242 lines. A deliberately line-oriented shell: `Key::parse` recognizes `j`/`k`/`down`/`up`/`r`/`refresh`/`q`/`quit` plus the bare arrow and Escape escape sequences as text tokens; `TuiState` holds `selected`, `quit`, `refresh_requested`, and a `status` string; `render(&AgendaOutput)` returns a whole-frame `String` with a fixed 40-character horizontal rule; `run()` drives a `BufRead` loop and calls a refresh callback only on `r`. Its own doc comment states that raw terminal input "is deliberately left to a later slice". Five in-module unit tests cover bounded navigation, quit/unknown transitions, the empty frame, occurrence rendering, and key aliases. Wired at `src/main.rs::run_tui` behind `mg-calr tui` with `--todo-projection`, `--start`, `--end`, `--timezone`, reading through `AgendaUseCases` over `ProjectionAgendaRepository`. **There is no raw mode, no alternate screen, no pane, no focus model, no color, no width awareness, no filter, no search, no mutation, and no overlay.** It is a correct, testable stepping stone, and §3.1 keeps it as the `--line` fallback.
- **Feature I2 Quickshell — absent.** No `integrations/` directory, no QML, no `watch` command, no `contract describe` command, and no `contracts/` directory at all. `README.md` lists "Quickshell integration" among what "remains open".
- **Feature I3 CalDAV Scheduling — absent, and its prerequisite is currently lossy.** No `src/schedule/`, no scheduling command, no HTTP/TLS/DNS crate anywhere in `Cargo.lock`. Per `specs/f-import-export-sync.md` §7.1, today's `EventMetadata::{organizer, attendees}` are plain `String`s with no parameter carriage, and `events.extension_properties` is a fixed struct that silently discards unknown keys — so organizer/attendee round-tripping does **not** hold today. F owns fixing that; feature I3 is blocked on it.
- **Feature I4 broader packaging — absent.** No `packaging/` directory (H specifies it and it is not yet built), no `LICENSE` file, and `README.md`/`docs/PRODUCT.md` both record MIT versus Apache-2.0 as unresolved.
- **Available and relied upon:** the A4 envelope and `AppError::{code,exit_code}` in `src/lib.rs`; `AgendaOutput`/`AgendaItem`/`AgendaQuery` in `src/application.rs`; XDG resolution in `src/config.rs`; embedded migrations in `src/storage.rs`; `unsafe_code = "forbid"` and `clippy::pedantic = "deny"` in `Cargo.toml`; 14 integration test files under `tests/`.
- **Planned/gated:** everything in §3 and §4. Branches B–H are themselves largely spec-complete and unimplemented, so branch I has no consumable D9 contract yet — which is precisely why this spec's deliverable-before-deferral is the manifest and its contract test rather than any client code.

### 7.2 Delta to spec

**New files.** `src/tui/{mod,state,keymap,ports,line}.rs`; `src/tui/render/{agenda,navigator,detail,status,overlay}.rs`; `src/cli/{watch,contract}.rs`; `contracts/client-interface-v1.json`; `contracts/client-interface-v1.lock`; `integrations/quickshell/{CalendarPill.qml,CalendarCard.qml,CalendarService.qml,README.md}`; `packaging/debian/{control,rules,changelog,copyright}`; `packaging/fedora/mg-calr.spec`; `packaging/nix/{flake.nix,home-manager-module.nix}`; `docs/PACKAGING.md`; `tests/{client_interface_contract,tui_boundary_contract,tui_pty,quickshell_widget}.rs`; `tests/fixtures/tui/*.txt` goldens; `tests/fixtures/scheduling/*.ics`.

**Modified files.** `src/tui.rs` → `src/tui/line.rs`, contents preserved so its five tests keep passing; `src/main.rs` (dispatch `watch` and `contract`, construct and inject the TUI ports, remove the inline `run_tui` loop); `src/lib.rs` (`AppError` variants `tui_unavailable`, `terminal_too_narrow`, `terminal_too_short`, `scheduling_not_implemented` with their codes and exits); `src/config.rs` (`[tui]` and `[client]` tables: `read_only`, `mouse`, `screen_reader`, `ascii`, `stale_after`, `pill.privacy`); `Cargo.toml` (`ratatui`, `crossterm`); `README.md` and `docs/ARCHITECTURE.md` (document the client interface boundary and the `--line` fallback); `tests/packaging_contract.rs` (extend the forbidden-verb scan to all four packaging directories).

**Migrations / schema changes.** **None for features I1, I2, and I4.** Feature I3 would add one append-only `migrations/00NN_scheduling.sql` for the message ledger sketched in §4.2; it is not authorized and must not be written before feature I3 is scheduled.

**New dependencies.** `ratatui`, `crossterm` (feature I1). Zero for feature I2. Build-time only for feature I4. Feature I3's HTTP/TLS client is unresolved (Q1) and explicitly not assumed.

### 7.3 Estimated scope

**XL overall**, decomposing into very unequal parts, which is itself the argument for splitting the branch rather than scheduling it as one unit:

- **Feature I1 — L.** The state machine and renderers are pure and testable, but the surface is wide: five panes and overlays, a full key grammar, a five-rung capability ladder, four width tiers, a screen-reader mode that is a genuinely separate rendering path, PTY goldens, and terminal-restoration guarantees on five exit paths including panic. Deliver as sub-slices: (1) ports + read-only three-pane workspace; (2) capability ladder and width tiers; (3) search and filter overlays; (4) mutations, confirm, and undo; (5) screen-reader mode. Each is M or smaller alone.
- **Feature I2 — M.** Small in code — the manifest, `contract describe`, an optional `watch`, and three QML files — but its evidence requirements (headless Quickshell E2E in a nested compositor, source-scanning contract tests, the network-namespace suite) are where the effort sits. The manifest and its contract tests are the **S** piece that should land first and early, before branch I is otherwise scheduled at all.
- **Feature I3 — XL and genuinely uncertain.** RFC 6638 is a protocol implementation, not a UI: it requires mg-calr's first HTTP/TLS client, credential handling on a live network path, a scheduling ledger with its own idempotency rules, inbox/outbox semantics, and conflict behavior against local edits. It is correctly the last thing in this branch, and it may reasonably never be built.
- **Feature I4 — M.** Each distro target is S on its own; the cost is the CI matrix, the DEP-5 copyright work, and the honest-scoping documentation about what is and is not archive-eligible.

### 7.4 Blocking dependencies

- **Features I1 and I2 both block on `d-views-query-output` being implemented**, not merely specified: `execute_view`, `QueryResult`, the D9 JSON schema, `snapshot_token`, `query_fingerprint`, the selector/chooser contract, and the width/color rules are the entire substrate both clients render. A TUI built before D would necessarily invent a second projection.
- Feature I1 additionally blocks on **B** (event lifecycle, B4 temporal semantics, B6/B7 recurrence and exceptions), **C** (todo lifecycle, blocked state), **E** (reminder ledger state and the snooze/dismiss use cases, for the `s`/`A` keys and the card's reminder section), and **G2** (targeted undo, for the `u` key).
- Feature I2 additionally blocks on **A4** (stable envelope and exit codes, already implemented) and on the manifest artifacts, which are the one deliverable of this spec that should land early.
- **Feature I3 blocks on F in full** — F1 lossless boundary, F11 parameter-preserving organizer/attendee storage, F8/F9 three-way conflict handling, F12 credential integration — and on an unresolved architectural decision (Q1) about introducing an HTTP client at all. It also blocks on there being a CalDAV server that implements RFC 6638 in the user's actual deployment; iCloud does, but the current transport is `vdirsyncer`, which does not.
- **Feature I4 blocks on H** (the Arch PKGBUILD, release artifacts, checksums, completions, man pages, and the packaging contract test it extends) **and hard-blocks on H's Q1**: an unresolved `LICENSE` makes a `.deb`, an RPM, and a Nix derivation equally unpublishable, not just an AUR package. Debian additionally blocks on a rustc ≥ 1.85 toolchain being available on the target release.
- **Nothing in branch I blocks any earlier branch.** That is the design intent: A–H must ship a stable interface, and I consumes it. The one obligation this spec places on earlier branches is the manifest and its contract test, so that "stable" is enforced rather than assumed.

---

## 8. Open Questions

- **Q1:** Does feature I3 justify introducing mg-calr's first HTTP/TLS client, given that F's whole transport design is "spawn `vdirsyncer`, open no socket ourselves", and `vdirsyncer` does not implement RFC 6638? The alternatives are (a) a first-party CalDAV scheduling client behind `sync-transport`; (b) scheduling stays permanently out of scope and mg-calr remains a preserve-and-display consumer of invitations; (c) wait for an external scheduling-capable helper to spawn. — blocks: §4.5 dependency decision, §4.3's feature I3 API, and the §7.3 XL estimate. This spec deliberately does not decide it; option (b) is a legitimate permanent answer.
- **Q2:** Should the Quickshell widget be first-party and vendored in this repository (assumed throughout, because it is what makes `quickshell_sources_contain_no_database_access` an executable test in mg-calr's own suite), or should it live in `~/dotfiles` with only the JSON contract owned here? — blocks: §4.1 placement, §5.2's two source-scanning tests, and whether the Arch package installs anything under `/usr/share/mg-calr/quickshell/`.
- **Q3:** Is `agenda.watch` worth building at all, or is a 60-second poll of `agenda --json` sufficient forever? Polling is simpler, has no long-lived database connection, and cannot leak a process; `watch` reduces latency and process churn. — blocks: §4.3's streaming contract only; the pill works either way and `watch` is marked `provisional` precisely so this can be deferred.
- **Q4:** If `watch` is built, should change detection use PostgreSQL `LISTEN`/`NOTIFY` instead of an interval digest query? `NOTIFY` needs an emitter — a trigger or an application-side publish — which is a schema and write-path change owned by A/B, not by branch I. — blocks: §4.3's change detection and §4.7's `watch` budget; the interval digest is the specified default and is sufficient.
- **Q5:** Which non-Arch targets are actually wanted, and at what honesty level — unofficial `.deb`/Copr artifacts published from CI, or a genuine attempt at Debian/Fedora archive inclusion (which requires the entire crate graph to be packaged by those distributions and is realistically out of reach)? — blocks: §4.6's distribution matrix scope and the §7.3 feature I4 estimate.
- **Q6:** Should the TUI's `y` key be able to place the identity token on the Wayland clipboard by spawning `wl-copy`, or is printing it to the status bar for manual selection sufficient? Spawning a clipboard helper is a new subprocess on a keystroke and a small exfiltration surface; it is also the obvious ergonomic choice. — blocks: §3.4's Detail-pane binding only; the specified default is status-bar printing with an opt-in `tui.clipboard_command`.

Resolved by this spec and **not** open: that clients consume public commands and never PostgreSQL; that the boundary is a versioned manifest with a lock and a contract test; that no client opens a network socket; that feature I3 is deferred while its iCalendar carriers are preserved now; that the pill has a privacy mode and never renders on a locked session; that the line-oriented shell survives as `--line`; and that no packaging target may run a privileged or database-mutating maintainer script.
