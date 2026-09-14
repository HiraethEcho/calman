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
            // drops any per-occurrence override for the same slot — unless the
            // user asks to delete this and ALL future occurrences, which
            // truncates the series before this occurrence instead.
            let master = st
                .list()
                .iter()
                .find(|t| t.uid == tgt.uid)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
            if !master.is_parent() {
                bail!("task `{}` is not a recurring parent", tgt.uid);
            }
            let occ_day = occ
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string();
            let all_future = q.apply_all_future
                || crate::cli::confirm(&format!(
                    "delete ALL FUTURE occurrences from {occ_day}?"
                ))?;
            if all_future {
                // Drop overrides at or after this occurrence.
                let ovs: Vec<String> = st
                    .list()
                    .iter()
                    .filter(|t| {
                        t.parent_uid.as_deref() == Some(tgt.uid.as_str())
                            && t.recurrence_id.is_some_and(|r| r >= occ)
                    })
                    .map(|t| t.uid.clone())
                    .collect();
                for u in ovs {
                    st.remove(&u)?;
                }
                let mut m = master.clone();
                let delete_master = crate::cli::series::truncate_before(&mut m, occ)?;
                if delete_master {
                    let all: Vec<String> = st
                        .list()
                        .iter()
                        .filter(|t| t.parent_uid.as_deref() == Some(tgt.uid.as_str()))
                        .map(|t| t.uid.clone())
                        .collect();
                    for u in all {
                        st.remove(&u)?;
                    }
                    st.remove(&tgt.uid)?
                        .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
                } else {
                    st.update(&tgt.uid, |t| {
                        t.rrule = m.rrule.clone();
                        Ok(())
                    })?
                    .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
                }
                continue;
            }
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
                // iOS 式：删掉一次实例后，母任务锚点同样滚动到下一次。
                // Roll the master anchor forward, like iOS does after delete.
                crate::cli::roll_master_to(t, occ);
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
