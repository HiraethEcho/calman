//! Subcommand handlers and shared CLI helpers.

pub mod add;
pub mod count;
pub mod delete;
pub mod done;
pub mod list;
pub mod modify;
pub mod sync;
#[cfg(feature = "tui")]
pub mod tui;

use crate::config::{Config, ContextKind, Source, SourceType};
use crate::model::{Task, TaskStatus};
use crate::source;
#[cfg(feature = "storage-ics")]
use crate::storage::ics::IcsStorage;
#[cfg(feature = "storage-jsonl")]
use crate::storage::jsonl::JsonlStorage;
use crate::storage::{Storage, Store};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
#[cfg(feature = "recur-expand")]
use chrono::{Duration, Local};

/// One merged row across selected sources, with a dynamic short ID.
pub struct Row {
    pub id: usize,
    pub source: String,
    pub task: Task,
    /// Virtual expanded occurrence index (recur-expand): renders as `id.occ`.
    pub occ: Option<usize>,
}

/// Filter cheat-sheet printed by `calman help` / `calman filters`.
pub fn print_filter_help() {
    print!(
        r#"calman — task & event manager (CLI)

COMMANDS
  calman add <text> [opts]        add todo (due:) or event (from:)
  calman list|ls|next [filter]   run a report (bare `calman` → next)
  calman done <id>               mark completed
  calman delete <id>             hard delete
  calman modify <id> [opts]      change fields
  calman count [filter]          print number of matches
  calman sync [source]           run external sync command
  calman help | filters          show this cheat-sheet

COMMON OPTIONS (add / modify)
  due:<date>        todo deadline (date-only → all-day todo)
  from:<date>       event start (date-only → all-day event)
  to:<date> for:<dur>  event end / length
  pri:H|M|L         priority (9/5/1)
  +tag -tag         tags
  source:<name>     write/list source (ics-dir: `name/collection`)
  rel:<id>          parent relation (RELATED-TO)
  recur:<rule>      recurrence (alias `repeat:`)
  location:<text> alert:<lead> desc:<text>
  wait:<date|dur> hide until <expr> relative to due/start (e.g. wait:+1d,
                    wait:sopd; hidden while `date + wait > now`, +WAITING shows)
  on:<date>         target one occurrence of a recurring series (needs `recur-expand`)
  <id>.<n>          nth upcoming occurrence (e.g. `done 5.2`, `modify 5.1 summary:x`)
                    expanded occurrences also get plain sequential IDs (`done 5` works)

RECURRENCE (recur: / repeat:)  → standard RFC 5545 RRULE
  raw passthrough : recur:FREQ=WEEKLY;BYDAY=TU,FR;UNTIL=20260925
  frequency        : daily weekly monthly yearly
  interval         : every 7d | 7d | every 2 weeks   (→ INTERVAL)
  weekdays         : every tuesday and friday | every weekend (→ BYDAY)
  end              : for 5 times | for 7 weeks (weeks×weekday→COUNT)
                     | count:5 | until:20260925 | until:eoy | until:eom
  e.g. every tuesday and friday for 7 weeks
       → FREQ=WEEKLY;BYDAY=TU,FR;COUNT=14
  series model     : master = status:recurring, virtual tag +PARENT
                     hidden from ls/list/next by default (show: `+PARENT`)
  occurrences      : done <id> on:<date> → Completed override record
                     delete <id> on:<date> → EXDATE (skip one)
                     modify <id>.<n> … → RECURRENCE-ID override (same UID)
                     expanded rows carry plain IDs; `done 5` targets one occurrence

DATE-ONLY DUE (fixed overdue policy)
  a date-only `due` is owed only AFTER its day passes (today's due is not overdue)
  stored as DUE;VALUE=DATE in ICS (iOS Reminders compatible)

FILTER GRAMMAR (shared by CLI args and report `filter`)
  type:todo | type:event | type:all        (+TODO / +EVENT aliases)
  source:work  -source:work                include / exclude a source
  due:<day> exact | due.before:<   strict < | due.by:<   <= | due.after:>=
  date:<day> (unified: todo→due, event→dtstart) + date.before:/date.by:/date.after:
  from:<day> exact | from.before:/from.by:/from.after:  (events' dtstart only)
  status:pending|in-progress|completed|cancelled|recurring|active
  +OVERDUE +PENDING +COMPLETED +CANCELLED +IN-PROCESS +TAGGED +UNTAGGED +SCHEDULED +PARENT
  +tag / -tag
  Composition: adjacent atoms = and; `and` binds tighter than `or`:
    A B or C D     = (A and B) or (C and D)
    (A or B) C     = (A or B) and C

SEPARATE EVENT / TODO
  calman type:event             future events only (report default still applies)
  calman type:todo +PENDING     active todos
  calman rc.report.next.filter='type:event' next   all events incl. past

REPORTS & OVERRIDES (Taskwarrior rc style)
  builtin: ls / list / next (bare `calman` → next)
  rc.report.<name>.columns=id,date,summary   custom columns (script-friendly)
  rc.report.<name>.labels=ID,DATE,TASK
  rc.report.<name>.filter=...  rc.report.<name>.sort=...

ICONS (nerdfont) — 3-level fallback: column `icons` > [icons.todo]/[icons.event] > builtin
  [icons.todo]   pending=○ in-progress=● completed=✓ cancelled=✕
  [icons.event]  pending=󰃭 (calendar; cancelled=✕)   event non-cancelled → calendar

SOURCES
  jsonl / ics / ics-dir (Radicale/vdirsync). ics-dir collections:
    source:remote           → expands all collections
    source:remote/sorge      → one collection (composite reference)

CONFIG (two tiers)
  config.default.toml  complete default reference — self-contained; lists every default option (what calman uses with no config file)
  config.example.toml  annotated custom sample (copy & edit)
  include = ["config.default.toml", "colorscheme.example.toml"]
"#
    );
}

/// Open the storage backend for a source.
pub fn open_storage(conf: &Config, src: &Source) -> Result<Store> {
    let loc = src.abs_location();
    let tz = conf.date.tz();
    Ok(match src.source_type {
        #[cfg(feature = "storage-jsonl")]
        SourceType::Jsonl => Store::Jsonl(JsonlStorage::open(&loc)?),
        #[cfg(not(feature = "storage-jsonl"))]
        SourceType::Jsonl => {
            bail!("this build was compiled without the `storage-jsonl` feature")
        }
        #[cfg(feature = "storage-ics")]
        SourceType::Ics => Store::Ics(IcsStorage::open(&loc, tz)?),
        #[cfg(not(feature = "storage-ics"))]
        SourceType::Ics => {
            bail!("this build was compiled without the `storage-ics` feature")
        }
        SourceType::IcsDir => {
            bail!(
                "IcsDir source `{}` must be resolved to a collection before opening storage",
                src.name
            )
        }
    })
}

/// Resolve the effective source list: `--source` override > context defaults.
///
/// Handles `IcsDir` expansion:
/// - `source:name/collection` → single virtual `Ics` source for that collection
/// - `source:name` (IcsDir) → expand all discovered collections
/// - `source:name` (regular) → use directly
pub fn resolve_sources(
    conf: &Config,
    override_: Option<&[String]>,
    ctx: ContextKind,
) -> Result<Vec<Source>> {
    let names: Vec<String> = match override_ {
        Some(ns) => ns.to_vec(),
        None => conf.context_sources(ctx),
    };
    let mut out = Vec::new();
    for name in names {
        let resolved = source::resolve_source_name(&conf.sources, &name)?;
        out.extend(resolved);
    }
    if out.is_empty() {
        bail!("no sources selected");
    }
    Ok(out)
}

/// Merge all tasks from `sources` into `Row`s with sequential short IDs.
///
/// Taskwarrior-style numbering: the **oldest** task gets ID 1. Rows are
/// ordered by `created_at` (ties broken by UID) before IDs are assigned, so
/// IDs stay stable across sessions regardless of storage iteration order.
pub fn load_merged(conf: &Config, sources: &[Source]) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for src in sources {
        let st = open_storage(conf, src)?;
        for t in st.list() {
            let mut t = t.clone();
            t.source = src.name.clone();
            rows.push(Row {
                id: 0,
                source: src.name.clone(),
                task: t,
                occ: None,
            });
        }
    }
    rows.sort_by(|a, b| {
        a.task
            .created_at
            .cmp(&b.task.created_at)
            .then_with(|| a.task.uid.cmp(&b.task.uid))
    });
    for (i, r) in rows.iter_mut().enumerate() {
        r.id = i + 1;
    }
    Ok(rows)
}

/// Like [`load_merged`], but with recurring series expanded into virtual
/// occurrence rows (Taskwarrior-style). All rows — real and virtual — get
/// sequential plain integer IDs; occurrence rows carry `occ` for `on:`/`id.n`
/// addressing. Without the `recur-expand` feature this is just `load_merged`.
pub fn load_merged_expanded(conf: &Config, sources: &[Source]) -> Result<Vec<Row>> {
    #[cfg(feature = "recur-expand")]
    {
        let mut rows = load_merged(conf, sources)?;
        expand_occurrences(&mut rows, conf);
        Ok(rows)
    }
    #[cfg(not(feature = "recur-expand"))]
    {
        load_merged(conf, sources)
    }
}

/// Expand recurring parents into virtual occurrence rows and renumber all
/// rows sequentially (recur-expand).
#[cfg(feature = "recur-expand")]
fn expand_occurrences(rows: &mut Vec<Row>, conf: &Config) {
    use crate::model::{Task, TaskStatus};
    use crate::recur_expand::expand_task;
    use chrono::{DateTime, Duration, Utc};
    use std::collections::HashMap;

    // Map overrides: parent_uid -> (recurrence_id -> task).
    let mut overrides: HashMap<String, HashMap<DateTime<Utc>, Task>> = HashMap::new();
    for r in rows.iter() {
        if let (Some(pid), Some(rid)) = (&r.task.parent_uid, r.task.recurrence_id) {
            overrides
                .entry(pid.clone())
                .or_default()
                .insert(rid, r.task.clone());
        }
    }
    // Override records are addressed via their parent's occurrence, not standalone.
    rows.retain(|r| r.task.recurrence_id.is_none());

    let now = Utc::now();
    let after = now - Duration::days(1);
    let before = now + Duration::days(366);
    let mut extra: Vec<Row> = Vec::new();

    for r in rows.iter() {
        // Expand active series only (done/cancelled masters stop expanding).
        if !r.task.is_parent() || r.task.status.is_done() {
            continue;
        }
        let occs = expand_task(&r.task, after, before);
        let occs: Vec<_> = if conf.defaults.recur_expand_count == 0 {
            occs
        } else {
            occs
                .into_iter()
                .take(conf.defaults.recur_expand_count)
                .collect()
        };
        let ovs = overrides.get(&r.task.uid);
        for occ in occs {
            let override_task = ovs.and_then(|m| m.get(&occ.occurrence_start)).cloned();
            let mut t = override_task.clone().unwrap_or_else(|| occ.master.clone());
            if override_task.is_none() {
                // Virtual occurrence from the template: single active instance
                // dated at its own slot. Marked with the master's uid so
                // `is_parent()`/`+PARENT` never match it.
                t.parent_uid = Some(r.task.uid.clone());
                t.status = TaskStatus::Pending;
                if t.is_event() {
                    let delta =
                        occ.occurrence_start - t.dtstart.unwrap_or(occ.occurrence_start);
                    t.dtstart = Some(occ.occurrence_start);
                    t.dtend = t.dtend.map(|e| e + delta);
                } else {
                    t.due = Some(occ.occurrence_start);
                }
                // Effective creation time = its own date (ID ordering).
                t.created_at = occ.occurrence_start;
            }
            // Stored overrides keep their own date/status (reschedule/completed).
            extra.push(Row {
                id: r.id,
                source: r.source.clone(),
                task: t,
                occ: Some(occ.index),
            });
        }
    }
    rows.extend(extra);

    // Two-segment ID ordering:
    //  1) not-done todos + events starting today-or-later — created ASC
    //  2) done todos + events starting before today — created DESC
    // Occurrence rows carry their own date as created_at (set above), so a
    // recurring series' future instances sit in the first segment.
    let today = Local::now().date_naive();
    let first = |r: &Row| -> bool {
        if r.task.is_event() {
            // Events are split by their start date (> yesterday ⇒ segment 1).
            r.task
                .dtstart
                .is_some_and(|d| d.with_timezone(&Local).date_naive() >= today)
        } else {
            // Todos are split by completion only; due date is irrelevant.
            !r.task.status.is_done()
        }
    };
    let cmp_asc = |a: &Row, b: &Row| {
        a.task
            .created_at
            .cmp(&b.task.created_at)
            .then_with(|| a.task.uid.cmp(&b.task.uid))
    };
    rows.sort_by(|a, b| match (first(a), first(b)) {
        (true, true) => cmp_asc(a, b),
        (false, false) => cmp_asc(b, a),
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
    });

    // Taskwarrior-style plain sequential IDs over the whole list.
    for (i, r) in rows.iter_mut().enumerate() {
        r.id = i + 1;
    }
}

/// Build a per-occurrence override sibling: same UID family, `recurrence_id`
/// = the occurrence's original DTSTART, acting as a completed or edited
/// replacement instance. Used by `done` (Completed) and `modify` (Pending).
pub fn override_for_occurrence(
    master: &Task,
    occ: DateTime<Utc>,
    status: TaskStatus,
) -> Task {
    let mut ov = master.clone();
    ov.uid = uuid::Uuid::new_v4().to_string();
    ov.parent_uid = Some(master.uid.clone());
    ov.recurrence_id = Some(occ);
    ov.rrule = None;
    ov.exdates = Vec::new();
    ov.status = status;
    if ov.is_event() {
        let delta = occ - ov.dtstart.unwrap_or(occ);
        ov.dtstart = Some(occ);
        ov.dtend = ov.dtend.map(|e| e + delta);
    } else {
        ov.due = Some(occ);
    }
    let now = chrono::Utc::now();
    ov.created_at = now;
    ov.updated_at = now;
    ov
}

/// A resolved target that may address a single occurrence of a recurring series.
pub struct OccurrenceTarget {
    pub uid: String,
    pub source: String,
    /// Original DTSTART of the target occurrence (None = whole task/series).
    pub occ_date: Option<DateTime<Utc>>,
}

/// Resolve ID arguments, including per-occurrence forms `id.n` and `on:<date>`.
///
/// `on:<date>` applies to every resolved id; `id.n` addresses the nth
/// upcoming occurrence (1-based) of that parent series. Occurrence resolution
/// requires the `recur-expand` feature.
pub fn resolve_targets_occ(
    conf: &Config,
    override_: Option<&[String]>,
    ids: &[String],
    occ_date: Option<DateTime<Utc>>,
) -> Result<Vec<OccurrenceTarget>> {
    let sources = resolve_sources(conf, override_, ContextKind::Cli)?;
    let rows = load_merged_expanded(conf, &sources)?;
    let mut out = Vec::new();
    for id in ids {
        if let Some((pid, pn)) = id.rsplit_once('.') {
            let parent_id: usize = pid.parse().map_err(|_| {
                anyhow::anyhow!("bad occurrence ID `{id}` (expected `<id>.<n>`)")
            })?;
            let n: usize = pn
                .parse()
                .map_err(|_| anyhow::anyhow!("bad occurrence number `{id}` (expected `<id>.<n>`)"))?;
            if n == 0 {
                bail!("occurrence numbers are 1-based: `{id}`");
            }
            let row = rows
                .get(parent_id.checked_sub(1).unwrap_or(usize::MAX))
                .ok_or_else(|| anyhow::anyhow!("no task with ID `{parent_id}`"))?;
            if !row.task.is_parent() {
                bail!("task `{parent_id}` is not a recurring parent");
            }
            let occ = resolve_nth_occurrence(&row.task, n)?;
            out.push(OccurrenceTarget {
                uid: row.task.uid.clone(),
                source: row.source.clone(),
                occ_date: Some(occ),
            });
        } else if let Ok(n) = id.parse::<usize>() {
            let row = rows
                .get(n.checked_sub(1).unwrap_or(usize::MAX))
                .ok_or_else(|| anyhow::anyhow!("no task with ID `{id}`"))?;
            out.push(target_from_row(row, id, occ_date)?);
        } else {
            let found = rows
                .iter()
                .find(|r| r.task.uid == *id)
                .ok_or_else(|| anyhow::anyhow!("no task with UID `{id}`"))?;
            out.push(target_from_row(found, id, occ_date)?);
        }
    }
    Ok(out)
}

/// Build an `OccurrenceTarget` for a real or virtual row; `on:<date>`
/// resolves a series occurrence by local day when the row is a master.
fn target_from_row(
    row: &Row,
    id: &str,
    occ_date: Option<DateTime<Utc>>,
) -> Result<OccurrenceTarget> {
    if row.occ.is_some() {
        // A virtual occurrence row: target that single occurrence, addressed
        // through its parent series (stored overrides keep their own storage
        // uid, so resolve to the master's uid).
        let occ = row
            .task
            .dtstart
            .or(row.task.due)
            .ok_or_else(|| anyhow::anyhow!("occurrence `{id}` has no date"))?;
        let uid = row
            .task
            .parent_uid
            .clone()
            .unwrap_or_else(|| row.task.uid.clone());
        return Ok(OccurrenceTarget {
            uid,
            source: row.source.clone(),
            occ_date: Some(occ),
        });
    }
    let occ = match occ_date {
        Some(d) => Some(resolve_occurrence_date(&row.task, d)?),
        None => None,
    };
    Ok(OccurrenceTarget {
        uid: row.task.uid.clone(),
        source: row.source.clone(),
        occ_date: occ,
    })
}

/// Resolve the nth upcoming occurrence (1-based) of a recurring parent.
pub fn resolve_nth_occurrence(t: &Task, n: usize) -> Result<DateTime<Utc>> {
    #[cfg(feature = "recur-expand")]
    {
        let now = Utc::now();
        let occs = crate::recur_expand::expand_task(
            t,
            now - Duration::days(1),
            now + Duration::days(366),
        );
        occs.into_iter()
            .find(|o| o.index == n)
            .map(|o| o.occurrence_start)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no occurrence `{n}` in the next year for recurring task `{}`",
                    t.uid
                )
            })
    }
    #[cfg(not(feature = "recur-expand"))]
    {
        let _ = (t, n);
        bail!("occurrence addressing requires the `recur-expand` feature")
    }
}

/// Find the occurrence whose local day matches `date` (the `on:<date>` form).
pub fn resolve_occurrence_date(t: &Task, date: DateTime<Utc>) -> Result<DateTime<Utc>> {
    #[cfg(feature = "recur-expand")]
    {
        let now = Utc::now();
        let day = date.with_timezone(&Local).date_naive();
        let occs = crate::recur_expand::expand_task(
            t,
            now - Duration::days(1),
            now + Duration::days(366),
        );
        occs.into_iter()
            .find(|o| o.occurrence_start.with_timezone(&Local).date_naive() == day)
            .map(|o| o.occurrence_start)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no occurrence on `{day}` for recurring task `{}`",
                    t.uid
                )
            })
    }
    #[cfg(not(feature = "recur-expand"))]
    {
        let _ = (t, date);
        bail!("occurrence addressing requires the `recur-expand` feature")
    }
}

/// Resolve a source name to its concrete `Source`, including composite
/// `ics-dir` references (`remote/sorge` → virtual `ics` source).
pub fn resolve_source(conf: &Config, name: &str) -> Result<Source> {
    let mut resolved = source::resolve_source_name(&conf.sources, name)?;
    resolved
        .pop()
        .ok_or_else(|| anyhow::anyhow!("unknown source `{name}`"))
}

/// Resolve ID arguments to `(uid, source_name)` pairs.
///
/// Numeric tokens are short IDs into the merged list; `--uid` tokens are
/// matched directly against stored UIDs. Mixed forms are allowed.
pub fn resolve_targets(
    conf: &Config,
    override_: Option<&[String]>,
    ids: &[String],
) -> Result<Vec<(String, String)>> {
    Ok(resolve_targets_occ(conf, override_, ids, None)?
        .into_iter()
        .map(|t| (t.uid, t.source))
        .collect())
}

#[cfg(all(test, feature = "storage-jsonl"))]
mod tests {
    use super::*;
    use crate::config::{Config, SourceType};
    use tempfile::tempdir;

    fn source(dir: &std::path::Path, name: &str) -> Source {
        Source {
            name: name.into(),
            source_type: SourceType::Jsonl,
            location: dir.display().to_string(),
            sync: None,
        }
    }

    #[test]
    fn ids_assigned_oldest_first() {
        let dir = tempdir().unwrap();
        let mut st = crate::storage::jsonl::JsonlStorage::open(dir.path()).unwrap();
        let first = Task::new("work", "newer");
        let mut second = Task::new("work", "older");
        // older created earlier: rewind its timestamp manually
        second.created_at = first.created_at - chrono::Duration::days(1);
        st.add(second).unwrap();
        st.add(first).unwrap();
        drop(st);

        let rows = load_merged(&Config::default(), &[source(dir.path(), "work")]).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, 1);
        assert_eq!(rows[0].task.summary, "older");
        assert_eq!(rows[1].id, 2);
        assert_eq!(rows[1].task.summary, "newer");
    }

    #[cfg(feature = "recur-expand")]
    #[test]
    fn two_segment_id_order() {
        use crate::model::TaskStatus;
        use chrono::{Duration, Utc};
        let dir = tempdir().unwrap();
        let mut st = crate::storage::jsonl::JsonlStorage::open(dir.path()).unwrap();
        let now = Utc::now();
        // created order: old-event(-12d) < done(-10d) < past(-9d) < future(-8d) < active(-1d)
        let mut old_ev = Task::new("work", "old-event");
        old_ev.dtstart = Some(now - Duration::days(400)); // last year
        old_ev.created_at = now - Duration::days(12);
        let mut done = Task::new("work", "done");
        done.status = TaskStatus::Completed;
        done.created_at = now - Duration::days(10);
        let mut past = Task::new("work", "overdue");
        past.due = Some(now - Duration::days(3));
        past.created_at = now - Duration::days(9);
        let mut future = Task::new("work", "future");
        future.dtstart = Some(now + Duration::days(2));
        future.created_at = now - Duration::days(8);
        let active = Task::new("work", "active");
        for t in [old_ev, done, past, future, active] {
            st.add(t).unwrap();
        }
        drop(st);

        let rows = load_merged_expanded(&Config::default(), &[source(dir.path(), "work")]).unwrap();
        let ids: Vec<(usize, &str)> = rows
            .iter()
            .map(|r| (r.id, r.task.summary.as_str()))
            .collect();
        // segment 1: not-done todos + today-or-future events, created ASC
        // (overdue has a past due but is NOT done → stays in segment 1)
        // segment 2: done todos + past events, created DESC
        // (old-event from last year is the earliest-created → ends the index)
        assert_eq!(
            ids,
            vec![
                (1, "overdue"),
                (2, "future"),
                (3, "active"),
                (4, "done"),
                (5, "old-event"),
            ]
        );
    }

    #[test]
    fn ids_not_shifted_by_filter() {
        let dir = tempdir().unwrap();
        let mut st = crate::storage::jsonl::JsonlStorage::open(dir.path()).unwrap();
        for (i, sum) in ["a", "b", "c"].iter().enumerate() {
            let mut t = Task::new("work", *sum);
            t.created_at += chrono::Duration::hours(i as i64);
            st.add(t).unwrap();
        }
        // complete the middle task
        let rows_all = load_merged(&Config::default(), &[source(dir.path(), "work")]).unwrap();
        let uid_b = rows_all[1].task.uid.clone();
        let mut st = crate::storage::jsonl::JsonlStorage::open(dir.path()).unwrap();
        st.update(&uid_b, |t| {
            t.status = crate::model::TaskStatus::Completed;
            Ok(())
        })
        .unwrap();

        let rows = load_merged(&Config::default(), &[source(dir.path(), "work")]).unwrap();
        // actives = a (1), c (3); completed b keeps id 2 in the full index
        let f = crate::filter::parse_expr(&["status:active".to_string()]).unwrap();
        let shown: Vec<usize> = rows
            .iter()
            .filter(|r| f.matches(&r.task))
            .map(|r| r.id)
            .collect();
        assert_eq!(shown, vec![1, 3]);
    }
}
