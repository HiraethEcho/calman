//! `modify` subcommand handler.
//!
//! Rules: bare words replace summary. `+allday` converts to all-day (clears
//! times). `start:` sets the event start: a date-only `start:` makes the event
//! all-day, a date-time `start:` makes it timed. `end:`/`duration:` optional.

use crate::args::ParsedArgs;
use crate::cli::{open_storage, resolve_source, resolve_targets_occ};
use crate::config::Config;
use crate::date::{
    DateValue, local_midnight, parse_date_value, parse_duration, resolve_end,
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
        && q.rel.is_none()
        && q.wait.is_none()
    {
        bail!("no changes specified");
    }

    let (start, start_allday) = if let Some(s) = &q.start {
        match parse_date_value(s)? {
            // Date-only `start:` → all-day event at local midnight.
            DateValue::Date(d) => (Some(local_midnight(d)), true),
            DateValue::Time(dt) => (Some(dt), false),
        }
    } else {
        (None, false)
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

    let default_duration = {
        let def = conf.date.default_event_duration.trim();
        if def.is_empty() {
            None
        } else {
            Some(parse_duration(def)?)
        }
    };
    let related = match &q.rel {
        Some(rel) => {
            let targets = crate::cli::resolve_targets(conf, None, std::slice::from_ref(rel))?;
            targets.first().map(|(uid, _)| uid.clone())
        }
        None => None,
    };

    for tgt in resolve_targets_occ(conf, override_, &q.ids, q.occ_date)? {
        let src = resolve_source(conf, &tgt.source)?;
        let mut st = open_storage(conf, &src)?;

        if let Some(occ) = tgt.occ_date {
            // Per-occurrence modify: create a RECURRENCE-ID override sibling.
            let master = st
                .list()
                .iter()
                .find(|t| t.uid == tgt.uid)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
            if !master.is_parent() {
                bail!("task `{}` is not a recurring parent", tgt.uid);
            }
            let mut ov =
                crate::cli::override_for_occurrence(&master, occ, TaskStatus::Pending);
            apply(
                &mut ov,
                &Upd {
                    text: q.text.clone(),
                    priority: q.priority,
                    due: q.due,
                    due_allday: q.due_allday,
                    status: q.status,
                    tags: q.tags.clone(),
                    anti_tags: q.anti_tags.clone(),
                    location: q.location.clone(),
                    repeat: q.repeat.clone(),
                    description: q.description.clone(),
                    allday: q.allday,
                    start,
                    start_allday,
                    end,
                    duration: dur,
                    alert,
                    related: related.clone(),
                    default_duration,
                    wait: q.wait.clone(),
                },
            )?;
            st.add(ov)?;
        } else {
            st.update(&tgt.uid, |t| {
                apply(
                    t,
                    &Upd {
                        text: q.text.clone(),
                        priority: q.priority,
                        due: q.due,
                        due_allday: q.due_allday,
                        status: q.status,
                        tags: q.tags.clone(),
                        anti_tags: q.anti_tags.clone(),
                        location: q.location.clone(),
                        repeat: q.repeat.clone(),
                        description: q.description.clone(),
                        allday: q.allday,
                        start,
                        start_allday,
                        end,
                        duration: dur,
                        alert,
                        related: related.clone(),
                        default_duration,
                        wait: q.wait.clone(),
                    },
                )
            })?
            .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
        }
    }
    println!("modified: {}", q.ids.join(", "));
    Ok(())
}

struct Upd {
    text: String,
    priority: Option<u8>,
    due: Option<DateTime<Utc>>,
    due_allday: bool,
    status: Option<TaskStatus>,
    tags: Vec<String>,
    anti_tags: Vec<String>,
    location: Option<String>,
    repeat: Option<String>,
    description: Option<String>,
    allday: bool,
    start: Option<DateTime<Utc>>,
    start_allday: bool,
    end: Option<DateValue>,
    duration: Option<Duration>,
    alert: Option<i64>,
    related: Option<String>,
    default_duration: Option<Duration>,
    wait: Option<String>,
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
        if !t.is_event() {
            t.allday = u.due_allday;
        }
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
        t.rrule = Some(crate::recurrence::normalize_recurrence(v)?);
    }
    if let Some(v) = &u.description {
        t.description = Some(v.clone());
    }
    if let Some(r) = &u.related {
        t.related_to = Some(r.clone());
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
        // Adding `start:` converts the item to a VEVENT.
        t.event = true;
        // Date-only `start:` keeps the event all-day; date-time makes it timed.
        t.allday = u.start_allday;
        t.dtstart = Some(s);
        if u.start_allday {
            t.dtend = None; // drop any stale timed end; all-day end is implicit
        }
    }

    if let Some(s) = t.dtstart {
        if let Some(e) = &u.end {
            t.dtend = Some(resolve_end(s, t.allday, *e)?);
        } else if let Some(d) = u.duration {
            t.dtend = Some(s + d);
        } else if u.start.is_some() && !t.allday && t.dtend.is_none() {
            // Newly timed event without explicit end → default duration (if any).
            if let Some(d) = u.default_duration {
                t.dtend = Some(s + d);
            }
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

    if let Some(w) = &u.wait {
        let anchor = t
            .due
            .or(t.dtstart)
            .ok_or_else(|| anyhow::anyhow!("wait needs a date anchor: give `due:` (todo) or `start:` (event)"))?;
        t.wait = Some(crate::args::resolve_wait(w, anchor)?);
    }
    Ok(())
}
