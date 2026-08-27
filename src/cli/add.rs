//! `add` subcommand handler.
//!
//! Todo: `calman add <text> due:<date> ...`
//! Event: `calman add <text> start:<date> [end:<date> | duration:<dur>] ...`
//! Date-only `start:` forms (`20260812`, `0826`, `17`, `today`) → all-day event.

use crate::args::ParsedArgs;
use crate::cli::open_storage;
use crate::config::Config;
use crate::date_parser::{
    DateValue, local_midnight, parse_date_value, parse_duration, resolve_end,
};
use crate::model::Task;
use crate::storage::Storage;
use anyhow::{Result, bail};
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
    let src = conf
        .source(&single)
        .ok_or_else(|| anyhow::anyhow!("unknown write source `{single}`"))?;

    let mut task = Task::new(&src.name, q.text.clone());
    task.priority = q.priority;
    task.tags = q.tags.clone();
    task.description = q.description.clone();
    task.location = q.location.clone();
    task.rrule = q.repeat.clone();
    if let Some(rel) = &q.rel {
        let targets = crate::cli::resolve_targets(conf, None, std::slice::from_ref(rel))?;
        task.related_to = targets.first().map(|(uid, _)| uid.clone());
    }

    if let Some(start_str) = &q.start {
        if q.due.is_some() {
            bail!("use either `start:` (event) or `due:` (todo), not both");
        }
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

        let dur = q.duration.as_deref().map(parse_duration).transpose()?;
        if q.end.is_some() && dur.is_some() {
            bail!("use either `end:` or `duration:`, not both");
        }
        if let Some(end_str) = &q.end {
            let end = parse_date_value(end_str)?;
            task.dtend = Some(resolve_end(task.dtstart.unwrap(), task.allday, end)?);
        } else if let Some(d) = dur {
            task.dtend = Some(task.dtstart.unwrap() + d);
        } else if !task.allday {
            task.dtend =
                Some(task.dtstart.unwrap() + parse_duration(&conf.date.default_event_duration)?);
        }

        if !task.allday
            && let (Some(s), Some(e)) = (task.dtstart, task.dtend)
            && e < s
        {
            bail!("end must be after start");
        }
    } else {
        if q.end.is_some() {
            bail!("`end:` requires `start:` (use an event)");
        }
        if q.duration.is_some() {
            bail!("`duration:` requires `start:` (use an event)");
        }
        if q.allday {
            bail!("`allday` requires `start:` (use an event)");
        }
        if let Some(d) = &q.due {
            task.due = Some(*d);
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

    let mut st = open_storage(src)?;
    st.add(task)?;
    if q.start.is_some() {
        println!("added event to `{single}`");
    } else {
        println!("added task to `{single}`");
    }
    Ok(())
}
