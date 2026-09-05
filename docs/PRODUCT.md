# Product scope

The accepted target product is a keyboard-first local calendar, reminders, and todo CLI for Arch/Hyprland, backed by PostgreSQL and eventually interoperable with iCloud through standards-based iCalendar plus vdirsyncer.

This commit implements the foundation slice only. The binding target tree is in `docs/FEATURE-TREE.md`; current implementation evidence is tracked by `docs/STATUS.md` and the foundation spec. Future capabilities remain absent until their own TDD slices land.

Core locked decisions relevant now: executable `mg-calr`; Rust 2024; PostgreSQL 18 compatibility; Unix-socket peer-auth default; XDG TOML configuration; stable JSON; deterministic typed errors; no sudo; no implicit network; no LICENSE until MIT versus Apache-2.0 is decided.
