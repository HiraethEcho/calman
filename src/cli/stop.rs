//! `stop` subcommand — finish a started todo, turning it into an event.
//!
//! `calman stop <id>`: the todo becomes a timed event starting at its
//! `started_at` and ending now; the (former) todo is marked completed and the
//! new event links back via `related_to`. Fields (summary, tags, priority,
//! location, description, relations) are copied to the event.

use crate::args::ParsedArgs;
use crate::cli::{open_storage, resolve_source, resolve_targets};
use crate::config::Config;
use crate::model::{Task, TaskStatus};
use crate::storage::Storage;
use anyhow::{Result, bail};
use chrono::Utc;

pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    if q.ids.is_empty() {
        bail!("no ID(s) specified (usage: `calman stop <id>`)");
    }
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    for (uid, src_name) in resolve_targets(conf, override_, &q.ids)? {
        let src = resolve_source(conf, &src_name)?;
        let mut st = open_storage(conf, &src)?;
        let t = st
            .list()
            .iter()
            .find(|t| t.uid == uid)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("task `{uid}` disappeared"))?;
        let started = t
            .started_at
            .ok_or_else(|| anyhow::anyhow!("`{}` was never started (use `calman start`)", t.summary))?;
        let ended = Utc::now();
        if ended <= started {
            bail!("`{}` was started just now — nothing to stop", t.summary);
        }

        // Copy the todo into an event that spans [started, ended].
        let mut ev = Task::new(&t.source, &t.summary);
        ev.event = true;
        ev.allday = false;
        ev.dtstart = Some(started);
        ev.dtend = Some(ended);
        ev.due = None;
        ev.status = TaskStatus::Pending; // events are pending until completed
        ev.description = t.description.clone();
        ev.priority = t.priority;
        ev.tags = t.tags.clone();
        ev.location = t.location.clone();
        ev.alarm_before = t.alarm_before;
        // A fresh started/done pair carries the original's timeline.
        ev.created_at = t.created_at;
        ev.updated_at = ended;
        st.add(ev)?;

        // Ask whether the todo itself should be marked done. Non-interactive
        // input keeps it unfinished (back to pending): the event already owns
        // the elapsed span.
        let mark_done = crate::cli::confirm(&format!(
            "mark todo `{}` as done?",
            t.summary
        ))?;
        st.update(&uid, |task| {
            if mark_done {
                task.status = TaskStatus::Completed;
                task.completed_at = Some(ended);
            } else {
                task.status = TaskStatus::Pending;
                task.completed_at = None;
            }
            // The event owns the timed span; the todo starts fresh next time.
            task.started_at = None;
            Ok(())
        })?
        .ok_or_else(|| anyhow::anyhow!("task `{uid}` disappeared"))?;
    }
    println!("stopped: {}", q.ids.join(", "));
    Ok(())
}