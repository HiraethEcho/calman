//! `delete` 子命令：硬删除任务或重复系列中的某次 occurrence。
//! `delete` subcommand handler.
//!
//! 数据流：解析目标 ID → 解析 source → 打开存储 → 删除/截断 → 打印结果。
//!
//! 两种删除语义：
//! - 普通任务：直接从存储移除（hard delete）。
//! - 单次 occurrence：默认加入 EXDATE（跳过这一次），`all-future` 则截断系列。

use crate::args::ParsedArgs;
use crate::cli::{open_storage, resolve_source, resolve_targets_occ};
use crate::config::Config;
use crate::storage::Storage;
use anyhow::{Result, bail};

/// 执行 delete。`resolve_targets_occ` 把 `id`、`id.n`、`on:<date>` 统一成目标列表。
pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    if q.ids.is_empty() {
        bail!("no ID(s) specified");
    }
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    // 每个目标可能位于不同 source，所以逐个解析、逐个打开对应存储。
    for tgt in resolve_targets_occ(conf, override_, &q.ids, q.occ_date)? {
        let src = resolve_source(conf, &tgt.source)?;
        let mut st = open_storage(conf, &src)?;
        if let Some(occ) = tgt.occ_date {
            // 有 occ_date → 删除的是“某一次实例”，不是整个系列。
            // 默认语义：把这次加进 EXDATE（跳过它），并清除同一时点的覆盖记录；
            // 若用户选择 all-future，则改为从此处截断系列。
            // Deleting one occurrence excludes it from the series (EXDATE) and
            // drops any per-occurrence override for the same slot — unless the
            // user asks to delete this and ALL future occurrences, which
            // truncates the series before this occurrence instead.
            let master = st
                .list()
                .iter()
                .find(|t| t.uid == tgt.uid) // 先找到父系列本身
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
            if !master.is_parent() {
                bail!("task `{}` is not a recurring parent", tgt.uid);
            }
            // 把 UTC 时间转成当地日期时间，用于提示语。
            let occ_day = occ
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string();
            // `all-future` 关键字直接选“全部未来”；否则 TTY 下询问，脚本默认只删这一次。
            let all_future = q.apply_all_future
                || crate::cli::confirm(&format!(
                    "delete ALL FUTURE occurrences from {occ_day}?"
                ))?;
            if all_future {
                // 先删除位于此次或之后的覆盖记录（它们不能残留在截断后的系列上）。
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
                // truncate_before：把主任务改成“只保留 occ 之前”的系列。
                // 返回 true 表示连第一次都没有了，应当整条删除。
                let delete_master = crate::cli::series::truncate_before(&mut m, occ)?;
                if delete_master {
                    // 整个系列消失：先删所有覆盖记录，再删主任务。
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
                    // 只用新的 RRULE 更新主任务：闭包 |t| 拿到可修改引用，在此改字段。
                    // 这样“读取→修改→写回”在存储层原子完成。
                    st.update(&tgt.uid, |t| {
                        t.rrule = m.rrule.clone();
                        Ok(())
                    })?
                    .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
                }
                continue;
            }
            // 单次删除：丢掉恰好命中这一时点的覆盖记录（否则会互相冲突）。
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
            // EXDATE 是排除日期列表：把 occ 加进去后，展开时就不会生成这一次。
            st.update(&tgt.uid, |t| {
                if !t.exdates.contains(&occ) {
                    t.exdates.push(occ);
                }
                Ok(())
            })?
            .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
        } else {
            // 没有 occ_date：整个任务硬删除。
            st.remove(&tgt.uid)?
                .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
        }
    }
    println!("deleted: {}", q.ids.join(", "));
    Ok(())
}
