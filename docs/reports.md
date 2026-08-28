# Reports

A *report* is a named table view: a filter, a sort order, and a set of columns.
calman ships three built-ins and lets you define more in `config.toml` or
override any of them inline with Taskwarrior-style `rc` tokens.

## Running reports

```sh
calman                          # bare → `[defaults].default_report` (default: `next`)
calman ls
calman list
calman next
calman +OVERDUE list            # CLI filters combine with the report filter
```

`bare calman` runs `[defaults].default_report`, which **defaults to `next`**
(not `list`).

## Built-in reports

| Report | Purpose | Sort | Default filter |
| :----- | :------ | :--- | :------------- |
| `ls` | Short list | `due+`, `created+` | `type:todo status:active or type:event due.after:sod` |
| `list` | Long list | `status-`, `pri-`, `due+` | `type:todo -status:completed -status:cancelled or type:event due.after:sod` |
| `next` | Up-next (the default) | `due+`, `pri-` | `type:todo status:active or type:event due.after:sod` |

The event half of each default filter keeps only **future events**
(`dtstart ≥ start of today` via `due.after:sod`); todos keep their status
filter. To show past events too, override the filter (see below).

### Default columns

- **`ls` / `next`**: `id` (4), `status` (icon), `date` (`todo_format=relative`),
  `summary`.
- **`list`**: `id` (4), `status` (icon), `pri`, `date` (`todo_format=relative`),
  `tags`, `summary`, `desc`.

`status` renders a merged **STATUS** column: todos show a status glyph, events
show a calendar glyph (see *Icons*).

## Columns, fields & formats

Available **fields**: `id`, `status`, `summary`, `desc`, `tags`, `date`
(alias `due`), `due`, `pri`, `type`, `source`.

A column is configured as:

```toml
{ field = "date", label = "DATE", width = 12, format = "relative", icon = true,
  todo_format = "relative", event_format = "%m/%d",
  icons = { completed = "✔", pending = "◌" } }
```

- `label` — header text (defaults to the upper-cased field).
- `width` — minimum column width.
- `format` — `relative` | `countdown` | `iso` | `truncate` | any `chrono`
  `strftime` pattern (e.g. `%Y-%m-%d`).
- `icon` — render a nerdfont glyph (status/type columns).
- `todo_format` / `event_format` — per-kind formatting **for the `date` column
  only**: events default to plain `MM/DD`; todos default to `relative`.
- `icons` — per-column glyph overrides (keyed by status/value).

Sort keys use `key+` (ascending) / `key-` (descending); a **trailing `/`**
inserts a blank separator line when the value changes:

```toml
sort = ["status-", "pri-", "due+"]
sort = ["due+", "created+/"]   # break between different due days
```

## Defining custom reports

Add a `[report.<name>]` table in `config.toml` (or an included file). Any name
overrides a built-in of the same name.

```toml
[report.overdue]
filter = "+OVERDUE"
sort = ["due+"]
columns = [
  { field = "id",      label = "ID",   width = 4 },
  { field = "date",    label = "DUE",  todo_format = "relative" },
  { field = "summary", label = "TASK" },
]
```

## `rc` overrides (script-friendly)

Override any report without editing config by passing `rc.report.<name>.<key>`:

```sh
calman rc.report.next.columns=id,date,summary \
      rc.report.next.labels=ID,DATE,TASK \
      next

calman rc.report.next.filter='type:event due.after:20260101' next
calman rc.report.list.sort=pri-,due+ list
```

Supported keys: **`columns`**, **`labels`**, **`filter`**, **`sort`**.
`columns` accepts `field` or `field.format` tokens (comma-separated); `labels`
and `sort` are comma-separated.

## Icons (nerdfont)

Three-level fallback — most specific wins:

1. **per-column** `icons = { … }`
2. **global** `[icons.todo]` / `[icons.event]` (keyed by status)
3. **built-in** defaults

Built-in status glyphs:

- **todo**: `pending` ○ · `in-progress` ● · `completed` ✓ · `cancelled` ✕
- **event**: any non-cancelled status → calendar glyph (󰃭); `cancelled` → ✕

Example global override:

```toml
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
```

(The merged STATUS column picks the table by item kind: `todo` → `[icons.todo]`,
`event` → `[icons.event]`.)

## Colors

Row-level colors use the Taskwarrior `custom.theme` vocabulary under `[theme]`:

```toml
[theme]
name = "default"
"rule.precedence.color" = "deleted,completed,active,overdue,due.today,due,uda.,blocked,blocking,scheduled,tagged"

[theme.color]
completed = "gray10 on gray2"
active = "underline"
overdue = "inverse"
"due.today" = "underline"
due = "bold"
blocked = "gray18"
"uda.priority.L" = "gray14"
"uda.priority.M" = "green"
"uda.priority.H" = "yellow"
blocking = "gray23"
scheduled = "gray22"
tagged = "gray21 bold"
```

- `rule.precedence.color` is a comma-separated list; the **first rule that both
  is defined and matches** wins.
- Style syntax: `fg [on bg] [bold|underline|italic|dim|inverse]`.
- Colors: `black red green yellow blue magenta cyan white`, `bright-*`,
  `gray`/`grey`, and `gray0`–`gray23` (256-scale).
- Supported rules: `deleted` (never matches — calman hard-deletes),
  `completed`, `active`, `overdue`, `due.today`, `due`, `blocked`, `blocking`,
  `scheduled`, `tagged`, `uda.priority.L|M|H`.

**What each rule matches** (first defined rule that matches wins):

| Rule | Matches |
| :--- | :------ |
| `completed` | status `completed` |
| `active` | status `pending` **or** `in-progress` (same as `+ACTIVE`) |
| `overdue` | a past `due`/`dtstart` and not done (covers both todos **and** events) |
| `due.today` | `due` falls on the local calendar day |
| `due` | has any `due` |
| `scheduled` | is an event (has `dtstart`) |
| `blocked` | a parent todo referenced by another item's `related_to` |
| `blocking` | a todo that has a `related_to` |
| `tagged` | has ≥1 tag |
| `uda.priority.L|M|H` | priority 1 / 5 / 9 |

## Sync (referenced by reports/CLI)

`calman sync` runs each source's `[source].sync` chain:

```toml
[[source]]
name = "work"
type = "jsonl"
location = "~/.local/share/calman/work/"
[source.sync]
pre_hook = "git pull --rebase origin main"
cmd = "git add . && git commit -m 'calman' && git push"
post_hook = "echo synced"
```

`{location}` and `{name}` are substituted into each hook command (the working
directory is set to `location`). A `.sync.lock` file skips a source already
being synced, and a non-zero hook exit stops the chain immediately.

> **Caveat:** `{location}` is substituted **unquoted** into the `sh -c` command
> line, so a path containing shell metacharacters could be misinterpreted. Keep
> sync locations free of spaces/special characters, or wrap them carefully in
> your hook scripts.
