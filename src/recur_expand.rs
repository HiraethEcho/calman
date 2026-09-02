//! # 使用 `rrule` crate 的可选重复展开（`recur-expand` feature）
//!
//! 中文说明：开启 `recur-expand` 特性时，本模块在内存中为重复主任务生成具体发生（occurrence），
//! 让 `list`/`next` 能输出“虚拟发生行”。
//! 注意：展开是只读的，**不写回存储**；对某次发生的修改（`done <id> on:<date>`、
//! `modify <id>.n`）通过 `Task.exdates`/`Task.recurrence_id` 字段实现，由既有 JSONL/ICS 后端序列化。
//!
//! English: optional recurrence expansion via the `rrule` crate.
//! When the `recur-expand` feature is enabled, this module provides
//! in-memory occurrence generation for recurring master tasks, so
//! `list`/`next` can emit virtual occurrence rows.
//!
//! **Important**: expansion is read-only — it never writes back to storage.
//! Per-occurrence mutations (`done <id> on:<date>`, `modify <id>.n`) happen via
//! `Task.exdates` / `Task.recurrence_id` fields; those are serialised normally
//! by the existing JSONL/ICS back-ends.

use chrono::{DateTime, TimeZone, Utc};
use rrule::{RRuleSet, Tz};
use std::cmp::Ordering;

use crate::model::Task;

/// 由重复主任务展开出的一个具体发生。
/// 中文：`master` 是整个系列的主任务，`occurrence_start` 是该次发生的原始开始时间，
/// `index` 是生成序列中的 1 基序号（1 = 下一个即将发生的实例）。
/// English: an expanded occurrence derived from a master recurring task.
#[derive(Debug, Clone)]
pub struct Occurrence {
    /// 所属的主/母任务。
    /// The parent/master task this occurrence belongs to.
    pub master: Task,
    /// 本次发生的原始 DTSTART。
    /// The original DTSTART of this specific occurrence.
    pub occurrence_start: DateTime<Utc>,
    /// 在生成的所有发生中的 1 基序号。
    /// 1-based index of this occurrence among all generated occurrences.
    pub index: usize,
}

/// 把重复主任务在 `[after, before)` 窗口内展开为有序发生列表，排除 `task.exdates` 中的日期。
/// 中文：返回按 `occurrence_start` 排序（未来升序在前，过去降序在后）的 `Vec<Occurrence>`。
/// 用 `Option<T>` 处理“非重复任务/缺 dtstart”的缺省情形，返回空 Vec 而不是报错。
/// English: expand a master recurring task into ordered occurrences within the
/// given `[after, before)` window, excluding dates in `task.exdates`.
/// Returns occurrences sorted by `occurrence_start`.
pub fn expand_task(task: &Task, after: DateTime<Utc>, before: DateTime<Utc>) -> Vec<Occurrence> {
    // 非重复任务：没有 `rrule` 字段 → 直接返回空列表。
    // `let Some(...) = ... else` 是“取出 Option 内容，否则提前返回”的简洁写法。
    let Some(rrule_str) = &task.rrule else {
        return Vec::new();
    };
    // 用 `dtstart` 作为系列起点，事件没有 dtstart 时退回 `due`（todo 场景）。
    let Some(dtstart) = task.dtstart.or(task.due) else {
        return Vec::new();
    };

    // 把 DTSTART + RRULE + EXDATE 拼成 rrule crate 的 RRuleSet 文本格式。
    // `format!` 按 `%Y%m%dT%H%M%SZ`（UTC）生成 ICS 风格时间戳。
    let mut s = format!(
        "DTSTART:{}\nRRULE:{}",
        dtstart.format("%Y%m%dT%H%M%SZ"),
        rrule_str
    );
    // 有排除日期（exdates）时追加 EXDATE 行；
    // 这里用迭代器 `.map().collect()` 把 Vec<DateTime> 转成 Vec<String> 再 join。
    if !task.exdates.is_empty() {
        let ex: Vec<String> = task
            .exdates
            .iter()
            .map(|d| d.format("%Y%m%dT%H%M%SZ").to_string())
            .collect();
        s.push_str("\nEXDATE:");
        s.push_str(&ex.join(","));
    }

    // `.parse()` 把文本解析成 RRuleSet；失败时返回空 Vec（不崩溃，容错处理）。
    let rset: RRuleSet = match s.parse() {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    // rrule crate 用自带时区类型 `Tz`；把窗口边界从 UTC 转成 UTC 时区实例，
    // 保证与 RRuleSet 内部时区体系一致（时区/日界处理在此完成）。
    let after_tz: DateTime<Tz> = Tz::UTC.from_utc_datetime(&after.naive_utc());
    let before_tz: DateTime<Tz> = Tz::UTC.from_utc_datetime(&before.naive_utc());
    // `after().before().all(10_000)`：取窗口内所有发生，上限 10000 次防止失控。
    let result = rset.after(after_tz).before(before_tz).all(10_000);

    // 把 crate 时区的时间转换回 `DateTime<Utc>`，收集为 Vec。
    let mut occs: Vec<_> = result
        .dates
        .into_iter()
        .map(|dt| DateTime::from_naive_utc_and_offset(dt.naive_utc(), Utc))
        .collect();

    // 排序：让下一个即将到来的发生排第 1（index 1），而不是窗口内最早的过去实例。
    // 未来实例升序在前（越近越小），过去实例降序在后（越近越大）——
    // 这样 `list` 用 `take(N)` 显示即将到来的项，而“昨天及更早”仍可用更大编号寻址。
    let now = Utc::now();
    occs.sort_by(|a, b| {
        let a_future = *a >= now;
        let b_future = *b >= now;
        match (a_future, b_future) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (true, true) => a.cmp(b),
            (false, false) => b.cmp(a),
        }
    });

    // 构造 `Occurrence`：用 `enumerate()` 产生 (i, occurrence_start)，
    // `i + 1` 得到 1 基序号；这里是结构体字面量（struct literal）构造。
    occs
        .into_iter()
        .enumerate()
        .map(|(i, occurrence_start)| Occurrence {
            master: task.clone(),
            occurrence_start,
            index: i + 1,
        })
        .collect()
}
