# Changelog

All notable changes to calman are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); calman follows
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-08-28

First release. Minimalist Taskwarrior-style task manager: CLI with JSONL / ICS
storage, filters, reports, recurrence (iOS-compatible), and optional TUI stub.

### Added

- **CLI commands**: `add`, `list`/`ls`/`next`, `done`, `delete`, `modify`,
  `count`, `sync`, `help`/`filters` (TUI command behind `tui` feature).
- **Storage backends**:
  - JSONL (`tasks.jsonl`, atomic writes) — default.
  - ICS (`*.ics` per item, VTODO/VEVENT, TZID local wall-time, all-day
    `VALUE=DATE`, VALARM) — iOS/CalDAV compatible.
  - `ics-dir` (Radicale/vdirsync): auto-discovers collections; composite
    references like `source:remote/sorge`.
- **Date parsing** (feature `date-ical`):
  - iCalendar-compact `T` forms: `20260812T090000`, `0826T0930`, `T0900`,
    trailing-digit fill (`25` = 25th this month).
  - `YYYY-MM-DD [HH:MM]`, `HH:MM`, named dates (`today`/`eow`/`som`/`eoy`/…),
    relative offsets (`+3d`, `-2w`).
  - `workweek_end` config drives `eoww` (default Friday 17:00).
  - ISO 8601 durations for `alert:`/`duration:` (`PT15M`, `P7D`, `P2W`).
- **Natural language** (feature `date-natural`, default):
  - `interim` for NL dates; `text2rrule` for `recur:"every tuesday"`.
- **Recurrence** (`recur:`/`repeat:`, alias):
  - Normalised to RFC 5545 `RRULE`; raw `FREQ=`/`RRULE:` passthrough.
  - ISO 8601 periods: `recur:P7D` → `FREQ=DAILY;INTERVAL=7`.
  - Series model: master = `status:recurring`, virtual tag `+PARENT`, hidden
    from default reports; `done` on master cancels the series.
  - `recur-expand` (default): nearest occurrence(s) shown with plain
    sequential IDs (`[defaults] recur_expand_count`, default 1).
  - Per-occurrence exceptions (iOS-compatible): `done`/`delete` → `EXDATE`;
    `modify` → same-UID `RECURRENCE-ID` override; addressing via plain ID,
    `id.n`, or `on:<date>`.
  - `recur`/`recurrence` report column renders the pattern as `P7D`-style
    ISO period.
- **Filters**: shared CLI/report grammar — `type:`, `source:`/`src:`,
  `status:` (incl. `recurring`), `due:*`, `pri:`, `+tag`/`-tag`, `and`/`or`/
  parens; virtual tags `+OVERDUE +PENDING +COMPLETED +CANCELLED +IN-PROCESS
  +TAGGED +UNTAGGED +SCHEDULED +PARENT`.
- **Reports**: builtin `ls`/`list`/`next`, `rc.report.*` overrides, nerdfont
  icons (3-level fallback), `[colorscheme]` row colors, display-width-aware
  truncation (`unicode-width`), CJK-safe rendering.
- **Config**: two tiers — `config.default.toml` (self-contained defaults) +
  `config.example.toml` (annotated sample); `include` merge; `[defaults]`,
  `[contexts]`, `[date]`, `[tui]`, `[icons]`, `[colorscheme]`, `[report.*]`.
- **Sync**: external command chains (`pre_hook`/`cmd`/`post_hook`) with
  `{location}` substitution and `.sync.lock`.
- **Cargo features**: `default = [storage-jsonl, storage-ics, date-ical,
  date-natural, recur-expand]`; optional `tui`; minimal build
  `--no-default-features --features storage-ics,date-ical`.

### Fixed

- ICS timezone handling (TZID + local wall-clock round-trip).
- All-day semantics: date-only `due:`/`start:` → `VALUE=DATE`; `modify
  start:<date>` becomes all-day and clears stale `DTEND`.
- `desc:` capture with `src:`/other attributes; multiline descriptions render
  single-line (`␤`) with truncation.
- Non-ASCII (CJK) args no longer panic in attribute prefix checks.
- Composite `ics-dir` sources work in `done`/`delete`/`modify`.
- Recurring parents hidden from default reports; `+PARENT` reveals them.
- Removed dead config (`[locale]`, `[date] week_start`).

### Changed

- Date syntax: `-HHMM` time form replaced by `T` marker
  (`start:-0900` → `start:T0900`).
- `[ui]` renamed to `[tui]`; `default_start_time` dropped; report defaults
  merged into `config.default.toml`; colorscheme rules as inline tables.
- Expanded occurrences show plain integer IDs (Taskwarrior-style); `id.n` /
  `on:<date>` retained as aliases.

[0.1.0]: https://github.com/HiraethEcho/calman/releases/tag/v0.1.0
