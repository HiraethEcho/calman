//! `start` subcommand — record when a todo started.
//!
//! `calman start <id>` sets `started_at` and promotes the status to
//! in-progress. `calman stop <id>` (see `stop.rs`) then turns the todo into a
//! timed event spanning [started_at, now].

use crate::args::ParsedArgs;
use crate::cli::{open_storage, resolve_source, resolve_targets};
use crate::config::Config;
use crate::model::TaskStatus;
use crate::storage::Storage;
use anyhow::{Result, bail};
use chrono::Utc;

pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    if q.ids.is_empty() {
        bail!("no ID(s) specified (usage: `calman start <id>`)");
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
        if t.is_event() {
            bail!("`{}` is an event, not a startable todo", t.summary);
        }
        if t.is_parent() {
            bail!("`{}` is a recurring series master — start its occurrences instead", t.summary);
        }
        if t.status.is_done() {
            bail!("`{}` is already completed", t.summary);
        }
        if t.started_at.is_some() {
            bail!("`{}` is already started", t.summary);
        }
        st.update(&uid, |task| {
            task.started_at = Some(Utc::now());
            task.status = TaskStatus::InProgress;
            Ok(())
        })?
        .ok_or_else(|| anyhow::anyhow!("task `{uid}` disappeared"))?;
    }
    println!("started: {}", q.ids.join(", "));
    Ok(())
}