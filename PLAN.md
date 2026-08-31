# calman — Phased Development Plan

## Phase 1: Core Engine & CLI Foundation (Estimated 3‑4 weeks)
**Goal**: A usable CLI tool that can add, list, modify, delete tasks and invoke external sync commands.

### 1.1 Basic Framework
- [ ] Initialise Rust project (`cargo init`).
- [ ] Add dependencies: `clap` (derive), `serde`, `serde_json`, `toml`, `toml_edit`, `chrono`, `anyhow`, `thiserror`.
- [x] Implement configuration loader (`config.rs`):
    - Read `~/.config/calman/config.toml`.
    - Generate default config if absent.
    - Implement fallback logic for `contexts`.

### 1.2 Data Model & Storage
- [x] Define `Task` struct and `TaskStatus` enum (`model.rs`).
- [x] Implement `Storage` Trait:
    - `JsonlStorage`: read, append, atomic write, modify/delete by UID for `tasks.jsonl`.
    - `IcsStorage`: read/write `*.ics` files (using `ical` crate).
- [x] Implement state manager (`state.rs`): read/write `.calman-state.json`, manage `uid_counter`.

### 1.3 Sync Executor
- [x] Implement `sync/executor.rs`:
    - Parse `{location}` placeholder in `pre_hook`, `cmd`, `post_hook`.
    - Execute commands via `std::process::Command`.
    - Implement `.sync.lock` locking.
    - Implement short‑circuit logic (pre_hook failure stops execution).

### 1.4 CLI Command Skeleton
- [x] Define subcommand structure in `main.rs` with `clap`.
- [x] Implement `add` (including write‑source logic).
- [x] Implement `list` (cross‑source merging, short‑ID generation, table/JSON/CSV output).
- [x] Implement `done` / `delete` / `modify` (short‑ID + `--uid` support).
- [x] Implement `count` (including predefined filters like `+OVERDUE`).

---

## Phase 2: CLI Advanced Features & Integration (Estimated 2‑3 weeks)
**Goal**: Polish the CLI to match Taskwarrior’s usability.

### 2.1 Date Parsing Engine
- [x] Natural-language date parser (`date_parser.rs`, custom — not pest/nom):
  - `eow`, `eoww`, `eond`, `sod`, `sow`, `som`, `soy`, … day boundaries.
  - Offsets (`+3d`, `-2w`), weekdays (`fri+1`), ISO/compact dates, `HH:MM`.
  - Date-only forms land at local midnight (all-day semantics).

### 2.2 Filter Engine
- [x] Parse command‑line arguments for `list` and `count`.
- [x] Implement `Filter` struct: support `due`, `status`, `priority`, `tags`.
- [x] Map predefined filters (`+OVERDUE` etc.) to filter expressions.
- [x] Shared expression grammar (CLI + report `filter`): `and`/`or`/parens, `type:all`, `source:`/`-source:`, `due.before:`/`due.by:`/`due.after:`.

### 2.3 Integration Tests & CLI Documentation
- [x] Write integration tests (`tests/`) covering core commands.
- [x] `calman help` / `calman filters` cheat-sheet (full syntax).
- [x] `README.md` with quick start, config tiers, recurrence, filters, reports.

### 2.4 Report Engine & Relations
- [x] Config schema: `[report.<name>]` (columns/format/sort/filter), `[defaults] default_report`.
- [x] Builtin reports `ls`/`list`/`next`; bare `calman` → `next`.
- [x] Report renderer: fields (`id,status,summary,desc,tags,due,pri,type,source`), formats (`relative/countdown/iso/truncate`), `width`, `sort` (incl. `/` break).
- [x] Builtin default filters: future events only (dtstart ≥ sod), todos unchanged.
- [x] Merged STATUS column (event → calendar icon); DATE column + per-column `event_format`/`todo_format`.
- [x] CLI rc overrides: `rc.report.<name>.columns=/labels=/filter=/sort=`.
- [x] Nerdfont icons, 3-level fallback (column `icons` > `[icons.todo]`/`[icons.event]` > builtin).
- [x] Global row-level color rules `[[color]]`, first-match, `fg/bg/bold/underline/italic/dim`.
- [x] Builtin virtual tags: `OVERDUE`, `DONE`≡`COMPLETED`, `CANCELLED`, `IN-PROCESS`, `TAGGED`/`UNTAGGED`, `TODO`/`EVENT`, `SCHEDULED`.
- [x] `rel:<parent-id>` attribute on `add`/`modify` → `RELATED-TO` (`RELTYPE=PARENT`).
- [x] `RELATED-TO` read/write in `IcsStorage`; `related_to` persisted in JSONL.
- [x] Modular config: `include` merge in `config.rs` + `config.example.toml` / `report.example.toml` / `colorscheme.example.toml`.
- [x] Config two tiers: `*.default.toml` (minimal baseline) + `*.example.toml` (annotated samples).
- [x] Recurrence: `recur:`/`repeat:` → standard RFC 5545 `RRULE` (`recurrence.rs`); raw `FREQ=` passthrough + friendly grammar.
- [x] Date-only `due`/`from` → all-day (`VALUE=DATE`); overdue policy fixed (day-after), `due_date_overdue_today` removed.

### 2.5 fix.md Cleanup (config ergonomics)
- [x] Drop `[date] default_start_time`: remove field (config.rs `DateConfig` + default fn + `Config::default`), remove from `config.default.toml`. `modify start:<date>` (date-only) now becomes an all-day event, consistent with `add` (update `modify.rs` + header doc).
- [x] Move `default_filter` from `[ui]` into `[tui]`; delete the now-empty `[ui]` section (`config.rs` `UiConfig`→`TuiConfig`, `config.default.toml`).
- [x] Show default icons in `config.default.toml` `[icons.todo]`/`[icons.event]` (copy builtin: todo ○ ● ✓ ✕; event calendar glyph / ● / ✓ / ✕).
- [x] Merge `report.default.toml` into `config.default.toml` (add `[report.ls]`/`[report.list]`/`[report.next]`); delete `report.default.toml`. Update all references (SPEC.md, README.md, docs/install.md, colorscheme.example.toml, `src/cli/mod.rs` help, `config.example.toml`).
- [x] Rewrite `colorscheme.rules` as inline tables (user-preferred format) in `config.default.toml` + `colorscheme.example.toml`:
  ```toml
  [colorscheme.rules]
  completed = {fg="gray10"}
  overdue = {inverse=true}
  "priority.H" = {fg="red", bold=true}
  ```
  quoted keys for dotted `priority.*`; verify serde parses into `HashMap<String,RuleStyle>`.
- [x] Reconcile docs (DESIGN.md, feature.md, SPEC.md, README.md, docs/install.md, docs/reports.md) for the above: default_start_time gone, `[ui]`→`[tui]`, report merge, colorscheme inline format, all-day semantics.
- [x] Tests: keep `-0900`, `dur:`/`end:`, all-day event, `default_event_duration` green; add modify date-only-start→all-day; add colorscheme inline-table parse test.
- [x] `cargo build` + `cargo clippy --all-targets` clean + `cargo test` green; manual smoke verify.

---

## 2.6 Modularization & date/recurrence redesign
**Goal**: selective compilation via Cargo features; redesigned `T`-based date syntax; NL date + NL→RRULE via libraries.

### Features (`Cargo.toml`)
```
default = ["storage-jsonl", "storage-ics", "date-ical", "date-natural"]   # no tui by default
storage-jsonl = []
storage-ics   = ["dep:chrono-tz"]
date-ical     = []                                # baseline: T-compact format + raw RRULE passthrough
date-natural  = ["date-ical", "dep:interim", "dep:text2rrule"]
tui           = ["dep:ratatui", "dep:crossterm"]
recur-expand  = ["date-ical", "dep:rrule", "dep:chrono-tz"]   # optional: expand occurrences
```
Minimal build: `--no-default-features --features storage-ics,date-ical`.

### Libraries (search results)
- NL dates: `interim` (maintained `chrono-english` fork) — `next tuesday`, `in 3 days`, `tomorrow 8pm`. [github.com/conradludgate/interim](https://github.com/conradludgate/interim)
- NL→RRULE: `text2rrule` — `every tuesday` → `FREQ=WEEKLY;BYDAY=TU`. v0.1.x, needs Rust 1.85+. [github.com/carmiac/text2rrule](https://github.com/carmiac/text2rrule)
- RRULE expand (optional): `rrule` (rust-rrule). [github.com/fmeringdal/rust-rrule](https://github.com/fmeringdal/rust-rrule)
- Taskwarrior-style keywords (`eod`/`soq`/`sonww`/`sow`/`eom`/`eoy`…) kept custom, in `date-ical`.

### Modules
- `src/date/mod.rs` dispatches `parse_date_value`/`parse_datetime`/`parse_duration`/`resolve_end`/`local_midnight`/`named_date`.
  - `src/date/ical.rs` (`#[cfg(feature="date-ical")]`) — rewritten T-compact parser.
  - `src/date/natural.rs` (`#[cfg(feature="date-natural")]`) — `interim` wrapper.
- `src/recurrence.rs` `normalize_recurrence`: `FREQ=`/`RRULE:` passthrough (always); `date-natural` → `text2rrule`; `date-ical`-only → reject non-iCal.
- `src/storage/{jsonl,ics}.rs` each `#[cfg(feature="storage-*")]`; `storage/mod.rs` selects backend at runtime by config `type`; `SourceType` variants cfg-gated; `Config::load` errors on disabled source type / `tui` section.
- `src/cli/tui.rs` + `src/tui/` `#[cfg(feature="tui")]`; `main`/`cli/mod.rs` hide `tui` command; `TuiConfig` cfg-gated.

### Date parser redesign (`T` marker) — `src/date/ical.rs`
- `YYYYMMDDTHHMMSS` (≥8 digits + `T`) → full (`20260828T090000`).
- Digits only, no `T`: 8 → `YYYYMMDD` all-day; <8 → trailing digits of `YYYYMMDD`, prefix filled from today (`0823`→2026-08-23, `25`→2026-08-25, `260823`→2026-08-23).
- `<8` digits before `T` (`0828T0900`, `25T`): digits = trailing `YYYYMMDD` (year/month from today), after-`T` = `HHMMSS` (zero-padded).
- `T` prefix (`T0900`, `T09`): today's date + after-`T` `HHMMSS` (pad to 6); `T09`=today 09:00:00, `T`=today 00:00:00.
- Keep: `YYYY-MM-DD`, `YYYY-MM-DD HH:MM`, `HH:MM` (today), `now`, named boundaries, relative `+3d`/`-2w`/`+1h`.
- Remove `-HHMM` time syntax (→ `T` prefix); `start:-0900` → `start:T0900` (update docs/tests/examples).
- `DateValue::{Date,Time}` unchanged; `local_to_utc`/`resolve_end` retained.

### Recurrence model (Taskwarrior-inspired)
- New `TaskStatus::Recurring`. A task with `rrule` set (via `recur:`/`repeat:`) gets `status = Recurring` on `add` (unless explicit status given).
- Virtual tag `PARENT` ⇒ `rrule.is_some()` (status == Recurring). Filterable `+PARENT`/`-PARENT`; also `status:recurring`/`-status:recurring`.
- Default `list`/`next`/`ls` hide recurring parents: append `-status:recurring` (≡ `-PARENT`) to built-in `filter` in `config.default.toml` `[report.*]`.
- `modify`/`delete` on a recurring item operate on the **master** record (single VEVENT/VTODO + RRULE in its ics/jsonl file). `done` on a recurring parent ⇒ `Cancelled` (stops series); `delete` removes it.
- Optional `recur-expand` feature: `list`/`next` compute upcoming occurrences in-memory via `rrule` crate (`all_between`) and emit virtual marked rows (`⟳`), so series are visible without child records. Parent still hidden by default.
- ICS: `Recurring` renders as `NEEDS-ACTION`(VTODO)/`CONFIRMED`(VEVENT) + `RRULE:`; parse: `RRULE` present ⇒ `Recurring`.
- **Per-occurrence exceptions (iOS-compatible, RFC 5545)**: delete one occurrence ⇒ add its original `DTSTART` to master `EXDATE`; modify one occurrence ⇒ create sibling component (same `UID`, `RECURRENCE-ID` = that occurrence's original `DTSTART`, overridden fields); master keeps `RRULE`. iOS Calendar recognises both.
- **Occurrence addressing (CLI)**: `on:<date>` (occurrence's original start) or `.<n>` (nth upcoming). e.g. `calman done 5 on:2026-09-02`, `calman modify 5.2 summary:"x"`.
- **Data model**: `Task` gains `exdates: Vec<DateTime<Utc>>` + `recurrence_id: Option<DateTime<Utc>>` (overrides); jsonl stores them; ics writes `EXDATE` / `RECURRENCE-ID`. `recur-expand` uses `rrule` crate `all_between`; expanded virtual rows carry occurrence date for `on:`/`.n` targeting.

### Steps
1. [x] `Cargo.toml`: features + deps (`interim`, `text2rrule`; `rrule` only for `recur-expand`).
2. [x] Create `src/date/{mod,ical,natural}.rs`; migrate `date_parser.rs` logic into `ical.rs`, rewrite T rules.
3. [x] `src/recurrence.rs`: passthrough + `text2rrule` (cfg); keep raw validation.
4. [x] Gate storage modules; cfg-gate `SourceType` variants; `Config` validation.
5. [x] Gate `tui` module; hide command.
6. [x] Update `args.rs` (drop `-HHMM` special-case), `add.rs`/`modify.rs` unchanged (still call `parse_date_value`).
7. [x] Update docs (DESIGN/SPEC/usage/dates/recurrence/install/README) + tests.
8. [x] `cargo clippy --all-targets` + `cargo test` green; minimal `cargo build --no-default-features --features storage-ics,date-ical`.
9. [x] Recurrence model: `TaskStatus::Recurring` + `status_to_ics`/`status_from_ics`/`event_status_*` handling; `PARENT` virtual tag in `filter.rs`/`report.rs`; default report `filter` hides `status:recurring` in `config.default.toml`; `done` on recurring ⇒ `Cancelled`; `modify`/`delete` target master record.
10. [x] `recur-expand` (optional feature): occurrence generation in `list`/`next` via `rrule` crate; virtual marked rows; parent hidden by default.
11. [x] Per-occurrence exceptions (iOS-compatible): `Task.exdates`/`recurrence_id`; `on:<date>`/`.n` occurrence addressing in `args.rs`/`cli`; `done <id> on:<date>` ⇒ `EXDATE`; `modify <id>.<n>`/`on:` ⇒ sibling `RECURRENCE-ID` override component; ics writes `EXDATE`/`RECURRENCE-ID`; jsonl persists fields.

### Risks
- `text2rrule` needs Rust 1.85+ (edition 2024 — confirm toolchain).
- `interim`/`text2rrule` MSRV vs chrono version compatibility.
- `rrule` pulls `chrono-tz` (already used by `ics` — acceptable); `recur-expand` optional only.
- Feature-matrix: `Config` must validate enabled backends; add CI `cargo test --no-default-features --features …`.
- `T` ambiguity (`930` → 93h?) → document as `HH[MM[SS]]` only.

### 2.7 Post-2.6 hardening (completed)
- [x] `recur-expand` moved into **default features**; occurrence rows get plain sequential IDs (Taskwarrior-style), `id.n`/`on:<date>` kept as aliases.
- [x] `recur_expand_count` config (`[defaults]`, default 1 = nearest occurrence only).
- [x] `recur`/`recurrence` report column renders RRULE as ISO period (`P7D`/`P2W`/`P1M`/`P1Y`).
- [x] ISO 8601 durations/periods: `recur:P7D`, `alert:PT15M`, `for:P2W` (works without `date-natural`).
- [x] `eoww` uses `day_end`; `workweek_end` config was removed (with dead `week_start`/`[locale]`).
- [x] `src:` alias for `source:` (CLI + filter); `desc:` capture fixed with `src:`; multiline `desc` single-line render (`␤`) + display-width truncation (`unicode-width`); CJK-safe arg prefix checks.
- [x] Composite `ics-dir` sources (`remote/sorge`) fixed for `done`/`delete`/`modify` via `cli::resolve_source`.

---

## Phase 3: TUI Complete Implementation
**Goal**: Full left‑right two‑panel TUI with browsing, editing, and settings persistence.

### 3.1 TUI Skeleton & Layout
- [ ] Add `ratatui`, `crossterm`, set up custom event loop.
- [ ] Implement application state machine (`AppState`): List, Detail, Edit, Settings.
- [ ] Implement left‑right layout: list 40%, detail 60%.

### 3.2 List Panel & Detail Preview
- [ ] List rendering: highlight, scrolling, filtering (using `contexts.tui`).
- [ ] Detail rendering (`DetailPane`):
    - Render Header (Type, Source).
    - Render Status Bar (status, priority, tags).
    - Render dynamic time section (Todo/Event switching).
    - Render metadata (including Source field).

### 3.3 Modal Interaction & Editing
- [ ] Implement full state transitions: List → Detail → Edit and back.
- [ ] Implement field editors (text input, dropdown, date input).
- [ ] Implement `Ctrl+Enter` save and `Esc` discard.
- [ ] Implement `source` field editing (moving tasks between Sources).

### 3.4 Settings Overlay & Persistence
- [ ] Implement `SettingsOverlay` (floating window).
- [ ] Bind `Ctrl+p` to toggle.
- [ ] Use `toml_edit` to atomically write configuration.
- [ ] Implement keybinding mode switching (Vim / Arrow) with immediate effect.

### 3.5 Sync Indication & Miscellaneous
- [ ] Trigger `sync` from TUI (key `s`) and show external command output logs.
- [ ] Implement `AddDialog` (floating input form).
- [ ] Initialise internationalisation (i18n) framework.
- [ ] Performance optimisation: rendering/scrolling with 1000+ tasks.

---

## 4. Milestones & Releases
- **v0.1.0 (Alpha)**: Phase 1 complete – usable CLI for basic task management.
- **v0.2.0 (Alpha)**: Phase 2 complete – CLI matches core Taskwarrior features.
- **v0.3.0 (Beta)**: Phase 3 complete – usable TUI.
- **v1.0.0 (Stable)**: Phase 3 complete – stable TUI, full documentation, official release.

## 5. Risks & Mitigations
| Risk                              | Mitigation |
| :-------------------------------- | :--------- |
| **ICS format incompatibility**    | Use mature `ical` crate; write compatibility tests against Radicale. |
| **Concurrent write conflicts**    | Rely on `.sync.lock` and external command atomicity (e.g., Git). |
| **TUI performance**               | JSONL streamed; detail render only for selected item, avoid full traversal. |
| **Config write conflicts**        | Use `toml_edit` to preserve comments; if file changed externally, prompt user to restart. |
