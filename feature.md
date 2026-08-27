# features

features that i want.

## cli

### filter

add type:todo, type:event, type:all

### date

for date, support eonnd, soppw. that is, the day after next day, the week before previous week.

### recursive and repeat

design repeat even and todo cmd. use `recur:` not `repeat:`
every 7d, week, 3d, month, 30d etc.
two way for duration. repeat times and until date. for example, i want do something every friday and for 5 times, or i want to exercise every weekend, until end of the year.

also, how to set every Tuesday and Friday for each week, and 7 weeks total? (a typical case for school class)

### report

3 builtin reports, overridable in `config.toml`:

- `ls` — short, few columns
- `list` — long, more columns
- `next` — default report. bare `calman` → `next` via `[defaults] default_report`

CLI habits follow taskwarrior; internal semantics follow CalDAV/iCalendar.

```toml
[defaults]
default_report = "next"

[report.ls]
filter = "status:active"
sort = ["due+", "created+"]
columns = [
  { field = "id",      label = "ID",   width = 4 },
  { field = "type",    label = "TYPE", icon = true },
  { field = "status",  label = "ST",   icon = true },
  { field = "due",     label = "DUE",  format = "relative" },
  { field = "summary", label = "SUMMARY" },
]

[report.list]
filter = "-status:completed -status:cancelled"
sort = ["status-", "pri-", "due+"]
columns = [
  { field = "id",      label = "ID",     width = 4 },
  { field = "type",    label = "TYPE",   icon = true },
  { field = "status",  label = "STATUS", icon = true },
  { field = "pri",     label = "PRI" },
  { field = "due",     label = "DUE",    format = "relative" },
  { field = "tags",    label = "TAGS" },
  { field = "summary", label = "SUMMARY" },
  { field = "desc",    label = "DESC" },
]

[report.next]
filter = "status:active"
sort = ["due+", "pri-"]
columns = [
  { field = "id",      label = "ID",   width = 4 },
  { field = "type",    label = "TYPE", icon = true },
  { field = "due",     label = "DUE",  format = "countdown" },
  { field = "summary", label = "SUMMARY" },
]
```

Columns:

- fields: `id, status, summary, desc, tags, due, pri, type, source`
- `summary` = CalDAV `SUMMARY`; `desc` = `DESCRIPTION` (two separate fields)
- `due` column: todo → `due`, event → `dtstart`
- `format`: `relative | countdown | iso | truncate`; `width` = min-width
- `sort`: `key+`/`key-`, trailing `/` = break line (taskwarrior style); keys incl. `id, created, updated, due, pri, status, summary`

Icons (nerdfont), 3-level fallback: column `icons` > global `[icons]` > builtin defaults:

```toml
[icons]
[icons.status]
pending = "○"
in-progress = "●"
completed = "✓"
cancelled = "✕"

[icons.type]
todo = "󰄰"
event = "󰃭"

[report.ls]
columns = [
  { field = "status", label = "ST", icon = true,
    icons = { completed = "✔" } },   # per-column override, rest fall back
]
```

Colors: global rules, row-level, first match wins:

```toml
[[color]]
name = "overdue"
fg = "red"
bold = true
filter = "status:pending due.before:now"
```

Style fields: `fg, bg, bold, underline, italic, dim`.

Builtin virtual tags (evaluated at runtime, usable in filters):

- `OVERDUE` — derived: `due < now` && status active
- `DONE` ≡ `COMPLETED` — alias; maps to `STATUS:COMPLETED` + `COMPLETED` timestamp
- `CANCELLED` — `STATUS:CANCELLED`
- `IN-PROCESS` — `STATUS:IN-PROCESS`
- `TAGGED` / `UNTAGGED` — derived from `CATEGORIES`
- `TODO` / `EVENT` — task type
- `SCHEDULED` — event 的定时即 `DTSTART`, 无需额外字段
- `DELETED` — local only (hard delete), no CalDAV mapping

No `WAITING` concept — drop it; events use `DTSTART`.

Relations: `RELATED-TO` default `RELTYPE=PARENT` → value = parent UID; child stores parent. `A RELATED-TO:B` → B parent, A subtask.

Syntax: `calman add "subitem" rel:<parent-id>` or `calman <id> modify rel:<parent-id>` writes the parent UID into the child's `related_to`.

## todo to event

let me start a todo. when stop it, create an event.

## tui

use a standard tui.toml for config.
