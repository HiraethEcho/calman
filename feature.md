# features

features that i want.

## cli

### filter

filter expression shared between CLI args and `report.toml` `filter` strings.

- `and` / `or` / `(` / `)` supported (implicit `and` between adjacent atoms)
- `type:todo`, `type:event`, `type:all`
- `source:<name>` and `-source:<name>` (exclude a source)
- `due:<date>` = exact day; `due.before:<date>` = strictly before;
  `due.by:<date>` = on or before; `due.after:<date>` = on or after
- todos compare `due`; events compare `dtstart`
- virtual tags: `+OVERDUE +PENDING +COMPLETED +CANCELLED +IN-PROCESS +TAGGED
  +UNTAGGED +SCHEDULED`; events count as `PENDING`

```toml
[report.next]
filter = "type:todo status:active or type:event due.after:sod"
```

CLI custom report overrides (Taskwarrior rc style, for scripts):

```
calman rc.report.next.columns=id,date,summary rc.report.next.labels=ID,DATE,TASK next
calman rc.report.next.filter='type:event due.after:sod' list
```

supported rc keys: `columns`, `labels`, `filter`, `sort`.
`columns` accepts `field` or `field.format` (comma-separated).

Precedence: adjacent atoms = `and`; `and` binds tighter than `or`.
Use parens when the range is ambiguous:

```
A B or C D   = (A and B) or (C and D)   # NOT A and (B or C) and D
(A or B) C   = (A or B) and C
```

Separating events and todos:

```
calman type:event              # events only; builtin default still limits to future
calman type:todo +PENDING      # active todos only
calman rc.report.next.filter='type:event due.after:20260101' next  # all events incl. past
calman count type:event        # all events regardless of report defaults
```

### date

for date, support eond, sopw. that is, the end of next day, the start of the previous week.

a date-only `due:` (e.g. `due:20260826`, `due:tomorrow`) is an all-day todo:
- stored as `DUE;VALUE=DATE` in ICS (no time component)
- `task.allday = true`; events keep the same flag for `DTSTART;VALUE=DATE`
- overdue rule is config-driven: `[date] due_date_overdue_today`
  - `false` (default): overdue only after the due day passes
  - `true`: overdue starting on the due day itself

### recursive and repeat

use `recur:` (alias `repeat:`) for both todo and event.
calman normalizes it to a standard RFC 5545 `RRULE` (iOS Reminders / CalDAV
compatible). It accepts either a raw rule or a friendly expression:

- raw passthrough: `recur:FREQ=WEEKLY;BYDAY=TU,FR;UNTIL=20260925`
- frequency: `daily` `weekly` `monthly` `yearly`
- interval: `every 7d` / `7d` / `every 2 weeks` (→ `INTERVAL=`)
- weekdays: `every tuesday and friday` / `every weekend` (→ `BYDAY=`)
- end: `for 5 times` / `for 7 weeks` (weeks × weekday count → `COUNT=`)
  / `count:5` / `until:20260925` / `until:eoy` / `until:eom`

examples:
- `recur:every friday for 5 times` → `FREQ=WEEKLY;BYDAY=FR;COUNT=5`
- `recur:every tuesday and friday for 7 weeks` → `FREQ=WEEKLY;BYDAY=TU,FR;COUNT=14`
- `recur:every weekend until:eoy` → `FREQ=WEEKLY;BYDAY=SA,SU;UNTIL=20261231`
- iPhone-created: `RRULE:FREQ=DAILY;UNTIL=20260925` (round-trips as-is)

design repeat even and todo cmd. also, how to set every Tuesday and Friday for
each week, and 7 weeks total? (a typical case for school class)


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

Icons (nerdfont), 3-level fallback: column `icons` > global `[icons]` > builtin defaults.
Merged STATUS column is per-kind: todos use `[icons.todo]`, events use `[icons.event]`.
Both tables keyed by status: `pending`, `in-progress`, `completed`, `cancelled`.
(VEVENT only has TENTATIVE/CONFIRMED/CANCELLED in iCalendar; completed is
internal-only.)

```toml
[icons]
[icons.todo]
pending = "○"
in-progress = "●"
completed = "✓"
cancelled = "✕"

[icons.event]
pending = "○"
in-progress = "●"
completed = "✓"
cancelled = "✕"

[report.ls]
columns = [
  { field = "status", label = "ST", icon = true,
    icons = { completed = "✔", pending = "◌" } },   # per-column override, rest fall back
]
```

Colors: global row-level rules use the `[colorscheme]` table:

```toml
[colorscheme]
priority = ["completed", "cancelled", "overdue", "today", "due",
            "blocked", "blocking", "scheduled", "tagged",
            "priority.H", "priority.M", "priority.L"]

[colorscheme.rules]
completed = {fg="gray10"}
overdue = {inverse=true}
```

Style: `fg [on bg] [bold|underline|italic|dim|inverse]`; rules: `completed cancelled overdue today due priority.L priority.M priority.H scheduled tagged blocked blocking`. First match in `priority` order wins.

Builtin virtual tags (evaluated at runtime, usable in filters):

- `OVERDUE` — derived: `due < now` && status active
- `DONE` ≡ `COMPLETED` — alias; maps to `STATUS:COMPLETED` + `COMPLETED` timestamp
- `CANCELLED` — `STATUS:CANCELLED`
- `IN-PROGRESS` — `STATUS:IN-PROGRESS`
- `TAGGED` / `UNTAGGED` — derived from `CATEGORIES`
- `TODO` / `EVENT` — task type
- `SCHEDULED` — event 的定时即 `DTSTART`, 无需额外字段
- `DELETED` — local only (hard delete), no CalDAV mapping

No `WAITING` concept — drop it; events use `DTSTART`.

Relations: `RELATED-TO` default `RELTYPE=PARENT` → value = parent UID; child stores parent. `A RELATED-TO:B` → B parent, A subtask.

Syntax: `calman add "subitem" rel:<parent-id>` or `calman <id> modify rel:<parent-id>` writes the parent UID into the child's `related_to`.

### more on report

- default reports show only future events (dtstart >= start of today); todos keep
  their current default filter
- type + status merged into one STATUS column: events render the calendar icon,
  todos render their status icon; events still count as `PENDING` for filters
- DUE column renamed DATE: event → plain date (default `MM/DD`), todo →
  relative (default); both configurable per-column via `event_format` and
  `todo_format`:

```toml
columns = [
  { field = "date", label = "DATE", todo_format = "relative", event_format = "%Y-%m-%d" },
]
```

## todo to event

let me start a todo. when stop it, create an event.

## tui

use a standard tui.toml for config.
