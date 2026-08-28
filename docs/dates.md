# Writing dates & times

calman parses dates with a custom natural-language engine (no external grammar
library). Dates are interpreted in your **system local timezone** and stored as
UTC; on display they are converted back to local time.

There are two shapes of value:

- **Date-only** — a calendar day, no time. Used for **all-day** items.
- **Timed** — a specific instant.

## All-day semantics

A **date-only** `due:` (todo) or `start:` (event) becomes an **all-day** item:

- stored as `DUE;VALUE=DATE` / `DTSTART;VALUE=DATE` in ICS (no time component),
  which is what iOS Reminders and most CalDAV clients expect;
- `task.allday = true` internally.

A **timed** value keeps its clock time.

> Overdue rule for all-day todos is **config-driven** — see
> `[date].due_date_overdue_today` below.

## Named dates (date-only → all-day)

These resolve to a calendar day boundary and therefore create all-day items:

| Token(s) | Meaning |
| :------- | :------ |
| `today`, `sod` | start of today (00:00) |
| `tomorrow`, `sond` | start of tomorrow (00:00) |
| `yesterday` | start of yesterday (00:00) |
| `sow`, `soww` | start of this week — Monday 00:00 |
| `som` | start of this month (1st, 00:00) |
| `soq` | start of this quarter (1st, 00:00) |
| `soy` | start of this year (Jan 1, 00:00) |
| `sonw`, `sonww` | start of next week (next Monday, 00:00) |
| `sonm` | start of next month (00:00) |
| `sony` | start of next year (00:00) |
| `sopw`, `sopww` | start of previous week (prev Monday, 00:00) |
| `sopm` | start of previous month (00:00) |
| `sopy` | start of previous year (00:00) |

The week starts on **Monday** (`sow`/`eow`, `soww`/`eoww`).

## Named dates (with time)

These carry a time-of-day and produce **timed** items:

| Token(s) | Meaning |
| :------- | :------ |
| `now` | the current instant |
| `eod` | end of today (23:59:59) |
| `eond` | end of next day (tomorrow 23:59:59) |
| `eow` | end of this week (Sunday 23:59:59) |
| `eoww` | end of the working week (**Friday `[date].workweek_end`**, default 17:00) |
| `eonw`, `eonww` | end of next week (next Sunday 23:59:59) |
| `eopw`, `eopww` | end of previous week (prev Sunday 23:59:59) |
| `eom` | end of this month (last day, 23:59:59) |
| `eonm` | end of next month (23:59:59) |
| `eopm` | end of previous month (23:59:59) |
| `eoq` | end of this quarter (23:59:59) |
| `eony` | end of next year (Dec 31, 23:59:59) |
| `eopy` | end of previous year (23:59:59) |

## Relative offsets

`+`/`-` followed by a number and a unit (applied to *now*):

| Form | Meaning |
| :--- | :------ |
| `+3d` / `-2w` | plus 3 days / minus 2 weeks |
| `+2h` | plus 2 hours |
| `+30s` | plus 30 seconds |
| `+1m` | plus ~30 days (month approximation) |
| `+1y` | plus ~365 days (year approximation) |

```sh
calman add "pay rent" due:+5d
calman add "retro" due:-1w
calman add "standup" start:+2h
```

> Note: `+1m` means **about a month**, *not* one minute. Clock times use the
> `HH:MM` form below.

## Absolute & compact forms

| Form | Example | Result |
| :--- | :------ | :----- |
| ISO date | `2026-08-26` | date-only (all-day) |
| ISO date+time | `2026-08-26 14:30` | timed |
| Compact date | `20260826` | date-only (all-day) |
| Compact this year | `0826` | Aug 26 this year, date-only |
| Compact this month | `17` | 17th of this month, date-only |
| Compact date+time | `20260826T090000` | Aug 26 2026, 09:00 |
| Month-day+time | `0826T0930` | Aug 26 this year, 09:30 |
| Day+time this month | `25T0930` | 25th this month, 09:30 |
| Today at time | `T0900` | today 09:00 |
| Time only | `14:30` | today at 14:30 |

```sh
calman add "release" due:2026-09-01
calman add "call" start:2026-08-26 15:00
calman add "dentist" start:0826T0930
calman add "coffee" start:T0900
calman add "wrap up" due:17          # 17th of this month, all-day
```

## Notes & limits

- Times are **24-hour** `HH:MM`. The parser does **not** currently accept
  `2pm`-style clocks or bare weekday names (e.g. `monday`, `fri+1`); use the
  named periods above (`sow`/`eow`, …) or explicit dates instead.
- `eoww` (end of working week) is computed as **Friday `[date].workweek_end`**
  (default `17:00`, format `HH:MM` or `HHMM`).
- All-day `due`/`start` lands at local midnight; timed values keep their
  wall-clock time and are converted to UTC for storage.

## `due_date_overdue_today`

For an all-day todo, "overdue" is governed by `[date].due_date_overdue_today`
in `config.toml`:

```toml
[date]
due_date_overdue_today = false   # default: overdue only after the due day passes
# due_date_overdue_today = true  # overdue starting on the due day itself
```

Timed todos are overdue as soon as their `due` instant is in the past,
regardless of this setting. The `+OVERDUE` virtual tag respects this rule
(see [Filters](filters.md)).
