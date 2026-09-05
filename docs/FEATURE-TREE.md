# mg-calr — Binding Feature Tree

**Confirmed source:** accepted interview and implementation authorization, 2026-08-23.
**Current slice:** A foundation only. `I` is explicitly later.

- **A. Application foundation**
  - A1 Configuration and XDG layout
  - A2 Database connection, initialization diagnostics, and migrations
  - A3 Stable identity and short-ID resolution
  - A4 Error contracts, exit codes, logging, and JSON envelopes
  - A5 Transaction history and audit model
- **B. Calendar and event management**
  - B1 Calendar list/discovery/default selection
  - B2 Guided and non-interactive event creation
  - B3 Inspect/edit/move/trash/restore/purge
  - B4 Timed/all-day/timezone/DST semantics
  - B5 Metadata and unknown-property preservation
  - B6 RRULE expansion
  - B7 Exceptions and this-and-future splitting
  - B8 Event reminders
- **C. Todo management**
  - C1 Guided/non-interactive creation
  - C2 Inspect/edit/complete/trash/restore/purge
  - C3 Project/tags/priority/notes/due dates
  - C4 Recurring templates and instances
  - C5 Nested subtasks
  - C6 Dependency DAG and blocking
  - C7 Parent completion invariants
  - C8 Todo reminders
- **D. Views, querying, output**
  - D1 Today/default agenda
  - D2 Day; D3 Week; D4 Month grid/agenda
  - D5 Combined event/todo projection
  - D6 Filters; D7 full-text search
  - D8 Human/color/width rendering
  - D9 Stable JSON contracts
  - D10 chooser/short-ID disambiguation
- **E. Reminder system**
  - E1 Schedule/delivery ledger; E2 idempotent scanner
  - E3 action service; E4 snooze/dismiss
  - E5 catch-up; E6 DND deferral
  - E7 backend abstraction; E8 systemd/recovery
- **F. Import, export, synchronization**
  - F1 Lossless iCalendar boundary
  - F2 iCalendar import/export; F3 JSON interchange
  - F4 durable vdir/ledger; F5 vdirsyncer discovery/config
  - F6 export projection; F7 import reconciliation
  - F8 three-way conflicts; F9 resolution; F10 tombstones
  - F11 organizer/attendee preservation
  - F12 iCloud diagnostics and secret-command integration
- **G. Safety and operations**
  - G1 dry-run bulk framework; G2 targeted undo
  - G3 backup/verification/restore; G4 doctor
  - G5 migration compatibility/rollback; G6 crash recovery
- **H. Packaging and developer experience**
  - H1 PKGBUILD; H2 releases/checksums; H3 dev install
  - H4 completions/man pages; H5 example configuration
  - H6 PostgreSQL/iCloud setup; H7 CI/clean-machine smoke
- **I. Later branches — approved but deferred**
  - I1 Full-screen TUI
  - I2 Quickshell pill/card integration
  - I3 CalDAV Scheduling (invitations/RSVP/free-busy)
  - I4 broader Linux packaging

## Dependency order

A → B core → recurrence → C → E → D/search/audit → F codecs → F sync → G/H. Later branch I consumes stable application/JSON interfaces and never reads PostgreSQL directly.
