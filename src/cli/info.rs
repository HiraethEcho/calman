//! `info` subcommand — show full details of one or more items.
//!
//! Accepts plain IDs (`calman <id> info` / `calman info <id>`), per-occurrence
//! IDs (`id.n`), `on:<date>`, source overrides and UIDs.

use crate::args::ParsedArgs;
use crate::cli::{Row, load_merged_expanded, resolve_sources};
use crate::config::{Config, ContextKind};
use anyhow::{bail, Result};
use chrono::{DateTime, Local, Utc};

pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    if q.ids.is_empty() {
        bail!("no ID(s) specified (usage: `calman info <id>` or `calman <id> info`)");
    }
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    let sources = resolve_sources(conf, override_, ContextKind::Cli)?;
    let rows = load_merged_expanded(conf, &sources)?;

    for id in &q.ids {
        let row = find_row(&rows, id)?;
        print_row(row);
        println!();
    }
    Ok(())
}

/// Locate a row by numeric ID (plain or `id.n` occurrence) or UID.
fn find_row<'a>(rows: &'a [Row], id: &str) -> Result<&'a Row> {
    if let Some((pid, pn)) = id.rsplit_once('.') {
        let parent_id: usize = pid.parse().map_err(|_| {
            anyhow::anyhow!("bad occurrence ID `{id}` (expected `<id>.<n>`)")
        })?;
        let n: usize = pn.parse().map_err(|_| {
            anyhow::anyhow!("bad occurrence number `{id}` (expected `<id>.<n>`)")
        })?;
        if n == 0 {
            bail!("occurrence numbers are 1-based: `{id}`");
        }
        let parent = rows
            .get(parent_id.checked_sub(1).unwrap_or(usize::MAX))
            .ok_or_else(|| anyhow::anyhow!("no task with ID `{parent_id}`"))?;
        if !parent.task.is_parent() {
            bail!("task `{parent_id}` is not a recurring parent");
        }
        rows.iter()
            .find(|r| {
                r.task.parent_uid.as_deref() == Some(parent.task.uid.as_str())
                    && r.occ == Some(n)
            })
            .ok_or_else(|| {
                anyhow::anyhow!("no occurrence `{n}` for recurring task `{parent_id}`")
            })
    } else if let Ok(n) = id.parse::<usize>() {
        rows.get(n.checked_sub(1).unwrap_or(usize::MAX))
            .ok_or_else(|| anyhow::anyhow!("no task with ID `{id}`"))
    } else {
        rows.iter()
            .find(|r| r.task.uid == *id)
            .ok_or_else(|| anyhow::anyhow!("no task with UID `{id}`"))
    }
}

fn fmt_dt(d: DateTime<Utc>) -> String {
    d.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string()
}

fn fmt_day(d: DateTime<Utc>) -> String {
    d.with_timezone(&Local).format("%Y-%m-%d").to_string()
}

fn status_txt(s: crate::model::TaskStatus) -> &'static str {
    match s {
        crate::model::TaskStatus::Pending => "pending",
        crate::model::TaskStatus::InProgress => "in-progress",
        crate::model::TaskStatus::Recurring => "recurring",
        crate::model::TaskStatus::Completed => "completed",
        crate::model::TaskStatus::Cancelled => "cancelled",
    }
}

fn print_row(r: &Row) {
    let t = &r.task;
    println!(
        "[{}] {}",
        if let Some(n) = r.occ {
            format!("{}.{}", r.id, n)
        } else {
            r.id.to_string()
        },
        t.summary
    );
    let field = |k: &str, v: String| {
        println!("    {k:<10}: {v}");
    };
    field("status", status_txt(t.status).to_string());
    field("source", r.source.clone());
    field(
        "type",
        if t.is_event() {
            if t.allday {
                "event (all-day)".into()
            } else {
                "event".into()
            }
        } else {
            "todo".into()
        },
    );
    if let Some(d) = t.due {
        if t.allday {
            field("due", format!("{} (all-day)", fmt_day(d)));
        } else {
            field("due", fmt_dt(d));
        }
    }
    if let (Some(s), Some(e)) = (t.dtstart, t.dtend) {
        field("from", format!("{} → {}", fmt_dt(s), fmt_dt(e)));
    } else if let Some(s) = t.dtstart {
        if t.allday {
            field("from", format!("{} (all-day)", fmt_day(s)));
        } else {
            field("from", fmt_dt(s));
        }
    }
    if let Some(x) = &t.rrule {
        let mut v = x.clone();
        if !t.exdates.is_empty() {
            let ex = t
                .exdates
                .iter()
                .map(|d| fmt_day(*d))
                .collect::<Vec<_>>()
                .join(", ");
            v.push_str(&format!("\n                  excluded: {ex}"));
        }
        field("recur", v);
    }
    if let Some(w) = t.wait {
        field("wait", format!("{w}s before date"));
    }
    if let Some(s) = t.started_at {
        field("started", fmt_dt(s));
    }
    if let Some(p) = t.priority {
        field("priority", p.to_string());
    }
    if !t.tags.is_empty() {
        field("tags", t.tags.join(", "));
    }
    if let Some(l) = &t.location {
        field("location", l.clone());
    }
    if let Some(d) = &t.description {
        // Indent continuation lines so multi-line descriptions align.
        let indented = d.replace('\n', "\n                    ");
        field("desc", indented);
    }
    if let Some(a) = t.alarm_before {
        field("alert", format!("{a}s before"));
    }
    if let Some(rel) = &t.related_to {
        field("related", rel.clone());
    }
    if let Some(pid) = &t.parent_uid {
        field("parent", pid.clone());
    }
    field("created", fmt_dt(t.created_at));
    field("updated", fmt_dt(t.updated_at));
    if let Some(c) = t.completed_at {
        field("completed", fmt_dt(c));
    }
    field("uid", t.uid.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Row;
    use crate::model::Task;

    #[test]
    fn fmt_day_uses_local() {
        let d = DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(fmt_day(d).starts_with("2026-09-01"));
    }

    #[test]
    fn find_row_plain_and_uid() {
        let t = Task::new("work", "x");
        let rows = vec![Row {
            id: 1,
            source: "work".into(),
            task: t.clone(),
            occ: None,
        }];
        assert_eq!(find_row(&rows, "1").unwrap().task.uid, t.uid);
        assert_eq!(find_row(&rows, &t.uid).unwrap().id, 1);
        assert!(find_row(&rows, "99").is_err());
    }
}