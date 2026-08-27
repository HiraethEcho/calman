# calman — Phased Development Plan

## Phase 1: Core Engine & CLI Foundation (Estimated 3‑4 weeks)
**Goal**: A usable CLI tool that can add, list, modify, delete tasks and invoke external sync commands.

### 1.1 Basic Framework
- [ ] Initialise Rust project (`cargo init`).
- [ ] Add dependencies: `clap` (derive), `serde`, `serde_json`, `toml`, `toml_edit`, `chrono`, `anyhow`, `thiserror`.
- [x] Implement configuration loader (`config.rs`):
    - Read `~/.config/calman/config.toml`.
    - Generate default config if absent.
    - Implement fallback logic for `contexts`.

### 1.2 Data Model & Storage
- [x] Define `Task` struct and `TaskStatus` enum (`model.rs`).
- [x] Implement `Storage` Trait:
    - `JsonlStorage`: read, append, atomic write, modify/delete by UID for `tasks.jsonl`.
    - `IcsStorage`: read/write `*.ics` files (using `ical` crate).
- [x] Implement state manager (`state.rs`): read/write `.calman-state.json`, manage `uid_counter`.

### 1.3 Sync Executor
- [x] Implement `sync/executor.rs`:
    - Parse `{location}` placeholder in `pre_hook`, `cmd`, `post_hook`.
    - Execute commands via `std::process::Command`.
    - Implement `.sync.lock` locking.
    - Implement short‑circuit logic (pre_hook failure stops execution).

### 1.4 CLI Command Skeleton
- [x] Define subcommand structure in `main.rs` with `clap`.
- [x] Implement `add` (including write‑source logic).
- [x] Implement `list` (cross‑source merging, short‑ID generation, table/JSON/CSV output).
- [x] Implement `done` / `delete` / `modify` (short‑ID + `--uid` support).
- [x] Implement `count` (including predefined filters like `+OVERDUE`).

---

## Phase 2: CLI Advanced Features & Integration (Estimated 2‑3 weeks)
**Goal**: Polish the CLI to match Taskwarrior’s usability.

### 2.1 Date Parsing Engine
- [ ] Define grammar using `pest` or `nom`.
- [ ] Implement `DateParser`:
    - Handle `eow`, `eoww`, `eond`, etc.
    - Handle offsets (`+3d`).
    - Handle weekdays (`fri+1`).
    - Integrate `chrono` for UTC conversion.

### 2.2 Filter Engine
- [ ] Parse command‑line arguments for `list` and `count`.
- [ ] Implement `Filter` struct: support `due`, `status`, `priority`, `tags`.
- [ ] Map predefined filters (`+OVERDUE` etc.) to filter expressions.

### 2.3 Integration Tests & CLI Documentation
- [ ] Write integration tests (`tests/`) covering core commands.
- [ ] Polish `--help` output.

### 2.4 Report Engine & Relations
- [x] Config schema: `[report.<name>]` (columns/format/sort/filter), `[defaults] default_report`.
- [x] Builtin reports `ls`/`list`/`next`; bare `calman` → `next`.
- [x] Report renderer: fields (`id,status,summary,desc,tags,due,pri,type,source`), formats (`relative/countdown/iso/truncate`), `width`, `sort` (incl. `/` break).
- [x] Nerdfont icons, 3-level fallback (column `icons` > `[icons]` global > builtin).
- [x] Global row-level color rules `[[color]]`, first-match, `fg/bg/bold/underline/italic/dim`.
- [x] Builtin virtual tags: `OVERDUE`, `DONE`≡`COMPLETED`, `CANCELLED`, `IN-PROCESS`, `TAGGED`/`UNTAGGED`, `TODO`/`EVENT`, `SCHEDULED`.
- [x] `rel:<parent-id>` attribute on `add`/`modify` → `RELATED-TO` (`RELTYPE=PARENT`).
- [x] `RELATED-TO` read/write in `IcsStorage`; `related_to` persisted in JSONL.
- [x] Modular config: `include` merge in `config.rs` + `config.example.toml` / `report.example.toml` / `theme.example.toml`.

---

## Phase 3: TUI Complete Implementation
**Goal**: Full left‑right two‑panel TUI with browsing, editing, and settings persistence.

### 3.1 TUI Skeleton & Layout
- [ ] Add `ratatui`, `crossterm`, set up custom event loop.
- [ ] Implement application state machine (`AppState`): List, Detail, Edit, Settings.
- [ ] Implement left‑right layout: list 40%, detail 60%.

### 3.2 List Panel & Detail Preview
- [ ] List rendering: highlight, scrolling, filtering (using `contexts.tui`).
- [ ] Detail rendering (`DetailPane`):
    - Render Header (Type, Source).
    - Render Status Bar (status, priority, tags).
    - Render dynamic time section (Todo/Event switching).
    - Render metadata (including Source field).

### 3.3 Modal Interaction & Editing
- [ ] Implement full state transitions: List → Detail → Edit and back.
- [ ] Implement field editors (text input, dropdown, date input).
- [ ] Implement `Ctrl+Enter` save and `Esc` discard.
- [ ] Implement `source` field editing (moving tasks between Sources).

### 3.4 Settings Overlay & Persistence
- [ ] Implement `SettingsOverlay` (floating window).
- [ ] Bind `Ctrl+p` to toggle.
- [ ] Use `toml_edit` to atomically write configuration.
- [ ] Implement keybinding mode switching (Vim / Arrow) with immediate effect.

### 3.5 Sync Indication & Miscellaneous
- [ ] Trigger `sync` from TUI (key `s`) and show external command output logs.
- [ ] Implement `AddDialog` (floating input form).
- [ ] Initialise internationalisation (i18n) framework.
- [ ] Performance optimisation: rendering/scrolling with 1000+ tasks.

---

## 4. Milestones & Releases
- **v0.1.0 (Alpha)**: Phase 1 complete – usable CLI for basic task management.
- **v0.2.0 (Alpha)**: Phase 2 complete – CLI matches core Taskwarrior features.
- **v0.3.0 (Beta)**: Phase 3 complete – usable TUI.
- **v1.0.0 (Stable)**: Phase 3 complete – stable TUI, full documentation, official release.

## 5. Risks & Mitigations
| Risk                              | Mitigation |
| :-------------------------------- | :--------- |
| **ICS format incompatibility**    | Use mature `ical` crate; write compatibility tests against Radicale. |
| **Concurrent write conflicts**    | Rely on `.sync.lock` and external command atomicity (e.g., Git). |
| **TUI performance**               | JSONL streamed; detail render only for selected item, avoid full traversal. |
| **Config write conflicts**        | Use `toml_edit` to preserve comments; if file changed externally, prompt user to restart. |
