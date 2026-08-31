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
        && q.from.is_none()
        && q.to.is_none()
        && q.location.is_none()
        && q.repeat.is_none()
        && q.description.is_none()
        && q.span.is_none()
        && !q.allday
        && q.alert.is_none()
        && q.rel.is_none()
        && q.wait.is_none()
    {
        bail!("no changes specified");
    }

    let (from, from_allday) = if let Some(s) = &q.from {
        match parse_date_value(s)? {
            // Date-only `start:` → all-day event at local midnight.
            DateValue::Date(d) => (Some(local_midnight(d)), true),
            DateValue::Time(dt) => (Some(dt), false),
        }
    } else {
        (None, false)
    };
    let to = q.to.as_deref().map(parse_date_value).transpose()?;
    let dur = q.span.as_deref().map(parse_duration).transpose()?;
    if to.is_some() && dur.is_some() {
        bail!("use either `to:` or `for:`, not both");
    }
    if (to.is_some() || dur.is_some()) && q.from.is_none() && q.allday {
        bail!("`to:`/`for:` need a timed event; give `from:` too or drop allday");
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

    let upd = Upd {
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
        from,
        from_allday,
        to,
        span: dur,
        alert,
        related: related.clone(),
        default_duration,
        wait: q.wait.clone(),
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
            // Re-modifying the same occurrence updates its override instead of
            // stacking duplicate siblings.
            let existing = st.list().iter().find(|t| {
                t.parent_uid.as_deref() == Some(tgt.uid.as_str())
                    && t.recurrence_id == Some(occ)
            });
            if let Some(e) = existing {
                let uid = e.uid.clone();
                st.update(&uid, |t| apply(t, &upd))?
                    .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
            } else {
                let mut ov =
                    crate::cli::override_for_occurrence(&master, occ, TaskStatus::Pending);
                apply(&mut ov, &upd)?;
                st.add(ov)?;
            }
        } else {
            st.update(&tgt.uid, |t| apply(t, &upd))?
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
    from: Option<DateTime<Utc>>,
    from_allday: bool,
    to: Option<DateValue>,
    span: Option<Duration>,
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
        // Adding a recurrence promotes an active item to series master.
        if t.status.is_active() {
            t.status = crate::model::TaskStatus::Recurring;
        }
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

    if let Some(s) = u.from {
        // Adding `from:` converts the item to a VEVENT.
        t.event = true;
        // Date-only `from:` keeps the event all-day; date-time makes it timed.
        t.allday = u.from_allday;
        t.dtstart = Some(s);
        if u.from_allday {
            t.dtend = None; // drop any stale timed end; all-day end is implicit
        }
    }

    if let Some(s) = t.dtstart {
        if let Some(e) = &u.to {
            t.dtend = Some(resolve_end(s, t.allday, *e)?);
        } else if let Some(d) = u.span {
            t.dtend = Some(s + d);
        } else if u.from.is_some() && !t.allday && t.dtend.is_none() {
            // Newly timed event without explicit end → default duration (if any).
            if let Some(d) = u.default_duration {
                t.dtend = Some(s + d);
            }
        }
    } else if u.to.is_some() || u.span.is_some() {
        bail!("target has no start; use `from:` to make it an event first");
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
