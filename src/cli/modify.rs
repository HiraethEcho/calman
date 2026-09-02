//! `modify` 子命令：修改任务的字段。
//! `modify` subcommand handler.
//!
//! 规则：裸词替换 summary。`+allday` 转为全天（清掉时间）。`start:` 设置事件开始：
//! 仅日期的 `start:` 使事件全天，日期时间的 `start:` 使事件计时。`end:`/`duration:` 可选。
//!
//! 数据流：解析目标 → 组装字段（`Upd`）→ 打开存储 → 普通任务闭包更新；
//! occurrence 目标则新建/更新覆盖记录，或 `all-future` 时拆分子系列。

use crate::args::ParsedArgs;
use crate::cli::{open_storage, resolve_source, resolve_targets_occ};
use crate::config::Config;
use crate::date::{
    DateValue, local_midnight, parse_date_value, parse_duration, resolve_end,
};
use crate::model::{Task, TaskStatus};
use crate::storage::{Storage, Store};
use anyhow::{Result, bail};
use chrono::{DateTime, Duration, Local, Utc};

/// 执行 modify。
///
/// 先把命令行参数整理成一个 `Upd`（统一字段包），再逐个目标应用。
pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    if q.ids.is_empty() {
        bail!("no ID(s) specified");
    }
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());

    if q.text.is_empty()
        && q.priority.is_none()
        && q.due.is_none()
        && q.status.is_none()
        && q.tags.is_empty()
        && q.anti_tags.is_empty()
        && q.from.is_none()
        && q.to.is_none()
        && q.location.is_none()
        && q.repeat.is_none()
        && q.description.is_none()
        && q.span.is_none()
        && !q.allday
        && q.alert.is_none()
        && q.rel.is_none()
        && q.wait.is_none()
    {
        // 上面这一长串 `&&` 都在检查“用户到底改了什么”：
        // 如果所有字段都没给，就报“no changes specified”而不是空跑一次。
        bail!("no changes specified");
    }

    // 解析新的开始时间：Date-only → 当地午夜且全天；Time → 定时事件。
    let (from, from_allday) = if let Some(s) = &q.from {
        match parse_date_value(s)? {
            // Date-only `start:` → all-day event at local midnight.
            DateValue::Date(d) => (Some(local_midnight(d)), true),
            DateValue::Time(dt) => (Some(dt), false),
        }
    } else {
        (None, false)
    };
    let to = q.to.as_deref().map(parse_date_value).transpose()?;
    let dur = q.span.as_deref().map(parse_duration).transpose()?;
    if to.is_some() && dur.is_some() {
        bail!("use either `to:` or `for:`, not both");
    }
    if (to.is_some() || dur.is_some()) && q.from.is_none() && q.allday {
        bail!("`to:`/`for:` need a timed event; give `from:` too or drop allday");
    }
    let alert = match &q.alert {
        Some(a) => {
            let lead = parse_duration(a)?;
            let secs = lead.num_seconds();
            if secs <= 0 {
                bail!("alert must be positive, got `{a}`");
            }
            Some(secs)
        }
        None => None,
    };

    let default_duration = {
        let def = conf.date.default_event_duration.trim();
        if def.is_empty() {
            None
        } else {
            Some(parse_duration(def)?)
        }
    };
    let related = match &q.rel {
        Some(rel) => {
            let targets = crate::cli::resolve_targets(conf, None, std::slice::from_ref(rel))?;
            targets.first().map(|(uid, _)| uid.clone())
        }
        None => None,
    };

    // 把本次要改的字段打包成一个 Upd，交给下面的 apply/闭包统一使用。
    let upd = Upd {
        text: q.text.clone(),
        priority: q.priority,
        due: q.due,
        due_allday: q.due_allday,
        status: q.status,
        tags: q.tags.clone(),
        anti_tags: q.anti_tags.clone(),
        location: q.location.clone(),
        repeat: q.repeat.clone(),
        description: q.description.clone(),
        allday: q.allday,
        from,
        from_allday,
        to,
        span: dur,
        alert,
        related: related.clone(),
        default_duration,
        wait: q.wait.clone(),
    };

    for tgt in resolve_targets_occ(conf, override_, &q.ids, q.occ_date)? {
        let src = resolve_source(conf, &tgt.source)?;
        let mut st = open_storage(conf, &src)?;

        if let Some(occ) = tgt.occ_date {
            // 修改某一次 occurrence：不直接改主任务，而是建/改 RECURRENCE-ID 覆盖记录。
            // Per-occurrence modify: create a RECURRENCE-ID override sibling.
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
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string();
            if q.apply_all_future
                || crate::cli::confirm(&format!(
                    "apply change to ALL FUTURE occurrences from {occ_day}?"
                ))?
            {
                // all-future → 从这里把系列“劈开”：前半段保留旧规则，
                // 新起一条 successor 系列承载本次修改。
                split_series(&mut st, &master, occ, &upd)?;
                println!("split series {}", q.ids.join(", "));
                continue;
            }
            // 同一 occurrence 再次修改时，更新已有覆盖记录，避免堆叠重复兄弟。
            // Re-modifying the same occurrence updates its override instead of
            // stacking duplicate siblings.
            let existing = st.list().iter().find(|t| {
                t.parent_uid.as_deref() == Some(tgt.uid.as_str())
                    && t.recurrence_id == Some(occ)
            });
            if let Some(e) = existing {
                let uid = e.uid.clone();
                st.update(&uid, |t| apply(t, &upd))?
                    .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
            } else {
                let mut ov =
                    crate::cli::override_for_occurrence(&master, occ, TaskStatus::Pending);
                apply(&mut ov, &upd)?;
                st.add(ov)?;
            }
        } else {
            // 普通任务：闭包 `|t| apply(t, &upd)` 在存储层内原子地完成
            // “拿到可修改引用 → 改字段 → 保存”。
            st.update(&tgt.uid, |t| apply(t, &upd))?
            .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", tgt.uid))?;
        }
    }
    println!("modified: {}", q.ids.join(", "));
    Ok(())
}

/// 在某次 occurrence 处把重复系列“劈开”：新建一条从 `occ` 开始的 successor 系列，
/// 并应用 `upd` 的修改；旧主任务只保留 `occ` 之前的实例。
/// 若 `occ` 就是系列的第一次，旧主任务直接删除。
/// Split a recurring series at an occurrence, applying `upd` to a new
/// successor series that starts there. The old master keeps every occurrence
/// before `occ`; when `occ` is the series' first occurrence the old master is
/// deleted outright.
fn split_series(
    st: &mut Store,
    master: &Task,
    occ: DateTime<Utc>,
    upd: &Upd,
) -> Result<()> {
    // 先算出 `occ` 在原始序列里的绝对下标；算不出说明不是该系列的实例。
    let Some(idx) = crate::cli::series::absolute_index(master, occ) else {
        bail!("`{occ}` is not a recurring occurrence of `{}`", master.summary);
    };

    // 删除该时点上已有的覆盖记录：这次修改由 successor 系列接管，旧记录会冲突。
    // Drop any existing override at this slot; the successor carries the edit.
    let ovs: Vec<String> = st
        .list()
        .iter()
        .filter(|t| {
            t.parent_uid.as_deref() == Some(master.uid.as_str())
                && t.recurrence_id == Some(occ)
        })
        .map(|t| t.uid.clone())
        .collect();
    for u in ovs {
        st.remove(&u)?;
    }

    // successor：复制旧主任务，换新 UID、清掉父/覆盖关系，然后把 `upd` 应用上去。
    // Successor series: same content as the old master, but the first
    // occurrence is `occ` (unless the user set a new date) and it gets a new
    // UID / identity.
    let mut nm = master.clone();
    nm.uid = uuid::Uuid::new_v4().to_string();
    nm.parent_uid = None;
    nm.recurrence_id = None;
    let had_from = upd.from.is_some();
    let had_due = upd.due.is_some();
    apply(&mut nm, upd)?;
    // 若用户没改日期，把 successor 的首次日期对齐到 `occ`（保持时间差）。
    if !had_from && nm.is_event() {
        let s = nm.dtstart.ok_or_else(|| {
            anyhow::anyhow!("event `{}` has no start", nm.summary)
        })?;
        let delta = occ - s;
        nm.dtstart = Some(occ);
        nm.dtend = nm.dtend.map(|e| e + delta);
    } else if !had_due && !nm.is_event() {
        nm.due = Some(occ);
    }
    // 原来有 COUNT 的话，要从新起点重新数：总数减去已过去的次数。
    // A bounded original COUNT keeps counting from the new start.
    if upd.repeat.is_none()
        && let Some(rr) = master.rrule.as_deref()
        && let Some(rem) = crate::cli::series::remaining_count(rr, idx)
    {
        nm.rrule = Some(crate::cli::series::set_count(rr, rem));
    }
    st.add(nm)?; // 先写入 successor 系列

    // 再把旧主任务截断到 `occ` 之前；若返回 true，旧系列已无剩余实例，整体删除。
    // Truncate the old master before this occurrence.
    let mut mm = master.clone();
    let delete_old = crate::cli::series::truncate_before(&mut mm, occ)?;
    if delete_old {
        let all: Vec<String> = st
            .list()
            .iter()
            .filter(|t| t.parent_uid.as_deref() == Some(master.uid.as_str()))
            .map(|t| t.uid.clone())
            .collect();
        for u in all {
            st.remove(&u)?;
        }
        st.remove(&master.uid)?
            .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", master.uid))?;
    } else {
        st.update(&master.uid, |t| {
            t.rrule = mm.rrule.clone();
            Ok(())
        })?
        .ok_or_else(|| anyhow::anyhow!("task `{}` disappeared", master.uid))?;
    }
    Ok(())
}

/// 一次修改请求的全部字段集合（“改什么”的数据包）。
/// 与命令行参数一一对应，方便在多个目标上重复应用同一组修改。
struct Upd {
    text: String,
    priority: Option<u8>,
    due: Option<DateTime<Utc>>,
    due_allday: bool,
    status: Option<TaskStatus>,
    tags: Vec<String>,
    anti_tags: Vec<String>,
    location: Option<String>,
    repeat: Option<String>,
    description: Option<String>,
    allday: bool,
    from: Option<DateTime<Utc>>,
    from_allday: bool,
    to: Option<DateValue>,
    span: Option<Duration>,
    alert: Option<i64>,
    related: Option<String>,
    default_duration: Option<Duration>,
    wait: Option<String>,
}

/// 把 `u`（修改请求）实际应用到任务 `t` 上。
///
/// `&mut Task` 是可修改引用：函数可以改字段，改完由调用方负责保存。
fn apply(t: &mut Task, u: &Upd) -> Result<()> {
    if !u.text.is_empty() {
        t.summary = u.text.clone(); // 裸词 → 新 summary
    }
    if let Some(p) = u.priority {
        t.priority = Some(p);
    }
    if let Some(d) = u.due {
        t.due = Some(d);
        if !t.is_event() {
            t.allday = u.due_allday;
        }
    }
    if let Some(s) = u.status {
        t.status = s;
        // 手动置为 Completed 时顺便记录完成时间。
        if s == TaskStatus::Completed {
            t.completed_at = Some(Utc::now());
        }
    }
    // 加标签：先查重，避免同一个标签出现两次。
    for tag in &u.tags {
        if !t.tags.iter().any(|x| x == tag) {
            t.tags.push(tag.clone());
        }
    }
    for anti in &u.anti_tags {
        // retain：只保留“不等于要删的标签”的元素 → 删除匹配标签。
        t.tags.retain(|x| !x.eq_ignore_ascii_case(anti));
    }
    if let Some(v) = &u.location {
        t.location = Some(v.clone());
    }
    if let Some(v) = &u.repeat {
        t.rrule = Some(crate::recurrence::normalize_recurrence(v)?);
        // Adding a recurrence promotes an active item to series master.
        // 加上 recurrence 后，普通活跃任务升级为系列父任务（Recurring）。
        if t.status.is_active() {
            t.status = crate::model::TaskStatus::Recurring;
        }
    }
    if let Some(v) = &u.description {
        t.description = Some(v.clone());
    }
    if let Some(r) = &u.related {
        t.related_to = Some(r.clone());
    }
    if let Some(secs) = u.alert {
        t.alarm_before = Some(secs);
    }

    if u.allday {
        // Convert to all-day: keep dates, drop times + DTEND.
        // 转全天：只保留日期（当地午夜），丢掉结束时间。
        t.allday = true;
        t.dtstart = t
            .dtstart
            .map(|d| local_midnight(d.with_timezone(&Local).date_naive()));
        t.dtend = None;
    }

    if let Some(s) = u.from {
        // Adding `from:` converts the item to a VEVENT.
        // 给的是 `from:` → 把 todo 转成事件（VEVENT）。
        t.event = true;
        // Date-only `from:` keeps the event all-day; date-time makes it timed.
        // 仅日期的 from 保持全天；日期时间的 from 变成定时事件。
        t.allday = u.from_allday;
        t.dtstart = Some(s);
        if u.from_allday {
            t.dtend = None; // drop any stale timed end; all-day end is implicit
            // 全天事件的结束时间是隐含的，清掉旧的定时结束。
        }
    }

    if let Some(s) = t.dtstart {
        if let Some(e) = &u.to {
            t.dtend = Some(resolve_end(s, t.allday, *e)?);
        } else if let Some(d) = u.span {
            t.dtend = Some(s + d);
        } else if u.from.is_some() && !t.allday && t.dtend.is_none() {
            // Newly timed event without explicit end → default duration (if any).
            if let Some(d) = u.default_duration {
                t.dtend = Some(s + d);
            }
        }
    } else if u.to.is_some() || u.span.is_some() {
        bail!("target has no start; use `from:` to make it an event first");
    }

    // Keep allday invariant: DTEND must stay after DTSTART (exclusive).
    // 保持全天不变量：结束必须严格晚于开始；否则把结束清掉。
    if t.allday
        && let (Some(s), Some(e)) = (t.dtstart, t.dtend)
        && e <= s
    {
        t.dtend = None;
    }

    if let Some(w) = &u.wait {
        // wait 需要一个日期锚点（due 或 start），据此解析出“什么时候解除隐藏”。
        let anchor = t
            .due
            .or(t.dtstart)
            .ok_or_else(|| anyhow::anyhow!("wait needs a date anchor: give `due:` (todo) or `start:` (event)"))?;
        t.wait = Some(crate::args::resolve_wait(w, anchor)?);
    }
    Ok(())
}
