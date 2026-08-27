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
- **Fields**: `id, status, summary, desc, tags, due, pri, type, source`. `summary` = CalDAV `SUMMARY`, `desc` = `DESCRIPTION` (two separate fields).
- **Icons**: nerdfont; fallback column `icons` > global `[icons]` > builtin defaults.
- **Colors**: taskwarrior-style theme rules in `[theme.color]`, row-level, ordered by `"rule.precedence.color"`; styles `fg [on bg] [bold|underline|italic|dim|inverse]`.
- **Builtin virtual tags**: `OVERDUE`, `DONE`≡`COMPLETED`, `CANCELLED`, `IN-PROCESS`, `TAGGED`/`UNTAGGED`, `TODO`/`EVENT`, `SCHEDULED` (event = `DTSTART`). No `WAITING`; `DELETED` is local‑only.
- **Relations**: `rel:<parent-id>` on `add`/`modify` → `RELATED-TO` (`RELTYPE=PARENT`); child stores parent UID.
- **Delete** = hard delete locally, no CalDAV mapping.
