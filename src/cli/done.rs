//! `done` 子命令：把任务标记为完成。
//! `done` subcommand handler.
//!
//! 数据流：解析目标 ID → 打开存储 → 标记完成/写覆盖记录 → 打印结果。
//!
//! 两种完成语义：
//! - 普通任务：直接设置状态 Completed。
//! - 重复系列的某次 occurrence：写一条 Completed 覆盖记录，系列本身继续。

use crate::args::ParsedArgs;
use crate::cli::{open_storage, resolve_source, resolve_targets_occ};
use crate::config::Config;
use crate::model::TaskStatus;
use crate::storage::Storage;
use anyhow::{Result, bail};

/// 执行 done。
///
/// Rust 概念：`Result<()>` 表示“成功时没有返回值，失败时带错误”；
/// 本函数内部用 `?` 把存储层错误直接向上传播。
pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    if q.ids.is_empty() {
        bail!("no ID(s) specified");
    }
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    for tgt in resolve_targets_occ(conf, override_, &q.ids, q.occ_date)? {
        let src = resolve_source(conf, &tgt.source)?;
        let mut st = open_storage(conf, &src)?;
        if let Some(occ) = tgt.occ_date {
            // 完成某一次 occurrence：不直接改主任务，而是写/更新一条覆盖记录。
            // Completing one occurrence records a Completed override sibling
            // (visible as a done item), updating an existing override if the
            // same occurrence was already modified.
            let master = st
                .list()
                .iter()
                .find(|t| t.uid == tgt.uid) // 找到重复系列父任务
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
            if !master.is_parent() {
                bail!("task `{}` is not a recurring parent", tgt.uid);
            }
            // 同一时点若已被 modify 成覆盖记录，这里直接把它改成 Completed，避免重复。
            let existing = st.list().iter().find(|t| {
                t.parent_uid.as_deref() == Some(tgt.uid.as_str())
                    && t.recurrence_id == Some(occ)
            });
            if let Some(e) = existing {
                let uid = e.uid.clone();
                // 闭包 `|t| { ... }`：存储层把任务的可变引用传进来，改完自动保存。
                st.update(&uid, |t| {
                    t.status = TaskStatus::Completed;
                    t.completed_at = Some(chrono::Utc::now());
                    Ok(())
                })?;
            } else {
                // 没有现成覆盖记录 → 新建一条 Completed 覆盖记录。
                let mut ov =
                    crate::cli::override_for_occurrence(&master, occ, TaskStatus::Completed);
                ov.completed_at = Some(chrono::Utc::now());
                st.add(ov)?;
            }
        } else {
            // 普通任务：闭包内直接改状态；`is_parent()` 时完成整个系列 = 取消它。
            st.update(&tgt.uid, |t| {
                if t.is_parent() {
                    // Completing a recurring series master stops the series.
                    // 完成整个重复系列的主任务 → 取消系列（不再生成新实例）。
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
