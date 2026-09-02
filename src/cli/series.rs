//! 重复系列拆分辅助函数（用于按 occurrence 修改/删除）。
//! Series-splitting helpers for per-occurrence `modify`/`delete`.
//!
//! `id.n` 的序号是相对“现在”数的（1 = 下一个即将到来），
//! 所以要截断系列，必须先把 occurrence 换算成它自己序列里的绝对下标。
//! 绝对下标 = 该 occurrence 之前存活的展开实例数 + 之前被 EXDATE 排除的空位，
//! 因为 EXDATE 会占用 COUNT 的序号。
//! Occurrence addressing (`id.n`) numbers instances relative to *now* (`1` =
//! next upcoming), so truncating a series needs the absolute 1-based index of
//! the occurrence within its own recurrence. That index is computed from the
//! expanded survivors before the occurrence + the EXDATE'd slots before it
//! (EXDATEs shift the COUNT sequence, so they must be added back).

use crate::model::Task;
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};

/// 1-based absolute index of `occ` in `master`'s series (counting excluded
/// slots), or `None` when the task is not recurring / `occ` predates DTSTART.
/// `occ` 在 `master` 系列里的 1-based 绝对下标（把被排除的空位也算进去）；
/// 非重复任务或 `occ` 早于 DTSTART 时返回 `None`。
/// 1-based absolute index of `occ` in `master`'s series (counting excluded
/// slots), or `None` when the task is not recurring / `occ` predates DTSTART.
pub fn absolute_index(master: &Task, occ: DateTime<Utc>) -> Option<usize> {
    #[cfg(feature = "recur-expand")]
    {
        // 系列从 DTSTART 开始；occ 不在其后则不是有效实例。
        let dtstart = master.dtstart.or(master.due)?;
        if occ < dtstart || master.rrule.is_none() {
            return None;
        }
        // 展开 [dtstart, occ) 区间里“仍然存活”的实例数。
        let survived_before = crate::recur_expand::expand_task(
            master,
            dtstart - chrono::Duration::seconds(1),
            occ - chrono::Duration::seconds(1),
        )
        .len();
        // 再补上被 EXDATE 排除但占序号的空位。
        let ex_before = master
            .exdates
            .iter()
            .filter(|e| **e < occ)
            .count();
        Some(survived_before + ex_before + 1)
    }
    #[cfg(not(feature = "recur-expand"))]
    {
        let _ = (master, occ);
        None
    }
}

/// Replace an RRULE's `COUNT` (dropping a conflicting `UNTIL`), so the series
/// keeps exactly `count` occurrences from its DTSTART.
/// 把 RRULE 里的 `COUNT` 换成新值（同时去掉冲突的 `UNTIL`），
/// 这样系列从 DTSTART 起正好还剩 `count` 次。
/// Replace an RRULE's `COUNT` (dropping a conflicting `UNTIL`), so the series
/// keeps exactly `count` occurrences from its DTSTART.
pub fn set_count(rrule: &str, count: usize) -> String {
    // split(';') 把 RRULE 拆成片段；filter 去掉 COUNT 和 UNTIL 两段。
    let mut parts: Vec<String> = rrule
        .split(';')
        .filter(|p| {
            !p.is_empty() && !p.starts_with("COUNT=") && !p.starts_with("UNTIL=")
        })
        .map(str::to_string)
        .collect();
    parts.push(format!("COUNT={count}"));
    parts.join(";")
}

/// Remaining number of occurrences `count` in an RRULE (original `COUNT=…`),
/// accounting for the `abs_idx`-th occurrence becoming the new series start.
/// 计算 RRULE 剩余次数：原 `COUNT=` 减去已过去的 `abs_idx - 1` 次。
/// Remaining number of occurrences `count` in an RRULE (original `COUNT=…`),
/// accounting for the `abs_idx`-th occurrence becoming the new series start.
pub fn remaining_count(rrule: &str, abs_idx: usize) -> Option<usize> {
    // find_map 找到第一个满足闭包的片段并转换；and_then 串联 Option。
    let total = rrule
        .split(';')
        .find_map(|p| p.strip_prefix("COUNT="))
        .and_then(|v| v.parse::<usize>().ok())?;
    Some(total.saturating_sub(abs_idx - 1))
}

/// Truncate `master` so its series ends *before* `occ` (the occurrence and
/// everything after it is removed). Returns `true` when the series has no
/// remaining occurrences and the master should be deleted outright.
/// 把 `master` 截断为“到 `occ` 之前结束”的系列（本次及其后全部移除）。
/// 返回 `true` 表示系列已无剩余实例，主任务应整体删除。
/// Truncate `master` so its series ends *before* `occ` (the occurrence and
/// everything after it is removed). Returns `true` when the series has no
/// remaining occurrences and the master should be deleted outright.
pub fn truncate_before(master: &mut Task, occ: DateTime<Utc>) -> Result<bool> {
    let Some(idx) = absolute_index(master, occ) else {
        bail!("`{}` is not a recurring occurrence of this task", occ);
    };
    // 第一次就是要截断的点 → 前面没有任何实例，直接整条删除。
    if idx == 1 {
        return Ok(true);
    }
    let rrule = master.rrule.as_deref().ok_or_else(|| {
        anyhow::anyhow!("task `{}` has no recurrence rule", master.uid)
    })?;
    // 否则把 COUNT 改成 idx-1，系列自然在 occ 前收尾。
    master.rrule = Some(set_count(rrule, idx - 1));
    Ok(false)
}

// 测试模块：`cargo test` 时编译。
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Task;
    use chrono::TimeZone;

    fn dt(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    #[test]
    fn set_count_replaces_and_drops_until() {
        assert_eq!(
            set_count("FREQ=WEEKLY;BYDAY=MO;COUNT=5", 3),
            "FREQ=WEEKLY;BYDAY=MO;COUNT=3"
        );
        assert_eq!(
            set_count("FREQ=WEEKLY;UNTIL=20261231T000000Z", 4),
            "FREQ=WEEKLY;COUNT=4"
        );
    }

    #[test]
    fn remaining_count_subtracts_prefix() {
        assert_eq!(remaining_count("FREQ=WEEKLY;COUNT=10", 4), Some(7));
        assert_eq!(remaining_count("FREQ=WEEKLY", 4), None);
    }

    #[cfg(feature = "recur-expand")]
    #[test]
    fn absolute_index_counts_exdate_slots() {
        let mut t = Task::new("work", "weekly");
        t.dtstart = Some(dt(2026, 9, 1, 9)); // Tuesday
        t.rrule = Some("FREQ=WEEKLY".into());
        t.exdates = vec![dt(2026, 9, 1, 9)];
        // 09-08 is the 2nd algorithm occurrence (09-01 excluded by EXDATE).
        assert_eq!(absolute_index(&t, dt(2026, 9, 8, 9)), Some(2));
        assert_eq!(absolute_index(&t, dt(2026, 9, 22, 9)), Some(4));
    }

    #[cfg(feature = "recur-expand")]
    #[test]
    fn truncate_before_keeps_prefix_and_detects_first() {
        let mut t = Task::new("work", "series");
        t.dtstart = Some(dt(2026, 9, 1, 9));
        t.rrule = Some("FREQ=WEEKLY;COUNT=5".into());
        let mut t2 = t.clone();
        assert!(!truncate_before(&mut t2, dt(2026, 9, 15, 9)).unwrap()); // 3rd occ
        assert_eq!(t2.rrule.as_deref(), Some("FREQ=WEEKLY;COUNT=2"));
        assert!(truncate_before(&mut t, dt(2026, 9, 1, 9)).unwrap()); // 1st occ
    }
}
