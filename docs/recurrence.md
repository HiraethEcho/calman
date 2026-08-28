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

### 2. Friendly grammar

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

## Notes

- `recur:` and `repeat:` are exact aliases.
- Empty or unrecognised input (no frequency) is rejected with an error.
- On `modify`, setting `recur:` replaces the previous rule; the rule is stored
  as-is in the item's `rrule` field and serialized to `RRULE:` in ICS.
