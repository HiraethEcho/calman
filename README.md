# calman

A minimalist, keyboard-driven task manager for the terminal. Local-first, Taskwarrior-flavored, with pluggable sync.

```
calman add buy milk priority:H +home due:eow
calman 1 modify new content pri:L -home due:20260824
calman +OVERDUE
calman count status:pending
```

Bare `calman` lists tasks — no typing required.

## Philosophy

- **Storage is files** — tasks live in plain `jsonl` (default) or `ics` files you own.
- **Sync is commands** — calman never syncs itself. You configure external commands (`git`, `pimsync`, `rclone`, …) and calman runs them.
- **Interface is efficiency** — one short ID per task, natural-language dates, filters everywhere, no ceremony.

## Status

| Phase | Scope | State |
|---|---|---|
| 1 | Core CLI: add/list/done/delete/modify/count/sync | ✅ usable |
| 2 | Full date grammar, query engine, integration tests | 🔜 planned |
| 3 | TUI (ratatui) | 🔜 planned |

## Build

Requires Rust edition 2024 (1.85+).

```sh
cargo build --release
```

### Feature matrix

Choose storage backends and optionally the TUI:

| Build | Command |
|---|---|
| both storages (default) | `cargo build` |
| jsonl only | `cargo build --no-default-features --features storage-jsonl` |
| ics only | `cargo build --no-default-features --features storage-ics` |
| both + TUI | `cargo build --features tui` |
| jsonl + TUI | `cargo build --no-default-features --features storage-jsonl,tui` |
| ics + TUI | `cargo build --no-default-features --features storage-ics,tui` |

## Quickstart

First run auto-creates a default config and data directory; nothing to install.

```sh
calman add "buy milk" priority:H +shopping +home due:eow
calman add "write report" due:eom +work
calman                          # list all
calman +PENDING                 # active tasks
calman +OVERDUE                 # overdue, not completed
calman priority:H               # high-priority tasks
calman -home                    # exclude tag
calman 1 done                   # complete task 1
calman 2 modify status:completed # or: calman done 2
calman delete 2                 # remove permanently
calman count status:pending     # number, for scripts
calman sync                     # run external sync hooks
calman source:work list          # limit to one source
calman source:work,personal count
```

### Add

```
calman add <SUMMARY> [ATTRS...]
```

Attributes:

| Attr | Meaning |
|---|---|
| `priority:H`/`M`/`L` or `pri:` | priority (9/5/1) |
| `due:<date>` | due date (todo) |
| `+<tag>` | add tag |
| `start:` / `end:` | event start / end (presence of `start:` = event) |
| `duration:` | `45min`, `1h`, `1h30m`, `2d` — alternative to `end:` (`dur:` alias) |
| `allday` | date-only event (`VALUE=DATE` in .ics) |
| `alert:` | reminder before start, e.g. `alert:15min` → VALARM `-PT15M` |
| `location:` / `repeat:` | event location / RRULE |
| `desc:` | long description |

Event examples:

```sh
calman add "Team meeting" start:20260826-0900 duration:1h alert:15min +work
calman add "Conference" start:20260812 end:20260813        # all-day, 2 days
calman add "Birthday" start:0826                          # this year Aug 26
calman add "Call" start:-0900 duration:45min              # today 09:00
```

Date-only `start:` → all-day event. All-day `end` is inclusive; stored `DTEND` = day after. No `end`/`duration` → `[date] default_event_duration` (default `1h`); all-day without end = single day.

Modify: `+allday` converts to all-day (drops times). `start:` converts back to timed; date-only `start:` uses `[date] default_start_time` (default `09:00`).

### Modify

`calman <ID>... modify <TEXT> [ATTRS...]` — bare words replace the summary. `+tag` adds, `-tag` removes.

```
calman 1 modify new content pri:L -home due:20260824
```

### Dates

Taskwarrior-style named dates, resolved to local time, stored as UTC:

```
sod eod today tomorrow yesterday now
sow eow soww eoww          # week + working-week (Fri 17:00)
som eom soq eoq soy eoy
sond eond sonw eonw sopw eopw sonm eonm sopm eopm sony eony sopy eopy
```

Also: `YYYY-MM-DD`, `YYYYMMDD`, `YYYY-MM-DD HH:MM`, `HH:MM`, relative `+3d -2w +1m +1y +2h`.

## Storage

`[[source]]` entries in the config define backends:

- **jsonl** — one `tasks.jsonl`, streamed, git-diff-friendly. Atomic writes (tmp + rename).
- **ics** — one `<UID>.ics` per task (VTODO/VEVENT), CalDAV/vdir-compatible.

Data locations expand `~`. Every source keeps `.calman-state.json` metadata.

### CalDAV note

All dates are stored as concrete UTC timestamps (`DUE:…Z` etc.) — fully CalDAV-compatible. Recurrence lives in `RRULE`.

## Sync

Sync is delegated to user commands per source:

```toml
[[source]]
name = "work"
type = "jsonl"
location = "~/.local/share/calman/work/"

[source.sync]
pre_hook  = "git pull --rebase origin main"
cmd       = "git add . && git commit -m 'calman sync' && git push"
post_hook = "echo 'synced'"
```

Hooks run with the source directory as cwd. `{location}` and `{name}` placeholders are substituted. `pre_hook`/`cmd` failure aborts the chain and releases the `.sync.lock`. calman never resolves conflicts — your commands decide.

## Configuration

- Default: `~/.config/calman/config.toml` (or `$XDG_CONFIG_HOME/calman/config.toml`)
- Full commented example: `~/.config/calman/config.example.toml`

Settings: write source, per-context source lists (`cli`/`sync`/`tui`), date engine (`week_start`, `workweek_end`), UI theme/keybindings, locale, sources + sync hooks.

## Sources & contexts

Multiple sources can be managed at once. Contexts control which sources each command sees:

```
[contexts]
cli  = ["work", "personal"]
sync = ["work"]
tui  = ["work", "personal"]
```

Empty list → all sources. Per-invocation override: `calman source:work list`.

`source:<name>` is a Taskwarrior-style attribute usable anywhere; comma-separate for multiple sources.

## Project docs

- `SPEC.md` — goals + tech stack
- `DESIGN.md` — complete behavior manual
- `PLAN.md` — phased roadmap
- `AGENTS.md` — developer guide

## License

GPL-3.0-or-later.