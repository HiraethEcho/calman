## cli

recursive task. every 7d, week, 3d, month, 30d etc. repeat times, until date
repeat event. how to set every Tuesday and Friday for each week, and 7 weeks total?

for date, support eonnd, soppw.

config on report. order of items: index( created time ), urgency, due. uda: index, status, summary, desc, tags, due, pri, type (todo or event). colors
filter for different report. in short, just like taskwarrior.

the configuration goes like config.toml, theme.toml, report.toml, sync.toml etc?

## source

Radicale server has multiple collections, vdirsync/pimsync syncs to `radicale/{collection}/item.ics`.

### Reference format

Path-like: `source:<name>/<collection>`

Examples:
- `source:personal/calendars` → `.../radicale/calendars/item.ics`
- `source:work/My Projects` → `.../work/My Projects/item.ics`

Spaces in collection names: use quotes → `source:personal/"My Calendars"`

### Config format

Auto-discover from path:

```toml
[[source]]
name = "personal"
path = "/home/user/.contacts/radicale"
# calman recursively discovers all item.ics under subdirs

[[source]]
name = "work"
path = "/home/user/.contacts/work-cal"
```

- Recursive scanning: all nested directories scanned
- Collection name = relative path from `path` to `item.ics` parent
- Naming conflicts resolved by source prefix (e.g. `personal/calendars` vs `work/calendars`)

## todo to event

let me start a todo. when stop it, create an event.
