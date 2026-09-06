# Spec status

**This tracks specification authoring, not implementation.** A spec being written
says nothing about whether the behaviour exists. `../README.md` is the authority on
what is actually built and how to verify it.

Branch letters come from [FEATURE-TREE.md](FEATURE-TREE.md). A branch is `authored`
once its spec states binding scope; `reviewed` once a blind scorecard exists under
[reviews/](reviews/).

| Branch | Name | Spec | Blind review |
|---|---|---|---|
| A | Application foundation | [authored](specs/a-foundation.md) | [reviewed](reviews/a-foundation.md) |
| B | Calendar and event core | [authored](specs/b-event-calendar-core.md) | [reviewed](reviews/b-event-calendar-core.md) |
| B0 | Event domain foundation | [authored](specs/b0-event-domain-foundation.md) | — |
| B1 | PostgreSQL event persistence | [authored](specs/b1-postgres-event-persistence.md) | — |
| B2 | Calendar and event query CLI | [authored](specs/b2-calendar-event-query-cli.md) | — |
| C | Todo core | [authored](specs/c-todo-core.md) | [reviewed](reviews/c-todo-core.md) |
| D | Views, query, output | [authored](specs/d-views-query-output.md) | [reviewed](reviews/d-views-query-output.md) |
| E | Reminder system | [authored](specs/e-reminder-system.md) | [reviewed](reviews/e-reminder-system.md) |
| F | Import, export, sync | [authored](specs/f-import-export-sync.md) | [reviewed](reviews/f-import-export-sync.md) |
| G | Safety operations | [authored](specs/g-safety-operations.md) | [reviewed](reviews/g-safety-operations.md) |
| H | Packaging and devex | [authored](specs/h-packaging-devex.md) | [reviewed](reviews/h-packaging-devex.md) |
| I | Deferred branches | [authored](specs/i-deferred-branches.md) | [reviewed](reviews/i-deferred-branches.md) |

Quality bar these were written against: [QUALITY-CRITERIA.md](specs/QUALITY-CRITERIA.md).

A blind review grades the spec, never the code. Several reviews record blocking
defects in the spec they reviewed — read the scorecard before implementing a branch.
