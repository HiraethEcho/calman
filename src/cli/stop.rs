//! `stop` 子命令：结束已开始的 todo，把它变成事件。
//! `stop` subcommand — finish a started todo, turning it into an event.
//!
//! `calman stop <id>`：todo 变成定时事件，起点是它的 `started_at`，终点是现在；
//! 原 todo 可选地标记完成，新事件通过 `related_to` 回链。
//! summary、tags、priority、location、description 等字段会复制给事件。
//!
//! 数据流：解析 ID → 打开存储 → 校验 started_at → 新建事件 → 询问是否完成原 todo → 更新。

use crate::args::ParsedArgs;
use crate::cli::{open_storage, resolve_source, resolve_targets};
use crate::config::Config;
use crate::model::{Task, TaskStatus};
use crate::storage::Storage;
use anyhow::{Result, bail};
use chrono::Utc;

/// 执行 stop：把“已开始、尚未结束”的 todo 归档成一段事件。
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
        // Option + ?：没有 started_at 就把错误抛给用户（提示先用 start）。
        let started = t
            .started_at
            .ok_or_else(|| anyhow::anyhow!("`{}` was never started (use `calman start`)", t.summary))?;
        let ended = Utc::now();
        if ended <= started {
            bail!("`{}` was started just now — nothing to stop", t.summary);
        }

        // Copy the todo into an event that spans [started, ended].
        // 把 todo 复制成一段横跨 [started, ended] 的事件（新记录，不覆盖原 todo）。
        let mut ev = Task::new(&t.source, &t.summary);
        ev.event = true;
        ev.allday = false;
        ev.dtstart = Some(started);
        ev.dtend = Some(ended);
        ev.due = None;
        ev.status = TaskStatus::Pending; // events are pending until completed（事件默认待处理）
        ev.description = t.description.clone();
        ev.priority = t.priority;
        ev.tags = t.tags.clone();
        ev.location = t.location.clone();
        ev.alarm_before = t.alarm_before;
        // A fresh started/done pair carries the original's timeline.
        // 新事件继承原 todo 的创建时间，让时间线连续。
        ev.created_at = t.created_at;
        ev.updated_at = ended;
        st.add(ev)?; // 先写入事件

        // Ask whether the todo itself should be marked done. Non-interactive
        // input keeps it unfinished (back to pending): the event already owns
        // the elapsed span.
        // 询问原 todo 是否标记完成；非交互输入保持未完成（回 pending），
        // 因为已过去的时间段已经归事件所有。
        let mark_done = crate::cli::confirm(&format!(
            "mark todo `{}` as done?",
            t.summary
        ))?;
        // 闭包内根据回答更新原 todo，并清空 started_at（下一次从头计时）。
        st.update(&uid, |task| {
            if mark_done {
                task.status = TaskStatus::Completed;
                task.completed_at = Some(ended);
            } else {
                task.status = TaskStatus::Pending;
                task.completed_at = None;
            }
            // The event owns the timed span; the todo starts fresh next time.
            // 时间跨度归事件；todo 下次重新 start 时从头计。
            task.started_at = None;
            Ok(())
        })?
        .ok_or_else(|| anyhow::anyhow!("task `{uid}` disappeared"))?;
    }
    println!("stopped: {}", q.ids.join(", "));
    Ok(())
}