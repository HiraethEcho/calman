# iCalendar (RFC 5545) File Format Reference

A practical, accurate reference for implementing an `.ics` writer or reader. All examples are valid `text/calendar` (RFC 5545) content. Unless noted, every content line is terminated by **CRLF** (`\r\n`) — not bare LF — and the file is `UTF-8` encoded with a `BEGIN:VCALENDAR` / `END:VCALENDAR` wrapper.

---

## 1. Overall structure

The root object is `VCALENDAR`. A file may contain one `VCALENDAR` with many components.

**Required properties** on `VCALENDAR`:
- `VERSION:2.0` — MUST be exactly `2.0`.
- `PRODID` — identifies the generating product, conventionally `//<org>/<product>/<lang>`.

**Optional** top-level properties:
- `CALSCALE` — only defined value is `GREGORIAN` (the default; you can usually omit it).
- `METHOD` — used for iTip scheduling (`PUBLISH`, `REQUEST`, `REPLY`, `CANCEL`, …). Omit it for a plain published calendar feed.

**Main components** (each wrapped in its own `BEGIN`/`END`):
- `VEVENT` — a scheduled occurrence (meeting, reminder, all-day item).
- `VTODO` — an action item / task.
- `VJOURNAL`, `VFREEBUSY` — optional, less common.
- `VTIMEZONE` — timezone definitions referenced by `TZID` (see §10).
- `VALARM` — must live *inside* a `VEVENT` or `VTODO` (see §8).

**Required per-component properties:**
- `VEVENT`: `UID`, `DTSTAMP`. (`DTSTART` is strongly recommended.)
- `VTODO`: `UID`, `DTSTAMP`.

### Minimal valid `.ics` — a todo

```text
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Example Corp//CalMan 1.0//EN
BEGIN:VTODO
UID:todo-0001@example.com
DTSTAMP:20260828T000000Z
DTSTART:20260828T090000Z
DUE:20260828T170000Z
SUMMARY:Write the quarterly report
STATUS:NEEDS-ACTION
END:VTODO
END:VCALENDAR
```

### Minimal valid `.ics` — an event

```text
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//Example Corp//CalMan 1.0//EN
BEGIN:VEVENT
UID:event-0001@example.com
DTSTAMP:20260828T000000Z
DTSTART:20260828T100000Z
DTEND:20260828T110000Z
SUMMARY:Team standup
END:VEVENT
END:VCALENDAR
```

> **Rule:** `UID` + `DTSTAMP` are mandatory on every `VEVENT`/`VTODO`. `UID` MUST be globally unique and stable across edits of the same item (changing `UID` creates a *new* item). `DTSTAMP` is the moment the calendar representation was created/last revised.

---

## 2. Content line format

Every property is one logical line:

```text
[group.]NAME[;PARAM=VALUE[;PARAM=VALUE...]]:VALUE<CRLF>
```

- **NAME** is uppercase `A-Z0-9-` (case-insensitive on parse, upper on write).
- **Parameters** are `;KEY=VALUE`, order-independent.
- A **property group** is an optional prefix before a dot (`group.PROPERTY`):

  ```text
  X-PROJ.SUMMARY:Launch work        ; group "X-PROJ" groups related props
  X-PROJ.UID:proj-7
  ```

### TEXT value escaping

Inside `TEXT`-typed values (`SUMMARY`, `DESCRIPTION`, `LOCATION`, `CATEGORIES`, …) these must be backslash-escaped:

| Literal | Must be written as |
|---------|--------------------|
| `\` (backslash) | `\\` |
| `;` (semicolon) | `\;` |
| `,` (comma)      | `\,` |
| newline          | `\N` or `\n` |

Examples:

```text
SUMMARY:Project\; Phase 1
LOCATION:Room A\, Building B
DESCRIPTION:Line one\NLine two (a real line break inside one property)
DESCRIPTION:Path C:\\Program Files\\App
```

### Parameter value quoting

If a parameter *value* contains `:`, `;`, `,`, a double quote, or a leading space, it MUST be wrapped in double quotes:

```text
ATTENDEE;CN="Doe, John";ROLE=REQ-PARTICIPANT:mailto:john@example.com
```

### Line folding (75-octet rule)

A content line **SHOULD NOT exceed 75 octets** (bytes), excluding the terminating CRLF. To fold, split at an octet boundary, insert `CRLF`, then a **single space (or tab)** on the continuation line. Unfolding = delete the CRLF **and** that one leading space/tab.

```text
DESCRIPTION:This is a very long description that will certainly exceed the seven
 ty-five octet limit and therefore must be folded onto a continuation line.
```

Unfolds to: `This is a very long description that will certainly exceed the seventy-five octet limit and therefore must be folded onto a continuation line.`

> **UTF-8 caution:** "75 octets" means **bytes**, not characters. Never split a multi-byte UTF-8 sequence across the fold boundary — fold only at a complete-character boundary.

---

## 3. Date/time properties

Key date/time properties:
- `DTSTART` — start (event) or start (todo).
- `DTEND` — event end (**exclusive**; see below).
- `DUE` — todo deadline.
- `COMPLETED` — when a todo was completed (use with `STATUS:COMPLETED`).
- `CREATED` — item creation time.
- `LAST-MODIFIED` — last edit time.
- `DTSTAMP` — calendar representation timestamp (required on every component).

### The three value forms

**(a) Date-only (all-day)** — uses `VALUE=DATE`:

```text
DTSTART;VALUE=DATE:20260828
DTEND;VALUE=DATE:20260829
```

For an all-day event on a single day, `DTEND` is the **day after** the last day (it is exclusive). An all-day item on Aug 28 alone ⇒ `DTSTART;VALUE=DATE:20260828` + `DTEND;VALUE=DATE:20260829`.

**(b) Floating local time** (no `Z`, no `TZID`):

```text
DTSTART:20260828T100000
```

Means "10:00 in whatever timezone the viewer is in." No absolute instant.

**(c) UTC** (trailing `Z`):

```text
DTSTART:20260828T100000Z
```

An absolute instant, unambiguous everywhere.

**(d) Named timezone** (`TZID` parameter):

```text
DTSTART;TZID=Asia/Shanghai:20260828T100000
```

Wall-clock time in a specific IANA zone (and its DST rules apply if the zone has them).

### When to use which

| Form | Use when | Portability notes |
|------|----------|-------------------|
| `VALUE=DATE` | All-day items: birthdays, holidays, day-long tasks | Safest; no tz ambiguity. |
| Floating `T…` | Time tied to the *viewer's* local clock (e.g., "daily 9am alarm", "lunch at noon") | Adapts to viewer tz but wrong if the item crosses zones. |
| UTC `…Z` | Machine sync, precise global scheduling, recurring sync engines | Unambiguous; phones convert to local. Best for sync. |
| `TZID=…` | A meeting at a specific place's wall-clock time (e.g., "10am Shanghai") | Most human-friendly; phones render correctly and handle DST. |

**iPhone / cross-device portability:** Apple Calendar, Google Calendar, and Outlook all handle `TZID=` referencing an IANA zone *and* embedded `VTIMEZONE`. UTC is the safest for sync correctness. **Avoid floating times** for anything that must fire at a specific absolute moment across timezones — they can shift by the viewer's offset.

---

## 4. DURATION

Format: `P` `[n]W`  **or**  `P [n]D [T [n]H [n]M [n]S]`, with an optional **leading/trailing sign**.

```text
DURATION:P1W          ; 1 week
DURATION:P1D          ; 1 day
DURATION:PT1H         ; 1 hour
DURATION:PT30M        ; 30 minutes
DURATION:P1DT2H30M    ; 1 day, 2 hours, 30 minutes
DURATION:+P1H         ; explicit positive (same as P1H)
DURATION:-PT30M       ; negative: 30 minutes before the reference
```

**Rules:**
- A leading `-` means *before*; `+` (or none) means *after*. This is primarily how `VALARM` `TRIGGER` values work (§8).
- For `VEVENT`, **exactly one** of `DTEND` or `DURATION` may appear. If both are present, `DURATION` is the authoritative one (and `DTEND` is ignored).
- `DTEND` (when used) is **exclusive**: the event ends at that instant.
- Equivalent: `DTEND = DTSTART + DURATION`.

**Why DURATION beats DTEND for recurring events:** With `DURATION`, every generated instance ends at `instance_start + DURATION`, so the event length is preserved consistently across Daylight-Saving transitions (a 1-hour meeting stays 1 hour even when clocks jump). `DTEND` is a fixed absolute instant and does not track the per-instance start the same way.

---

## 5. Recurrence

### `RRULE`

`RRULE` defines the recurrence set. Syntax:

```text
RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,TH;UNTIL=20261231T000000Z
```

**Parts:**

| Part | Meaning | Notes |
|------|---------|-------|
| `FREQ` | `SECONDLY`/`MINUTELY`/`HOURLY`/`DAILY`/`WEEKLY`/`MONTHLY`/`YEARLY` | **Required.** |
| `INTERVAL` | repeat every Nth `FREQ` unit | Default `1`. |
| `COUNT` | total number of occurrences | Positive int. Mutually exclusive with `UNTIL`. |
| `UNTIL` | hard stop | `DATE` or `DATE-TIME` **with `Z`** (UTC). Never floating. |
| `BYDAY` | weekdays `SU MO TU WE TH FR SA`, optionally ordinal | `1MO`, `2WE`, `-1SU` (last Sunday), `-2FR`. |
| `BYMONTHDAY` | days of month | `1`, `15`, `-1` (last day). |
| `BYMONTH` | months | `1..12`. |
| `BYSETPOS` | pick the Nth result | `1`..`366` or `-1`..`-366`; used *with* another `BYxxx`. |
| `BYWEEKNO`, `BYYEARDAY`, `BYHOUR`, `BYMINUTE`, `BYSECOND`, `WKST` | finer control | `WKST` defaults to `MO`. |

- Multiple `BYxxx` parts combine with **AND**. Multiple values *within* one part (e.g., `BYDAY=TU,FR`) combine with **OR**.
- `COUNT` and `UNTIL` are mutually exclusive; use one or neither (open-ended).
- `UNTIL` for `FREQ=DAILY` style is usually a `DATE-TIME` with `Z`; for all-day series you may use a bare `DATE`.

### `RECURRENCE-ID` — overriding a single instance

To change one occurrence, emit a *separate* component with the **same `UID`** as the series, plus `RECURRENCE-ID` set to the original instance's start time (same format/zone as the series `DTSTART`).

```text
BEGIN:VEVENT
UID:series-standup@example.com
DTSTAMP:20260201T000000Z
RECURRENCE-ID:20260202T090000Z      ; the specific instance being changed
DTSTART:20260202T100000Z            ; new start for just this instance
SUMMARY:Standup moved to 10:00
END:VEVENT
```

> Bump `SEQUENCE` on the override when it represents a newer revision.

### `EXDATE` — exclude instances

List occurrence start-times to suppress (format must match the series `DTSTART`; if `TZID` is used, `EXDATE` needs the same `TZID`):

```text
EXDATE:20260112T090000Z,20260119T090000Z
```

### `RDATE` — add explicit extra occurrences

```text
RDATE:20260115T140000Z,20260120T140000Z
```

### Full recurrence examples

**Daily, exactly 5 times:**
```text
DTSTART;VALUE=DATE:20260828
RRULE:FREQ=DAILY;COUNT=5
```

**Weekly on Tue & Fri until a date:**
```text
DTSTART:20260901T090000Z
RRULE:FREQ=WEEKLY;BYDAY=TU,FR;UNTIL=20261001T000000Z
```

**Monthly on the 1st:**
```text
DTSTART;VALUE=DATE:20260901
RRULE:FREQ=MONTHLY;BYMONTHDAY=1
```

**Yearly (every year on the start date):**
```text
DTSTART;VALUE=DATE:20261125
RRULE:FREQ=YEARLY
```

**Monthly on the last Sunday (BYSETPOS + BYDAY):**
```text
DTSTART;VALUE=DATE:20260928
RRULE:FREQ=MONTHLY;BYDAY=SU;BYSETPOS=-1
```

---

## 6. STATUS

Status values are component-specific. Unknown/invalid values MUST be ignored (treat as if the property were absent / fall back to the default for that component).

| Component | Valid values | Default / meaning |
|-----------|--------------|-------------------|
| `VEVENT` | `TENTATIVE`, `CONFIRMED`, `CANCELLED` | `CONFIRMED` implied if absent |
| `VTODO` | `NEEDS-ACTION`, `IN-PROCESS`, `COMPLETED`, `CANCELLED` | `NEEDS-ACTION` |
| `VJOURNAL` | `DRAFT`, `FINAL`, `CANCELLED` | `DRAFT` |

```text
BEGIN:VEVENT
UID:e1@example.com
DTSTAMP:20260828T000000Z
DTSTART:20260828T100000Z
SUMMARY:Interview
STATUS:CONFIRMED
END:VEVENT

BEGIN:VTODO
UID:t1@example.com
DTSTAMP:20260828T000000Z
SUMMARY:Submit expense
STATUS:IN-PROCESS
END:VTODO
```

> If you write `STATUS:BOGUS`, a conformant reader ignores it (it does not crash or invent a status).

---

## 7. RELATED-TO (dependencies / links)

`RELATED-TO` links one component to another by the **other component's `UID`**. `RELTYPE` defaults to `PARENT`.

```text
BEGIN:VTODO
UID:child-task@example.com
DTSTAMP:20260828T000000Z
SUMMARY:Write appendix
RELATED-TO;RELTYPE=CHILD:parent-project@example.com
END:VTODO

BEGIN:VTODO
UID:parent-project@example.com
DTSTAMP:20260828T000000Z
SUMMARY:Publish research paper
RELATED-TO;RELTYPE=PARENT:grandproject@example.com
END:VTODO
```

- `RELTYPE=PARENT` → this item is a child of the referenced UID.
- `RELTYPE=CHILD` → this item is a parent of the referenced UID.
- `RELTYPE=SIBLING` → same-level relation.
- The value is always a `UID`, never a human string. Build dependency graphs by matching `UID`s.

---

## 8. VALARM

A `VALARM` MUST be nested inside a `VEVENT` or `VTODO`. Required fields depend on `ACTION`:

- `ACTION` — `DISPLAY`, `AUDIO`, or `EMAIL` (**required**).
- `TRIGGER` — when to fire, relative (default) or absolute (**required**).
- `DISPLAY` additionally requires `DESCRIPTION`.
- `EMAIL` additionally requires `DESCRIPTION`, `SUMMARY`, and at least one `ATTENDEE`.
- `AUDIO` may carry an `ATTACH` (sound URI); otherwise a default system sound is used.

`TRIGGER` is a `DURATION` (§4). Negative = **before** the reference; `REL=START` (default) anchors to `DTSTART`/`DUE`, `REL=END` anchors to `DTEND`/`DUE`/completion.

```text
BEGIN:VEVENT
UID:meeting@example.com
DTSTAMP:20260828T000000Z
DTSTART:20260828T100000Z
DTEND:20260828T110000Z
SUMMARY:Review
BEGIN:VALARM
ACTION:DISPLAY
TRIGGER:-PT15M                 ; 15 minutes before DTSTART
DESCRIPTION:Review meeting starts in 15 minutes
END:VALARM
BEGIN:VALARM
ACTION:DISPLAY
TRIGGER;REL=END:P15M           ; 15 minutes after DTEND
DESCRIPTION:Follow-up time
END:VALARM
END:VEVENT
```

Absolute trigger (rare) uses `VALUE=DATE-TIME`:

```text
BEGIN:VALARM
ACTION:DISPLAY
TRIGGER;VALUE=DATE-TIME:20260828T093000Z
DESCRIPTION:Fixed-time reminder
END:VALARM
```

`EMAIL` alarm sketch:

```text
BEGIN:VALARM
ACTION:EMAIL
TRIGGER:-PT1H
DESCRIPTION:Reminder: standup in 1 hour
SUMMARY:Standup reminder
ATTENDEE:mailto:team@example.com
END:VALARM
```

---

## 9. Other practical properties

| Property | Type | Notes / example |
|----------|------|-----------------|
| `SUMMARY` | TEXT | Short title. `SUMMARY:Team sync` |
| `DESCRIPTION` | TEXT | Long text; escape `\,` `\;` `\\` and use `\N`/`\n` for newlines. |
| `LOCATION` | TEXT | `LOCATION:Room 4B\, HQ` |
| `GEO` | `lat;lon` | `GEO:37.386013;-122.082932` (floats, semicolon-separated). |
| `CATEGORIES` | TEXT list | `CATEGORIES:WORK,FINANCE` (comma list, each escaped if needed). |
| `PRIORITY` | integer 0–9 | `1` = highest, `9` = lowest, `0` = undefined. For `VTODO`. |
| `UID` | text | Globally unique & stable. `UID:20260828T100000Z-abc@host`. |
| `SEQUENCE` | integer | Revision counter; increment on each meaningful update (default `0`). |
| `URL` | URI | `URL:https://example.com/task/123` |
| `ATTENDEE` | URI (mailto:/https:) | `ATTENDEE;CN=Jane;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION:mailto:jane@example.com` |

`ATTENDEE` common parameters: `CN` (display name), `ROLE` (`CHAIR`/`REQ-PARTICIPANT`/`OPT-PARTICIPANT`/`NON-PARTICIPANT`), `PARTSTAT` (`NEEDS-ACTION`/`ACCEPTED`/`DECLINED`/…), `RSVP` (`TRUE`/`FALSE`), `CUTYPE` (`INDIVIDUAL`/`GROUP`/`RESOURCE`/…).

A richer `VEVENT` example:

```text
BEGIN:VEVENT
UID:rich-event@example.com
DTSTAMP:20260828T000000Z
DTSTART;TZID=Europe/Paris:20260901T140000
DTEND;TZID=Europe/Paris:20260901T150000
SUMMARY:Quarterly planning
DESCRIPTION:Agenda attached.\NBring Q2 numbers.
LOCATION:Room 4B\, HQ
GEO:48.8566;2.3522
CATEGORIES:WORK,PLANNING
PRIORITY:3
URL:https://example.com/meet/123
ATTENDEE;CN=Jane Doe;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED:mailto:jane@example.com
STATUS:CONFIRMED
SEQUENCE:2
END:VEVENT
```

---

## 10. Timezones (`VTIMEZONE`)

A `VTIMEZONE` component defines a `TZID` you can then reference via `DTSTART;TZID=…`. It contains `STANDARD` and/or `DAYLIGHT` sub-components.

**Required fields per `STANDARD`/`DAYLIGHT`:**
- `TZOFFSETFROM` — offset *before* the transition (e.g., `-0500`).
- `TZOFFSETTO` — offset *after* the transition (e.g., `-0400`).
- `DTSTART` — when the rule takes effect (local time, no `Z`).
- `RRULE` (or `RDATE`) — when the transition repeats (e.g., last Sunday of March).

Optional: `TZNAME` (`EST`/`EDT`), `COMMENT`.

### Example A — zone with DST (US Eastern)

```text
BEGIN:VTIMEZONE
TZID:America/New_York
BEGIN:DAYLIGHT
DTSTART:19700308T020000
TZOFFSETFROM:-0500
TZOFFSETTO:-0400
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU
TZNAME:EDT
END:DAYLIGHT
BEGIN:STANDARD
DTSTART:19701101T020000
TZOFFSETFROM:-0400
TZOFFSETTO:-0500
RRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=-1SU
TZNAME:EST
END:STANDARD
END:VTIMEZONE
```

### Example B — zone without DST (China)

```text
BEGIN:VTIMEZONE
TZID:Asia/Shanghai
BEGIN:STANDARD
DTSTART:19910101T000000
TZOFFSETFROM:+0800
TZOFFSETTO:+0800
TZNAME:CST
END:STANDARD
END:VTIMEZONE
```

### When you can skip embedding `VTIMEZONE`

- **Referencing a known IANA zone:** `DTSTART;TZID=America/New_York:...` works on modern clients (Apple Calendar, Google Calendar, Outlook, iOS, Android) because they resolve the IANA name against the system tz database. In that case you may *omit* the `VTIMEZONE` block.
- **Embed it when:** you target strict/legacy parsers, calendar files shared by email, or environments without a system tz database. Embedding makes the file fully self-contained.
- **Never do:** invent a `TZID` that is neither an IANA name nor defined by an embedded `VTIMEZONE` — readers will mis-handle it.
- **UTC and floating times never need `VTIMEZONE`.**

---

## Quick writer checklist

1. Wrap everything in `BEGIN:VCALENDAR` / `END:VCALENDAR`; set `VERSION:2.0` and `PRODID`.
2. Every `VEVENT`/`VTODO` needs `UID` + `DTSTAMP`.
3. Terminate all lines with **CRLF**; fold lines >75 **bytes** with `CRLF` + single space (never split a UTF-8 char).
4. Escape `\,` `\;` `\\` and use `\N`/`\n` inside TEXT.
5. Pick one date form deliberately (UTC for sync, `TZID` for human events, `VALUE=DATE` for all-day).
6. For events, use **either** `DTEND` **or** `DURATION`, not both; prefer `DURATION` for recurring series.
7. `COUNT` XOR `UNTIL` in `RRULE`; `UNTIL` must be UTC (`Z`).
8. Override a recurrence instance with same `UID` + `RECURRENCE-ID`.
9. Quote parameter values containing `:`, `;`, `,`, `"`, or leading space.
10. `STATUS` values are component-specific; ignore unknown ones rather than failing.
