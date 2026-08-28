//! `delete` subcommand handler.

use crate::args::ParsedArgs;
use crate::cli::{open_storage, resolve_targets};
use crate::config::Config;
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
        let mut st = open_storage(conf, src)?;
        st.remove(&uid)?
            .ok_or_else(|| anyhow::anyhow!("task `{uid}` disappeared"))?;
    }
    println!("deleted: {}", q.ids.join(", "));
    Ok(())
}
