# calman — Documentation

`calman` is a minimalist, keyboard-driven **task & event manager** with a
Taskwarrior-style CLI and a planned TUI. Data lives locally as `jsonl` or
`ics` (CalDAV/vdir compatible); sync is delegated entirely to external commands
(`git`, `pimsync`, `rclone`, …).

## Pages

- [Introduction](introduction.md) — what calman is and its design philosophy
- [Install](install.md) — build from source, config directory, two config tiers, first run
- [Usage](usage.md) — every command with syntax, common options, and examples
- [Writing dates](dates.md) — date/time syntax for `due:` / `from:` and all-day semantics
- [Tasks & events](tasks.md) — adding todos and events, attributes, relations
- [Filters](filters.md) — the shared filter grammar used by the CLI and reports
- [Reports](reports.md) — built-in reports, `rc` overrides, icons, and colors
- [Recurrence](recurrence.md) — `recur:` / `repeat:` normalised to RFC 5545 `RRULE`
- [iCalendar format](icalendar.md) — `.ics` file format reference (RFC 5545): structure, date/time forms, `RRULE`, `STATUS`, `VTIMEZONE`

## Learning the source code 学习源码

This repository is also a **Rust learning project**. Source files carry
bilingual comments (中文 + English), and the guides below teach you to read
them from scratch — no coding experience needed.

本仓库同时也是 **Rust 学习项目**。源码带中英双语注释，下面指南从零教你读懂。

- [LEARN.md](learning/LEARN.md) — where to start 从这里开始
- [Rust basics](learning/rust-basics.md) — Rust 基础
- [Code workflow](learning/code-workflow.md) — 程序流程
- [Module map](learning/module-map.md) — 模块导览 & 阅读顺序

## Other project docs (repository root)

- `README.md` — quick start

> Tip: run `calman help` (or `calman filters`) any time to print a compact
> cheat-sheet of commands, attributes, recurrence, the filter grammar, and the
> config tiers.

> Tip: run `calman help` (or `calman filters`) any time to print a compact
> cheat-sheet of commands, attributes, recurrence, the filter grammar, and the
> config tiers.

## Inspiration

calman draws on a few existing tools:

- **Taskwarrior** — studied its core usage and command-line interface; calman borrows its CLI syntax and philosophy.
- **khal** — a TUI for calendars and events; an influence on calman's planned TUI and on its calendar/event model.
- **cfait** — borrowed the idea of storing each item as its own `ics` file.
