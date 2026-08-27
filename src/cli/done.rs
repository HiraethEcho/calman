//! `done` subcommand handler.

use crate::args::ParsedArgs;
use crate::cli::{open_storage, resolve_targets};
use crate::config::Config;
use crate::model::TaskStatus;
use crate::storage::Storage;
use anyhow::{Result, bail};

pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    if q.ids.is_empty() {
        bail!("no ID(s) specified");
    }
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    for (uid, source) in resolve_targets(conf, override_, &q.ids)? {
        let src = conf
            .source(&source)
            .ok_or_else(|| anyhow::anyhow!("unknown source `{source}`"))?;
        let mut st = open_storage(src)?;
        st.update(&uid, |t| {
            t.status = TaskStatus::Completed;
            t.completed_at = Some(chrono::Utc::now());
            Ok(())
        })?
        .ok_or_else(|| anyhow::anyhow!("task `{uid}` disappeared"))?;
    }
    println!("done: {}", q.ids.join(", "));
    Ok(())
}
