# calman

A minimalist, keyboard-driven task & event manager with a Taskwarrior-style CLI
and (planned) TUI. Data is stored locally as `jsonl` or `ics` (CalDAV/vdir
compatible); sync is delegated to external commands (`git`, `pimsync`,
`rclone`, …).

## Install

```sh
cargo build --release          # binary: target/release/calman
# or copy the shipped config tiers into ~/.config/calman/
```

## Quick start

```sh
calman add "buy milk" due:tomorrow pri:H +home
calman add "standup" start:tomorrow recur:daily        # event, daily
calman                                          # → `next` report
calman list source:work
calman done 1
calman count +OVERDUE
calman help                                     # full cheat-sheet
```

## Configuration (two tiers)

calman ships two config tiers; copy what you need into `~/.config/calman/`.

- **`*.default.toml`** — minimal, working baseline (no comments):
  `config.default.toml`, `report.default.toml`, `theme.default.toml`.
- **`*.example.toml`** — heavily annotated custom samples:
  `config.example.toml`, `report.example.toml`, `theme.example.toml`.

A typical `config.toml` includes the defaults and overrides a few keys:

```toml
include = ["report.default.toml", "theme.default.toml"]

[defaults]
write_source = "work"
default_report = "next"

[date]
due_date_overdue_today = false     # all-day `due` overdue only after its day

[[source]]
name = "work"
type = "jsonl"
location = "~/.local/share/calman/work/"

[[source]]
name = "remote"
type = "ics-dir"                   # Radicale/vdirsync layout
location = "~/calman/remote"
```

### Sources

`jsonl` (fast, git-friendly), `ics` (one `.ics` per item), and `ics-dir`
(auto-discovers collections). For an `ics-dir` source, reference a single
collection with a composite name:

```sh
calman add "meet" start:tomorrow source:remote/sorge
calman list source:remote          # expands all collections
```

## Tasks & events

- **Todo**: `add <text> due:<date> …`
- **Event**: `add <text> start:<date> [end:<date> | duration:<dur>] …`

A date-only `due:`/`start:` is stored as an all-day item (`DUE;VALUE=DATE` /
`DTSTART;VALUE=DATE` in ICS) — compatible with iOS Reminders.

## Recurrence (`recur:` / `repeat:`)

Normalized to a standard RFC 5545 `RRULE` (iOS/Outlook compatible).

```sh
calman add "gym" due:tomorrow recur:every monday and wednesday for 8 weeks
calman add "pay" due:eom recur:FREQ=MONTHLY;UNTIL=20261231
```

- frequency: `daily weekly monthly yearly`
- interval: `every 7d` / `7d` / `every 2 weeks`
- weekdays: `every tuesday and friday` / `every weekend`
- end: `for 5 times` / `for 7 weeks` / `count:5` / `until:20260925` / `until:eoy` / `until:eom`
- raw passthrough: `recur:FREQ=WEEKLY;BYDAY=TU,FR`

## Filters

Shared by CLI args and report `filter` strings.

```
type:todo | type:event | type:all        (+TODO / +EVENT)
source:work   -source:work
due:today (exact day)  due.before:<  due.by:<=  due.after:>=
status:pending|in-progress|completed|cancelled|active
+OVERDUE +PENDING +COMPLETED +CANCELLED +IN-PROCESS +TAGGED +UNTAGGED +SCHEDULED
+tag / -tag
```

Composition — `and` binds tighter than `or`:

```
calman type:event due.after:sod
calman type:todo +PENDING or type:event
calman '(status:active or status:in-progress) source:work'
```

## Reports & overrides

Built-in reports: `ls`, `list`, `next` (bare `calman` runs `next`). Customize
inline with Taskwarrior-style `rc` overrides:

```sh
calman rc.report.next.columns=id,date,summary rc.report.next.labels=ID,DATE,TASK next
calman rc.report.next.filter='type:event' list
```

## Icons (nerdfont)

3-level fallback: column `icons` > `[icons.todo]` / `[icons.event]` > builtin.

```toml
[icons.todo]
pending = "○"  in-progress = "●"  completed = "✓"  cancelled = "✕"
[icons.event]
pending = "󰃭"  cancelled = "✕"    # non-cancelled events show the calendar glyph
```

## Sync

```toml
[[source.sync]]
pre_hook = "git pull --rebase origin main"
cmd = "git add . && git commit -m 'calman' && git push"
post_hook = "echo synced"
```

```sh
calman sync work
```

## Documentation

- `SPEC.md` — product spec & decisions
- `DESIGN.md` — detailed functional design
- `PLAN.md` — phased roadmap
- `feature.md` — feature backlog & design notes
- `AGENTS.md` — developer guide

## License

See repository.
