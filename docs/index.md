# calman — Documentation

`calman` is a minimalist, keyboard-driven **task & event manager** with a
Taskwarrior-style CLI and a planned TUI. Data lives locally as `jsonl` or
`ics` (CalDAV/vdir compatible); sync is delegated entirely to external commands
(`git`, `pimsync`, `rclone`, …).

## Pages

- [Introduction](introduction.md) — what calman is and its design philosophy
- [Install](install.md) — build from source, config directory, two config tiers, first run
- [Usage](usage.md) — every command with syntax, common options, and examples
- [Writing dates](dates.md) — date/time syntax for `due:` / `start:` and all-day semantics
- [Tasks & events](tasks.md) — adding todos and events, attributes, relations
- [Filters](filters.md) — the shared filter grammar used by the CLI and reports
- [Reports](reports.md) — built-in reports, `rc` overrides, icons, and colors
- [Recurrence](recurrence.md) — `recur:` / `repeat:` normalised to RFC 5545 `RRULE`
- [iCalendar format](icalendar.md) — `.ics` file format reference (RFC 5545): structure, date/time forms, `RRULE`, `STATUS`, `VTIMEZONE`

## Other project docs (repository root)

- `README.md` — quick start
- `SPEC.md` — product spec & decisions
- `DESIGN.md` — detailed functional design (man-page style)
- `PLAN.md` — phased roadmap
- `feature.md` — feature backlog & design notes (scratch file; may lag the code)
- `AGENTS.md` — developer guide

> Tip: run `calman help` (or `calman filters`) any time to print a compact
> cheat-sheet of commands, attributes, recurrence, the filter grammar, and the
> config tiers.

## Inspiration

calman draws on a few existing tools:

- **Taskwarrior** — studied its core usage and command-line interface; calman borrows its CLI syntax and philosophy.
- **khal** — a TUI for calendars and events; an influence on calman's planned TUI and on its calendar/event model.
- **cfait** — borrowed the idea of storing each item as its own `ics` file.
