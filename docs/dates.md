# Writing dates & times

calman parses dates with a custom natural-language engine (no external grammar
library). Dates are interpreted in your **system local timezone** and stored as
UTC; on display they are converted back to local time.

There are two shapes of value:

- **Date-only** — a calendar day, no time. Used for **all-day** items.
- **Timed** — a specific instant.

Date syntax (all accept either form):

| Form | Meaning |
| :--- | :------ |
| `YYYYMMDD` / `YYYY-MM-DD` | a specific day (all-day) |
| `MM-DD` | that month/day **this year** (e.g. `08-26`, `9-30`) |
| `T[HH[MM[SS]]]` | today at that time (e.g. `T09` = 09:00) |
| `YYYYMMDDT[HH[MM[SS]]]` | specific day + time |
| `HH:MM` | today at that wall-clock time |

## All-day semantics

A **date-only** `due:` (todo) or `from:` (event) becomes an **all-day** item:

- stored as `DUE;VALUE=DATE` / `DTSTART;VALUE=DATE` in ICS (no time component),
  which is what iOS Reminders and most CalDAV clients expect;
- `task.allday = true` internally.

A **timed** value keeps its clock time.

> Overdue rule for all-day todos is **fixed**: a date-only `due` counts as
> overdue only after the due day passes (see [Filters](filters.md)).

## Named dates (date-only → all-day)

Only these resolve to a **calendar day** (all-day). All-day is expressed by
`YYYYMMDD` / `YYYY-MM-DD` syntax, never by `s…`/`e…` boundary tokens:

| Token(s) | Meaning |
| :------- | :------ |
| `today` | today (all-day) |
| `tomorrow`, `sond` | tomorrow (all-day) |
| `yesterday` | yesterday (all-day) |

## Named date boundaries (timed, configurable)

Start/end boundaries carry a clock time from `[date]`:

- `day_start` (default `00:00:00`) — all `s…` **start** tokens
- `day_end` (default `23:59:59`) — all `e…` **end** tokens

| Token(s) | Meaning |
| :------- | :------ |
| `sod` | start of today (`day_start`) |
| `sow`, `soww` | start of this week — Monday `day_start` |
| `som` | start of this month (1st, `day_start`) |
| `soq` | start of this quarter (1st, `day_start`) |
| `soy` | start of this year (Jan 1, `day_start`) |
| `sonw`, `sonww` | start of next week (next Monday, `day_start`) |
| `sonm` | start of next month (`day_start`) |
| `sony` | start of next year (`day_start`) |
| `sopw`, `sopww` | start of previous week (prev Monday, `day_start`) |
| `sopm` | start of previous month (`day_start`) |
| `sopy` | start of previous year (`day_start`) |
| `eod` | end of today (`day_end`) |
| `eond` | end of next day (tomorrow `day_end`) |
| `eow` | end of this week — Sunday `day_end` |
| `eoww` | end of working week — Friday `day_end` |
| `eom` / `eoq` / `eoy` | end of this month / quarter / year (`day_end`) |
| `eonw` / `eonww` | end of next week (`day_end`) |
| `eonm` / `eony` | end of next month / year (`day_end`) |
| `eopw` / `eopww` | end of previous week (`day_end`) |
| `eopm` / `eopy` | end of previous month / year (`day_end`) |

The week starts on **Monday** (`sow`/`eow`, `soww`/`eoww`).

```toml
[date]
day_start = "08:00:00"   # sod/sow/som/…
day_end = "18:30:00"     # eod/eow/eom/…（含 eoww）
```

## Other timed tokens

| Token(s) | Meaning |
| :------- | :------ |
| `now` | the current instant |

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
calman add "standup" from:+2h
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
calman add "call" from:2026-08-26 15:00
calman add "dentist" from:0826T0930
calman add "coffee" from:T0900
calman add "wrap up" due:17          # 17th of this month, all-day
```

## Notes & limits

- Times are **24-hour** `HH:MM`. The parser does **not** currently accept
  `2pm`-style clocks or bare weekday names (e.g. `monday`, `fri+1`); use the
  named periods above (`sow`/`eow`, …) or explicit dates instead.
- `eoww` (end of working week) is computed as **Friday `[date].day_end`**
  (same clock time as `eow`).
- All-day `due`/`start` lands at local midnight; timed values keep their
  wall-clock time and are converted to UTC for storage.

## Overdue

For an **all-day** todo, `+OVERDUE` only fires after the due day passes
(today's due is not overdue). Timed todos are overdue as soon as their `due`
instant is in the past. The behavior is fixed — there is no config toggle
(see [Filters](filters.md)).
