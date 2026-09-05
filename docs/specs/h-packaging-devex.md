# Spec: Packaging and Developer Experience

**Feature ID:** h-packaging-devex
**Parent feature:** root
**Spec author agent:** Packaging Spec Agent
**Date:** 2026-08-29
**Iteration:** 1

---

## 1. Purpose

### 1.1 One-sentence job

Let an Arch Linux user install `mg-calr` from a package or a source checkout and reach a working, self-documenting, correctly configured first run — with completions, man pages, an honest example configuration, and printed (never executed) administrator setup steps — while giving the maintainer a CI gate that proves the shipped artifact actually works on a clean machine.

### 1.2 Why it matters

`mg-calr` is a local, keyboard-first CLI whose hardest prerequisite is not the binary but the environment around it: a PostgreSQL server with a peer-auth role, an XDG configuration tree, an eventual iCloud credential that must never touch a config file, and a schema that must be migrated by an unprivileged user. Today none of that is packaged. A user obtains the tool only by `cargo build`, discovers prerequisites by reading `README.md`, gets no shell completion for a command surface that already exposes ~40 subcommands and ~50 flags, and has no man page. The product's own locked decisions — no sudo, no implicit network, XDG-only configuration, redacted credentials — are enforced today only by the author's discipline. H turns those decisions into packaging artifacts and CI gates so that the install path cannot quietly violate them, and so that "it works" is demonstrated on a fresh container rather than asserted from a warm development workstation.

### 1.3 Success signal

A tag build produces a byte-identical source tarball on two independent runners; the `clean-machine-smoke` CI job installs the resulting package into a fresh `archlinux:base` container with no Rust toolchain, executes the PostgreSQL setup commands extracted verbatim from `docs/SETUP-POSTGRESQL.md`, and completes a synthetic end-to-end flow (`init` → provision → `database migrate` → `calendar create` → `event create` → `agenda --json` → `interop export` → `pacman -R`) with zero network interfaces available to the application, zero `sudo` invocations by `mg-calr` itself, and zero regenerated-artifact drift.

---

## 2. User Stories

> As an Arch user, I want `sudo pacman -U mg-calr-0.1.0-1-x86_64.pkg.tar.zst` (or `makepkg -si`) to install a working binary plus completions and man pages, so that I can start using the tool without reading the source tree.

> As a first-time user with no PostgreSQL server, I want my first `mg-calr` command to fail with a short diagnosis and a copyable list of administrator commands, so that I know exactly what to run and can be certain the tool did not change my system while diagnosing it.

> As a security-conscious user preparing iCloud synchronization, I want the setup documentation to route my app-specific password through an external secret command and to reject a password typed into `config.toml`, so that my credential never lands in a file, a backup, a log line, or `--json` output.

> As a keyboard-first user in zsh, I want `mg-calr ev<TAB>` and `mg-calr event --<TAB>` to complete subcommands and flags with descriptions, so that I do not need to memorize a large grammar — and I want TAB to stay instant and offline even when PostgreSQL is stopped.

> As the maintainer developing on this workstation, I want an unprivileged `scripts/dev-install.sh` that installs into `~/.local` without ever touching `/usr` or pacman-owned files, so that I can test the real installed layout without corrupting the packaged installation.

> As an automation author or downstream packager, I want reproducible release tarballs with published SHA-256 checksums that the PKGBUILD's `sha256sums` array provably matches, so that I can verify what I am building.

> As a reviewer, I want CI to fail on clippy warnings, unformatted code, drifted completions, a leaked secret, a broken package build, or a failed clean-machine install, so that a green run is real evidence rather than a habit.

---

## 3. UX Specification

This is a CLI product. "Screen" below means an installation surface, a printed document, or a terminal transcript. No graphical screen, modal, sheet, drawer, or popover exists anywhere in this feature.

### 3.1 Screen / view inventory

| Surface | How reached | New / modified | Layout pattern |
|---|---|---|---|
| `pacman -S` / `pacman -U` transcript | package install | new | pacman-owned progress + one post-install message block |
| `makepkg -si` transcript | `cd packaging/arch && makepkg -si` | new | makepkg-owned build log + same post-install message |
| Post-install message (`mg-calr.install`) | printed by both installs | new | ≤8 plain-ASCII lines, no ANSI, no color |
| `mg-calr init` readiness report | `mg-calr init [--json]` | modified (exists in `src/main.rs`) | human `{:#?}` debug block today → structured human block plus stable JSON |
| `mg-calr doctor --check packaging` | explicit invocation | new | non-mutating check matrix, one row per check ID |
| `mg-calr config example` | explicit invocation | new | writes the embedded annotated `example.toml` to stdout only |
| `man mg-calr` (man1) | `man mg-calr` | new | generated roff, standard man sections |
| `man mg-calr-event` … (man1, per top-level subcommand) | `man mg-calr-event` | new | generated roff |
| `man 5 mg-calr` | `man 5 mg-calr` | new | hand-written roff for `config.toml` |
| bash completion menu | `mg-calr <TAB>` in bash | new | bash-completion word list |
| zsh completion menu | `mg-calr <TAB>` in zsh | new | `_arguments`-driven grouped list with descriptions |
| fish completion menu | `mg-calr <TAB>` in fish | new | description-annotated list |
| `scripts/dev-install.sh` transcript | developer invocation | new | file plan, then confirmation, then per-file result lines |
| `docs/SETUP-POSTGRESQL.md`, `docs/SETUP-ICLOUD.md` | `/usr/share/doc/mg-calr/`, repo | new | Markdown with fenced, machine-extractable command blocks |
| `config/example.toml` | `/usr/share/doc/mg-calr/example.toml` | modified (exists, `[database]` only) | annotated TOML |

Nothing in this feature installs a file under `/etc`. A system-wide configuration file would shadow the XDG precedence contract (`CLI > env > TOML > default`) that `tests/config_contract.rs` pins, so `/etc/mg-calr/` is forbidden by the package file-list assertion in §5.2.

### 3.2 Interaction flows

**Primary flow — package install to first successful command.**

1. User runs `sudo pacman -U ./mg-calr-0.1.0-1-x86_64.pkg.tar.zst` (or `makepkg -si` in `packaging/arch/`). pacman prints its own progress; the scriptlet then prints:

```text
mg-calr installs no system configuration and provisions no database.
Next steps:
  1) mg-calr init            # non-mutating readiness check + admin steps
  2) follow /usr/share/doc/mg-calr/SETUP-POSTGRESQL.md
  3) mg-calr database migrate  # run as your own user, not with sudo
Shell completions are installed for bash, zsh, and fish.
```

2. User runs `mg-calr`. With no server running the command fails before doing anything:

```text
mg-calr: could not connect to PostgreSQL: connection to server on socket
"/run/postgresql/.s.PGSQL.5432" failed: No such file or directory. Verify
PostgreSQL is running and ask an administrator to create a peer-auth role
matching the OS user plus database mg_calr; mg-calr never provisions them
```

Exit status 69 (`AppError::exit_code`, `StorageError::Connect`), JSON code `database_unavailable`. No file, role, database, or unit is created.

3. User runs `mg-calr init`. This is a diagnosis, not a provisioner. Human output:

```text
connection: Unix socket /run/postgresql database mg_calr user jeff (Default)
database_reachable: false
migrations: (unknown — server unreachable)
administrator steps (run these yourself; mg-calr will not run them):
  # administrator  pacman -S postgresql
  # administrator  sudo -u postgres initdb -D /var/lib/postgres/data --encoding=UTF8 --locale=C.UTF-8
  # administrator  systemctl enable --now postgresql
  # administrator  sudo -u postgres createuser --login "$USER"
  # administrator  sudo -u postgres createdb --owner "$USER" mg_calr
  # you            mg-calr database migrate
Review each command before running it. See /usr/share/doc/mg-calr/SETUP-POSTGRESQL.md
```

`init` exits 0 when the report was produced successfully even if the database is unreachable, because producing a readiness report is the job; the `database_reachable:false` field and a nonzero-only-on-error convention are already the shipped behavior and are retained.

4. Administrator runs the printed commands. User runs `mg-calr database migrate` unprivileged; the existing advisory-locked, idempotent migration path applies migrations 1–5 and reports state.

5. User runs `mg-calr calendar create --name Personal`, then `mg-calr event create ...`, then `mg-calr agenda --start … --end … --timezone …`.

**Branch — prerequisite missing at a later step.** `mg-calr database migrate` against a reachable server where the role exists but the database does not fails with the same redacted connect error and exit 69; the message names the missing object class, never the connection URL, and repeats the single relevant administrator line rather than the whole list.

**Branch — package present but PATH shadowed by a dev install.** `mg-calr doctor --check packaging` reports `packaging.binary_source` with the resolved path, whether pacman owns it, and both versions when a `~/.local/bin` copy shadows `/usr/bin`. Recovery text names the file to remove; the command removes nothing.

**Secondary flow — source build.** `git clone` → `cd packaging/arch` → `makepkg -si`. `makepkg` fetches sources, verifies `sha256sums`, builds with `--frozen` (lockfile-pinned, no dependency resolution drift), runs `check()` (`cargo test --frozen`, which excludes the `#[ignore]`d PostgreSQL integration tests so the build needs no database), then installs.

**Secondary flow — dev install.** `scripts/dev-install.sh --dry-run` prints the exact file plan; without `--dry-run` it installs and records a manifest. It refuses to run as root (`EUID == 0` → exit 77), refuses any destination outside `$HOME`, and refuses to overwrite a path that `pacman -Qo` reports as owned. `scripts/dev-install.sh --uninstall` removes only manifest-recorded paths that still hash to their recorded digest.

**Completion flow.** In zsh, `mg-calr ev<TAB>` completes to `event`; `mg-calr event --<TAB>` lists `--json --no-input --no-color --database-url` plus subcommand flags with their doc-comment descriptions. In bash, the same word lists appear without descriptions. Completion is *static*: the generated scripts contain no invocation of `mg-calr` and no command substitution, so pressing TAB never opens a PostgreSQL connection, never reads a secret, never writes a file, and works with the server stopped and the network namespace empty.

No haptics, sounds, or animations exist anywhere in this feature.

### 3.3 Layout descriptions

**Post-install message.** Top → bottom: one-sentence non-provisioning statement; numbered next steps; completion note. ≤8 lines, ≤78 columns, plain ASCII only — pacman scriptlets can render on a bare VT before a font is loaded. No ANSI, no box drawing, no emoji, no URL that could be mistaken for a command. Data source: a static string in `packaging/arch/mg-calr.install`; a unit test asserts it is ASCII-only, ≤8 lines, and contains no `sudo`/`systemctl`/`createdb` invocation of its own.

**`man mg-calr` (man1).** Section order, top → bottom: `NAME`, `SYNOPSIS`, `DESCRIPTION`, `OPTIONS` (global `--json`, `--no-input`, `--no-color`, `--database-url`), `COMMANDS` (one line per top-level subcommand with its `about` text, linking to `mg-calr-<sub>(1)`), `EXIT STATUS`, `ENVIRONMENT`, `FILES`, `EXAMPLES`, `SECURITY`, `SEE ALSO`, `BUGS`, `AUTHORS`. `NAME`/`SYNOPSIS`/`DESCRIPTION`/`OPTIONS`/`COMMANDS` are generated by `clap_mangen` from the same `clap::Command` the binary parses with. `EXIT STATUS` is generated from the same table that drives `AppError::exit_code` (see §4.3) so it cannot drift: rows `0 success`, `64 usage / required input missing`, `65 invalid input or rejected import`, `66 referenced object not found`, `69 database unavailable or query failure`, `70 serialization failure`, `74 projection store unavailable`, `75 optimistic-lock version conflict`, `78 configuration unusable`, `101 panic (unwind, defect — report it)`. `ENVIRONMENT` lists `DATABASE_URL`, `NO_COLOR`, `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME`, `MG_CALR_RUN_DATABASE_TESTS`, `MG_CALR_TEST_DATABASE_URL`. `FILES` lists the five resolved XDG paths and `/usr/share/doc/mg-calr/`. `SECURITY` states the three product invariants verbatim: mg-calr never invokes `sudo` or a package manager; mg-calr opens no network socket outside explicitly named synchronization commands; connection URLs are always redacted in output.

**`man 5 mg-calr`.** `NAME`, `DESCRIPTION` (file location and XDG resolution), `PRECEDENCE` (CLI > environment > file > compiled default, with the exact table from `src/config.rs`), `[database] SECTION` (`url`, `socket_dir`, `user`, `dbname` — each with type, default, and precedence note), `CREDENTIALS` (why no password key exists and what to use instead), `UNKNOWN KEYS` (warn-not-fail policy), `EXAMPLE`, `SEE ALSO`. Hand-written source in `man/mg-calr.5.scd`.

**Empty / degraded states.** `doctor --check packaging` on a `cargo run` binary (no package, no installed completions) reports every row as `status:"blocked"` with `reason:"not installed from a package"` rather than `fail`; the command still exits 0 because a development checkout is a legitimate configuration. `mg-calr config example` never has an empty state — the content is compiled in with `include_str!`.

### 3.4 Input & gestures

Everything is keyboard-driven text. Completions respond to the shell's own binding (`TAB` in bash/zsh/fish default configurations). `man` navigation is the pager's. `scripts/dev-install.sh` prompts once, reading a `y/N` from stdin, and skips the prompt entirely under `--yes` or when stdin is not a TTY, in which case it refuses to proceed unless `--yes` was passed (never assume consent from a pipe). No pointer, touch, stylus, controller, voice, or camera input exists. Responsive behavior means terminal width only: all H-authored text wraps at or below 78 columns and man pages are validated at `MANWIDTH` 40, 80, and 200.

### 3.5 Transitions & animation

N/A — no animation, transition, spinner, or progress indicator is introduced by this feature. `pacman`, `makepkg`, and `cargo` render their own progress and are not modified or wrapped. `mg-calr`'s own output remains write-once: no cursor repositioning, no in-place redraw, no ANSI escape emitted by any H surface. Because nothing animates, reduced-motion behavior is identical to default behavior and no alternative rendering path is required. `scripts/dev-install.sh` prints one line per planned file rather than a progress bar, so it is legible in a pipe and to a screen reader.

### 3.6 Error states

| Trigger | Presentation | Recovery | Data-loss risk |
|---|---|---|---|
| PostgreSQL server absent/stopped at first run | inline stderr line; JSON `database_unavailable`; exit 69 | start server, follow `init` steps | none — command never wrote |
| peer role missing (`role "jeff" does not exist`) | inline stderr; same code/exit; one relevant administrator line repeated | administrator runs `createuser` | none |
| database `mg_calr` missing | inline stderr; same code/exit | administrator runs `createdb` | none |
| migrations not applied, user runs a data command | typed storage error; guidance is `mg-calr database migrate` | run migrate unprivileged | none |
| migration ledger drift (recorded version, different name) | `migration_drift`; exit 69; migration aborts in-transaction | restore from backup or reconcile; never auto-repaired | none — transaction rolls back |
| `config.toml` contains a credential-shaped key with a scalar value | `config_plaintext_credential`; exit 78; the key path is named, the value is never echoed | move the secret to the external secret command | none — config is read-only |
| `config.toml` unparseable | existing `config_invalid`; exit 78 | repair or remove the file | none |
| `config.toml` has an unrecognized key | non-fatal warning row in `doctor`/`config check`; command proceeds with documented precedence | fix or ignore | none |
| `makepkg` checksum mismatch | makepkg's own `FAILED` line; build stops before extraction | re-download; report a compromised or stale mirror | none |
| `makepkg` build fails clippy/test in `check()` | build aborts before `package()` | fix source; never `--nocheck` in CI | none |
| `dev-install.sh` run as root | banner + exit 77 before any filesystem write | rerun as the normal user | none |
| `dev-install.sh` target owned by pacman | per-file `REFUSED (owned by mg-calr)` line; script exits nonzero having written nothing | uninstall the package, or choose a different prefix | none — plan is computed fully before any write |
| completion script sourced by an old shell (bash < 4.2, zsh < 5.0) | shell's own syntax error | documented minimum shell versions in `SETUP` docs | none |
| `pacman -R` while a `mg-calr` process runs | pacman's own behavior; binary unlinked, running process unaffected | rerun after exit | none — user data lives in PostgreSQL and XDG dirs, which the package does not own |
| package upgrade across a migration | no automatic migration; new binary reports pending migrations on next database command | user runs `database migrate` | none — upgrades never mutate the database |

Every H error message is plain text, contains no ANSI, names a file path or an object class but never a connection URL, password, secret-command output, or SQL. Presentation is inline stderr in every case because these are single-shot CLI commands with no persistent frame in which to hang a banner or toast, and because a nonzero exit plus one stderr line is the composable form that scripts and `journalctl` both consume correctly.

### 3.7 Accessibility

- Every H surface is plain text; no information is carried by color, position, or shape alone. The post-install message, `init` report, `doctor` matrix, and dev-install plan use words (`blocked`, `REFUSED`, `administrator`) rather than symbols or colors to carry state.
- `NO_COLOR` and `--no-color` are honored; H introduces no new ANSI emitter at all, so both are trivially satisfied and are asserted by a test that greps all H output for `\x1b[`.
- Man pages are validated with `man --warnings` at `MANWIDTH` 40, 80, and 200; no table or example may require more than 78 columns, so a screen reader or a narrow console never receives truncated syntax.
- Completion descriptions (zsh, fish) come from the same doc comments as `--help`, so a screen-reader user hears the same explanation in the completion menu and in help output. Bash's description-free list remains fully usable because subcommand names are self-describing words, never abbreviations.
- Focus order and keyboard navigability are the shell's and the pager's; H adds no interactive widget with its own focus model. `scripts/dev-install.sh`'s single prompt is answerable by `y`/`n` + Enter and defaults to "no" on Enter alone.
- Documentation is Markdown and roff — both linearly readable by assistive tooling. No diagram, screenshot, or image is a required carrier of any instruction; the setup docs' fenced code blocks are the normative content and are read in reading order.
- Text scaling is the terminal's; no fixed-width art, box drawing, or column alignment is load-bearing in any H output.

---

## 4. Implementation Specification

### 4.1 Architecture placement

Current boundaries (`docs/ARCHITECTURE.md`) are preserved: `src/domain.rs` stays CLI/SQL-free, `src/config.rs` stays a pure resolver plus a filesystem boundary, `src/storage.rs` owns PostgreSQL, `src/main.rs` stays process dispatch. H adds packaging siblings and makes exactly one source change to the crate layout:

- **`src/cli.rs` (new, in the library).** The `Cli`, `Command`, and all `Args`/`Subcommand` types currently defined privately in `src/main.rs` move here and become `pub`. `src/main.rs` keeps `#[tokio::main] fn main`, dispatch, and rendering, and imports the parser from `mg_calr::cli`. This single move is what makes H4 structurally correct: completions and man pages are generated from the *same* `clap::Command` value that parses real user input, so they cannot describe a CLI that does not exist.
- **`src/bin/mg-calr-gen.rs` (new).** Generator binary behind `required-features = ["gen"]`. Subcommands `completions --out-dir DIR` and `man --out-dir DIR`. Not installed by the package; built only to regenerate artifacts.
- **`packaging/arch/PKGBUILD`, `packaging/arch/mg-calr.install`** — release package.
- **`packaging/arch/PKGBUILD-git`** — `mg-calr-git` VCS variant, `provides=('mg-calr')`, `conflicts=('mg-calr')`, `pkgver()` from `git describe`.
- **`completions/mg-calr.bash`, `completions/_mg-calr`, `completions/mg-calr.fish`** — committed generated artifacts.
- **`man/mg-calr.1`, `man/mg-calr-<sub>.1`** (generated), **`man/mg-calr.5.scd`** (hand-written source), **`man/sections/*.roff`** (the hand-written `EXIT STATUS`/`ENVIRONMENT`/`FILES`/`SECURITY` fragments appended by the generator).
- **`scripts/dev-install.sh`, `scripts/ci-local.sh`, `scripts/release.sh`** — POSIX shell, `set -euo pipefail`, ShellCheck-clean.
- **`docs/SETUP-POSTGRESQL.md`, `docs/SETUP-ICLOUD.md`, `docs/RELEASING.md`.**
- **`.github/workflows/ci.yml`, `.github/workflows/release.yml`, `deny.toml`, `.gitattributes`, `ci/smoke-manifest.toml`, `ci/smoke.sh`, `ci/Containerfile.clean`.**
- **`tests/packaging_contract.rs`** — pure Rust assertions over the packaging files (no container needed).

Layering rule: `packaging/`, `scripts/`, `ci/`, and `man/` may read the crate's public surface and its committed artifacts; no library or binary source may read anything under `packaging/` at runtime. The binary must remain runnable from a bare `cargo build` with no packaging artifact present.

### 4.2 Data model

No database migration, table, column, or index is introduced by H. The only persistent state H creates is a dev-install manifest outside the database.

```rust
/// Version and identity facts that must agree across every release artifact.
/// Produced once and asserted by CI; never read at runtime.
pub struct ReleaseIdentity {
    /// `CARGO_PKG_VERSION` — the single source of truth.
    pub crate_version: String,
    /// Annotated git tag, expected to be `v{crate_version}`.
    pub git_tag: String,
    /// PKGBUILD `pkgver`, expected to equal `crate_version`.
    pub pkgver: String,
    /// PKGBUILD `pkgrel`; resets to 1 on every `pkgver` bump.
    pub pkgrel: u32,
    /// Lowercase hex SHA-256 of the reproducible source tarball.
    pub source_sha256: String,
}

/// One row of the non-mutating packaging readiness matrix.
/// Serialized inside the existing schema-v1 success envelope.
pub struct PackagingCheck {
    /// Stable machine identifier, e.g. `packaging.binary_source`.
    pub id: &'static str,
    pub status: CheckStatus, // Pass | Fail | Blocked
    /// Stable error code when `status != Pass`; never a free-form string only.
    pub code: Option<&'static str>,
    /// Actionable recovery text; contains no secret, URL, or SQL.
    pub recovery: Option<String>,
}

/// Record of one file written by `scripts/dev-install.sh`, stored as JSON at
/// `$XDG_STATE_HOME/mg-calr/dev-install.json`. Uninstall removes only entries
/// whose current digest still equals `sha256`, so a hand-edited file survives.
pub struct DevInstallEntry {
    pub path: PathBuf,
    pub sha256: String,
    pub mode: u32,
}
```

Check IDs are frozen for schema v1: `packaging.binary_source`, `packaging.version_agreement`, `packaging.completions_installed`, `packaging.man_installed`, `packaging.example_config_present`, `packaging.no_system_config`, `config.unknown_keys`, `config.plaintext_credential`, `credentials.argv_exposure`.

### 4.3 API contracts

**Generator interface** (`src/bin/mg-calr-gen.rs`, feature `gen`):

```text
mg-calr-gen completions --out-dir DIR   # writes mg-calr.bash, _mg-calr, mg-calr.fish
mg-calr-gen man        --out-dir DIR   # writes mg-calr.1 and mg-calr-<sub>.1
```

Both are pure functions of `mg_calr::cli::Cli::command()` plus the static roff fragments in `man/sections/`. Both are byte-deterministic: no timestamp, hostname, build path, locale-dependent formatting, or environment value may appear in the output. `clap_mangen`'s date field is pinned to `SOURCE_DATE_EPOCH` (defaulting to the tag commit date) rather than "now". Exit 0 on success, 74 on I/O failure.

**Static-completion rule.** Generation uses `clap_complete::generate` only. `clap_complete::env::CompleteEnv` and any other dynamic-completion engine are forbidden, because a dynamic engine re-invokes the binary on every TAB, which would make completion a database- and secret-touching code path. Enforced by `tests/packaging_contract.rs`: the committed bash and zsh scripts must contain no `$(`, no backtick substitution, and no bare `mg-calr` invocation.

**Exit-status table.** A single `const EXIT_STATUS_TABLE: &[(u8, &str)]` lives beside `AppError` in `src/lib.rs`. `AppError::exit_code` must return a value present in the table (unit-tested exhaustively over every variant), and `mg-calr-gen man` renders `EXIT STATUS` from it. This is the mechanism that keeps documentation and behavior identical rather than merely consistent-at-authoring-time.

**Configuration credential rejection** (extends `src/config.rs`):

```rust
/// Rejects any file key whose name matches `(?i)^(pass(word)?|secret|token|
/// credential|api[_-]?key)$` and whose value is a scalar string.
/// The offending key path is reported; the value is never captured, logged,
/// formatted, or included in the error.
ConfigError::PlaintextCredential { key_path: String }  // code config_plaintext_credential, exit 78
```

Also added: `ConfigWarning::UnknownKey { key_path }`, surfaced by `doctor` and `config check` as a `config.unknown_keys` row and never as a hard failure, so a config written for a newer `mg-calr` still starts. Neither path performs any I/O beyond the already-existing config read; both are pure over the parsed `toml::Table`.

**`mg-calr config example`.** Prints the `include_str!("../config/example.toml")` content to stdout and exits 0. It never writes a file, never creates `$XDG_CONFIG_HOME/mg-calr/`, and never overwrites an existing config — the user performs the redirection themselves. `--json` wraps it as `{"schema_version":1,"command":"config.example","ok":true,"data":{"toml":"…"}}`.

**`mg-calr doctor --check packaging [--json]`.** Read-only. Reads only `/proc/self/exe`, the installed artifact paths, and (when available) `pacman -Qo` output. It executes no privileged command, opens no database connection, and opens no network socket. Auth: none required beyond the invoking user's own filesystem access.

**PKGBUILD contract** (`packaging/arch/PKGBUILD`, normative fields):

```bash
pkgname=mg-calr
pkgver=0.1.0                       # == CARGO_PKG_VERSION, CI-asserted
pkgrel=1
pkgdesc='Keyboard-first local calendar CLI backed by PostgreSQL'
arch=('x86_64')                    # aarch64 added only when CI builds it
url='<canonical repository URL>'
license=('MIT')                    # placeholder; release gated until Q1 resolves
depends=('gcc-libs' 'glibc')
optdepends=('postgresql: local database server for mg-calr'
            'bash-completion: command completion for bash')
makedepends=('cargo' 'scdoc')
source=("$pkgname-$pkgver.tar.gz::<release tarball URL>")
sha256sums=('<64 hex>')            # 'SKIP' is forbidden; CI greps for it
options=('!lto')                   # LTO is set in [profile.release], not here

prepare() { export RUSTUP_TOOLCHAIN=stable
            cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"; }
build()   { export RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=target
            cargo build --frozen --release --all-features; }
check()   { export RUSTUP_TOOLCHAIN=stable
            cargo test  --frozen --all-targets --all-features; }
package() { install -Dm755 target/release/mg-calr "$pkgdir/usr/bin/mg-calr"
            # completions, man1/man5, docs, example.toml — see §3.1 paths
          }
```

`check()` runs without a database because the PostgreSQL integration tests are `#[ignore]`d and gated behind `MG_CALR_RUN_DATABASE_TESTS`; makepkg never sets it. `package()` installs no unit file, no `/etc` path, and no `sysusers`/`tmpfiles` fragment. `mg-calr.install` contains only `post_install`/`post_upgrade` functions that `echo` the message from §3.3 — no `sudo`, `systemctl`, `initdb`, `createuser`, `createdb`, `psql`, or `mg-calr database migrate` may appear in any scriptlet, asserted by `tests/packaging_contract.rs`.

**Release artifact contract (H2).** `scripts/release.sh v0.1.0` produces, into `dist/`:

| Artifact | Construction | Determinism guarantee |
|---|---|---|
| `mg-calr-0.1.0.tar.gz` | `TZ=UTC git archive --format=tar --prefix=mg-calr-0.1.0/ v0.1.0 \| gzip -9n` | git tree order is deterministic; `--prefix` fixes paths; `gzip -n` drops name/mtime; `git archive` stamps entries from the tag commit; `.gitattributes` `export-ignore` fixes the file set |
| `mg-calr-0.1.0-vendor.tar.zst` (optional) | `cargo vendor --locked` then the same archive discipline with `--sort=name --owner=0 --group=0 --numeric-owner --mtime=@$SOURCE_DATE_EPOCH` | lockfile-pinned inputs; normalized metadata |
| `SHA256SUMS` | `sha256sum` over the artifacts, LC_ALL=C sorted | plain text, one line per artifact |
| `SHA256SUMS.asc` | detached OpenPGP signature | **gated** — produced only once a signing key exists (Q3) |

No prebuilt binary is published in v1. A prebuilt binary would carry a reproducibility claim the project cannot yet substantiate (toolchain, glibc, and build-path variance), and the PKGBUILD builds from source regardless.

**Checksum truthfulness.** `sha256sums` in both PKGBUILDs is regenerated by `updpkgsums` (pacman-contrib) inside `scripts/release.sh`, never hand-edited. CI job `checksums` independently rebuilds the tarball from the tag, recomputes SHA-256, and asserts three-way equality with `SHA256SUMS` and the PKGBUILD array, then runs `makepkg --verifysource`. A `SKIP` entry, a missing entry, or an array length mismatch fails the job.

### 4.4 State management

H introduces no application runtime state and no new store, controller, or view model. Ownership:

- **Installed-file state** is owned by pacman's local database (`/var/lib/pacman/local/mg-calr-*`). H never duplicates or second-guesses it; `doctor --check packaging` queries `pacman -Qo` read-only and degrades to `blocked` when pacman is absent.
- **Dev-install state** is owned by `$XDG_STATE_HOME/mg-calr/dev-install.json` (`DevInstallEntry` list). It is the only file H writes outside a package, it lives under the XDG *state* root (not data, not cache) because it is non-secret operational bookkeeping, and it is written atomically (temp file in the same directory + `rename`) so an interrupted install leaves either the old manifest or the new one.
- **Application data state** — calendars, events, todos, migrations — remains owned by PostgreSQL, unchanged. Install, upgrade, and removal never read or write it. This is the packaging-side expression of criterion I2: packaging must not become a second authority.
- **Local vs. server-synced boundary:** N/A in the sync sense — nothing in H synchronizes. The only boundary is package-owned (`/usr`, immutable, replaced wholesale on upgrade) versus user-owned (`$HOME`, XDG dirs, PostgreSQL, never touched by pacman).
- **Offline / draft persistence:** N/A — H has no editable document, no long-running session, and no partially-entered input to preserve. `scripts/dev-install.sh` computes its complete file plan before writing anything, so there is no half-applied state to resume; a failure mid-write is recoverable by rerunning, which is idempotent.

### 4.5 Dependencies

**New Rust crates.**

| Crate | Scope | Purpose | License |
|---|---|---|---|
| `clap_complete` 4.x | build/gen (`gen` feature) | shell completion generation from the clap tree | MIT OR Apache-2.0 |
| `clap_mangen` 0.2 | build/gen (`gen` feature) | roff man page generation from the clap tree | MIT OR Apache-2.0 |
| `zeroize` 1.x | runtime, gated on F12 | `Zeroizing<String>` for secret-command output | MIT OR Apache-2.0 |

`clap_complete` and `clap_mangen` are behind `required-features = ["gen"]` so a default `cargo build` does not compile them into the shipped binary. `zeroize` is listed here for planning only and lands with F12, not with H.

**New non-Rust tooling** (CI and maintainer machines only; never a runtime dependency of the installed binary): `scdoc` (man 5 source), `pacman-contrib` (`updpkgsums`), `namcap` (package linting), `shellcheck`, `gitleaks` (secret scan), `cargo-deny` (license/advisory gate), `podman` or `docker` (clean-machine container). All are makedepends/CI-only; the installed package depends on `gcc-libs` and `glibc` alone.

**New assets/resources.** Committed generated artifacts (`completions/*`, `man/*.1`), hand-written `man/mg-calr.5.scd` and `man/sections/*.roff`, the expanded `config/example.toml`, and the setup documents. No fonts, images, icons, audio, or ML models — the product has no graphical surface.

**Infrastructure.** No CDN, no hosted service, no telemetry endpoint. A git forge is required for CI and release hosting; the repository currently has no remote, which makes the forge choice an open question (Q4) rather than an assumption. `scripts/ci-local.sh` runs the identical job list locally so the gate exists even before a forge does. PostgreSQL 16/17/18 service containers are needed only by the migration matrix job.

### 4.6 Platform-specific considerations

- **Primary target:** Arch Linux `x86_64`, glibc, dynamically linked, on the developer's Hyprland workstation. `arch=('x86_64')` only; `aarch64` is added to the array in the same commit that adds an `aarch64` CI builder, never before, so the package never advertises an untested architecture.
- **Toolchain:** Rust edition 2024, `rust-version = "1.85"`. A dedicated `msrv` CI job builds with exactly 1.85 so the declared MSRV is evidence-backed. `RUSTUP_TOOLCHAIN=stable` is exported in `prepare`/`build`/`check` per Arch Rust packaging guidelines so a user's rustup override cannot silently change the build.
- **`[profile.release]`:** `lto = "thin"`, `codegen-units = 1`, `strip = false` (makepkg strips and can emit a `-debug` package), and `panic = "unwind"` explicitly retained. `panic = "abort"` is rejected: it would replace the deterministic exit 101 documented in `EXIT STATUS` with a SIGABRT, breaking the O3 failure contract and the tests that assert it.
- **PostgreSQL:** the product targets PostgreSQL 18. The migration CI matrix runs 16, 17, and 18; the `optdepends` and documentation state exactly the versions the matrix covers, and no broader claim is made.
- **Shells:** bash ≥ 4.2 with the `bash-completion` package for dynamic loading (hence the completion file *must* be named `mg-calr`, matching the command); zsh ≥ 5.0 with `compinit` and `/usr/share/zsh/site-functions` on `$fpath` (Arch default); fish ≥ 3.0 reading `/usr/share/fish/vendor_completions.d`.
- **No systemd unit** is shipped. Reminder delivery timers belong to E8; installing an inert or speculative unit now would be a capability claim the product cannot honor.
- **Feature flags / gradual rollout:** N/A as a runtime concept — this is a single-user local CLI with no server-side rollout surface. The only compile-time flag is `gen`, which selects the developer-only generator binary. Staged delivery is expressed instead by the `ci/smoke-manifest.toml` allowlist: each new user-facing command must be added to the smoke manifest (or explicitly listed as `excluded_with_reason`) before CI will pass, so the packaged surface and the tested surface grow together.

### 4.7 Performance budget

Measured on the reference Arch workstation and in the CI container; each number is a CI-enforced ceiling, not an aspiration.

- **Binary size:** stripped release binary ≤ 12 MiB; `.pkg.tar.zst` ≤ 5 MiB. CI records both per build and fails on a >15% regression versus the previous tag.
- **Startup:** `mg-calr --version` and `mg-calr config paths` p95 ≤ 20 ms cold, ≤ 10 ms warm. These paths read environment and an optional small TOML file only, and must not link or initialize a database client at startup.
- **Completion latency:** sourcing `/usr/share/bash-completion/completions/mg-calr` ≤ 5 ms; generating candidates for `mg-calr <TAB>` ≤ 10 ms. Static scripts execute no subprocess, so latency is independent of database and network state — verified with the server stopped.
- **Memory:** `--version`/`config paths` RSS ≤ 8 MiB. H adds no runtime allocation to any data path.
- **Build:** clean `makepkg` (fetch + build + check) ≤ 8 min on the reference workstation; incremental developer rebuild unaffected because generator crates are feature-gated out of the default build.
- **CI:** full PR pipeline wall clock ≤ 15 min with jobs parallelized; `clean-machine-smoke` ≤ 5 min end to end including container pull, package install, PostgreSQL provisioning, and the synthetic flow.
- **Network payload:** release tarball ≤ 1 MiB (source only; no vendored crates unless the optional vendor artifact is enabled, which is ≤ 25 MiB). The *installed application* transfers zero bytes over the network — see §6.4 I4.
- **Storage:** installed footprint ≤ 14 MiB (binary + 3 completion scripts + ~15 man pages + docs). Dev-install manifest ≤ 8 KiB. No cache directory is created by H; `$XDG_CACHE_HOME` remains unused and non-authoritative.

---

## 5. Test Specification

### 5.1 Unit tests

In `tests/packaging_contract.rs` (pure Rust, no container, runs in the default `cargo test`):

- `completions_are_regenerable_and_byte_stable` — generate into a temp dir twice under different `TZ`, `LANG`, and `PWD`; assert identical bytes and identical to the committed `completions/*`. Edge case: locale-dependent sorting.
- `man_pages_match_committed_artifacts` — same, for `man/*.1`; also asserts every top-level subcommand in the clap tree has a corresponding `mg-calr-<sub>.1`. Edge case: a subcommand added without regenerating.
- `completions_are_static_and_offline` — committed bash/zsh/fish scripts contain no `$(`, no backtick, and no line invoking `mg-calr`. Edge case: an accidental switch to a dynamic completion engine.
- `exit_status_table_is_total_and_documented` — every `AppError` variant's `exit_code()` appears in `EXIT_STATUS_TABLE`, and every table row appears in the rendered `mg-calr.1`. Edge case: a new error variant with an undocumented code.
- `example_toml_parses_and_documents_only_real_keys` — `resolve_config` accepts `config/example.toml`; every key mentioned (including in comments, extracted by a strict `^# *([a-z_]+) =` scan) is a recognized `FileConfig`/`FileDatabase` field. Edge case: a renamed field leaving stale documentation.
- `example_toml_resolves_to_compiled_defaults` — with all commented keys uncommented at their stated values, the resolved settings equal `/run/postgresql`, `mg_calr`, and the current user. Edge case: an example that documents a value the code does not actually default to.
- `example_toml_contains_no_credential` — no `password`/`secret`/`token` assignment, no `://user:pass@` userinfo. Edge case: a helpful-but-fatal "example" password.
- `plaintext_credential_key_is_rejected_without_echoing_value` — a config with `[icloud] password = "hunter2"` returns `config_plaintext_credential`, exit 78; the rendered error and its `Debug` form contain `icloud.password` and never `hunter2`. Edge case: the value appearing via a derived `Debug` impl.
- `unknown_config_key_warns_and_does_not_fail` — an unrecognized key yields a `config.unknown_keys` warning row and a successfully resolved config. Edge case: forward compatibility with a newer config file.
- `pkgbuild_version_agrees_with_crate` — parse `pkgver` from both PKGBUILDs; assert `== env!("CARGO_PKG_VERSION")`. Edge case: a version bump that forgets packaging.
- `pkgbuild_has_no_skip_checksum` — `sha256sums` contains no `SKIP` and is nonempty for every `source` entry.
- `pkgbuild_and_install_perform_no_privileged_action` — no `sudo`, `systemctl`, `initdb`, `createuser`, `createdb`, `psql`, `pacman`, or `database migrate` token appears in `prepare/build/check/package` or in `mg-calr.install`. Edge case: a convenience scriptlet added later.
- `package_installs_no_system_config` — no `$pkgdir/etc` path appears in `package()`.
- `post_install_message_is_ascii_and_bounded` — ≤8 lines, ≤78 columns, ASCII-only, no ANSI escape.
- `dev_install_plan_is_confined_to_home` — the plan builder, given a synthetic environment, produces only paths under `$HOME`; a `--prefix /usr` argument is rejected. Edge case: a prefix escaping via `..`.
- `smoke_manifest_covers_every_top_level_subcommand` — every subcommand in `Cli::command()` appears in `ci/smoke-manifest.toml` either as covered or as `excluded_with_reason`. Edge case: a shipped command nobody smoke-tests.
- `setup_docs_command_blocks_are_extractable_and_labeled` — every fenced block in `docs/SETUP-POSTGRESQL.md` is tagged `admin` or `user`; every `admin` line is a privileged command and every `user` line is not; no `user` line contains `sudo`.

### 5.2 Integration tests

Container-based, in CI and reproducible locally via `scripts/ci-local.sh`:

1. **Package build gate.** `archlinux:base-devel` container, non-root builder: `makepkg --syncdeps --noconfirm --check` for both `PKGBUILD` and `PKGBUILD-git`. Then `namcap PKGBUILD` and `namcap *.pkg.tar.zst` with zero errors and a reviewed warning allowlist.
2. **Package content assertion.** `bsdtar -tf` the built package; assert the exact expected path set (binary, three completion files at their canonical shell paths, `mg-calr.1`, one `mg-calr-<sub>.1` per subcommand, `mg-calr.5`, `/usr/share/doc/mg-calr/{README.md,example.toml,SETUP-POSTGRESQL.md,SETUP-ICLOUD.md}`); assert no path under `/etc`, `/var`, `/usr/lib/systemd`, or `/usr/lib/sysusers.d`.
3. **Checksum truth.** Rebuild the tarball from the tag, recompute SHA-256, assert three-way equality with `SHA256SUMS` and both PKGBUILD arrays; run `makepkg --verifysource`.
4. **Reproducibility twin build.** Build the tarball twice in containers differing in `TZ`, `umask`, hostname, and checkout path; assert identical SHA-256. Failure prints a `diffoscope` summary.
5. **Migration matrix.** For PostgreSQL 16, 17, 18: create a disposable database whose name contains `mg_calr_test`; run `MG_CALR_RUN_DATABASE_TESTS=1 MG_CALR_TEST_DATABASE_URL=… cargo test --test postgres_integration -- --ignored`; then run `mg-calr database migrate` twice and assert the second run is a no-op with an unchanged ledger; then run `database status` and assert no pending migrations and no drift.
6. **Upgrade path.** Provision a database with the previous release tag's binary, seed synthetic calendars/events/todos, install the new package, run `database migrate`, and assert every seeded row is intact and no `DROP TABLE` executed. Then `pacman -R` and assert the database and `$HOME` are byte-identical to their pre-removal state.
7. **Secret-scan gate.** `gitleaks detect --no-git=false` over full history plus a repo-specific deny-regex pass over the working tree, the built package contents, the generated completions and man pages, and `dist/`: no `password=`, no `postgres(ql)?://[^/@]*:[^@]*@`, no `-----BEGIN [A-Z ]*PRIVATE KEY-----`, no `[a-z]{4}-[a-z]{4}-[a-z]{4}-[a-z]{4}` (Apple app-specific-password shape). Any hit fails the pipeline.
8. **Lint and license gates.** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings` (the crate already sets `clippy::all = "deny"`, `clippy::pedantic = "deny"`, `unsafe_code = "forbid"`, and a test asserts those three lint settings are still present in `Cargo.toml`); `cargo deny check licenses advisories bans sources` against `deny.toml`; `shellcheck` on every script.
9. **No-HTTP-client assertion.** Parse `Cargo.lock` and assert it contains no `reqwest`, `hyper`, `ureq`, `curl`, `rustls`, `native-tls`, or `openssl` package. When F transport lands, this becomes an explicit one-entry allowlist rather than a deletion.
10. **Generated-artifact drift gate.** Regenerate completions and man pages into a temp dir and `diff -r` against the committed trees; on failure print the exact regeneration command.

### 5.3 UI / E2E tests

**`clean-machine-smoke` (the flagship gate).** Fresh `archlinux:base` container — deliberately *not* `base-devel`, with no Rust toolchain, no cargo cache, and no prior `mg-calr` state:

1. `pacman -U` the artifact from §5.2 job 1. Assert install succeeds and the post-install message appears verbatim.
2. As an unprivileged user with an empty `$HOME`: `mg-calr --version` equals `pkgver`; `mg-calr config paths --json` shows the five XDG paths under that `$HOME`.
3. Snapshot the server-side state digest (`pg_roles`, `pg_database`, `pg_hba.conf`), run `mg-calr init --json` with PostgreSQL absent, assert `database_reachable:false` and a nonempty `administrator_guidance` array, re-snapshot, and assert the digests are unchanged — proof that diagnosis mutates nothing.
4. Extract the `admin`-tagged fenced blocks from `docs/SETUP-POSTGRESQL.md` and execute them as root, then the `user`-tagged blocks as the user. The documentation is therefore executable truth: prose that rots fails CI.
5. Assert the created role has no `SUPERUSER`, `CREATEDB`, `CREATEROLE`, `REPLICATION`, or `BYPASSRLS` attribute.
6. Run the synthetic flow, entirely unprivileged, driven by `ci/smoke-manifest.toml`: `database migrate` → `database status` → `calendar create --name Personal` → `calendar list --json` → `event create` (timed, `America/Los_Angeles`, spanning the 2026-11-01 DST fold) → `event create --all-day-start/--all-day-end` → `event day-agenda` → `event show` → `event edit` → `event cancel` → `event restore` → `agenda --json` → `event export` → `event import` of that same export into a second disposable database with digest comparison → `interop export --json` → `interop import-todo` with a synthetic `mg.interop/1` snapshot → `todo scan-reminders --dry-run`. Every `--json` output is schema-validated and every exit status is asserted against the documented table.
7. **Network denial.** Run steps 2 and 6 inside `unshare -rn` (no interfaces beyond loopback-less namespace) with PostgreSQL reached over the `/run/postgresql` Unix socket bind-mounted in. Every command must succeed; any attempted network syscall fails the job.
8. **Completion E2E.** Under a PTY with PostgreSQL *stopped*: `bash -lc 'source /usr/share/bash-completion/completions/mg-calr; complete -p mg-calr'` registers a function; drive `mg-calr <TAB>` and `mg-calr event --<TAB>` and assert the candidate sets equal the clap tree's subcommand and flag names. Repeat in `zsh -lc 'autoload -Uz compinit; compinit -u; …'` and in fish. Assert `strace`/`ss` observes no socket connect during completion.
9. **Man E2E.** `man mg-calr`, `man mg-calr-event`, `man 5 mg-calr` all render; `man --warnings` emits nothing at `MANWIDTH` 40, 80, and 200; `EXIT STATUS` rows match the binary's actual exit codes for three probe commands.
10. **Removal.** `pacman -R mg-calr`; assert no `mg-calr` file remains under `/usr`, `$HOME` is unchanged, and the PostgreSQL database still exists with intact data.
11. **Dev-install E2E.** In a separate container with a source checkout: `scripts/dev-install.sh --dry-run` prints a plan and writes nothing; `--yes` installs into `~/.local` and produces a manifest; running it as root exits 77; running it while the package is installed refuses each pacman-owned path; `--uninstall` removes exactly the manifest entries and leaves a hand-edited file in place.

### 5.4 Visual / manual verification

- **Terminal themes:** post-install message, `init` report, `doctor --check packaging`, and `dev-install.sh` output reviewed on light and dark terminals and on the bare Linux VT — confirm no information depends on color and no ANSI is emitted (H introduces no color at all, so both themes render identically by construction).
- **Text size / width extremes:** man pages at `MANWIDTH` 40 and 200; `init` output at 40 and 200 columns; confirm no wrap breaks a copyable administrator command across lines in a way that changes it.
- **Screen size extremes:** an 80×24 VT during `pacman -U` — confirm the post-install message is fully visible without scrolling past pacman's own output.
- **Empty vs. populated states:** `doctor --check packaging` on (a) a `cargo run` binary with nothing installed, (b) a package-only install, (c) a package plus a shadowing dev install; confirm each renders a distinct, unambiguous, non-alarming report.
- **Documentation review:** `docs/SETUP-POSTGRESQL.md` and `docs/SETUP-ICLOUD.md` read end to end in a plain-text viewer; confirm every privileged line is visibly labeled, every command is copyable as a single line, and no instruction anywhere asks the reader to place a credential in a file.
- **Rendered completion menus** in bash, zsh, and fish photographed for the record; confirm zsh/fish descriptions match `--help` text.

---

## 6. Compliance & Safety Gate

### 6.1 Sensitive data classification

- [ ] No sensitive data involvement
- [x] **Handles sensitive data — describe protection measures.** H does not read calendar or todo content, but it defines the surfaces through which credentials could leak, so it is treated as credential-adjacent. Protections: no packaged file may contain a credential (secret-scan gate over history, working tree, built package, generated artifacts, and `dist/`); `config.toml` gains a hard rejection of credential-shaped keys (`config_plaintext_credential`, exit 78) that names the key and never the value; `docs/SETUP-ICLOUD.md` routes the Apple app-specific password exclusively through an external secret command (`secret_command = ["pass", "show", "icloud/mg-calr"]`, argv array, no shell, scrubbed environment, timeout, bounded output, held in `Zeroizing<String>`, never logged, never in `--json`, never written to disk or a vdir) and explicitly instructs the reader never to place it in a config file, an environment variable persisted in a shell rc, or a command-line argument; the existing `ConnectionSettings::safe_summary` URL redaction is retained and extended by a `credentials.argv_exposure` doctor warning when `--database-url` carries userinfo, because argv is visible in `/proc/*/cmdline` and shell history; `~/.pgpass` at mode 0600 is documented as the alternative to embedding a password in a URL. Peer authentication over `/run/postgresql` remains the default, so the common case needs no credential at all.
- [x] **Uses synthetic/test data only until compliance gate clears.** Every CI fixture — calendars, events, todos, the `mg.interop/1` snapshot, the projection store — is synthetic. No real iCloud account, real Apple ID, or real personal calendar is used anywhere in CI or in the documentation examples; the docs use `icloud/mg-calr` as a password-store path placeholder, never a real address.

### 6.2 Asset provenance

- [ ] No third-party assets
- [x] **Uses third-party assets — list each with source, license, and rights status.**

No fonts, images, icons, audio, video, datasets, or ML models are bundled — the product has no graphical surface. The third-party material is entirely Rust crates plus two data tables they embed. Licenses below were read from the vendored `Cargo.toml` of each crate in this workstation's registry, not assumed.

| Class | Representative crates in `Cargo.lock` | License posture | Rights status |
|---|---|---|---|
| CLI parsing / terminal styling | `clap`, `clap_builder`, `clap_derive`, `clap_lex`, `anstream`, `anstyle*`, `colorchoice`, `strsim`, `heck`, `is_terminal_polyfill` | `MIT/Apache-2.0` (dual) | clear; permissive |
| Generation (new) | `clap_complete`, `clap_mangen` | `MIT OR Apache-2.0` | clear; build-time only, not linked into the shipped binary |
| Serialization | `serde`, `serde_core`, `serde_derive`, `serde_json`, `itoa`, `zmij` | `MIT OR Apache-2.0`; **`zmij` is `MIT` only** | clear; `zmij` is a less-familiar transitive float-formatting crate and is flagged for individual review at its next version bump |
| TOML | `toml`, `toml_datetime`, `toml_parser`, `toml_writer`, `winnow` | `MIT OR Apache-2.0` | clear |
| Time | `chrono` (`MIT OR Apache-2.0`), `chrono-tz` (`MIT OR Apache-2.0`), `iana-time-zone` | dual-permissive **plus an embedded data asset**: `chrono-tz` compiles in the IANA time zone database, which is public domain | clear; the tzdata's public-domain status and its update cadence are recorded in `deny.toml` notes because stale tzdata is a correctness risk, not only a licensing one |
| Async runtime / PostgreSQL | `tokio` (**`MIT` only**), `tokio-postgres`, `postgres-protocol`, `postgres-types`, `mio`, `bytes`, `socket2`, `slab`, `futures-*` | mixed `MIT` and `MIT OR Apache-2.0` | clear; permissive |
| Identity / hashing | `uuid`, `sha2`, `digest`, `block-buffer`, `crypto-common`, `generic-array`, `hybrid-array`, `typenum`, `hmac`, `md-5`, `cpufeatures`, `cmov`, `ctutils`, `chacha20`, `base64` | `MIT OR Apache-2.0` / `Apache-2.0 OR MIT` | clear |
| Unicode | `unicode-normalization`, `unicode-bidi`, `unicode-properties`, `unicode-ident`, `stringprep`, `tinyvec` | `MIT/Apache-2.0` **plus embedded Unicode Character Database tables**, whose data terms are the Unicode license (UNICODE-DFS/Unicode-3.0) | clear; the UCD data terms are added to the `deny.toml` license allowlist explicitly rather than inherited silently |
| Platform / syscalls | `libc`, `rustix` (`Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`), `linux-raw-sys`, `errno`, `fs2` (`MIT/Apache-2.0`), `parking_lot*`, `lock_api`, `smallvec`, `scopeguard` | permissive; the LLVM exception is compatible with either project license choice | clear |
| Regex / text search (transitive) | `regex`, `regex-automata`, `regex-syntax`, `aho-corasick` (**`Unlicense OR MIT`**), `memchr`, `bstr` | permissive | clear |
| Hashing tables | `phf`, `phf_shared`, `siphasher` (**`MIT` only**), `hashbrown`, `indexmap`, `equivalent` | permissive | clear |
| Environment | `whoami` (`Apache-2.0 OR BSL-1.0 OR MIT`) | permissive; BSL-1.0 is in the allowlist | clear |
| Errors | `thiserror`, `thiserror-impl` | `MIT OR Apache-2.0` | clear |
| Dev-only (never shipped) | `assert_cmd`, `predicates*`, `tempfile`, `difflib`, `termtree`, `wait-timeout`, `normalize-line-endings`, `float-cmp` | permissive | clear; excluded from the package |
| Target-gated, never built on Linux | `windows-*`, `winapi*`, `wasm-bindgen*`, `js-sys`, `web-sys`, `objc2-*`, `core-foundation-sys`, `redox_syscall`, `libredox`, `wasi*` | permissive | present in the lockfile, absent from the Linux build; `cargo deny` still evaluates them |

**Enforcement, not assertion.** `deny.toml` sets an explicit allowlist — `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `BSL-1.0`, `Unicode-3.0`, `Unlicense`, `CC0-1.0` — and CI job `cargo deny check licenses advisories bans sources` fails on any crate outside it, any crate with no license field, any duplicate-version ban violation, and any advisory. New transitive crates therefore cannot enter silently.

**Unresolved project license (Q1).** `mg-calr` itself ships **no `LICENSE` file today**; `README.md` and `docs/PRODUCT.md` both record MIT versus Apache-2.0 as undecided. This is a real blocker for H, not a footnote: an AUR package without a resolvable `license=()` value is not publishable, and a source tarball with no license grant is not redistributable. The `release.yml` workflow therefore contains a hard `license-gate` job that fails every tagged release while `LICENSE` is absent, with a message pointing at Q1. Note for the decision: because effectively the entire dependency graph is dual `MIT OR Apache-2.0` (with `tokio`, `zmij`, `siphasher`, and `phf` MIT-only), either choice is compatible; the Rust-ecosystem convention of dual `MIT OR Apache-2.0` (which adds Apache's explicit patent grant while preserving MIT compatibility) would maximize downstream reuse. This spec does not decide it.

### 6.3 Language / claims audit

- [x] **Makes claims not supported by evidence?** No — and several statements were deliberately narrowed to keep it that way. `arch=('x86_64')` only, because no `aarch64` builder exists. `optdepends` and documentation name exactly the PostgreSQL versions the CI matrix runs (16/17/18). No prebuilt binary is published, because reproducibility of a binary is not yet demonstrated. `SHA256SUMS.asc` is described as gated on a signing key rather than promised. MSRV 1.85 is claimed only because a dedicated CI job builds at exactly that version.
- [x] **Promise capabilities not yet built?** No. `docs/SETUP-ICLOUD.md` must state in its first paragraph that `mg-calr` has **no synchronization command yet** and that the document prepares a credential for the future F12 transport; it must not imply that installing the package enables iCloud sync. No systemd unit is shipped, because reminder delivery (E8) does not exist. The post-install message lists only steps that work today. `mg-calr-gen` is not installed, so no user-facing command is advertised that has no man page.
- [x] **Use language restricted by domain regulations?** No. This is a personal calendar tool; no health, financial, legal, or safety claim appears in any packaging string, man page, or setup document. The word "secure" is not used as a bare adjective anywhere; security statements are specific and testable ("mg-calr never invokes sudo", "no network socket outside explicit synchronization commands").

### 6.4 Regulatory alignment

Walking Lens 3 by name:

- **I1 Lossless iCalendar — addressed at the packaging boundary, codec deferred to F1/F2.** H ships no iCalendar parser or serializer and must not become a place where fidelity is lost. Three binding packaging-level gates: (a) build configuration may not change serialization — the `clean-machine-smoke` flow performs `event export` → `event import` → re-export in the packaged release build and asserts byte equality with the same round trip from a debug build, so `lto`/`codegen-units`/optimization level cannot alter output; (b) no packaging step, scriptlet, upgrade hook, or migration rewrites stored data, so no round-trip property can be damaged by installing or upgrading; (c) `docs/SETUP-ICLOUD.md` may not instruct a user to hand-edit `.ics` files or pre-transform data outside the application, which would be a lossy path around the future codec. Full unknown-property preservation semantics are F1's to specify and prove; H's architecture — a single binary, no packaged transformer, no data-touching scriptlet — does not preclude them.
- **I2 Sync authority — confirmed.** PostgreSQL remains the sole authority and packaging must not create a competitor. Binding: the package installs no `/etc` configuration that could shadow XDG resolution; installs no embedded database; ships no cache or index that could be read as authoritative; installs no unit or timer that would write anything in the background; and never runs a migration at install or upgrade time — the user runs `database migrate` explicitly. `pacman -R` removes only `/usr` files and leaves the database and every XDG directory untouched, proven by the removal test in §5.3 step 10. The future durable vdir mirror (F4) will live under an XDG path owned by the application, not under `/var` owned by the package.
- **I3 Conflict/deletion — N/A for H's own behavior, with explicit deferral and an architecture note.** H creates, resolves, and tombstones nothing; it has no two sides to reconcile. Deferral: three-way conflict handling, deterministic resolution, and tombstone separation are F8/F9/F10's contract. Architecture note — H must not preclude them, so: migrations remain append-only and no packaging path may execute `DROP TABLE` (asserted by `tests/migration_contract.rs` today and by the packaging scriptlet grep in §5.1); the upgrade test in §5.2 job 6 proves that installing a newer package over an older one preserves every seeded row; downgrade is documented in `docs/RELEASING.md` as "install the older package, do **not** attempt to un-apply migrations", so a downgrade can never silently destroy data written by the newer schema. The one deletion H itself performs — `dev-install.sh --uninstall` — is digest-guarded and removes only files it recorded and that are still unmodified.
- **I4 Scope/network — always applies; confirmed and test-enforced.** The installed application performs no network access in any H path: install, first run, `init`, `doctor`, `config paths`, `config example`, completion, and man rendering are all offline, and PostgreSQL is reached over a Unix socket by default. Proofs: `clean-machine-smoke` runs the entire flow inside `unshare -rn`; the completion E2E runs with the database stopped and asserts no socket connect; §5.2 job 9 asserts the lockfile contains no HTTP or TLS client crate, so the binary has no HTTP capability to misuse; static completions are mandated precisely so TAB cannot become a hidden connect path; and no packaging scriptlet performs any download. Network use during `makepkg`'s source fetch and `cargo fetch --locked` is the *build system's*, occurs before the artifact exists, and is bounded by lockfile pinning plus SHA-256 verification — it is not application behavior. When F transport lands, its one HTTP client becomes an explicit allowlist entry in the same CI check rather than a silent addition.

Other lenses, addressed by this feature: **T3/T4** — packaging never mutates the database, upgrades preserve data, migrations stay append-only and advisory-locked, and the migration matrix plus upgrade test are CI gates. **C3** — no `/etc` config, XDG precedence preserved, the example config is proven to document the real compiled defaults, unknown keys warn rather than fail (a compatibility policy), and credential-shaped keys are rejected. **C4** — every H surface is plain text with no color dependence, validated at 40 and 200 columns. **C5** — `init` and `doctor --check packaging` are strictly non-mutating with stable machine check IDs and a prerequisite matrix, and the server-state digest comparison in §5.3 step 3 proves diagnosis changes nothing. **O1** — no credential in any packaged file, secret scan over history and artifacts, plaintext-credential rejection, argv-exposure warning, URL redaction retained. **O2** — privileged steps are printed and labeled, never executed; the provisioned role is asserted to have no `SUPERUSER`/`CREATEDB`/`CREATEROLE`; migrations run unprivileged; clean-machine recovery is executable *documentation* driven by doc-extracted commands. **O3** — `panic = "unwind"` is retained so exit 101 stays deterministic, and the generated `EXIT STATUS` table is derived from `AppError::exit_code` so the documented contract cannot drift. **O4** — this feature *is* the O4 evidence: CI clean-machine, migration, secret scan, package build, and synthetic E2E gates are all specified above.

---

## 7. Gap Analysis vs. Current State

### 7.1 What exists today

- **absent** — `packaging/` directory, `PKGBUILD`, `PKGBUILD-git`, `mg-calr.install`. Nothing in the repository produces an installable artifact.
- **absent** — any CI configuration. There is no `.github/`, no workflow file, no `.gitlab-ci.yml`, and `git remote -v` is empty. All quality commands exist only as prose in `README.md` under "Development".
- **absent** — man pages, `man/` directory, `scdoc` source.
- **absent** — shell completions and any generation path. `clap_complete` and `clap_mangen` are not in `Cargo.toml`.
- **absent** — `scripts/`, `deny.toml`, `.gitattributes`, secret scanning, release tooling, checksums, `dist/`.
- **absent** — `docs/SETUP-ICLOUD.md`, external secret-command support, and any credential-key rejection in `src/config.rs`. `FileDatabase` has `url`/`socket_dir`/`user`/`dbname` only and silently ignores unknown keys.
- **absent** — `LICENSE`. `README.md` and `docs/PRODUCT.md` both record MIT versus Apache-2.0 as unresolved; this **gates** AUR publication and tagged releases.
- **implemented** — the clap command tree, but *privately inside the binary*: `Cli`, `Command`, and every `Args`/`Subcommand` type are defined in `src/main.rs` (lines 26–400) and are not reachable from `src/lib.rs`. This is the single structural blocker for H4.
- **implemented** — administrator guidance as printed-only text: `src/main.rs:1211–1216` emits five `administrator_guidance` strings from `init`, including `sudo -u postgres createuser`/`createdb` examples and the explicit line "mg-calr never invokes sudo or provisions roles/databases"; `src/storage.rs:74` carries the same posture in the connect error. `doctor` currently returns an empty guidance vector.
- **implemented** — stable exit codes and JSON error envelope (`src/lib.rs`, `AppError::code`/`exit_code`), which the man page's `EXIT STATUS` section will be generated from.
- **implemented** — XDG resolution and precedence with pure tests (`src/config.rs`, `tests/config_contract.rs`).
- **implemented, minimal** — `config/example.toml` (12 lines) documents only `[database]` with `socket_dir`, `dbname`, commented `user`, and a commented `url` plus a "Never commit credentials" note. It is not installed anywhere, not printed by any command, and not verified by any test.
- **implemented, opt-in and manual** — the migration/integration test (`tests/postgres_integration.rs`, gated on `MG_CALR_RUN_DATABASE_TESTS` and `MG_CALR_TEST_DATABASE_URL`) and the offline migration contract (`tests/migration_contract.rs`, five embedded migrations, asserts no `DROP TABLE`). Nothing runs them automatically.
- **implemented** — lint posture in `Cargo.toml`: `unsafe_code = "forbid"`, `clippy::all = "deny"`, `clippy::pedantic = "deny"`. No `[profile.release]` section exists.
- **prototyped** — developer install: `README.md` documents `cargo fmt`/`clippy`/`test` and `cargo build`; `cargo install --path .` works implicitly but installs no completions or man pages and is not documented.
- **planned** — iCloud transport, vdirsyncer discovery, and the secret command (feature-tree F5/F12); systemd reminder units (E8). H documents and prepares for these without implementing them.

### 7.2 Delta to spec

**New files.** `packaging/arch/{PKGBUILD,PKGBUILD-git,mg-calr.install}`; `src/cli.rs`; `src/bin/mg-calr-gen.rs`; `completions/{mg-calr.bash,_mg-calr,mg-calr.fish}`; `man/{mg-calr.1,mg-calr-*.1,mg-calr.5.scd}` and `man/sections/*.roff`; `scripts/{dev-install.sh,ci-local.sh,release.sh}`; `docs/{SETUP-POSTGRESQL.md,SETUP-ICLOUD.md,RELEASING.md}`; `.github/workflows/{ci.yml,release.yml}`; `deny.toml`; `.gitattributes`; `ci/{smoke-manifest.toml,smoke.sh,Containerfile.clean}`; `tests/packaging_contract.rs`; `LICENSE` (gated on Q1).

**Modified files.** `src/main.rs` — move all clap type definitions out to `src/cli.rs`, keep dispatch and rendering; `src/lib.rs` — add `pub mod cli`, add `EXIT_STATUS_TABLE`, add the exhaustive exit-code test hook; `src/config.rs` — add `ConfigError::PlaintextCredential`, `ConfigWarning::UnknownKey`, and unknown-key collection over the parsed `toml::Table`; `Cargo.toml` — add `[features] gen`, the two generator dev/optional dependencies, `[[bin]] mg-calr-gen` with `required-features`, and a `[profile.release]` block (`lto = "thin"`, `codegen-units = 1`, `panic = "unwind"`); `config/example.toml` — expand to an annotated file covering every supported key with an explicit "credentials never go here" section; `README.md` — replace the ad-hoc setup snippet with links to the new setup documents and add an Installation section; `docs/PRODUCT.md`/`docs/ARCHITECTURE.md` — record the `src/cli.rs` boundary and the packaging invariants.

**Migrations / schema changes.** None. H adds no migration, and the packaging scriptlet grep exists specifically to keep it that way.

**New dependencies.** `clap_complete`, `clap_mangen` (feature-gated, build-time only). CI/maintainer tooling: `scdoc`, `pacman-contrib`, `namcap`, `shellcheck`, `gitleaks`, `cargo-deny`, a container runtime. No new runtime dependency for the installed binary beyond `gcc-libs` and `glibc`.

### 7.3 Estimated scope

**L.** The Rust delta is genuinely small — one module move, one 60-line generator binary, two config validations, a profile block — but the surface area is wide and each piece carries its own evidence requirement: two PKGBUILDs with a `check()` that must pass without a database; a reproducible release pipeline with three-way checksum agreement; three shells' completions plus roff generation plus a drift gate; two setup documents whose fenced blocks must be executable in CI; and an eleven-job pipeline whose flagship job provisions PostgreSQL in a fresh container, runs an end-to-end flow under a network namespace, and asserts that nothing privileged was executed. The container jobs are also the slowest to iterate on. Deliver as dependency-ordered slices — (1) `src/cli.rs` move plus generator plus committed artifacts and their drift test; (2) example config, setup docs, config credential rejection; (3) PKGBUILD plus package-content assertions; (4) release/checksum tooling; (5) CI pipeline; (6) clean-machine smoke — rather than one patch. Slices 1–3 are each S; 5–6 are M on their own.

### 7.4 Blocking dependencies

- **A1/A2/A4 — satisfied.** XDG configuration, the non-mutating `init`/`doctor` diagnostics, migrations, and the stable error/exit contract already exist and are what H packages and documents.
- **`src/cli.rs` extraction blocks H4 absolutely.** Completions and man pages cannot be generated from a `Cli` type that lives privately in a binary crate. This is the first task in the first slice.
- **Q1 (project license) blocks H1 publication and H2 releases**, not local `makepkg`. The `license-gate` job makes this explicit rather than letting an unlicensed tarball ship.
- **Q4 (CI host) blocks the hosted pipeline**, not the gates themselves — `scripts/ci-local.sh` runs the identical job list on the workstation, so H7's substance is deliverable before a forge exists.
- **F12 blocks the iCloud secret command's *implementation*, not H6's documentation or the plaintext-credential rejection.** H specifies and documents the credential contract and implements the config-side refusal; F12 implements the fetch, the `Zeroizing` handling, and the transport. `docs/SETUP-ICLOUD.md` must therefore state plainly that no sync command exists yet.
- **The smoke flow's command list is bounded by what B/C/D have shipped.** `ci/smoke-manifest.toml` is the coupling point: it covers exactly the commands that exist, and the manifest-coverage unit test forces every future command into it. E (reminder delivery), F (sync), and G (backup/undo) commands enter the smoke flow as they land; H does not wait on them.
- **E8 systemd units are explicitly out of scope for H1** and must not be pre-installed.

---

## 8. Open Questions

- **Q1:** MIT, Apache-2.0, or the Rust-conventional dual `MIT OR Apache-2.0` for `mg-calr` itself? — blocks: §6.2 rights status, the PKGBUILD `license=()` field, AUR publication, and every tagged release via the `license-gate` job. Recommendation for consideration only: dual `MIT OR Apache-2.0`, since the dependency graph is already overwhelmingly dual-licensed and Apache-2.0 adds an explicit patent grant. The decision is the user's.
- **Q2:** Publish to the AUR as `mg-calr` (release) and `mg-calr-git` (VCS), or keep both PKGBUILDs repository-local for now? — blocks: whether the release pipeline needs an AUR push step and a maintainer identity in the PKGBUILD `# Maintainer:` line. Does not block local `makepkg -si`.
- **Q3:** Is there an OpenPGP key available for signing `SHA256SUMS`, and should the PKGBUILD gain `validpgpkeys` plus a `.sig` source? — blocks: §4.3's `SHA256SUMS.asc` row only. SHA-256 checksums are mandatory regardless and are not gated on this.
- **Q4:** Where does CI run — GitHub Actions, a self-hosted runner, or `scripts/ci-local.sh` on the workstation only? The repository currently has no git remote. — blocks: the concrete workflow files, not the job definitions.
- **Q5:** `scdoc` for `man 5 mg-calr`, or hand-written roff to avoid a makedepend? — blocks: §4.5's makedepends list. `scdoc` is small, packaged in Arch's repositories, and far more maintainable; hand-written roff removes one build dependency.
- **Q6:** Should the PostgreSQL CI matrix cover 16/17/18, or narrow to 18 only, matching the `docs/PRODUCT.md` "PostgreSQL 18 compatibility" decision? — blocks: §4.6's stated version support and the `optdepends` wording. Whichever is chosen, the documentation must claim exactly what the matrix tests.
- **Q7:** Publish the optional `-vendor.tar.zst` so `makepkg` can build fully offline, at the cost of a ~25 MiB artifact per release? — blocks: §4.3's artifact table only.

Resolved by this spec and not open for implementation reinterpretation: no `/etc` configuration; no privileged operation in any scriptlet; static (never dynamic) completions; generation from the live clap tree with a CI drift gate; committed generated artifacts; no prebuilt binary in v1; `panic = "unwind"`; `arch=('x86_64')` until a builder exists; and credential-shaped config keys rejected with the value never echoed.
