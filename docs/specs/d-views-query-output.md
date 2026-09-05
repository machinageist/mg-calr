# Spec: Views, Querying, and Output

**Feature ID:** d-views-query-output
**Parent feature:** D (D1–D10)
**Spec author agent:** Hermes Agent
**Date:** 2026-08-23
**Iteration:** 2

---

## 1. Purpose

### 1.1 One-sentence job

Let a keyboard-first user inspect events and due todos for a bounded period, narrow the same result set with predictable queries, and consume an equivalent accessible human or stable JSON projection without risking mutation.

### 1.2 Why it matters

`mg-calr` is useful as a daily driver only when “what is happening?” is fast and trustworthy in a terminal and when later Quickshell/TUI clients can consume the same application contract. A unified projection avoids separate calendar and task silos, while strict selector and output rules prevent an ambiguous search result from becoming an unsafe mutation target.

### 1.3 Success signal

Against the synthetic DST/all-day fixture corpus, no-argument, day, week, month, filter, and search commands return the correct ordered items; human and JSON renderings have identical query fingerprints and ordered item identities; p95 warm query-and-render time remains within the budgets in §4.8.

---

## 2. User Stories

> As a user, I want `mg-calr` with no arguments to show today in my effective IANA timezone, so that my next event and due work are immediately visible.

> As a planner, I want chronological day/week agendas and a Sunday-first month grid plus agenda, so that short- and medium-range commitments are easy to scan.

> As a user with both events and todos, I want one ordered view with type labels, so that due work is visible without being mistaken for an event.

> As a keyboard-only user, I want structured filters, bounded full-text search, and a deterministic chooser, so that I can find an item without terse modifier grammar or a pointer.

> As an automation author, I want versioned, deterministic JSON generated from the same query as human output, so that scripts and later public clients do not scrape terminal text or query PostgreSQL.

> As a user in a narrow or non-color terminal, I want readable, untruncated identity and time semantics with no color-only information, so that the view remains operable.

> As a user who enters an ambiguous short ID, I want a clear non-mutating failure or an explicit chooser, so that the wrong event or todo is never inspected or changed.

---

## 3. UX Specification

### 3.1 Screen / view inventory

All views are new terminal command projections; none is a modal graphical screen.

| Feature | Command/path | Layout |
|---|---|---|
| D1 Today/default agenda | `mg-calr`, `mg-calr agenda`, `mg-calr agenda --today` | chronological agenda for one local civil day |
| D2 Day | `mg-calr day DATE` | chronological agenda for the selected local civil day |
| D3 Week | `mg-calr week [DATE]` | seven day sections, Sunday through Saturday, containing the selected date; default today |
| D4 Month | `mg-calr month [YYYY-MM]` | compact Sunday-first grid with day markers, followed by chronological month agenda; default current month |
| D5 Combined projection | every agenda/query/search view unless narrowed by `--kind` | one tagged, deterministically ordered event/todo result set |
| D6 Filter | view command plus repeatable filter flags, or `mg-calr query ...` | same agenda projection over an explicit/default date range |
| D7 Search | `mg-calr search QUERY ...` | relevance-ranked result list unless `--sort chronological`; bounded by an explicit/default range |
| D8 Human/color/width rendering | default renderer for every view/query command | linear terminal text or width-aware month grid; semantics never depend on color |
| D9 Stable JSON | append `--json` to any view/query command | compact versioned envelope from the same immutable result |
| D10 Chooser | mutation/inspect command with `--choose`, or automatic only in an interactive TTY after a structured selector returns multiple candidates | numbered, paged terminal list; selection is explicit and cancelable |

Global projection flags are `--json`, `--no-color`, and `--width COLUMNS`. `NO_COLOR` disables ANSI. `--json` is non-interactive and implies no ANSI; `--width` is rejected with JSON because JSON is width-independent. Date shortcuts are limited to the product-approved bounded forms (`today`, `tomorrow`, `next fri`); strict ISO dates remain accepted. No unconstrained natural-language parsing is introduced.

### 3.2 Interaction flows

#### Agenda and calendar views

1. Parse all input before opening the database. Resolve effective IANA timezone from explicit `--timezone`, configuration, then system timezone. An invalid or unavailable zone fails; it never silently falls back to UTC or a fixed offset.
2. Convert requested civil date/range to exact half-open instants in that zone. A local day may be 23, 24, or 25 hours. All-day bounds remain civil dates and are not converted through midnight instants for inclusion decisions.
3. Execute one read-only application query that expands recurrence only inside the bounded range and projects live events plus eligible todos.
4. Sort by the rules in §4.3 and assign one `query_fingerprint` to the normalized request and snapshot.
5. Render the returned projection as human text or JSON. Renderers may not re-query, re-filter, re-sort, inspect the host clock, or resolve timezones independently.
6. Empty human output says `No events or due todos.` and identifies the range. Empty JSON returns `items: []`; emptiness is success.

#### Month

1. Resolve the calendar month in the effective timezone.
2. Print a Sunday-first seven-column grid. Each day cell has a color-independent marker: `E` event, `T` todo, `B` both; `.` no due item. The selected/current date uses brackets in addition to any color.
3. Follow with the same month query's chronological agenda. The grid summary and agenda are derived from that single result snapshot; they cannot disagree.

#### Structured filtering (D6)

Filters are typed flags, combined with logical AND across fields and OR within repeated values of one field:

- `--from DATE|DATETIME`, `--to DATE|DATETIME` define a half-open range; both accept timezone-qualified explicit datetimes. A date `--to` is the exclusive civil date.
- `--kind event|todo` (repeatable).
- `--calendar SELECTOR` (event-only criterion), `--project TEXT` (todo-only criterion), `--tag TEXT` (repeatable OR), `--priority none|low|medium|high|urgent` (repeatable OR).
- `--status active|completed|cancelled` (repeatable OR). The default is `active`. `completed` applies only to todos and `cancelled` only to events; an inapplicable value excludes the other kind. Trashed/tombstoned records and unresolved conflict branches are not statuses and are never made visible by this flag.
- `--blocked yes|no`, `--all-day yes|no`, `--has-reminder yes|no`.

A criterion inapplicable to an item's kind excludes that item; it is not ignored. Unknown fields/values and contradictory ranges fail before database access. There is no eval syntax, SQL fragment, regex, shell expansion, or implicit mutation.

#### Full-text search (D7)

1. Treat `QUERY` as UTF-8 user text, not SQL. Normalize Unicode consistently for matching but preserve stored display text.
2. Search event title/description/location/URL/categories and todo title/notes/project/tags. Search does not include RFC UID, extension-property blobs, audit history, credentials, or reminder payloads.
3. Default range is `[today - 1 year, today + 1 year)` in the effective zone to bound recurrence and work. `--all-time` is permitted only for stored, non-recurring rows plus recurrence masters; occurrence expansion still requires `--from` and `--to`.
4. Rank exact title, title prefix, title token, then other-field token matches; break ties by projection chronology, kind, immutable UUID. Human and JSON expose the same final order and a non-contractual numeric `rank` only for search results.
5. Empty query and control characters fail. Input is parameterized; wildcards/operators are literal unless a future version explicitly adds grammar.

#### Selector and chooser (D10)

1. Full UUID is authoritative. A short ID is a lowercase, hyphen-free prefix of the canonical UUID hex, 8–32 characters; parsing is case-insensitive but output is lowercase. Short IDs are selectors only and are never stored as identity.
2. Resolution always queries all live allowed entity kinds in command scope. Exactly one match succeeds. Zero returns `selector_not_found`. More than one returns `selector_ambiguous` in `--no-input`, JSON, piped/non-TTY, and every bulk command.
3. An interactive TTY may enter the chooser only when the caller omitted `--no-input` and either requested `--choose` or the command explicitly documents chooser fallback. Candidates show kind, sufficient short ID, title, and temporal/project context. They use the same query projection as the preselection result.
4. Number selection requires Enter. `q`, Escape, EOF, or interrupt cancels with no mutation. The selected full typed UUID is re-resolved inside the eventual application transaction; if it disappeared or changed eligibility, the operation fails rather than selecting a replacement.
5. The displayed short-ID length expands deterministically from 8 characters until all candidates in that rendered set are unique. A command never accepts a prefix that was merely unique in a previously displayed subset.

### 3.3 Layout descriptions

#### Human agenda rows

Top to bottom: range heading; optional active-filter summary; day headings; item rows; optional continuation/error guidance. Each item row preserves these semantic fields in order:

1. kind token: `[E]` or `[T]`;
2. state token: `[ACTIVE]`, `[COMPLETED]`, or `[CANCELLED]`; it is always printed and never inferred from color or strike-through;
3. time token: `HH:MM–HH:MM`, `ALL-DAY`, or `DUE HH:MM`/`DUE DATE`;
4. temporal disambiguator for timed values: numeric UTC offset and IANA zone. On a fold, append `fold=0` for the earlier instant and `fold=1` for the later instant. The offset is retained at every width; the zone may move to the mandatory continuation line but may not disappear;
5. title;
6. stable displayed short ID plus canonical complete projection identity token. Event grammar is `event:<uuid>;uid64=<base64url-utf8-rfc-uid>;rid64=<base64url-recurrence-id|master>;rev64=<base64url-source-revision>`; todo grammar is `todo:<uuid>;iid64=<base64url-instance-id|master>;rev64=<base64url-source-revision>`. Base64url is RFC 4648 URL-safe without padding, so untrusted identity text cannot alter rows. The token is present even when two rows otherwise have equal title and local time;
7. secondary context when space permits: calendar for events; project, priority, and `BLOCKED` for todos.

Items are not collapsed by equal title/time. Recurrence occurrences include their occurrence/instance key in every human row, inspect output, and JSON and remain distinguishable. A fold fixture therefore renders, for example, `01:30 -07:00 America/Los_Angeles fold=0` and `01:30 -08:00 America/Los_Angeles fold=1`; ordering is by instant, never by the repeated wall-clock label. Multi-day timed events appear in every intersected day section with continuation arrows/text; they appear only once in a single flat JSON result and carry intersection metadata. All-day events use exclusive `end_date` and occupy every civil date `start_date <= d < end_date`.

Every non-empty and empty human result ends with a parseable, color-free summary line:

```text
RESULT total=2 range=2026-11-01/2026-11-02 fingerprint=<64-lowercase-hex> snapshot=<opaque-safe-token>
```

`total`, normalized range, fingerprint, snapshot token, ordered canonical row identity tokens, and item state are mandatory semantics and cannot be hidden by width or color. `--quiet-metadata` is not supported in schema v1. The snapshot token is an opaque non-secret correlation value, not a database transaction ID or credential. This makes the human/JSON identity comparison executable without scraping titles.

Width behavior:

- Effective width precedence: `--width` > `COLUMNS` when a positive integer > terminal detection > 80. Clamp supported rendering to 40–240 columns.
- At 80+ columns, one logical item uses one line when possible. At 60–79, secondary context moves to an indented continuation line. At 40–59, title is grapheme-ellipsized first and context wraps below, but kind, state, time/date, offset/fold, complete identity token, and result summary remain verbatim. Below 40, fail with `terminal_too_narrow` and suggest `--json` or `--width 40` rather than emit misleading layout.
- Width counts display cells, not bytes or Unicode scalar values; combining characters and wide glyphs cannot corrupt columns. IDs/occurrence keys, kind, state, dates/times, UTC offset/fold, `BLOCKED`, result metadata, and ambiguity indicators are never truncated. User text may be grapheme-ellipsized and is escaped so newlines, tabs, ESC, bidi controls, and other terminal control sequences cannot alter terminal structure.
- Piped human output defaults to no ANSI and width 80 unless explicitly widened.

#### Month grid

Heading is `Month YYYY-MM · Zone`. Weekday header is `Su Mo Tu We Th Fr Sa`. Grid always has complete seven-cell rows; days outside the month are blank. Each in-month cell contains day number plus `.`, `E`, `T`, or `B`. At 40–55 columns use this two-character marker mode; at 56+ optional color/calendar accents may be added without replacing marker text. The agenda below follows the same row contract.

### 3.4 Input & gestures

All behavior is keyboard-accessible. Arrow keys or `j`/`k` move chooser focus; PageUp/PageDown page; Home/End move to bounds; digits select a visible numbered item; Enter confirms; `q`/Escape cancel. When stdin or stdout is not a TTY, chooser controls are disabled and ambiguity is an error. No touch, pointer, stylus, voice, sound, haptic, or gesture is required. Shell-safe flags are the complete non-interactive interface.

### 3.5 Transitions & animation

N/A — commands produce a terminal snapshot. Chooser repaint is immediate and has no animation; reduced-motion behavior is therefore identical. Human non-chooser output is write-once and does not rewrite prior terminal lines.

### 3.6 Error states

| Trigger | Presentation | Recovery | Data-loss risk |
|---|---|---|---|
| invalid date, zone, filter, range, width, or empty search | typed usage error; field and accepted forms; exit 64 | correct input | none; fails before query |
| nonexistent local wall time in DST gap | `local_time_nonexistent`, zone and nearest valid bounds | supply offset/zone-qualified instant or valid wall time | none |
| ambiguous local wall time in DST fold | `local_time_ambiguous`, list both offsets | supply explicit offset/fold | none |
| database unavailable/query timeout | redacted `database_unavailable`/`query_timeout`; exit 69 | start/fix local database, narrow query, retry | none; read-only |
| recurrence expansion/result exceeds bound | `query_limit_exceeded`, range and limit guidance; no partial success | narrow range/filter or raise `--limit` up to 5,000 | none |
| CLI cursor requested | `pagination_not_supported`; exit 64 before database access | issue one bounded atomic query; embedded clients may hold a page session | none |
| embedded snapshot page session closed/mismatched | `snapshot_expired` or `cursor_query_mismatch`; no replacement page | restart the complete query intentionally | none |
| database role can write or lacks required read access | `database_role_unsafe`/`database_permission_denied`; doctor check ID and administrator-run recovery | select the documented read-only profile/fix grants | none; command does not proceed |
| malformed stored temporal row | `projection_integrity_error`; identify safe typed ID, not private notes | run doctor/repair workflow; no item silently omitted | none from query; output withheld |
| terminal narrower than 40 | `terminal_too_narrow` | use `--json`, pipe, or set valid width | none |
| zero/ambiguous selector | typed candidate count and safe candidate summaries in human mode; stable code in JSON | add kind/filter/full UUID or explicitly choose | none; mutation does not start |
| chooser cancellation/EOF/item changed | cancellation or `selection_stale` | rerun query and select again | none; transaction aborts |
| stdout broken pipe | quiet conventional termination; no retry loop | rerun consumer pipeline | none; query only |
| JSON serialization failure | JSON error envelope on stderr when possible; exit 70 | report defect; human fallback is not substituted | none |

Errors never include database URLs, SQL, credentials, unescaped notes, or extension-property contents. No command in this feature writes events, todos, audit records, sync state, or reminder delivery state.

### 3.7 Accessibility

- Kind, blocked/completed/cancelled state, current date, and grid density are represented by text/symbols, never color alone.
- ANSI is disabled by `--no-color`, `NO_COLOR` (presence wins), non-TTY output, and always under `--json`.
- Heading/row order is chronological and stable for screen readers. Human output avoids decorative box-drawing as a required semantic carrier.
- Chooser focus uses both `>` and the word `selected` in a screen-reader mode enabled by `--no-color` or config; every candidate has a number, kind, title, context, and unique selector. Focus starts on the first result; keyboard order equals visual order.
- User text is escaped and bidi/control-safe. Grapheme-aware width handling prevents split characters. At least 40 columns is supported, and arbitrarily large user text cannot displace identity/state semantics.
- JSON provides the nonvisual public interface for Quickshell/TUI and command-palette integrations; those clients must invoke public commands and never read PostgreSQL directly.

---

## 4. Implementation Specification

### 4.1 Architecture placement

Target placement follows the accepted one-package architecture while preserving current boundaries:

- `src/application/query.rs`: normalized `ViewQuery`, selector resolution, one read-only use case, ordering, limits, and query fingerprint.
- `src/domain/projection.rs`: transport-neutral event/todo occurrence projection and temporal invariants; no CLI/SQL/render knowledge.
- `src/storage/query.rs`: parameterized PostgreSQL repository, bounded recurrence/search inputs, snapshot transaction.
- `src/cli/view.rs`: Clap commands/typed filters, effective timezone/range parsing, TTY/chooser policy.
- `src/render/human.rs`, `src/render/month.rs`, `src/render/json.rs`, `src/render/width.rs`: pure projections from one `QueryResult`.
- `src/cli/chooser.rs`: terminal interaction over already-returned candidates; never repository access.

Until the package grows these directories, `src/lib.rs` exports the application-facing query types. `src/main.rs` remains process dispatch only. Render code cannot depend on storage, and storage cannot emit display strings.

### 4.2 Data model

Conceptual Rust contracts (exact timezone library awaits B4/B6 evidence, but semantics are binding):

```rust
/// Half-open user range plus typed, validated predicates.
pub struct ViewQuery {
    pub range: CivilOrInstantRange,
    pub zone: IanaZoneId,
    pub kinds: KindSet,
    pub filters: QueryFilters,
    pub text: Option<SearchText>,
    pub sort: SortMode,
    pub limit: u32,
}

/// Immutable result from one repeatable-read, read-only snapshot.
pub struct QueryResult {
    pub normalized_query: NormalizedQuery,
    pub query_fingerprint: String,
    pub snapshot_token: SnapshotToken,
    pub generated_at: OffsetDateTime,
    pub items: Vec<ProjectedItem>,
    pub total: u64,
    pub truncated: bool,
}

/// Identity is copied, never synthesized from display fields. Equality and
/// deduplication use this complete value.
pub enum ProjectionIdentity {
    Event {
        event_id: EventId,
        rfc_uid: RfcUid,
        recurrence_id: Option<RecurrenceId>,
    },
    Todo {
        todo_id: TodoId,
        instance_id: Option<TodoInstanceId>,
    },
    Tombstone {
        tombstone_id: TombstoneId,
        deleted_entity_id: EntityId,
    },
    ConflictBranch {
        conflict_id: ConflictId,
        branch_id: ConflictBranchId,
        entity_id: EntityId,
    },
}

/// Version is equality-relevant for a rendered snapshot but is not identity.
pub struct ProjectionVersion {
    pub identity: ProjectionIdentity,
    pub source_revision: SourceRevision,
}

pub enum ProjectedItem {
    Event(ProjectedEventOccurrence),
    Todo(ProjectedTodo),
}

/// Identifies a derived occurrence without replacing event UUID/RFC UID authority.
pub struct OccurrenceKey {
    pub event_id: EventId,
    pub recurrence_id: Option<RecurrenceId>,
}

pub struct SelectorResolution<T> {
    pub normalized_selector: String,
    pub candidates: Vec<T>, // zero, one, or many; caller must branch explicitly
}
```

`ProjectionIdentity` is the immutable, lossless view-layer identity boundary; `ProjectionVersion` pairs it with an opaque source revision for snapshot comparisons without making revision part of identity. Normal agenda/search `ProjectedItem` variants admit only authoritative live event/todo records, but repository adapters must represent tombstone and unresolved conflict identities distinctly while deciding eligibility; they may not collapse them into the live UUID, select a conflict winner, recycle an RFC UID, or deduplicate branches. Tombstone/conflict variants are reserved for a separately authorized inspection command and are not exposed by D1–D10. That later command can reuse the type without changing default visibility. Event occurrence identity is `(event_id, rfc_uid, recurrence_id)` and remains stable across revisions; two repeated wall times in a DST fold remain distinguishable by authoritative recurrence identity plus resolved instant. Todo recurrence identity is `(todo_id, instance_id)` under the C contract. A source revision may change only when authoritative content changes; it is never synthesized from display text.

`query_fingerprint` is a lowercase SHA-256 hex digest over RFC 8785 canonical JSON containing `schema_version`, normalized query, ordered complete `ProjectionVersion` values (immutable identity plus source revision), and `snapshot_token`. It excludes render mode, width, color, generated time, titles, notes, raw iCalendar properties, reminder payloads, and credentials. Therefore the same `QueryResult` rendered twice has the same fingerprint, while a changed target set, ordering, source revision, or snapshot is detectable.

D6/D7 require PostgreSQL indexes selected by evidence: range/type indexes and a weighted full-text expression/index over explicitly searchable fields. A migration may add generated `tsvector` columns or expression GIN indexes, but must not change source text, leak search vectors through JSON, or index secret/unknown-property blobs. Recurrence occurrences remain derived and bounded; no unbounded future occurrence table is introduced.

### 4.3 API contracts

#### Application interface

```rust
async fn execute_view(query: ViewQuery, repo: &dyn QueryRepository)
    -> Result<QueryResult, QueryError>;

async fn resolve_selector(scope: SelectorScope, selector: Selector)
    -> Result<SelectorResolution<ChooserCandidate>, QueryError>;

fn render_human(result: &QueryResult, options: HumanOptions)
    -> Result<String, RenderError>;

fn render_json(result: &QueryResult, command: &str)
    -> Result<String, RenderError>;
```

The repository starts one read-only `REPEATABLE READ` transaction (or stronger equivalent), captures database `now` and an opaque `snapshot_token` once, executes only parameterized queries, and commits/rolls back without writes. Limits are deterministic: default 500 projected items and maximum 5,000 via `--limit`. If the complete result exceeds the selected limit, the command fails atomically with `query_limit_exceeded`; it never returns a silently truncated first page.

**Snapshot-bound pagination rule:** schema-v1 CLI output does not support a resumable `--cursor`; `next_cursor` is reserved and MUST be `null`, and an input cursor fails before database access with `pagination_not_supported`. This is deliberate: a PostgreSQL exported snapshot cannot be resumed after its exporting transaction exits, and neither a daemon nor a writable page cache is allowed by this feature. Interactive agenda/chooser paging operates only over the already-materialized result in memory. An application embedding may use `SnapshotPageSession<'tx>` to fetch several pages only while the same read-only transaction remains open; each page carries the same `snapshot_token`, normalized-query digest, and fingerprint basis. The final page closes the session. A changed query, connection loss, transaction close, process exit, or token mismatch returns `snapshot_expired`/`cursor_query_mismatch`, never a fresh page. Before any future CLI cursor is advertised, storage must supply an immutable revision/history mechanism or a transaction lease with bounded lifetime and tests proving no duplicates, omissions, or changed revisions under concurrent insert/update/delete. Re-querying at a new snapshot with an old position cursor is forbidden.

#### Stable JSON (D9)

Success preserves the foundation envelope and emits exactly one compact UTF-8 object on stdout with a trailing newline:

```json
{"schema_version":1,"command":"agenda","ok":true,"data":{"query":{"zone":"America/Los_Angeles","start":"2026-11-01","end_exclusive":"2026-11-02","kinds":["event","todo"],"filters":{},"search":null,"sort":"chronological"},"query_fingerprint":"<64 lowercase hex>","snapshot_token":"<opaque-safe-token>","generated_at":"2026-11-01T08:00:00Z","total":2,"truncated":false,"next_cursor":null,"items":[{"kind":"event","id":"<uuid>","short_id":"<unique lowercase prefix>","identity":{"rfc_uid":"fold-fixture@example.invalid","recurrence_id":null,"source_revision":"<opaque>"},"title":"Synthetic fold fixture","calendar":{"id":"<uuid>","name":"Test"},"time":{"type":"timed","start":"2026-11-01T01:30:00-07:00","end":"2026-11-01T01:45:00-07:00","timezone":"America/Los_Angeles","fold":0},"state":"active","status":"confirmed","busy":true,"reminder_summary":{"has_reminder":false,"definition_ids":[]}},{"kind":"todo","id":"<uuid>","short_id":"<unique lowercase prefix>","identity":{"instance_id":null,"source_revision":"<opaque>"},"title":"Synthetic due task","due":{"date_time":"2026-11-01T09:00:00-08:00","timezone":"America/Los_Angeles","fold":0},"state":"active","priority":"high","project":"Fixture","tags":[],"completed":false,"blocked":false,"reminder_summary":{"has_reminder":false,"definition_ids":[]}}]}}
```

Binding rules:

- Field names, enum strings, item ordering, null-vs-absent policy, timestamp/date formats, and envelope are compatibility contracts for schema version 1. All documented common fields are present; kind-specific inapplicable fields are absent, while applicable optional values are `null`.
- UUIDs are canonical lowercase hyphenated strings. Dates are RFC 3339 full dates; instants are RFC 3339 with numeric offset, plus an IANA timezone where civil interpretation matters. Every local datetime includes `fold:0|1`; `fold` is `0` outside an overlap. All-day time is `{type:"all_day",start_date,end_date_exclusive}` and never fabricated as midnight UTC.
- Every item has `state:"active"|"completed"|"cancelled"`; event `status` remains the RFC-derived event status and todo `completed` remains a supported-field value, but neither replaces the common accessibility state. The state agrees with the query predicate or the projection fails integrity checks.
- Every item has the kind-specific `identity` object and `source_revision`. Event identity always includes RFC UID and nullable recurrence ID; todo identity always includes nullable instance ID. `short_id` is presentation only. Integrations persist/use `id` plus the complete occurrence/instance identity. Default D commands never serialize tombstone/conflict variants.
- `snapshot_token` is mandatory and matches the human summary. In schema-v1 CLI output `truncated` is always `false` and `next_cursor` always `null`; exceeding `--limit` is an error, not partial success.
- Event and todo remain a tagged union under `kind`; clients must ignore unknown optional fields but reject unknown `schema_version` or unknown `kind` unless explicitly forward-compatible. New optional fields may be added within v1; removals, meaning/type changes, enum removals, or order changes require a schema-version change.
- Search items add `rank`; non-search items omit it. Month adds `data.month_grid` derived from the same items, with seven-cell weeks and marker counts; agenda `items` remains canonical.
- Errors retain `{schema_version:1,ok:false,error:{code,message}}` on stderr and may add non-sensitive `details` (`field`, accepted values, candidate count, recovery). No partial success object is emitted on fatal query/integrity errors.
- `--json` never prompts, emits progress, ANSI, localized keys, or human headings. Locale does not affect JSON. Stable golden fixtures freeze exact byte output after substituting only the manifest-declared volatile fields (`generated_at`, IDs, `source_revision`, `snapshot_token`, fingerprint).

#### Machine-readable contract artifacts

Implementation acceptance requires these checked-in artifacts; prose or snapshots alone do not substitute for them:

| Artifact | Binding content |
|---|---|
| `contracts/view-result-v1.schema.json` | JSON Schema 2020-12 for success, event/todo tagged unions, identity objects, fold/state enums, month grid, and `next_cursor: null` |
| `contracts/error-v1.schema.json` | error envelope, allowed D error codes and non-sensitive detail shapes |
| `contracts/human-row-v1.abnf` | ABNF for mandatory kind/state/time-offset-fold/identity tokens and `RESULT` summary; escaped title/context are opaque terminals |
| `tests/fixtures/views/manifest-v1.json` | fixture IDs, source revisions, zones, exact normalized queries, expected ordered complete identities, expected states, snapshot grouping, and golden filenames |
| `tests/fixtures/views/*.json` and `*.txt` | byte-stable JSON and human goldens from the same manifest case |
| `tests/fixtures/views/preservation/*.ics` | synthetic RFC 5545 masters/exceptions with unknown `X-` properties, parameters, folded lines, malformed-but-preserved opaque values, and expected source-byte/property digests |
| `tests/fixtures/views/concurrency.json` | page-session schedules for insert/update/delete between pages and expected snapshot identity sequence |

The schema uses `additionalProperties:true` only at documented forward-compatible object boundaries and `unevaluatedProperties:false` inside identity/time discriminated unions so a missing recurrence, fold, or source revision cannot pass. CI validates every golden with the schemas, validates ABNF-extracted human identities against the JSON identities, and verifies the manifest references no missing/orphan fixture. Canonical JSON used for fingerprints follows RFC 8785; golden wire JSON retains the documented field order above.

Stable error/exit mappings are machine data in `contracts/view-errors-v1.json`: usage/validation and `pagination_not_supported` exit 64; `selector_not_found`/`selector_ambiguous`/`selection_stale` exit 66; database/snapshot/query timeout exits 69; projection/serialization internal failures exit 70; SIGINT and broken pipe retain conventional platform behavior. Each entry includes `code`, `exit`, `retryable`, permitted detail keys, and recovery text key. Human and JSON tests load this table rather than duplicate literals.

#### Same-query projection invariant

CLI dispatch calls `execute_view` exactly once and passes that same immutable `QueryResult` by reference to one renderer. A contract-test repository panics on a second query. Human ABNF and JSON schema test projections must yield identical ordered complete `ProjectionVersion` values (immutable identity plus source revision), item states, `total`, normalized range, `query_fingerprint`, and `snapshot_token`. Width/color are presentation-only and cannot affect query normalization, candidate set, ordering, limits, or short-ID resolution. Month grid counts must sum from the same item intersections represented in its agenda. Embedded application pagination is tested separately through one `SnapshotPageSession` and one held transaction; it is not implemented as repeated `execute_view` calls.

### 4.4 State management

PostgreSQL is authoritative. Query state is immutable and command-scoped: normalized request, database snapshot token, ordered projected items, and source revisions. The CLI has no cursor state. An embedded `SnapshotPageSession` owns only a transaction-lifetime position cursor and cannot outlive its held snapshot. There is no daemon, global mutable cache, draft, local index outside PostgreSQL, or background refresh. The host clock and zone database are read once during request normalization; database `now` is read once per snapshot. Chooser state (focus/page) is ephemeral over already-materialized candidates and contains full typed IDs but no mutation authority. Offline database failure is explicit; stale cached agenda data is never presented as current.

### 4.5 Preservation and deferred-domain boundaries

D is a read projection, not an alternate recurrence engine, reminder scheduler, iCalendar serializer, sync engine, conflict resolver, or deletion workflow. The following are nevertheless binding view-layer contracts and D cannot be declared complete until their prerequisite adapters and preservation tests pass.

#### Recurrence and temporal identity

- B6/B7 and C recurrence code own expansion. D supplies a mandatory half-open bound and maximum candidate count and accepts already-typed master/exception/instance identities; it never reparses RRULE text, invents recurrence IDs, or rewrites an exception onto its master.
- Moved exceptions retain both authoritative recurrence ID (original recurrence slot identity) and resolved display instant. Cancelled exceptions appear only under `--status cancelled`; deleted exceptions remain tombstones and never reappear as generated master occurrences.
- Folded recurrence instances include resolved instant, numeric offset, IANA zone, and fold bit. Deduplication uses complete projection identity, not local wall time. A missing/duplicate exception identity is `projection_integrity_error`, not an omission.

#### Reminder identity and delivery idempotency

- `--has-reminder` means at least one enabled reminder definition is attached to the projected authoritative item/occurrence. The repository returns only typed reminder definition IDs and an aggregate boolean; renderer/search code cannot inspect or emit payloads, delivery destinations, DND state, claim tokens, or secret material.
- B8/C8 remain owners of scheduling and must provide immutable `reminder_definition_id` plus occurrence/instance identity. Delivery owns a durable database uniqueness constraint over `(reminder_definition_id, occurrence_or_instance_identity, scheduled_instant, delivery_channel)` and transactional claim/outcome state. D neither creates, claims, acknowledges, retries, resets, nor deletes those rows.
- A view snapshot taken concurrently with claim/retry reads definition membership only and cannot change delivery eligibility. The preservation fixture records claim/outcome rows and audit digest before and after every D command, forced renderer failure, broken pipe, timeout, and transaction cancellation and requires byte/row equality. Duplicate-definition identity or unstable occurrence identity fails projection. Reminder exactly-once presentation and sleep/DND retry behavior remain B8/C8/E responsibilities; D honestly gates `--has-reminder` and D-complete status on their durable identity/uniqueness contract rather than claiming to implement delivery.

#### Lossless iCalendar boundary

- The authoritative event row carries immutable internal UUID, RFC UID, recurrence ID, source revision, supported typed fields, and a repository-owned opaque RFC 5545 property store. D reads supported typed fields and identity only. It never parses, normalizes, serializes, drops, merges, indexes, logs, or returns the opaque property store.
- Every query transaction is read-only. Preservation tests load the synthetic `.ics` corpus listed in §4.3, capture canonical property multiset and source-byte digests before the query, run every D view/filter/search/JSON/human/error path, then export through the owning B/F lossless adapter and require identical unknown property names, values, parameters, multiplicity, ordering/folding metadata, and malformed-but-preserved opaque bytes. Supported fields must map to the same identities without manufacturing a second UID. Any digest change, even on serialization failure or cancellation, fails D acceptance.
- Import/export and CalDAV synchronization remain F responsibilities. This contract proves that D cannot damage or bypass their preservation state; it does not falsely advertise an iCalendar export command.

#### Tombstones and conflicts

- Repository eligibility returns a typed tri-state: authoritative live item, tombstone, or unresolved conflict set. Default D1–D10 admit only the live case. A tombstone is identified by `(tombstone_id, deleted_entity_id, source_revision, deletion_provenance)`; a conflict branch by `(conflict_id, branch_id, entity_id, source_revision, content_fingerprint)`. These identities are never interchangeable with a live row or each other.
- If storage presents both a live row and tombstone for the same revision, an empty conflict set, duplicated branch identity, or an implicit winner, D fails the whole result with `projection_integrity_error`. Selector resolution searches only authoritative live eligible rows and cannot resurrect a tombstone or select one conflict branch.
- Golden repository fixtures cover local-delete/remote-update, remote-delete/local-update, delete/delete, moved recurrence exception versus deleted master, and two same-UID branches. They assert default exclusion, distinct preserved branch/tombstone identities, unchanged provenance/fingerprints/audit rows before and after the query, and no chosen winner. Deterministic resolution, restore, purge, undo, and delete/export round trips remain explicit F/deletion commands and are not claimed here; D's type boundary permits those commands to inspect both sides without changing default agenda semantics.

### 4.6 Dependencies

- Requires A3 typed full/short selector resolution and A4 stable errors/envelope.
- Requires B1–B7 for calendar metadata, temporal semantics, bounded recurrence/exception expansion and B8's immutable reminder-definition plus durable claim-uniqueness contract.
- Requires C1–C7 for todo fields, recurrence, completion and blocked projection; C8's immutable reminder-definition plus durable claim-uniqueness contract.
- Requires F's tested lossless opaque-property and typed tombstone/conflict repository contracts before preservation acceptance, but D does not expose sync, resolution, restore, purge, or iCalendar serialization commands.
- PostgreSQL full-text search is preferred; no external search service or network dependency.
- A Unicode grapheme/display-width crate and terminal capability crate may be added after license/maintenance review. The chosen IANA timezone/RRULE dependencies come from B4/B6 evidence, not this spec.
- No images, fonts, web assets, HTTP client, sync client, or Quickshell dependency.

#### Configuration and readiness contract

View configuration uses the foundation's distinct XDG roots and pure resolver. Precedence for every setting is `CLI flag > documented environment variable > TOML profile > compiled default`; absence differs from an explicitly empty/invalid value, which fails instead of falling through. The binding matrix is:

| Setting | CLI | Environment | TOML | Default |
|---|---|---|---|---|
| timezone | `--timezone` | `MG_CALR_TIMEZONE` | `view.timezone` | validated system IANA zone |
| width | `--width` | positive `COLUMNS` | `view.width` | terminal detection, then 80 |
| color | `--no-color` | presence of `NO_COLOR`, then `TERM=dumb` | `view.color` | capability detection |
| result limit | `--limit` | `MG_CALR_VIEW_LIMIT` | `view.limit` | 500 |
| database profile | foundation global flag | foundation documented env selector | XDG config profile | local peer-auth profile |

`XDG_CONFIG_HOME` locates configuration, `XDG_DATA_HOME` locates application data/migration metadata, `XDG_STATE_HOME` locates non-secret operational state, and `XDG_CACHE_HOME` is never authoritative. D does not write any of them. Table-driven tests isolate the process environment and cover every pairwise precedence, unset/empty/malformed values, missing files, symlinks, non-UTF-8 paths where supported, redacted database URLs, host-zone mismatch, and deterministic defaults. Configuration contents, query text, and credentials never enter fingerprints or diagnostics.

`mg-calr doctor --check views [--json]` remains non-mutating and reports a versioned prerequisite matrix with stable check IDs: `database.connect`, `database.read_only_role`, `database.schema_revision`, `timezone.database`, `recurrence.adapter`, `reminder.identity_uniqueness`, `ical.preservation_adapter`, `conflict.typed_state`, `terminal.width`, and `contracts.installed`. Each machine row is `{id,status:"pass"|"fail"|"blocked",required_for,code,recovery}` and contains no secret. `mg-calr init` may print unprivileged setup steps but D never invokes package managers, `sudo`, role creation, migrations, or secret prompts. Help marks commands unavailable until required checks pass rather than claiming partial capability.

The runtime database role is an unprivileged login granted `CONNECT`, schema `USAGE`, and `SELECT` only on the required projection objects; it has no table/sequence `INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, DDL, role, replication, bypass-RLS, or network-sync privilege. Startup verifies effective read-only transaction mode and fails `database_role_unsafe` if the configured D profile can mutate projection/audit/reminder/sync tables. Clean-machine recovery is executable but administrator-run: doctor emits exact separately copyable role/database/migration commands from versioned documentation, never executes them, never requests sudo, and never prints a credential. An integration role matrix proves least-privilege success and denies writes, schema changes, reminder claims, audit writes, tombstone changes, and conflict resolution.

### 4.7 Platform-specific considerations

Initial runtime target is Arch Linux/Hyprland in UTF-8 terminals, with a portable Linux core. Rendering must work in dumb/non-TTY pipes and terminals without truecolor. Color support is optional ANSI and capability-aware; `TERM=dumb` disables it. Locale may affect human weekday/month labels only after explicit localization support; v1 defaults to deterministic English, Sunday-first. The JSON/public-command contract is the later I1/I2 integration boundary; no direct database integration is allowed. Feature rollout follows dependency milestones: event-only views can land after B core, but combined/query contracts are not declared D-complete until C and recurrence semantics pass.

### 4.8 Performance budget

Measured on a documented baseline workstation against local PostgreSQL with warm filesystem/database caches and a synthetic set of 100,000 stored items:

- Today/day p95 application query ≤100 ms and total human render ≤150 ms for ≤500 projected items.
- Week p95 query ≤150 ms; month grid+agenda ≤250 ms.
- Structured filter/search result p95 ≤300 ms; no sequential scan over the entire 100,000-row fixture in the accepted `EXPLAIN (ANALYZE, BUFFERS)` plans for indexed predicates.
- First human byte ≤200 ms for normal day/week results, but output starts only after integrity/order checks; no partial misleading result is preferred over the budget.
- Process memory attributable to projection ≤32 MiB for the default 500-item result and ≤128 MiB at the 5,000 maximum. Rendering is O(returned items + displayed graphemes), not O(total database rows).
- Recurrence expansion is range- and count-bounded: maximum 10,000 candidate occurrences before filters and 5,000 returned items per request; exceeding either returns `query_limit_exceeded` without partial output.
- JSON and human identity/order extraction differ by ≤10% CPU from one another for the same result, excluding terminal I/O; neither causes a second database query.
- No network payload and no persistent client storage. Added indexes and their measured migration/storage cost must be reported before implementation acceptance.

---

## 5. Test Specification

### 5.1 Unit tests

- `civil_day_is_half_open_across_dst_gap`: America/Los_Angeles 2026-03-08 resolves to `[00:00-08:00, 00:00-07:00)` (23 hours); includes boundary intersections once and does not drift titles/times.
- `civil_day_is_half_open_across_dst_fold`: America/Los_Angeles 2026-11-01 resolves to 25 hours; distinct 01:30 occurrences at `-07:00 fold=0` and `-08:00 fold=1` retain distinct instants/complete occurrence identities and order in both human ABNF extraction and JSON.
- `ambiguous_and_nonexistent_wall_input_require_resolution`: bare fold/gap wall datetimes return typed alternatives/errors; no silent offset choice.
- `all_day_uses_exclusive_civil_bounds`: `[2026-03-07, 2026-03-09)` appears on exactly March 7 and 8 independent of DST and host timezone.
- `multi_day_intersection_and_flat_identity`: sectioned human view shows continuations while canonical JSON contains one item with unchanged instant bounds.
- `combined_order_is_total_and_stable`: all-day events first; then timed event starts and todo due instants; ties by kind (`event`, then `todo`), full UUID, occurrence key; undated todos are excluded from calendar views unless explicitly requested by query.
- `filter_boolean_semantics`: AND across fields, OR within repeated field, and inapplicable-kind exclusion match the documented matrix.
- `search_ranking_and_ties_are_deterministic`: weighted searchable fields and UUID tie-break produce stable order; operator-like text remains literal.
- `same_result_drives_human_json_and_month`: one repository call; ABNF-extracted complete identities/order/state/total/range/fingerprint/snapshot match schema-validated JSON and month marker counts derive from item intersections.
- `width_is_grapheme_and_cell_aware`: 40/59/60/79/80/240 widths with ASCII, combining marks, emoji/wide glyphs, bidi and control characters preserve mandatory identity/state/offset/fold/result semantics and never emit raw controls.
- `color_policy_precedence`: flag, `NO_COLOR`, `TERM=dumb`, non-TTY and JSON combinations contain no ANSI; color never changes plain-text tokens.
- `short_prefix_resolution`: minimum length, case normalization, zero/one/many matches, cross-kind collisions, tombstone/conflict exclusion, and deterministic displayed prefix expansion.
- `json_union_contract`: schema validation, null/absent policy, complete identity, fold/state enums, date/time representations, UUIDs, and stable order match golden schema-v1 fixtures.
- `cli_cursor_is_refused_atomically`: `next_cursor` is null, input cursor fails before repository access, and over-limit output is empty except the typed error.
- `snapshot_page_session_is_single_transaction`: an embedded page session retains one token and revision sequence while concurrent insert/update/delete is invisible; close/query mismatch/connection loss cannot re-query or resume.
- `projection_state_matrix_is_accessible`: active/completed/cancelled filters, inapplicable-kind exclusion, human text tokens, and JSON state agree for every fixture.
- `identity_variants_never_collapse`: live, tombstone, and every conflict branch with the same entity/RFC UID remain distinct; default eligibility admits only authoritative live and never picks a branch.
- `recurrence_exception_identity_is_preserved`: moved, cancelled, deleted, fold, and orphan/duplicate exception vectors preserve recurrence identity or fail atomically.
- `reminder_query_is_observational`: definition IDs drive `has_reminder`; claim/outcome/audit fixtures are unchanged across success and every injected failure.
- `machine_contracts_are_closed_and_complete`: all result/error/human goldens validate; manifest references are bijective; canonical fingerprint vectors match across Rust and an independent fixture implementation.
- `configuration_precedence_matrix`: isolated CLI/environment/TOML/default and XDG cases match §4.6 with redaction and no writes.

### 5.2 Integration tests

Using only a disposable database URL visibly named `mg_calr_test` and explicit opt-in:

1. Seed synthetic calendars/events/todos at every exact range boundary, both DST transitions, all-day spans, equal-time ties, recurrence masters/exceptions/moved/cancelled/deleted occurrences, blocked/completed tasks, duplicate text, control characters, colliding UUID prefixes, reminders with claim history, opaque iCalendar properties, tombstones, and unresolved conflict branches.
2. Assert no-argument/today/day/week/month/filter/search queries against `manifest-v1.json` identity/order/state oracles in UTC and America/Los_Angeles while the process host timezone differs; schema and ABNF validators must agree.
3. Run each query in human and JSON mode through a counting repository/process harness; verify one transaction/snapshot per command, one query result per renderer, and matching snapshot/fingerprint/complete identities. CLI over-limit/cursor attempts cannot issue a second query.
4. Hold an embedded `SnapshotPageSession` open, commit concurrent insert/update/delete from another connection between each page, and assert the original identity/revision sequence has no duplicate, omission, or replacement. Then close/abort/timeout the snapshot and assert resume fails rather than opening a new snapshot.
5. Concurrently update an item between selector display and transaction re-resolution; assert `selection_stale` and no mutation/audit row.
6. Before/after hash the opaque RFC 5545 property store, supported identity mapping, reminder definition/claim/outcome state, tombstone/conflict provenance, and audit rows across every success and injected failure path. Re-export preservation fixtures through the owning adapter and compare property/byte semantics specified in §4.5.
7. Execute the role matrix using a SELECT-only D role; prove query success and explicit denial of DML, DDL, reminder claim, audit write, tombstone restore/purge, conflict resolution, and sync/network capabilities. Run `doctor --check views --json` on clean, partial, unsafe-role, and ready installations and schema-validate the prerequisite matrix and recovery commands.
8. Use `EXPLAIN (ANALYZE, BUFFERS)` on the 100,000-row synthetic corpus and enforce §4.8 budgets/index usage with recorded hardware tolerance.
9. Corrupt temporal, recurrence identity, and typed eligibility fixtures inside isolated transactions; assert the whole projection fails with `projection_integrity_error` and does not silently omit/collapse a row.
10. Deny outbound networking (namespace/socket test) and assert every D and D-doctor command succeeds/fails without network attempts; only the explicitly selected local PostgreSQL connection is permitted.

### 5.3 UI / E2E tests

- Spawn a PTY at widths 40, 59, 60, 79, 80, and 240; snapshot populated/empty day, week, month, filtered and searched outputs in color and no-color modes.
- Drive chooser entirely by keyboard: arrows, `j/k`, paging, numeric selection, Enter, Escape, `q`, EOF and SIGINT. Assert focus order, unique context, cancellation, and no writes before confirmation.
- Pipe all human commands to a file and screen-reader-friendly text check; assert no ANSI/cursor-control repaint and chronological linear reading order.
- Run JSON with stdin closed and colliding selectors; assert it never prompts, stdout is empty on error, stderr is one valid envelope, and exit status/code are stable.
- Break stdout mid-stream; assert conventional termination without panic, stack trace, secret, or retry loop.

### 5.4 Visual / manual verification

- Terminal capabilities: truecolor, 16-color, monochrome, `TERM=dumb`, `NO_COLOR`, TTY and pipe.
- Width extremes: 40 and 240 columns, plus rejected 39; long ASCII/Unicode/control-bearing synthetic titles.
- Empty, single-item, 500-item and over-limit results.
- DST gap/fold days, leap day, month starting Saturday/Sunday, cross-year week, all-day and multi-day items.
- Screen-reader linear output and chooser focus announcement; keyboard-only operation.
- Compare semantically identical human/JSON identity dumps and inspect compact JSON with a standard parser.

---

## 6. Compliance & Safety Gate

### 6.1 Sensitive data classification

- [ ] No sensitive data involvement
- [x] Handles sensitive data — calendar titles, descriptions, locations, URLs, todo notes/projects/tags, attendee metadata, and schedules may be sensitive. Queries use local PostgreSQL only; output is explicit user-requested stdout/stderr. Logs and errors omit content, query text, URLs, SQL, extension blobs, and credentials by default. Debug tracing must use typed IDs/counts and redacted predicates. Terminal control/bidi characters are escaped. Search indexes remain in the authoritative local database and exclude unknown-property/credential material.
- [x] Uses synthetic/test data only until compliance gate clears

### 6.2 Asset provenance

- [x] No third-party assets
- [ ] Uses third-party assets

Crates added for width/terminal/search support require normal license and maintenance review; they are dependencies, not bundled content assets.

### 6.3 Language / claims audit

- [x] No user-visible claim in this spec is treated as implemented; §7 marks D absent.
- [x] Help/error text must describe only commands and contracts shipped in the same implementation slice.
- [x] No regulated health/financial/legal claim is made.

### 6.4 Regulatory alignment

- **I1 lossless iCalendar:** D never serializes iCalendar, but §4.5 makes the owning adapter's opaque property store and RFC UID/recurrence identity a hard prerequisite. Read-only before/after digest plus re-export fixtures cover unknown properties, parameters, multiplicity, folding/order metadata, malformed opaque values, exceptions, errors, and cancellation. D cannot be accepted if any view path changes or bypasses those semantics; export/sync remains deferred to F.
- **I2 sync authority:** PostgreSQL remains authority; no cache, direct client DB access, vdir, or competing search service is introduced. Snapshot and source-revision fingerprints are carried through the public projection.
- **I3 conflict/deletion:** Typed repository eligibility and complete tombstone/branch identities preserve every side and provenance while default views exclude non-authoritative candidates. Contradictions fail atomically; selectors cannot resurrect or choose. Golden local/remote delete/update and recurrence-conflict fixtures prove preservation and no winner. Resolution/restore/purge remain deferred explicit commands rather than being falsely implemented in D.
- **I4 scope/network:** D commands access only explicitly configured local PostgreSQL and perform no network operation. The integration network-denial and database-role matrices prove this boundary; synchronization remains only under explicit F commands.

Criteria alignment additionally covers T1 complete UUID/RFC UID/occurrence/tombstone/conflict identities, T2 human-visible fold/gap/all-day/exception vectors, T3 one-snapshot atomic queries and transaction-bound embedded pages, T4 preserved provenance with no restore/purge claim, and T5 an observational reminder boundary gated on durable uniqueness. C1 remains guided and keyboard-first; C2 has schemas/ABNF/goldens and noninteractive ambiguity safety; C3 has pure XDG precedence fixtures; C4 has mandatory state/fold/width/focus semantics; C5 has a stable machine prerequisite matrix. I1–I3 preservation gates do not claim deferred serializer/sync/resolution behavior; I4 is network-denial tested. O1 redaction, O2 executable unprivileged role/clean-machine recovery, O3 typed atomic failures/snapshot expiry, and O4 unit/contract/integration/performance/security gates are binding. Ambiguous selectors never mutate, satisfying the automation auto-fail boundary.

---

## 7. Gap Analysis vs. Current State

### 7.1 What exists today

- **Implemented foundation only:** `src/domain.rs` has nominal UUID identifiers/errors; `src/config.rs` resolves XDG/config; `src/storage.rs` connects/migrates; `src/main.rs` exposes foundation diagnostics and the schema-v1 envelope.
- **Implemented scaffolding, not D behavior:** `migrations/0001_foundation.sql` contains event/todo temporal and metadata columns but no query repository, recurrence expansion, search index, combined projection, selector resolver, or view command.
- **Absent:** D1–D10 commands, application/domain projection types, human/month renderers, JSON item schema, machine-readable JSON/ABNF/error contracts, width/color implementation beyond foundation ANSI absence, chooser, search/filter parsing, snapshot page-session API, SELECT-only view role checks, preservation adapters/fixtures, performance corpus, and temporal view fixtures.
- **Planned/gated:** B/C temporal, recurrence, event/todo invariants and CRUD are prerequisites. Current passing foundation tests do not demonstrate any view/query capability.

### 7.2 Delta to spec

- Add application/domain/query/render/CLI modules listed in §4.1 and expose their interfaces from `src/lib.rs`.
- Extend `src/main.rs` command dispatch without moving query/render business logic into it.
- Add a forward migration for measured range/full-text indexes; no source-content rewrite or alternate authority.
- Add typed query errors/exits plus `contracts/view-result-v1.schema.json`, `contracts/error-v1.schema.json`, `contracts/view-errors-v1.json`, `contracts/human-row-v1.abnf`, manifest-driven D command/item goldens, and independent canonical-fingerprint vectors while preserving the foundation envelope.
- Add timezone/range normalization and consume B6/B7/C bounded recurrence/exception identity without reparsing or rewriting it.
- Add complete live/RFC UID/occurrence/todo-instance/source-revision identity, typed tombstone/conflict eligibility, and observational reminder summaries; default views still expose only authoritative live items.
- Add deterministic selector resolution and PTY chooser with transaction-time re-resolution.
- Keep CLI results atomic and cursor-free; add the optional embedded `SnapshotPageSession` only over one held read-only transaction.
- Extend foundation config/doctor with the XDG precedence and schema-validated view prerequisite matrix; verify a SELECT-only runtime role and administrator-run recovery.
- Add synthetic unit, disposable PostgreSQL integration, PTY, golden JSON/human, RFC 5545 opaque-preservation, tombstone/conflict, reminder-state, snapshot-concurrency, configuration, role-denial, performance, network-denial, terminal-escape, and accessibility fixtures.
- Add only evidence-approved display-width/terminal dependencies; reuse B temporal/RRULE libraries.

### 7.3 Estimated scope

**L.** D1–D10 are one coherent read model but span temporal projection, recurring occurrences, combined event/todo ordering, PostgreSQL search/indexing, stable machine contracts, Unicode terminal layout, interactive ambiguity handling, and substantial fixture/performance/security evidence. Implement as dependency-ordered TDD sub-slices behind one binding query contract rather than one large patch.

### 7.4 Blocking dependencies

- A3 stable typed identity plus the short-selector contract, A4 typed errors/envelope, pure XDG configuration, doctor matrix support, and database snapshot/repository support.
- B1–B5 event/calendar fields and strict timed/all-day/IANA semantics; B6–B7 bounded recurrence and exception identity; B8 durable reminder-definition identity/claim uniqueness and read-only predicate support.
- C1–C7 todo fields, due semantics, recurrence instance identity, parent/dependency blocked state; C8 durable reminder-definition identity/claim uniqueness and read-only predicate support.
- A lossless RFC 5545 opaque-property adapter and typed tombstone/conflict eligibility contract from the owning B/F storage slices. Full sync, iCalendar commands, conflict resolution, restore, purge, and reminder delivery remain deferred; only their preservation interfaces and fixtures block D-complete status.
- Evidence-backed IANA timezone/RRULE choices, a PostgreSQL full-text/index design benchmark, and a SELECT-only projection role.
- D implementation slices can precede E delivery and F synchronization because they remain read-only and network-free. D-complete acceptance waits only for the preservation interfaces above, not transport or resolution behavior. I1/I2 clients are later consumers of D9 and cannot block CLI delivery.

---

## 8. Open Questions

- **Q1:** Which maintained Unicode display-width/grapheme and terminal-capability crates pass license/maintenance review? — blocks D8 implementation dependency selection, not its behavior contract.
- **Q2:** Should human weekday/month labels remain deterministic English for all v1 locales or follow an explicit `--locale` in a later compatibility revision? — does not block v1; recommended default is deterministic English.
- **Q3:** What documented baseline hardware and allowable CI variance will enforce the p95 budgets? — blocks final performance gate calibration, not functional implementation.

Short-ID encoding/length, default bounded search range, Sunday-first month semantics, JSON compatibility policy, chooser noninteractive behavior, and DST/all-day inclusion are resolved by this spec and are not open implementation choices.
