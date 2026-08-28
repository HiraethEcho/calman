# AGENTS.md — calman Developer Guide for AI Assistants

## 1. Project Overview
- **Name**: calman — a minimalist, keyboard‑driven task manager with CLI and TUI.
- **Language**: Rust (Edition 2024)
- **Purpose**: Provide a Taskwarrior‑like experience with pluggable sync (via external commands). Data stored locally as JSONL or ICS files.
- **Key documentation**:
  - `SPEC.md` – high‑level features and tech stack.
  - `DESIGN.md` – complete manual‑style specification of all behaviour.
  - `PLAN.md` – phased development roadmap.

## 2. Development Environment Setup
- **Additional tools** (optional but recommended):
  - `cargo-watch` – for automatic rebuilds during development.
  - `cargo-tarpaulin` or `cargo-llvm-cov` – for code coverage.
  - `git` – for version control.
- **System dependencies** (for running tests that call external commands):
  - `git` – needed for sync tests using `git`.
  - `pimsync` – optional, for CalDAV tests (may be mocked).

## 3. Project Directory Structure (Suggested)
Organise your code as follows:
```
calman/
├── Cargo.toml
├── .rustfmt.toml       (use default rustfmt)
├── .gitignore
├── src/
│   ├── main.rs         # entry point, CLI parsing with clap
│   ├── config.rs       # load/save config, Contexts, Source
│   ├── model.rs        # Task struct, TaskStatus enum
│   ├── storage/
│   │   ├── mod.rs      # Storage trait
│   │   ├── jsonl.rs    # JsonlStorage
│   │   ├── ics.rs      # IcsStorage
│   │   └── state.rs    # metadata (.calman-state.json)
│   ├── sync/
│   │   └── executor.rs # run pre_hook/cmd/post_hook, lock management
│   ├── date_parser.rs  # natural‑language date parsing
│   ├── filter.rs       # shared filter expression engine (CLI + report filter)
│   ├── recurrence.rs   # `recur:`/`repeat:` → RFC 5545 RRULE normalizer
│   ├── id_manager.rs   # short‑ID resolution
│   ├── cli/
│   │   ├── mod.rs      # subcommand handlers
│   │   ├── add.rs
│   │   ├── list.rs
│   │   ├── done.rs
│   │   ├── delete.rs
│   │   ├── modify.rs
│   │   ├── count.rs
│   │   ├── sync.rs
│   │   └── tui.rs      # launches TUI (calls tui module)
│   └── tui/
│       ├── mod.rs      # TUI application entry, event loop
│       ├── app.rs      # state machine, modes
│       ├── ui.rs       # overall layout rendering
│       ├── list_pane.rs
│       ├── detail_pane.rs
│       ├── editor.rs   # field editing widgets
│       ├── settings_overlay.rs
│       ├── add_dialog.rs
│       └── theme.rs
├── tests/              # integration tests
└── docs/               (optional)
    ├── SPEC.md
    ├── DESIGN.md
    └── PLAN.md
```

## 5. Testing
- **Unit tests**: Place in the same file as the code, under `#[cfg(test)]`.
- **Integration tests**: Put in `tests/` directory; they should test end‑to‑end CLI behaviour.
- **Test coverage**: Aim for >80% coverage on critical path modules (storage, sync, filter, date_parser).
- **Running tests**: `cargo test --workspace`
- **Special test considerations**:
  - Use temporary directories (`tempfile` crate) for storage tests to avoid polluting user's home.
  - Mock external commands for sync tests (e.g., a dummy `git` script) to avoid network calls.

## 6. Building and Running
- **Debug build**: `cargo build`
- **Release build**: `cargo build --release`
- **Run**: `cargo run -- [ARGS]` (e.g., `cargo run -- add "test" due:today`)
- **Run TUI**: `cargo run -- tui`
- **Logging**: Prefix with `RUST_LOG=debug` for verbose output.

## 7. Debugging
- Use `dbg!()` macros sparingly; prefer `tracing` for persistent logging.
- For TUI issues, run with `RUST_BACKTRACE=1`.
- If the TUI crashes, it may leave the terminal in a bad state. Run `reset` to recover.

## 8. How to Add a New CLI Command
1. Define the subcommand in `main.rs` using `clap` (derive style).
2. Add a new module under `cli/` (e.g., `mycmd.rs`).
3. Implement the handler function that takes parsed arguments, uses `Storage` and `Filter` etc.
4. Register the handler in `cli/mod.rs`.
5. Add integration tests in `tests/`.

## 9. How to Extend the Storage Backend (e.g., support `yaml`)
- Implement the `Storage` trait in a new file under `storage/`.
- Update `config.rs` to recognise the new `type` value.
- Ensure atomic writes and correct `.calman-state.json` handling.

## 10. Current Development Phase (from PLAN.md)
We are in **Phase 2: CLI Advanced Features** (CLI is stable; TUI is Phase 3).
Phase 1 core engine + storage + sync are complete. Priority tasks done:
- [x] Config loader, data model, JSONL/ICS storage, sync executor
- [x] CLI: add/list/done/delete/modify/count/sync
- [x] Date parser (custom, not pest/nom)
- [x] Shared filter grammar (CLI + report `filter`): and/or/parens, type/source/due/status/virtual tags
- [x] Report engine: builtin ls/list/next, merged STATUS column, DATE column, rc overrides, nerdfont icons
- [x] Recurrence (`recur:` → RRULE), date-only all-day due, relations
- [x] Config two tiers (`*.default.toml` / `*.example.toml`), `calman help` cheat-sheet

TUI is **Phase 3** – do not start on it until CLI is stable.

## 11. Commit Message Convention
Follow the [Conventional Commits](https://www.conventionalcommits.org/) format:
- `feat:` – new feature
- `fix:` – bug fix
- `docs:` – documentation changes
- `test:` – test updates
- `refactor:` – code refactoring
- `chore:` – build/tooling changes

Include a short description and, if needed, a body with motivation/design decisions.

## 12. Communication
If you have questions about the design that are not covered by `SPEC.md` and `DESIGN.md`, ask the user before implementing. Do not assume behaviour that contradicts the detailed specifications.

<!-- LITESPEC:START -->

# LiteSpec Instructions (lite workflow)

## Files

- `SPEC.md` — goal, what we're building, decisions (rare changes)
- `PLAN.md` — phases, tasks, progress checkboxes (frequent changes)
- `DESIGN.md` — architecture depth, only when a phase needs it
- `HANDOFF.md` — parked notes from /rest, read by /pickup on resume
- `README.md` / `CHANGELOG.md` — human mirrors, updated at phase-complete only

## Workflow

- New session / pickup → `/pickup` (reads AGENTS.md block + SPEC/PLAN/HANDOFF)
- New work → update `SPEC.md` (Decisions) + `PLAN.md` (tasks) before building
- Implement next task → `/task` (tick the checkbox)
- Phase complete → `/archive` (rollup: PLAN done + SPEC decisions + README/CHANGELOG if user-facing)
- Pause / handoff → `/rest` (parked note in PLAN.md + HANDOFF.md)

## Rules

- Progress is derived from `PLAN.md` checkboxes only — never store a status line elsewhere.
<!-- LITESPEC:END -->
