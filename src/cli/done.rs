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
            // iOS 式完成：写一个独立的 COMPLETED 副本（新 UID、无 RECURRENCE-ID、
            // PERCENT-COMPLETE:100、DTSTART/DUE=实例时间），并把母任务的锚点
            // 滚动到下一次 occurrence——与 iPhone 完全一致，使副本在
            // Reminders 里正确显示。旧式的 RECURRENCE-ID 覆盖仍会被更新为完成。
            // iOS-style completion: standalone COMPLETED copy + master roll.
            let master = st
                .list()
                .iter()
                .find(|t| t.uid == tgt.uid)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
            if !master.is_parent() {
                bail!("task `{}` is not a recurring parent", tgt.uid);
            }
            let existing = st.list().iter().find(|t| {
                t.parent_uid.as_deref() == Some(tgt.uid.as_str())
                    && t.recurrence_id == Some(occ)
            });
            if let Some(e) = existing {
                // 旧式覆盖：只改状态（保留 RECURRENCE-ID 形态，兼容历史数据）。
                // Legacy override: mark completed in place.
                let uid = e.uid.clone();
                st.update(&uid, |t| {
                    t.status = TaskStatus::Completed;
                    t.completed_at = Some(chrono::Utc::now());
                    t.percent_complete = Some(100);
                    Ok(())
                })?;
            } else {
                // 新式：独立完成副本（iOS 风格）。
                // New-style: standalone iOS-compatible completed copy.
                let copy = crate::cli::completed_occurrence_copy(&master, occ);
                st.add(copy)?;
            }
            // 滚动母任务锚点到下一次实例（早于锚点的历史完成不滚动）。
            // Roll the master anchor to the next occurrence.
            st.update(&tgt.uid, |t| {
                crate::cli::roll_master_to(t, occ);
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
