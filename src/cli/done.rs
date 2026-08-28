//! `done` subcommand handler.

use crate::args::ParsedArgs;
use crate::cli::{open_storage, resolve_source, resolve_targets_occ};
use crate::config::Config;
use crate::model::TaskStatus;
use crate::storage::Storage;
use anyhow::{Result, bail};

pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    if q.ids.is_empty() {
        bail!("no ID(s) specified");
    }
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    for tgt in resolve_targets_occ(conf, override_, &q.ids, q.occ_date)? {
        let src = resolve_source(conf, &tgt.source)?;
        let mut st = open_storage(conf, &src)?;
        if let Some(occ) = tgt.occ_date {
            // Completing one occurrence excludes it from the series (EXDATE).
            st.update(&tgt.uid, |t| {
                if !t.exdates.contains(&occ) {
                    t.exdates.push(occ);
                }
                Ok(())
            })?
            .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
        } else {
            st.update(&tgt.uid, |t| {
                if t.is_parent() {
                    // Completing a recurring series master stops the series.
                    t.status = TaskStatus::Cancelled;
                } else {
                    t.status = TaskStatus::Completed;
                    t.completed_at = Some(chrono::Utc::now());
                }
                Ok(())
            })?
            .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
        }
    }
    println!("done: {}", q.ids.join(", "));
    Ok(())
}
