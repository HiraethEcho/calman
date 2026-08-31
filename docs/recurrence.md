# Recurrence

Recurrence uses `recur:` (alias `repeat:`) on `add` or `modify`. calman
normalises the value into a standard **RFC 5545 `RRULE`** string — the same
format iOS Reminders, Outlook, and CalDAV servers expect — and stores it as the
item's `rrule` (rendered as `RRULE:` in ICS).

```sh
calman add "gym" due:tomorrow recur:every monday and wednesday for 8 weeks
calman add "pay rent" due:eom recur:FREQ=MONTHLY;UNTIL=20261231
calman add "standup" start:tomorrow recur:daily
```

Recurrence applies to **both todos and events**.

## Two input styles

### 1. Raw passthrough

Any string beginning with `FREQ=` (or `RRULE:`) is uppercased and emitted
verbatim (the `RRULE:` prefix is stripped). This guarantees round-tripping of
rules created by other apps:

```sh
recur:FREQ=WEEKLY;BYDAY=TU,FR;UNTIL=20260925
# → FREQ=WEEKLY;BYDAY=TU,FR;UNTIL=20260925

recur:RRULE:FREQ=DAILY;UNTIL=20260925
# → FREQ=DAILY;UNTIL=20260925
```

### 2. ISO 8601 period

A plain period (`P7D`, `P2W`, `P1M`, `P1Y`) maps to a frequency + interval.
Always available, even in builds without the `date-natural` feature:

```sh
recur:P7D → FREQ=DAILY;INTERVAL=7
recur:P2W → FREQ=WEEKLY;INTERVAL=2
recur:P1M → FREQ=MONTHLY
```

`alert:`/`duration:` accept ISO 8601 durations too: `PT15M`, `PT1H30M`,
`P7D`, `P2W`.

### 3. Friendly grammar

The friendly parser is whitespace/comma-driven and ignores the words `every`
and `and`. It understands:

| Concept | Forms | Produces |
| :------ | :---- | :------- |
| Frequency | `daily` `weekly` `monthly` `yearly` | `FREQ=…` |
| Interval | `every 7d` · `7d` · `every 2 weeks` | `INTERVAL=n` |
| Weekdays | `every tuesday and friday` · `every weekend` | `BYDAY=…` |
| End by count | `for 5 times` · `for 7 weeks` · `count:5` | `COUNT=n` |
| End by date | `until:20260925` · `until:eoy` · `until:eom` | `UNTIL=YYYYMMDD` |

Weekday codes: `mon`→`MO`, `tue`→`TU`, `wed`→`WE`, `thu`→`TH`, `fri`→`FR`,
`sat`→`SA`, `sun`→`SU`. `weekend` expands to `SA,SU`; `weekday` to `MO–FR`.

`for N weeks` multiplies `N` by the number of selected weekdays to compute
`COUNT` (e.g. `tuesday and friday for 7 weeks` → `COUNT=14`).

## Examples

| You write | Resulting `RRULE` |
| :-------- | :---------------- |
| `recur:daily` | `FREQ=DAILY` |
| `recur:every 7d` | `FREQ=DAILY;INTERVAL=7` |
| `recur:every 2 weeks` | `FREQ=WEEKLY;INTERVAL=2` |
| `recur:every tuesday and friday` | `FREQ=WEEKLY;BYDAY=TU,FR` |
| `recur:every weekend` | `FREQ=WEEKLY;BYDAY=SA,SU` |
| `recur:every friday for 5 times` | `FREQ=WEEKLY;BYDAY=FR;COUNT=5` |
| `recur:every tuesday and friday for 7 weeks` | `FREQ=WEEKLY;BYDAY=TU,FR;COUNT=14` |
| `recur:daily until:20260925` | `FREQ=DAILY;UNTIL=20260925` |
| `recur:monthly until:eom` | `FREQ=MONTHLY;UNTIL=<last day of this month>` |
| `recur:yearly until:eoy` | `FREQ=YEARLY;UNTIL=1231` (this year) |
| `recur:FREQ=WEEKLY;BYDAY=TU,FR` | `FREQ=WEEKLY;BYDAY=TU,FR` (passthrough) |

```sh
# a school-class pattern: Tue & Fri, 7 weeks total
calman add "class" start:tomorrow recur:"every tuesday and friday for 7 weeks"

# bills on the last day of each month, forever
calman add "rent" due:eom recur:monthly

# iPhone-created rule round-trips exactly
calman add "pill" due:today recur:"FREQ=DAILY;UNTIL=20260925"
```

## iOS / CalDAV compatibility

- Output is a bare `RRULE` value (no `RRULE:` prefix); the ICS writer adds the
  prefix, so exported `.ics` files are valid RFC 5545.
- Raw passthrough preserves rules produced by iOS Reminders and Outlook
  byte-for-byte, so importing/exporting keeps recurrence intact.
- All-day recurrence (`due:`/`start:` date-only) is stored with `VALUE=DATE`,
  matching how iOS Reminders represents all-day repeating items.

## Series model (Taskwarrior-style)

- A task created with `recur:` gets `status = recurring` (the series master).
- The master carries the `RRULE`; it is a virtual `+PARENT` (filterable,
  e.g. `calman list +PARENT`, `calman list status:recurring`).
- Masters are hidden from `ls`/`list`/`next` by default (reports append
  `-status:recurring`). Pass `+PARENT` to reveal them.
- `done <id>` on a master ⇒ `cancelled` (stops the series). `delete <id>`
  removes it entirely.
- `modify <id>` edits the master record itself (its `.ics`/`.jsonl` file).

## Per-occurrence exceptions (iOS-compatible)

With the optional `recur-expand` Cargo feature (default on), `list`/`next`
expand masters into virtual occurrence rows (id `1.1`, `1.2`, … with the
occurrence's date).
By default only the **nearest** upcoming occurrence per series is shown;
`[defaults] recur_expand_count` controls how many (default `1`, `0` = all
future within the 1-year window):

```toml
[defaults]
recur_expand_count = 1   # nearest only (default)
# recur_expand_count = 0 # all future occurrences
```

Occurrences can be addressed individually — expanded rows carry plain
sequential IDs (Taskwarrior-style), and `id.n` / `on:<date>` remain valid
aliases:

```
calman done 5                     # complete the 5th row (an expanded occurrence) → Completed override record
calman done 5.2                  # complete the 2nd upcoming occurrence of series 5
calman done 5 on:2026-09-02      # complete the occurrence starting that day
calman delete 5.2                # delete one occurrence → EXDATE (skip that instance)
calman modify 5.1 summary:x      # override 1st occurrence → RECURRENCE-ID sibling
calman modify 5 on:2026-09-02 summary:x
```

- `done` on an occurrence writes a **Completed** `RECURRENCE-ID` override
  sibling: the instance stays visible under `+COMPLETED`, the series continues.
- `delete` on an occurrence appends its original `DTSTART` to the master's
  `EXDATE`s — iOS Calendar hides that instance.
- `modify` on an occurrence stores a new sibling component with the **same
  `UID`** as the master plus `RECURRENCE-ID` = the occurrence's original
  `DTSTART` — iOS Calendar shows the overridden fields for that instance.
- Override records are stored with `recurrence_id`/`parent_uid` in JSONL and
  serialised as `RECURRENCE-ID` components in ICS.
- Occurrence addressing requires `recur-expand`; without it, `on:`/`id.n`
  error out.

## Notes

- `recur:` and `repeat:` are exact aliases.
- Empty or unrecognised input (no frequency) is rejected with an error.
- On `modify`, setting `recur:` replaces the previous rule; the rule is stored
  as-is in the item's `rrule` field and serialized to `RRULE:` in ICS.
