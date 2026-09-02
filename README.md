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
calman add "standup" from:tomorrow recur:daily        # event, daily
calman                                          # → `next` report
calman list source:work
calman done 1
calman start 1 && calman stop 1   # time it, then turn it into an event
calman count +OVERDUE
calman help                                     # full cheat-sheet
```

## Configuration (two tiers)

calman ships two config tiers; copy what you need into `~/.config/calman/`.

- **`*.default.toml`** — minimal, working baseline (no comments):
  `config.default.toml` (self-contained: every default incl. `[report.*]`,
  `[colorscheme]`, `[icons]` — no separate `report.default.toml`).
- **`*.example.toml`** — heavily annotated custom samples:
  `config.example.toml`, `report.example.toml`, `colorscheme.example.toml`.

A typical `config.toml` includes the defaults and overrides a few keys:

```toml
include = ["config.default.toml", "colorscheme.example.toml"]

[defaults]
write_source = "work"
default_report = "next"

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
calman add "meet" from:tomorrow source:remote/sorge
calman list source:remote          # expands all collections
```

## Tasks & events

- **Todo**: `add <text> due:<date> …`
- **Event**: `add <text> from:<date> [to:<date> | for:<dur>] …`

A date-only `due:`/`from:` is stored as an all-day item (`DUE;VALUE=DATE` /
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
- series end: `for 5 times` / `for 7 weeks` / `count:5` / `until:20260925` / `until:eoy` / `until:eom` (and as separate tokens: `recur:daily count:5`)
- raw passthrough: `recur:FREQ=WEEKLY;BYDAY=TU,FR`
- ISO 8601 period: `recur:P7D` → `FREQ=DAILY;INTERVAL=7` (also `P2W`/`P1M`/`P1Y`)
- natural language (default build): `recur:"every tuesday"`

**Series model**: a task with `recur:` becomes the recurring **master**
(`status:recurring`, virtual tag `+PARENT`). Masters are hidden from
`ls`/`list`/`next` by default — show with `calman list +PARENT`. `done` on a
master cancels the whole series.

**Per-occurrence exceptions** (`recur-expand` is default):
`list` shows the nearest upcoming occurrence per series as a virtual row with a
plain sequential ID (`[defaults] recur_expand_count = 1`; `0` = all future);
address one via its plain ID, `on:<date>`, or `<id>.<n>`:

```sh
calman done 5.2                 # complete one occurrence → Completed override record
calman modify 5.1 summary:x     # override occurrence → RECURRENCE-ID sibling
calman info 5.2                 # full details of one occurrence
calman modify 5.2 all-future summary:x   # split series here: old keeps past, new edited series starts at #2
calman delete 5.2 all-future    # truncate series here (#2 and everything after removed)
```
TTY prompts on `modify`/`delete` of an occurrence ask whether to apply to all
future occurrences first; non-TTY defaults to the single instance.

## Filters

Shared by CLI args and report `filter` strings.

```
type:todo | type:event | type:all        (+TODO / +EVENT)
source:work   -source:work
due:today (exact day)  due.before:<  due.by:<=  due.after:>=
status:pending|in-progress|completed|cancelled|recurring|active
+OVERDUE +PENDING +DUE +COMPLETED +CANCELLED +IN-PROCESS +TAGGED +UNTAGGED +PARENT
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

- User docs: `docs/` (`index.md` is the entry page)
- **Learning the source** 学习源码: `docs/learning/LEARN.md` — bilingual Rust
  guides + module map + code workflow (源码带中英双语注释)

## License

See repository.
