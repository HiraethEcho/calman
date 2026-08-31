# Usage

calman uses a **Taskwarrior-style free-form** command line. Filters and
attributes can appear before *or* after the command, and the command itself is
optional.

```sh
calman                                   # bare → runs the `next` report
calman +OVERDUE                          # list, filtered
calman +OVERDUE list                     # filter may precede the command
calman add "buy milk" due:tomorrow pri:H +home
calman 1 done                            # or: calman done 1
calman 1 modify new text pri:L -home due:20260824
```

**Short IDs:** items shown in a report are numbered across the selected
sources (oldest item = ID 1, stable across sessions). IDs resolve against the
merged list from the `source:` override or `contexts.cli`; raw UIDs are also
accepted.

**Global `source:` option:** `source:work,personal` (comma-separated) may appear
anywhere and selects which sources a command operates on. Precedence:
`source:` override > `[contexts]` > all sources.

**`rc` overrides:** `rc.report.<name>.<key>=<value>` tokens (e.g.
`rc.report.next.columns=id,date,summary`) may appear anywhere and temporarily
override a report — see [Reports](reports.md).

---

## `add` — create a todo or event

```sh
calman add <TEXT> [ATTRIBUTES...]
```

What you pass decides the kind:

- **Todo** — give `due:` (and no `from:`).
- **Event** — give `from:` (optionally `to:` or `for:`).

`add` writes to `source:` if given (exactly **one** source; multiple is an
error), otherwise to `[defaults].write_source`.

```sh
calman add "buy milk" due:tomorrow pri:H +home
calman add "standup" from:tomorrow recur:daily          # event, daily
calman add "lunch" from:T1200 for:45min +team
calman add "review" from:2026-09-01 to:2026-09-01 15:00 source:work
```

See [Tasks & events](tasks.md) for the full attribute table.

---

## `list` / `ls` / `next` — run a report

```sh
calman [FILTERS...] [list | ls | next]
calman                                   # ≡ calman next
calman type:event due.after:sod
calman list source:work
```

`ls`, `list`, and `next` are three built-in reports (see [Reports](reports.md)).
`bare calman` runs `[defaults].default_report`, which defaults to **`next`**.

---

## `done` — complete an item

```sh
calman done <ID> [ID...]
calman <ID>... done                      # IDs may precede the command
```

Sets status to `completed` and records `completed_at`.

```sh
calman done 1
calman done 3 4 5
```

---

## `delete` — remove an item

```sh
calman delete <ID> [ID...]
```

Permanently removes the item from storage (hard delete; no CalDAV mapping for
deletion).

```sh
calman delete 2
```

---

## `modify` — change fields

```sh
calman <ID>... modify [TEXT] [ATTRIBUTES...]
```

Bare words **replace the summary** (Taskwarrior semantics). `+tag` adds a tag,
`-tag` removes it. The same attributes as `add` apply (`due:`, `from:`, `to:`,
`for:`, `pri:`, `status:`, `location:`, `recur:`/`repeat:`, `alert:`,
`desc:`, `rel:`, `allday`).

```sh
calman 1 modify new content pri:L -home due:20260824
calman 2 modify +urgent status:in-progress
calman 3 modify from:tomorrow allday    # convert to an all-day event
```

Notes:
- `+allday` converts the target to all-day (drops times and `DTEND`); any
  `from:` makes it a timed event again. There is no `-allday`.
- A date-only `from:` on `modify` makes the event all-day (local midnight, no
  implicit hour) — the same as `add`.

---

## `count` — count matches

```sh
calman count [FILTERS...]
```

Prints a single number (no extra text) — ideal for scripts:

```sh
calman count +OVERDUE
echo "You have $(calman count status:pending) pending tasks."
calman count type:event source:work
```

---

## `sync` — run external sync

```sh
calman sync [source:...]
```

Runs each source's `pre_hook` → `cmd` → `post_hook` chain (per
`[source].sync`). Sources come from the `source:` override, else
`[contexts].sync`, else every source that defines a `cmd`. A `.sync.lock` file
prevents concurrent runs; a non-zero hook exit aborts immediately. See
[Reports](reports.md) → *Sync* for the placeholder substitution details (and a
minor quoting caveat).

```sh
calman sync
calman sync source:work
```

---

## `help` / `filters` — cheat-sheet

```sh
calman help
calman filters
```

Both print the same compact reference: commands, common options, recurrence
grammar, date-only `due` rules, the filter grammar, report/override syntax,
icons, and the two config tiers.

---

## `tui` — terminal UI (stub)

```sh
calman tui
```

The TUI is **Phase 3** and not yet wired up in current builds; the command
prints `tui not implemented yet (Phase 3)` and exits. The planned layout is a
left/right two-panel interface (list 40% / detail 60%) with browse/edit modes.
