# Filters

calman has **one filter language** shared by the CLI (`list`, `count`, and
report filters) and by `[report.<name>].filter` strings. Filters are evaluated
against the **unified** item: todos compare `due`, events compare `dtstart`.

## Atoms

| Atom | Meaning |
| :--- | :------ |
| `type:todo` \| `type:event` \| `type:all` | Kind selector. Aliases `+TODO` / `+EVENT`. |
| `source:<name>` / `src:<name>` | Item belongs to `<name>`. |
| `-source:<name>` / `-src:<name>` | Item does **not** belong to `<name>`. |
| `due:<day>` | Exact calendar day (`due:today` = today). |
| `due.before:<date>` | Strictly **before** (`<`). |
| `due.by:<date>` | On or before (`<=`). |
| `due.after:<date>` | On or after (`>=`). |
| `date:<day>` | Exact day on the **unified** date (todo→`due`, event→`dtstart`). |
| `date.before:` / `date.by:` / `date.after:` | Same operators on the unified date. |
| `from:<day>` / `from.before:` / `from.by:` / `from.after:` | VEVENT `dtstart` only; todos never match. |
| `status:pending\|in-progress\|completed\|cancelled\|recurring\|active` | Lifecycle status (`active` = pending/in-progress; `recurring` = series master). |
| `+WAITING` / `-WAITING` | Hidden-by-wait: item has a `wait` in the future. Default reports exclude them. |
| `priority:<lvl>` (alias `pri:<lvl>`) | `high`/`h`/`9`, `medium`/`m`/`5`, `low`/`l`/`1`, or 0–9. |
| `+tag` / `-tag` | Has / lacks the tag. |
| `+VIRTUAL` / `-VIRTUAL` | Virtual tag (see below). |

`due:` comparisons are **date-only** for the exact-day form (same local
calendar day); `due.before/by/after` compare instants. Any date token understood
by the [date engine](dates.md) works, including `now`, `sod`, `eow`, `eom`, and
offsets like `+3d`.

## Virtual tags

| Tag | Matches |
| :-- | :------ |
| `+OVERDUE` | Date in the past and not completed/cancelled (all-day due is overdue only after its day). |
| `+PENDING` / `+ACTIVE` | Active (not completed/cancelled). **Events count as pending.** |
| `+COMPLETED` / `+DONE` | Completed. |
| `+CANCELLED` / `+CANCELED` | Cancelled. |
| `+IN-PROGRESS` | In-progress (status). |
| `+STARTED` | `calman start <id>` recorded a start time (`started_at`). |
| `+TAGGED` | Has at least one tag. |
| `+UNTAGGED` | Has no tags. |
| `+SCHEDULED` | Is an event (has `dtstart`). |
| `+TODO` / `+EVENT` | Type selector (alias of `type:todo` / `type:event`). |

Negated forms (`-OVERDUE`, `-PENDING`, …) invert the match.

## Composition & precedence

- Tokens are whitespace-separated. **Adjacent atoms are joined by `and`.**
- `and` **binds tighter than** `or`.
- Parentheses group explicitly.

```text
A B or C D        =  (A and B) or (C and D)
(A or B) C        =  (A or B) and C
```

```sh
calman type:event due.after:sod
calman type:todo +PENDING or type:event
calman '(status:active or status:in-progress) src:work'
calman count +OVERDUE
calman list -status:completed -status:cancelled +TAGGED
calman list +PARENT              # show recurring series masters
calman list status:recurring
calman list +WAITING             # show items hidden by wait
```

## Reports vs. CLI filters

A built-in report has its **own default filter** (e.g. `next` shows only future
events). Your CLI filters are **combined** with the report filter — both must
match. To relax a report's restriction, override its filter with `rc`:

```sh
# see ALL events, including past ones, by overriding the report filter
calman rc.report.next.filter='type:event' next

# list command still applies the `list` report's default filter
calman type:event
```

`count` ignores report defaults and matches purely against your filters:

```sh
calman count type:event          # all events, regardless of report defaults
```
