//! `add` subcommand handler.
//!
//! Todo: `calman add <text> due:<date> ...`
//! Event: `calman add <text> from:<date> [to:<date> | for:<dur>] ...`
//! Date-only `from:` forms (`20260812`, `0826`, `17`, `today`) → all-day event.

use crate::args::ParsedArgs;
use crate::cli::open_storage;
use crate::config::Config;
use crate::date::{
    DateValue, local_midnight, parse_date_value, parse_duration, resolve_end,
};
use crate::model::{Task, TaskStatus};
use crate::source::resolve_source_name;
use crate::storage::Storage;
use anyhow::{Context, Result, bail};
use chrono::Local;

pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    if q.text.is_empty() {
        bail!("task summary required");
    }

    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    let single = match override_ {
        Some(ns) if ns.len() == 1 => ns[0].clone(),
        Some(_) => bail!("`add` accepts exactly one source (e.g. source:work)"),
        None => conf.write_source().to_string(),
    };
    // Resolve through `resolve_source_name` so `IcsDir` collection refs
    // (e.g. `remote/sorge`) map to a concrete virtual source.
    let resolved = resolve_source_name(&conf.sources, &single)
        .with_context(|| format!("resolve source `{single}`"))?;
    let src = match resolved.len() {
        1 => resolved.into_iter().next().unwrap(),
        0 => bail!("unknown write source `{single}`"),
        _ => bail!(
            "`add` to an IcsDir source needs a specific collection, e.g. source:{}/<collection>",
            single
        ),
    };

    let mut task = Task::new(&src.name, q.text.clone());
    task.priority = q.priority;
    task.tags = q.tags.clone();
    task.description = q.description.clone();
    task.location = q.location.clone();
    task.rrule = q
        .repeat
        .as_deref()
        .map(crate::recurrence::normalize_recurrence)
        .transpose()?;
    if let Some(st) = q.status {
        task.status = st;
    }
    if task.rrule.is_some() && task.status.is_active() {
        task.status = TaskStatus::Recurring;
    }
    if let Some(rel) = &q.rel {
        let targets = crate::cli::resolve_targets(conf, None, std::slice::from_ref(rel))?;
        task.related_to = targets.first().map(|(uid, _)| uid.clone());
    }

    if let Some(start_str) = &q.from {
        if q.due.is_some() {
            bail!("use either `from:` (event) or `due:` (todo), not both");
        }
        task.event = true;
        let start = parse_date_value(start_str)?;
        task.allday = q.allday || matches!(start, DateValue::Date(_));
        task.dtstart = Some(match start {
            DateValue::Date(d) => local_midnight(d),
            DateValue::Time(dt) => dt,
        });
        if q.allday {
            let d = task.dtstart.unwrap().with_timezone(&Local).date_naive();
            task.dtstart = Some(local_midnight(d));
        }

        let dur = q.span.as_deref().map(parse_duration).transpose()?;
        if q.to.is_some() && dur.is_some() {
            bail!("use either `to:` or `for:`, not both");
        }
        if let Some(end_str) = &q.to {
            let end = parse_date_value(end_str)?;
            task.dtend = Some(resolve_end(task.dtstart.unwrap(), task.allday, end)?);
        } else if let Some(d) = dur {
            task.dtend = Some(task.dtstart.unwrap() + d);
        } else if !task.allday {
            let def = conf.date.default_event_duration.trim();
            if !def.is_empty() {
                task.dtend = Some(task.dtstart.unwrap() + parse_duration(def)?);
            }
        }

        if !task.allday
            && let (Some(s), Some(e)) = (task.dtstart, task.dtend)
            && e < s
        {
            bail!("to must be after from");
        }
    } else {
        if q.to.is_some() {
            bail!("`to:` requires `from:` (use an event)");
        }
        if q.span.is_some() {
            bail!("`for:` requires `from:` (use an event)");
        }
        if q.allday {
            bail!("`allday` requires `from:` (use an event)");
        }
        if let Some(d) = &q.due {
            task.due = Some(*d);
            // Date-only `due` → all-day todo (ICS DUE;VALUE=DATE).
            task.allday = task.allday || q.due_allday;
        }
    }

    if let Some(a) = &q.alert {
        let lead = parse_duration(a)?;
        let secs = lead.num_seconds();
        if secs <= 0 {
            bail!("alert must be positive, got `{a}`");
        }
        task.alarm_before = Some(secs);
    }

    if let Some(w) = &q.wait {
        let anchor = task
            .due
            .or(task.dtstart)
            .ok_or_else(|| anyhow::anyhow!("wait needs a date anchor: give `due:` (todo) or `start:` (event)"))?;
        task.wait = Some(crate::args::resolve_wait(w, anchor)?);
    }

    let mut st = open_storage(conf, &src)?;
    st.add(task)?;
    if q.from.is_some() {
        println!("added event to `{single}`");
    } else {
        println!("added task to `{single}`");
    }
    Ok(())
}
