# Introduction

## What calman is

calman is a small, fast, keyboard-driven **task & event manager**. It gives you
a first-class command line for managing locally stored task data and gets out of
the way for synchronisation: it never implements sync itself — it runs
*your* external commands.

Two kinds of item live in calman:

- **Todos** (iCalendar `VTODO`) — have a `due` date.
- **Events** (iCalendar `VEVENT`) — have a `dtstart` (and optionally `dtend`).

The distinction is automatic: if you give a `from:` it becomes an event; if you
give a `due:` it becomes a todo. Internally both are the same `Task` record, so
filters, reports, and recurrence treat them uniformly.

## Philosophy

1. **Files, not sync.** Data is stored as plain `jsonl` (one line per item) or
   `ics` (one file per item, CalDAV/vdir compatible). No database, no daemon,
   no network — `git` or any tool you like handles sync.
2. **CLI habits follow Taskwarrior; internals follow CalDAV/iCalendar (RFC 5545).**
   You get short IDs, natural-language dates, and a shared filter grammar, while
   the on-disk format interoperates with Radicale, vdirsyncer, iOS Reminders,
   and Outlook.
3. **Interface is efficiency.** The CLI is the primary surface today; a
   left/right two-panel TUI (list + detail) is planned for Phase 3.
4. **Configuration as code.** Everything — sources, reports, icons, colors — is
   described in `config.toml` and merged TOML files.

## Tech stack

| Concern            | Choice                                  |
| :----------------- | :-------------------------------------- |
| Language           | Rust (Edition 2024)                     |
| CLI parsing        | `clap` (derive style)                   |
| TUI (planned)      | `ratatui` + `crossterm`                 |
| iCalendar          | `ical` / `vobject` crates               |
| JSONL              | `serde_json`                            |
| Dates              | `chrono` + a custom natural-language parser |
| Config             | `toml` + `serde` (`toml_edit` for edits)|
| Sync execution     | `std::process::Command` (shell out)     |
| Logging            | `tracing` / `env_logger`                |

## Where to go next

- New user? Start with [Install](install.md), then [Usage](usage.md).
- Want natural-language dates? See [Writing dates](dates.md).
- Building custom views? See [Reports](reports.md) and [Filters](filters.md).
