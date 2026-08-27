//! `modify` subcommand handler.
//!
//! Rules: bare words replace summary. `+allday` converts to all-day (clears
//! times). `start:` auto-converts to a timed (non-allday) event; date-only
//! `start:` uses `[date] default_start_time`.

use crate::args::ParsedArgs;
use crate::cli::open_storage;
use crate::config::Config;
use crate::date_parser::{
    DateValue, local_midnight, parse_date_value, parse_duration, resolve_end, time_on_date,
};
use crate::model::{Task, TaskStatus};
use crate::storage::Storage;
use anyhow::{Result, bail};
use chrono::{DateTime, Duration, Local, Utc};

pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    if q.ids.is_empty() {
        bail!("no ID(s) specified");
    }
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());

    if q.text.is_empty()
        && q.priority.is_none()
        && q.due.is_none()
        && q.status.is_none()
        && q.tags.is_empty()
        && q.anti_tags.is_empty()
        && q.start.is_none()
        && q.end.is_none()
        && q.location.is_none()
        && q.repeat.is_none()
        && q.description.is_none()
        && q.duration.is_none()
        && !q.allday
        && q.alert.is_none()
    {
        bail!("no changes specified");
    }

    let start = if let Some(s) = &q.start {
        match parse_date_value(s)? {
            DateValue::Date(d) => Some(time_on_date(d, &conf.date.default_start_time)?),
            DateValue::Time(dt) => Some(dt),
        }
    } else {
        None
    };
    let end = q.end.as_deref().map(parse_date_value).transpose()?;
    let dur = q.duration.as_deref().map(parse_duration).transpose()?;
    if end.is_some() && dur.is_some() {
        bail!("use either `end:` or `duration:`, not both");
    }
    if (end.is_some() || dur.is_some()) && q.start.is_none() && q.allday {
        bail!("`end:`/`duration:` need a timed event; give `start:` too or drop allday");
    }
    let alert = match &q.alert {
        Some(a) => {
            let lead = parse_duration(a)?;
            let secs = lead.num_seconds();
            if secs <= 0 {
                bail!("alert must be positive, got `{a}`");
            }
            Some(secs)
        }
        None => None,
    };

    let default_duration = parse_duration(&conf.date.default_event_duration)?;

    for (uid, source) in crate::cli::resolve_targets(conf, override_, &q.ids)? {
        let src = conf
            .source(&source)
            .ok_or_else(|| anyhow::anyhow!("unknown source `{source}`"))?;
        let mut st = open_storage(src)?;
        st.update(&uid, |t| {
            apply(
                t,
                &Upd {
                    text: q.text.clone(),
                    priority: q.priority,
                    due: q.due,
                    status: q.status,
                    tags: q.tags.clone(),
                    anti_tags: q.anti_tags.clone(),
                    location: q.location.clone(),
                    repeat: q.repeat.clone(),
                    description: q.description.clone(),
                    allday: q.allday,
                    start,
                    end,
                    duration: dur,
                    alert,
                    default_duration,
                },
            )
        })?
        .ok_or_else(|| anyhow::anyhow!("task `{uid}` disappeared"))?;
    }
    println!("modified: {}", q.ids.join(", "));
    Ok(())
}

struct Upd {
    text: String,
    priority: Option<u8>,
    due: Option<DateTime<Utc>>,
    status: Option<TaskStatus>,
    tags: Vec<String>,
    anti_tags: Vec<String>,
    location: Option<String>,
    repeat: Option<String>,
    description: Option<String>,
    allday: bool,
    start: Option<DateTime<Utc>>,
    end: Option<DateValue>,
    duration: Option<Duration>,
    alert: Option<i64>,
    default_duration: Duration,
}

fn apply(t: &mut Task, u: &Upd) -> Result<()> {
    if !u.text.is_empty() {
        t.summary = u.text.clone();
    }
    if let Some(p) = u.priority {
        t.priority = Some(p);
    }
    if let Some(d) = u.due {
        t.due = Some(d);
    }
    if let Some(s) = u.status {
        t.status = s;
        if s == TaskStatus::Completed {
            t.completed_at = Some(Utc::now());
        }
    }
    for tag in &u.tags {
        if !t.tags.iter().any(|x| x == tag) {
            t.tags.push(tag.clone());
        }
    }
    for anti in &u.anti_tags {
        t.tags.retain(|x| !x.eq_ignore_ascii_case(anti));
    }
    if let Some(v) = &u.location {
        t.location = Some(v.clone());
    }
    if let Some(v) = &u.repeat {
        t.rrule = Some(v.clone());
    }
    if let Some(v) = &u.description {
        t.description = Some(v.clone());
    }
    if let Some(secs) = u.alert {
        t.alarm_before = Some(secs);
    }

    if u.allday {
        // Convert to all-day: keep dates, drop times + DTEND.
        t.allday = true;
        t.dtstart = t
            .dtstart
            .map(|d| local_midnight(d.with_timezone(&Local).date_naive()));
        t.dtend = None;
    }

    if let Some(s) = u.start {
        // Any explicit `start:` makes the event timed.
        t.allday = false;
        t.dtstart = Some(s);
    }

    if let Some(s) = t.dtstart {
        if let Some(e) = &u.end {
            t.dtend = Some(resolve_end(s, t.allday, *e)?);
        } else if let Some(d) = u.duration {
            t.dtend = Some(s + d);
        } else if u.start.is_some() && !t.allday && t.dtend.is_none() {
            // Newly timed event without explicit end → default duration.
            t.dtend = Some(s + u.default_duration);
        }
    } else if u.end.is_some() || u.duration.is_some() {
        bail!("target has no start; use `start:` to make it an event first");
    }

    // Keep allday invariant: DTEND must stay after DTSTART (exclusive).
    if t.allday
        && let (Some(s), Some(e)) = (t.dtstart, t.dtend)
        && e <= s
    {
        t.dtend = None;
    }
    Ok(())
}
