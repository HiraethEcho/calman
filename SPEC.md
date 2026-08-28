# calman — Project Specification

## 1. Project Positioning
**calman** is a **minimalist, keyboard-driven task management frontend**. It does **not** implement any synchronization logic itself; instead, it delegates sync operations entirely to user‑configured external commands (`pimsync`, `git`, `rclone`, etc.).

It follows the Unix philosophy: **do one thing and do it well** — provide a first‑class CLI and TUI experience for managing locally stored task data, while giving the user full control over *how* that data is synchronised.

## 2. Core Design Principles
1.  **Storage is files**: Task data is stored locally as `jsonl` (default, high‑performance) or `ics` (compatible with CalDAV / vdir format).
2.  **Sync is commands**: Synchronisation means executing a user‑defined shell command chain (`pre_hook` → `cmd` → `post_hook`).
3.  **Interface is efficiency**: The CLI faithfully replicates Taskwarrior’s habits (natural‑language dates, short IDs, filters); the TUI uses a “list + detail” left‑right two‑panel layout to minimise interface noise.
4.  **Modal interaction**: The TUI strictly distinguishes between *browse* and *edit* modes to prevent accidental modifications.
5.  **Configuration as code**: All TUI settings (visible sources, filters, keybindings) are persisted to `config.toml`.

## 3. Technology Stack

| Module              | Choice                                    | Rationale |
| :------------------ | :---------------------------------------- | :-------- |
| **Language**        | Rust (Edition 2024)                       | No GC, zero‑runtime, extreme performance and low memory footprint |
| **TUI**             | `ratatui` + `crossterm`                   | Most mature Rust TUI library, cross‑platform |
| **CLI parsing**     | `clap` (Derive style)                     | Supports complex subcommands and argument validation |
| **HTTP (optional)** | `reqwest`                                 | (Only if needed by user‑defined commands) |
| **iCalendar**       | `ical` / `vobject` crates                 | Read/write `.ics` files (VTODO and VEVENT) |
| **JSONL**           | `serde_json` streaming parser             | Line‑by‑line, memory‑friendly, Git‑diff‑friendly |
| **Date parsing**    | `chrono` + self‑built parser (`pest`/`nom`) | Taskwarrior‑style natural language |
| **Configuration**   | `toml` + `serde`; write with `toml_edit`  | Preserve formatting and comments |
| **Command execution**| `std::process::Command`                  | Invoke system `sh` to run user sync commands |
| **Logging**         | `tracing` / `env_logger`                  | Leveled logging for debugging |

## 4. Main Feature Highlights
- **Multi‑source management**: Manage several data sources (e.g., `work`, `personal`) with separate default source lists for CLI, Sync, and TUI contexts.
- **Dual storage formats**: Support `jsonl` (default, fast) and `ics` (compatible with vdirsyncer/pimsync ecosystem).
- **Atomic writes**: All data writes use a “temporary file + rename” mechanism to prevent file corruption.
- **External sync**: Through `pre_hook`, `cmd`, `post_hook` calling external tools (e.g., `git`, `pimsync`, `rclone`), with `{location}` placeholder substitution.
- **CLI experience**: Short‑ID system, natural‑language dates (`due:today`, `eow`, `eond`, etc.), predefined filters (`+OVERDUE`), `count` subcommand.
- **TUI experience**: Left‑right two‑panel layout (list + preview/edit), supports both Vim‑style and arrow‑key keybindings, settings overlay persists to config.

## 5. Decisions

- **CLI habits** follow taskwarrior; **internal semantics** follow CalDAV/iCalendar (RFC 5545).
- **Reports**: `ls`/`list`/`next`, configured in `config.toml` under `[report.<name>]`; bare `calman` → `[defaults] default_report` (default `next`).
- **Report schema**: `columns[]` (field, label, width, format, icon), `sort` (`key+`/`key-`, trailing `/` break), `filter`.
- **Report defaults**: builtin reports show only future events (dtstart ≥ start of today); TYPE+STATUS merged into one STATUS column (event → calendar icon); DUE renamed DATE (event plain `MM/DD`, todo relative; per-column `event_format`/`todo_format`).
- **Fields**: `id, status, summary, desc, tags, due, pri, type, source` (`date` = alias of `due`).
- **Filter grammar**: one expression language shared by CLI args and report `filter` strings — `and`/`or`/parens, `type:todo|event|all`, `source:`/`-source:`, `due:` exact day, `due.before:` strict `<`, `due.by:` `<=`, `due.after:` `>=`, virtual tags (`+PENDING` etc.); todos use `due`, events use `dtstart`.
- **CLI report overrides**: Taskwarrior-style `rc.report.<name>.columns=…` / `labels=…` / `filter=…` / `sort=…` for scripted custom reports without editing config. `summary` = CalDAV `SUMMARY`, `desc` = `DESCRIPTION` (two separate fields).
- **Icons**: nerdfont; fallback column `icons` > global `[icons.todo]`/`[icons.event]` > builtin defaults; merged STATUS column renders a per-kind status glyph.
- **Colors**: taskwarrior-style theme rules in `[theme.color]`, row-level, ordered by `"rule.precedence.color"`; styles `fg [on bg] [bold|underline|italic|dim|inverse]`.
- **Builtin virtual tags**: `OVERDUE`, `DONE`≡`COMPLETED`, `CANCELLED`, `IN-PROCESS`, `TAGGED`/`UNTAGGED`, `TODO`/`EVENT`, `SCHEDULED` (event = `DTSTART`). No `WAITING`; `DELETED` is local‑only.
- **Relations**: `rel:<parent-id>` on `add`/`modify` → `RELATED-TO` (`RELTYPE=PARENT`); child stores parent UID.
- **Delete** = hard delete locally, no CalDAV mapping.
- **Recurrence**: `recur:` (alias `repeat:`) normalized to a standard RFC 5545 `RRULE` by `recurrence.rs`. Accepts raw passthrough (`FREQ=…;…`) or a friendly grammar: `daily/weekly/monthly/yearly`, `every 7d`/`2 weeks` (→`INTERVAL`), `every tue and fri`/`weekend` (→`BYDAY`), `for 5 times`/`for 7 weeks` (weeks × weekday count →`COUNT`), `count:N`, `until:<date>`/`until:eoy`/`until:eom`. iOS Reminders / CalDAV compatible.
- **Date-only `due`**: an all-day todo; stored as `DUE;VALUE=DATE` (ICS) and `task.allday = true` (same flag events use for `DTSTART;VALUE=DATE`). Overdue semantics are config-driven: `[date] due_date_overdue_today` — `false` (default) = overdue only after the due day; `true` = overdue from the due day itself.
- **Config two tiers**: `*.default.toml` = minimal working baseline (no comments, shipped); `*.example.toml` = annotated custom samples. `config.toml` uses `include = ["report.default.toml", "theme.default.toml"]`; included files fill missing keys, main file wins.
- **Sources**: `jsonl` / `ics` / `ics-dir` (Radicale/vdirsync layout, auto-discovers collections). Composite reference `source:<name>/<collection>` resolves to one virtual Ics source; bare `source:<name>` (IcsDir) expands all collections. `add` resolves composite refs via `resolve_source_name`.
