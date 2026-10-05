# calman — Detailed Functional Design

This document serves as the **complete manual page (man page)** for calman. It describes the configuration format, data model, CLI command syntax, TUI interaction logic, and all internal mechanisms in detail. A third‑party developer could re‑implement a functionally identical application based solely on this document.

---

## 1. Configuration File Design (`~/.config/calman/config.toml`)

### 1.1 Complete Structure
```toml
[defaults]
write_source = "work"           # Default target for `add` (single string)

[contexts]
cli = ["work", "personal"]      # Sources for `list`, `count`, `done`, etc.
sync = ["work"]                 # Sources for `sync` by default
tui = ["work", "personal"]      # Sources shown when `tui` starts

[date]
day_start = "00:00:00"         # sod/sow/som/…
day_end = "23:59:59"           # eod/eow/eom/…（含 eoww）
default_event_duration = ""     # empty ⇒ instant event (DTSTART only)

[tui]
default_filter = "all"          # "todo" | "event" | "all"
vim_keys = true                 # true = Vim style (j/k/gg/G), false = arrow keys

[[source]]
name = "work"                   # Unique name
type = "ics"                    # "jsonl" or "ics"
location = "~/.local/share/calman/work/"

[source.sync]                   # Sync configuration (optional)
pre_hook = "git pull --rebase origin main"
cmd = "git add . && git commit -m 'calman sync' && git push"
post_hook = "echo 'done'"
```

### 1.2 Field Details
- **`include`** (list of strings): extra TOML files merged before the main one, for modular configs (`report.toml`, `colorscheme.toml` …).
  - Paths relative to the main config file's directory; absolute and `~/`-expanded paths allowed.
  - Merge: tables recurse; scalars/lists in the main file win; keys missing from the main file are taken from the include.
  - One level only — included files cannot contain `include`.
  - Ship‑with examples: `config.example.toml`, `report.example.toml`, `colorscheme.example.toml`.
- **`defaults.write_source`**: Must be a single valid `source.name`.
- **`contexts`**:
  - If a context is not defined, it falls back to **all `[[source]]` entries** (for `sync`, entries without `sync.cmd` are skipped).
- **`source.sync`**:
  - If `cmd` is absent, the source is considered “non‑syncable” and will be ignored in the `sync` fallback.
  - Placeholders `{location}` and `{name}` are supported and replaced with actual values.
  - **The working directory** for command execution is set to `source.location`.

---

## 2. Data Model and Storage

### 2.1 Unified Data Model (Internal Representation)
```rust
pub struct Task {
    // --- Identity and ownership ---
    pub uid: String,                // UUID v4
    pub source: String,             // Corresponds to source.name

    // --- Core fields ---
    pub summary: String,
    pub description: Option<String>,
    pub status: TaskStatus,         // Pending, InProgress, Completed, Cancelled
    pub priority: Option<u8>,       // 0‑9, 0 lowest
    pub tags: Vec<String>,

    // --- VTODO (tasks) ---
    pub due: Option<DateTime<Utc>>,
    pub percent_complete: Option<u8>,
    pub completed_at: Option<DateTime<Utc>>,

    // --- VEVENT (events) ---
    pub dtstart: Option<DateTime<Utc>>,
    pub dtend: Option<DateTime<Utc>>,
    pub rrule: Option<String>,      // e.g., "FREQ=WEEKLY;COUNT=10"
    pub location: Option<String>,
    pub allday: bool,               // true → `VALUE=DATE` in .ics, date-only
    pub alarm_before: Option<i64>,  // seconds before start (VALARM TRIGGER)

    // --- Relations ---
    pub related_to: Option<String>, // parent task UID

    // --- Timestamps ---
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum TaskStatus { Pending, InProgress, Completed, Cancelled }
```
**Type determination**:
- If `dtstart` is not `None`, the item is treated as an **Event**.
- Otherwise, it is treated as a **Todo**.

### 2.2 Storage Format Details
#### A. `type = "jsonl"` (recommended)
- **File**: `<location>/tasks.jsonl`.
- **Format**: One JSON object per line (containing all `Task` fields).
- **Reading**: Streamed line by line, building an index (UID → byte offset) for fast random writes.
- **Writing**: On modification, **write entirely to a temporary file** `tasks.jsonl.tmp`, then `rename` over the original (atomic).
- **Append**: `add` simply appends a new line, no full rewrite needed (unless modifications are made).

#### B. `type = "ics"` (compatibility mode)
- **Directory**: `<location>/`.
- **Files**: One `<UID>.ics` file per task.
- **Content**: Standard iCalendar format, with `BEGIN:VCALENDAR` ... `END:VCALENDAR`.
- **Writing**: Also uses “write temporary, then rename”.

### 2.3 Metadata File
- **File**: `<location>/.calman-state.json`
- **Applies to**: `jsonl` and `ics` source locations. An `ics‑dir` root holds collections, not items, so no metadata file is written there.
- **Content**:
    ```json
    {
      "version": 1,
      "uid_counter": 1234,
      "last_modified": "2026-08-24T10:00:00Z"
    }
    ```

> UIDs are random v4 UUIDs — `uid_counter` is a monotonic write counter (incremented on every `touch()`), never read to generate a UID.

---

## 3. Synchronisation Mechanism (Sync)

### 3.1 Execution Flow

An `ics‑dir` source is **not** expanded into its collections. It syncs once at
its root, and `{location}` is that root. To sync one collection instead, name it
explicitly with `source:<name>/<collection>`.

1.  Resolve the target source list (`source:` attribute takes precedence; otherwise use `contexts.sync`; fallback to all sync‑enabled sources).
2.  For each source:
    a. Check if `location/.sync.lock` exists; if yes, skip and warn.
    b. Create `.sync.lock`.
    c. Execute sequentially: `pre_hook` → `cmd` → `post_hook`.
    d. If `pre_hook` or `cmd` exits with a non‑zero code, **terminate immediately**, delete the lock file, and report error.
    e. Delete the lock file.
    f. Update `last_modified` in `.calman-state.json`. An `ics‑dir` root is skipped: it holds collections, not items, so no metadata file is written there.

### 3.2 Conflict Resolution Strategy
- **calman does not resolve conflicts**.
- Users must handle conflicts in their custom commands (e.g., `git pull --rebase`, or `pimsync`’s conflict markers).
- If an external command corrupts the data file, calman will refuse to read it and prompt the user to fix it manually.

---

## 4. CLI Mode (Command‑Line Interface)

### 4.1 Global Options
```bash
calman [source:<source1,source2,...>] <COMMAND> [ARGS]
```
- `source:` accepts a **comma‑separated list** (e.g., `source:work,personal`) and can appear anywhere in the argument list.
- **Precedence**: `source:` override > `[contexts].{context}` > all sources (sync skips non‑syncable).

### 4.2 Subcommand Specifications

The CLI is **Taskwarrior‑style free‑form**: filters/attributes can appear before or
after the command, the command is optional, and the default report is `next`.

```bash
calman                          # ≡ calman next
calman +PENDING                 # list, filtered
calman +OVERDUE list            # command may follow filters
calman add <TEXT> [ATTRS...]
calman <ID>... done             # or: calman done <ID>...
calman modify <TEXT> [ATTRS...] # bare text replaces the summary
```

#### A. `add` — Add a task/event
- **Syntax**: `calman add <SUMMARY> [ATTRS...]`
- **Write target**: If a `source:` attribute is given (must be a single source), it overrides `defaults.write_source`. Providing multiple sources yields an error.
- **Attributes** (Taskwarrior style):
    - `priority:H|M|L` (9/5/1) or numeric 0‑9; alias `pri:`
    - `due:<DATE>`: natural‑language date (Todo)
    - `+<tag>`: add a tag (repeatable)
    - `desc:<TEXT>`: long description
    - Event fields (`from:` present → event, absent → todo):
        - `from:<DATE>` / `to:<DATE>` — forms below; `to` is the end (longer than `from`)
        - `for:<DUR>` — `45min`, `1h`, `1h30m`, `2d`; alternative to `to:`
        - `allday` — force all‑day (date‑only); date‑only `from:` auto‑all‑day
        - `alert:<DUR>` — VALARM lead time, e.g. `alert:15min` → `TRIGGER:-PT900S`
        - `location:<TEXT>`, `recur:<RULE>` — recurrence; alias `repeat:`
    - Date‑only `due:` (Todo) is an all‑day todo: stored as `DUE;VALUE=DATE` and
      `task.allday = true`; all-day overdue is fixed (day after the due day).
    - `recur:`/`repeat:` is normalized to a standard RFC 5545 `RRULE` by
      `recurrence.rs`: raw `FREQ=…` passthrough (always), `text2rrule` natural
      language (`every tuesday`) under `date-natural`, or the built-in friendly
      grammar (`daily/weekly/monthly/yearly`, `every 7d`/`2 weeks` (→`INTERVAL`),
      `every tue and fri`/`weekend` (→`BYDAY`), `for 5 times`/`for 7 weeks`
      (weeks×weekday→`COUNT`), `count:N`, `until:<date>`/`until:eoy`/`until:eom`).
    - A task with `rrule` is the series **master**: `status = recurring`, virtual
      tag `+PARENT`, hidden from `ls`/`list`/`next` by default; `done` on master
      ⇒ `cancelled`; `modify`/`delete` target the master record.
    - Optional `recur-expand` feature: `list`/`next` expand masters into virtual
      occurrence rows (plain sequential IDs); occurrences addressed via
      `on:<date>` or `<id>.<n>`. `done` on an occurrence ⇒ Completed
      `RECURRENCE-ID` override sibling; `delete` on an occurrence ⇒ `EXDATE`;
      `modify` on an occurrence ⇒ sibling component with same `UID` +
      `RECURRENCE-ID` (iOS Calendar compatible).
    - No `to:`/`for:` → `[date] default_event_duration`; empty (default)
      ⇒ instant event (no `DTEND`); all‑day with no end → single day (no `DTEND`).

**Date input forms** (start/end/due): `20260812` (all‑day), `20260812T090000`, `0826T0900` (this year, trailing‑fill), `T0900` (today), `0826` (this year), `25` (this month, trailing‑fill), `YYYY-MM-DD [HH:MM]`, `HH:MM` (today), named dates, `+3d` / `-2w`.

#### B. `list` — Long list report
- **Syntax**: `calman [FILTERS...] [list]`
- **Data sources**: Uses `contexts.cli` or `source:`-specified sources.
- **Output**: Table with a dynamic short ID per row (numbered across all selected sources). No `--format` flag.
- **Filters**: shared expression grammar with `[report]` `filter` strings.
    - `and` / `or` / `(` / `)`; implicit `and` between adjacent atoms.
    - `status:<status>`: `pending`, `in‑progress`, `completed`, `cancelled`, `recurring`, `active`
    - `type:<type>`: `todo` | `event` | `all`; `+todo`/`+event` aliases
    - `source:<name>`; `-source:<name>` excludes a source
    - `priority:<level>`: `high`/`h`/`9`, `medium`/`m`/`5`, `low`/`l`/`1`, or 0‑9
    - `due:<date>`: exact calendar day; `due.before:<date>`: strictly before;
      `due.by:<date>`: on or before; `due.after:<date>`: on or after
    - todos compare `due`, events compare `dtstart`
    - `+<tag>`: contains tag; `-<tag>`: excludes tag
    - `+OVERDUE`: overdue and not completed
    - `+PENDING`: active tasks, events count as pending
    - `+COMPLETED` / `+CANCELLED` / `+IN-PROCESS` / `+TAGGED` / `+UNTAGGED` / `+SCHEDULED`

Precedence: adjacent atoms are `and`; `and` binds tighter than `or`.
`A B or C D` = `(A and B) or (C and D)`; use parentheses to group.
`type:event` / `type:todo` select a single kind; builtin report filters still
apply on top (future events only) unless overridden with `rc.report.*.filter=`.

CLI custom report overrides (Taskwarrior rc style, script-friendly):

```bash
calman rc.report.next.columns=id,date,summary rc.report.next.labels=ID,DATE,TASK next
```

#### C. `done` — Complete a task
- **Syntax**: `calman done <ID> [ID...]` (or `calman <ID>... done`)
- **Scope**: short IDs are resolved against the merged list from current `source:` override or `contexts.cli`; UIDs also accepted.
- **Behaviour**: Sets `status` to `Completed`, records `completed_at`.

#### D. `delete` — Delete a task
- **Syntax**: `calman delete <ID> [ID...]`
- **Behaviour**: Permanently removes from storage.

#### E. `modify` — Modify a task
- **Syntax**: `calman <ID>... modify <TEXT> [ATTRS...]`
- **Example**: `calman 1 modify new content pri:L -bar due:20260824`
- **Bare words replace the summary** (Taskwarrior semantics). `+<tag>` adds, `-<tag>` removes.
- **Attributes**: `summary` via bare text; `desc:`, `due:`, `priority:`/`pri:`, `status:`, `from:`, `to:`, `location:`, `repeat:`, `for:`, `alert:`.
- `+allday` → convert to all‑day, drops times/DTEND. Any `from:` → converts to timed (non‑allday); a date‑only `from:` makes the event all‑day (no implicit hour). No `-allday`.

#### F. `count` — Count tasks
- **Syntax**: `calman count [FILTERS...]`
- **Output**: Plain number (no extra text).
- **Use**: Scripting, e.g., `alert $(calman count +OVERDUE) overdue tasks.`

#### G. `sync` — Run synchronisation
- **Syntax**: `calman sync` (`source:` attribute may precede it)
- **Default sources**: `contexts.sync`. If not set or empty, sync all sources that have a `cmd` defined.
- **Output**: Live stdout/stderr from external commands.

#### H. `tui` — Launch the terminal UI
- **Syntax**: `calman tui` (`source:` attribute may precede it)
- **Default sources**: `contexts.tui`. If not set or empty, show all sources.

---

## 5. TUI Mode (Terminal User Interface)

### 5.1 Layout Specification
- **Left‑right ratio**: List 40%, Detail 60%.
- **Top status bar**: Shows active source list, sync status, and shortcut hints.
- **Bottom status bar**: Shows current mode (List/Detail/Edit) and context‑sensitive keys.

### 5.2 Interaction Modes (State Machine)

#### A. List Mode (default)
- **Focus**: Left panel.
- **Navigation**:
    - Vim mode (`tui.vim_keys=true`): `j/k` up/down, `gg` top, `G` bottom.
    - Arrow mode (`tui.vim_keys=false`): `↑/↓` up/down, `Home` top, `End` bottom.
- **Right preview**: Shows full information of selected item (read‑only).
- **Enter detail**: Press `Enter`.

#### B. Detail Mode (browse)
- **Focus**: Right panel.
- **Scrolling**: `j/k` or `↑/↓` scroll long text (description/metadata).
- **Enter edit**:
    - Vim mode: press `i`.
    - Arrow mode: **automatically enters edit mode**.
- **Return**: Press `Esc` to go back to List mode.

#### C. Edit Mode
- **Focus**: Right panel, current field highlighted.
- **Field navigation**: `Tab` next field, `Shift+Tab` previous field.
- **Save**: `Ctrl+Enter` saves all changes and returns to List mode.
- **Abort**: `Esc` discards all changes and returns to List mode.

### 5.3 Detail Panel Content Structure
```
📋 Todo / 📅 Event                [Source: work]
─────────────────────────────────────────────────
<Summary (large)>

● Pending   ████████░░░ (7/9)    #shopping #errand
─────────────────────────────────────────────────
── Time ──
Due:  2026-08-24 17:00  ⚠️ (overdue)
─────────────────────────────────────────────────
── Description ──
<description wraps automatically>
─────────────────────────────────────────────────
── Metadata ──
Source:     work
UID:        abc-123
Related:    parent-uid (if any)
Created:    2026-08-20 14:30
Updated:    2026-08-22 09:15
```
**Editable fields (Tab order)**:
1.  `source` (dropdown to move task to another Source)
2.  `summary` (text input)
3.  `status` (dropdown)
4.  `priority` (number/slider)
5.  `tags` (text input, comma‑separated)
6.  `due` (Todo) / `dtstart` (Event) (date input)
7.  `percent_complete` (Todo) / `dtend` (Event) (number/date input)
8.  `location` (Event) (text input)
9.  `rrule` (Event) (text input)
10. `related_to` (text input)
11. `description` (multi‑line text input)

### 5.4 Settings Overlay
- **Invoke**: `Ctrl+p`.
- **Features**:
    - **Visible sources**: Checkboxes; modifies `contexts.tui` and refreshes list immediately.
    - **Write source**: Dropdown; modifies `defaults.write_source`.
    - **Filter**: Radio buttons (`Todo`, `Event`, `Both`); modifies `ui.default_filter`.
    - **Keybindings**: Radio (`Vim` / `Arrow`); modifies `tui.vim_keys`.
- **Persistence**: Clicking **`Apply`** atomically writes to `config.toml` using `toml_edit`. **`Cancel`** discards changes.

### 5.5 Global Key Bindings
| Key          | List Mode        | Detail Mode          | Edit Mode                 |
| :----------- | :--------------- | :------------------- | :------------------------ |
| `j` / `↓`    | Move down        | Scroll (Vim)         | Input to current field    |
| `k` / `↑`    | Move up          | Scroll (Vim)         | Input to current field    |
| `Enter`      | Enter Detail     | —                    | Confirm field value       |
| `Tab`        | —                | —                    | Next field                |
| `i` (Vim only)| —               | Enter Edit           | —                         |
| `Ctrl+Enter` | —                | —                    | **Save and return**       |
| `Esc`        | —                | Return to List       | **Discard and return**    |
| `a`          | Add dialog       | —                    | —                         |
| `d`          | Complete         | —                    | —                         |
| `D`          | Delete (confirm) | —                    | —                         |
| `Ctrl+p`     | Open Settings    | Open Settings        | Open Settings             |
| `q`          | Quit             | Quit                 | Must `Esc` first          |

---

## 6. Date Parsing Engine

### 6.1 Supported Input Formats
- **Absolute**: `2026-08-25`, `08/25/2026`, `2026-08-25 14:30`.
- **Relative offsets**: `+3d` (3 days later), `-2w` (2 weeks ago), `+1m` (1 month), `+1y` (1 year).
- **Keywords**: `today` (all‑day), `tomorrow`, `yesterday`, `now`, `eod`;
  start boundaries `sod`/`sow`/`soww`/`som`/… use `[date] day_start`;
  end boundaries `eow`/`eoww`/`eom`/… use `[date] day_end`; period bounds
  `sow`/`eow` (week, Monday day_start / Sunday day_end), `soww`/`eoww`
  (working week, Monday day_start / Friday day_end),
  `som`/`eom`, `soq`/`eoq`, `soy`/`eoy`, `sond`/`eond`, `sonw`/`eonw`,
  `sopw`/`eopw`, `sonm`/`eonm`, `sopm`/`eopm`, `sony`/`eony`, `sopy`/`eopy`.
- **Compact forms (T-style)**: `20260812` (all‑day), `20260812T090000` (full),
  `0826` / `25` (this year/month, trailing‑fill), `0826T0930` / `25T`
  (this year/month + time), `T0900` (today at 09:00).
- **Weekdays**: `monday`/`mon`, `friday`/`fri`, with `fri+1` (next Friday).
- **Times**: `2pm`, `14:30` (if alone, applied to today).

### 6.2 Timezone Handling
1.  Parsing assumes **system local timezone** (Local).
2.  Storage converts to **UTC** (`DateTime<Utc>`) via `chrono`.
3.  Display converts back to local timezone.
