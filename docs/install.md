# Install

## Build from source

calman is a standard Rust project (Edition 2024). With a Rust toolchain
installed:

```sh
git clone <repo> calman && cd calman
cargo build --release          # binary at target/release/calman
cargo run -- --help            # run without installing
```

Useful during development:

```sh
cargo watch -x run -- add "try me" due:tomorrow     # rebuild on change
RUST_LOG=debug cargo run -- next                    # verbose logging
cargo test --workspace                              # unit + integration tests
```

## Configuration directory

calman reads its config from (in order of preference):

1. `$XDG_CONFIG_HOME/calman/config.toml`
2. `$HOME/.config/calman/config.toml`

`XDG_CONFIG_HOME`, if set and non-empty, overrides `$HOME/.config`. All `~/`
paths in the config (source `location`, `include` files) are expanded to `$HOME`.

On **first run**, if no `config.toml` exists, calman generates a minimal default
one for you and continues.

## Two configuration tiers

calman ships two tiers of config so you can start small and grow:

| Tier | Files | Purpose |
| :--- | :---- | :------ |
| **default** (complete reference) | `config.default.toml`, `report.default.toml`, `theme.default.toml` | `config.default.toml` is a **self-contained** file listing *every* default option — exactly what calman uses when no config file exists. `report.default.toml` / `theme.default.toml` are the matching default report/theme tables. |
| **example** (annotated) | `config.example.toml`, `report.example.toml`, `theme.example.toml` | Heavily commented custom samples to study and copy from. |

`config.default.toml` mirrors `Config::default()` — copy it as your `config.toml`
to start from fully-known defaults and override only the keys you need. It is
**self-contained** (no `include`): it defines the default `work` source, the
`[defaults]`, `[contexts]`, `[date]`, `[ui]`, `[locale]`, `[icons]` sections and
documents `[theme]`, `[report.*]` and the reserved `[date]` keys inline.

A typical custom `config.toml` builds on the defaults via `include`:

```toml
include = ["config.default.toml", "report.default.toml", "theme.default.toml"]

[defaults]
write_source = "work"
# default_report = "next"

[date]
due_date_overdue_today = false

[[source]]
name = "work"
type = "jsonl"
location = "~/.local/share/calman/work/"

[[source]]
name = "remote"
type = "ics-dir"
location = "~/calman/remote"
```

- `include` paths are resolved relative to the `config.toml` directory; absolute
  and `~/`-prefixed paths also work.
- Included files are merged **first**; keys present in the main file win. Only
  one level of `include` is supported.
- Including `config.default.toml` re-applies the default `work` source; if you
  also list `[[source]]` entries in your main file, those **replace** the
  included array (arrays do not merge). To customise sources, either omit
  `config.default.toml` from the include and define your own, or rely on the
  default `work` source and add extra `[[source]]` entries only in a file that
  does **not** include `config.default.toml`.

## First run checklist

```sh
# 1. copy the complete default reference (self-contained — no other files needed)
mkdir -p ~/.config/calman
cp config.default.toml ~/.config/calman/config.toml

# 2. (optional) study the annotated examples for ideas
# cp config.example.toml ~/.config/calman/config.toml
# cp report.example.toml theme.example.toml ~/.config/calman/

# 3. try it
calman add "buy milk" due:tomorrow pri:H +home
calman                                  # runs the `next` report
```

If no config exists yet, the very first `calman` invocation writes a default
`config.toml` automatically — you can then edit it.

## Sources

Three storage backends are available:

- **`jsonl`** — a single `tasks.jsonl` file; git-friendly, fast. Recommended.
- **`ics`** — one `<UID>.ics` file per item; CalDAV/vdir compatible.
- **`ics-dir`** — a Radicale/vdirsync-style directory; collections are
  auto-discovered and referenced as `name/collection` (see [Tasks & events](tasks.md)).

See `config.example.toml` for fully annotated source and sync examples.
