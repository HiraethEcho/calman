//! `delete` subcommand handler.

use crate::args::ParsedArgs;
use crate::cli::{open_storage, resolve_source, resolve_targets_occ};
use crate::config::Config;
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
            // Deleting one occurrence excludes it from the series (EXDATE) and
            // drops any per-occurrence override for the same slot.
            let orphan: Vec<String> = st
                .list()
                .iter()
                .filter(|t| {
                    t.parent_uid.as_deref() == Some(tgt.uid.as_str())
                        && t.recurrence_id == Some(occ)
                })
                .map(|t| t.uid.clone())
                .collect();
            for uid in orphan {
                st.remove(&uid)?;
            }
            st.update(&tgt.uid, |t| {
                if !t.exdates.contains(&occ) {
                    t.exdates.push(occ);
                }
                Ok(())
            })?
            .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
        } else {
            st.remove(&tgt.uid)?
                .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
        }
    }
    println!("deleted: {}", q.ids.join(", "));
    Ok(())
}
