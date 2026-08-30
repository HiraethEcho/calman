# Tasks & events

calman stores a single unified record. Whether it is a **todo** or an **event**
depends on which date attribute you supply:

- **Todo** — `due:` present, `start:` absent.
- **Event** — `start:` present (with optional `end:` or `duration:`).

Giving **both** `due:` and `start:` is an error.

## Adding a todo

```sh
calman add "buy milk" due:tomorrow pri:H +home +errand
calman add "file taxes" due:2026-04-15 desc:"federal + state"
```

A date-only `due:` (e.g. `due:tomorrow`, `due:20260826`) is stored as an
**all-day** todo (`DUE;VALUE=DATE` in ICS).

## Adding an event

```sh
calman add "standup" start:tomorrow recur:daily
calman add "lunch" start:T1200 duration:45min +team
calman add "demo" start:2026-09-01 end:2026-09-01 15:00 location:"Zoom"
calman add "conference" start:2026-10-12 allday
```

- `start:` with a date-only value → all-day event.
- If neither `end:` nor `duration:` is given, a timed event defaults to
  `[date].default_event_duration`; empty (default) ⇒ instant event (only
  `DTSTART`, no `DTEND`); an all-day event with no end spans a single day.
- `allday` (or `+allday`) forces all-day even with a timed `start:`.

## Attributes

| Attribute | Kind | Effect |
| :-------- | :--- | :----- |
| `due:<date>` | todo | Deadline. Date-only → all-day todo. |
| `start:<date>` | event | Start time. Date-only → all-day event. |
| `end:<date>` | event | End time/date (alternative to `duration:`). All-day `end` is the **last included day** (stored as day-after). |
| `duration:<dur>` | event | Length, e.g. `45min`, `1h`, `1h30m`, `2d`, or ISO 8601 `PT15M`/`P7D`. Alternative to `end:`. |
| `allday` / `+allday` | event | Force all-day (drops times & `DTEND`). |
| `pri:H\|M\|L` (or `pri:<0-9>`) | both | Priority — `H`=9, `M`=5, `L`=1, or 0–9. |
| `+tag` / `-tag` | both | Tags (`-tag` only removes on `modify`). |
| `source:<name>` / `src:<name>` | add (single) / list / count / sync | Target or selection. `ics-dir` uses `name/collection`. |
| `rel:<id>` | both | Parent relation — written as `RELATED-TO;RELTYPE=PARENT` (child stores parent UID). |
| `recur:<rule>` / `repeat:<rule>` | both | Recurrence, normalised to RFC 5545 `RRULE` (see [Recurrence](recurrence.md)). |
| `location:<text>` | event | `LOCATION` property. |
| `alert:<lead>` | both | `VALARM` lead time before start/due, e.g. `alert:15min` → `TRIGGER:-PT900S`, or ISO `alert:PT15M`. |
| `wait:<expr>` | both | Taskwarrior-style wait (hidden from reports until then). Date form (`wait:2026-09-01`) or relative offset (`wait:-1d`, `wait:PT12H`) against the task date; per-occurrence for recurring series. |
| `desc:<text>` | both | `DESCRIPTION` (separate from `summary`/`SUMMARY`). |
| `status:<x>` | modify | `pending` / `in-progress` / `completed` / `cancelled` / `recurring`. |

Example combining several:

```sh
calman add "1:1 with boss" start:2026-08-29 14:30 duration:30min \
  pri:H +1on1 location:"Office" alert:10min recur:weekly
```

## Relations (subtasks)

`rel:<id>` links an item to a parent. On `add` or `modify`, calman resolves the
ID (short ID or UID) to the parent's UID and stores it as
`RELATED-TO;RELTYPE=PARENT`:

```sh
calman add "write spec"                       # → ID 1
calman add "draft intro" rel:1                # child of 1
calman 1 modify rel:                         # (clearing not supported; set a new parent)
```

The report color engine exposes `blocked` (a parent todo whose UID is referenced
by another item) and `blocking` (a todo that has a `related_to`) rules — see
[Reports](reports.md).

## Recurrence

Recurrence uses `recur:` (alias `repeat:`) and is normalised to a standard
RFC 5545 `RRULE` (iOS / Outlook / CalDAV compatible). See the dedicated
[Recurrence](recurrence.md) page for the full grammar and examples.

## Storage on disk

- **`jsonl`** source → `<location>/tasks.jsonl` (one JSON object per line),
  plus `.calman-state.json` metadata.
- **`ics`** source → `<location>/<UID>.ics` (one `VEVENT`/`VTODO` per file).
- **`ics-dir`** source → a Radicale/vdirsync directory; collections are
  auto-discovered and referenced as `source:<name>/<collection>`:

```sh
calman add "meet" start:tomorrow source:remote/sorge     # one collection
calman list source:remote                                # expands all collections
```
