//! `start` 子命令：记录 todo 的开始时间。
//! `start` subcommand — record when a todo started.
//!
//! `calman start <id>` 设置 `started_at`，并把状态升级为 in-progress。
//! `calman stop <id>`（见 `stop.rs`）随后把这个 todo 转成横跨 [started_at, now] 的事件。
//!
//! 数据流：解析 ID → 打开存储 → 校验状态 → 闭包更新 started_at/status → 打印。

use crate::args::ParsedArgs;
use crate::cli::{open_storage, resolve_source, resolve_targets};
use crate::config::Config;
use crate::model::TaskStatus;
use crate::storage::Storage;
use anyhow::{Result, bail};
use chrono::Utc;

/// 执行 start：只能启动未开始、未完成、非事件的普通 todo。
pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    if q.ids.is_empty() {
        bail!("no ID(s) specified (usage: `calman start <id>`)");
    }
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    // resolve_targets 返回 (uid, source) 对；每个目标可能在不同源。
    for (uid, src_name) in resolve_targets(conf, override_, &q.ids)? {
        let src = resolve_source(conf, &src_name)?;
        let mut st = open_storage(conf, &src)?;
        let t = st
            .list()
            .iter()
            .find(|t| t.uid == uid) // 先读出当前任务，用于下面的校验
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("task `{uid}` disappeared"))?;
        // 一连串 bail：事件/系列父任务/已完成/已启动都不允许再 start。
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
        // 闭包 `|task| ...`：拿到 &mut Task，写入开始时间和状态。
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