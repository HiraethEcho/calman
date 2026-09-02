//! `add` 子命令：新建任务/事件。
//! `add` subcommand handler.
//!
//! Todo（待办）：`calman add <text> due:<date> ...`
//! Event（事件）：`calman add <text> from:<date> [to:<date> | for:<dur>] ...`
//! 仅日期的 `from:` 形式（`20260812`、`0826`、`17`、`today`）→ 全天事件。
//!
//! 数据流：解析参数 → 确定写入源 → 构造 Task → 打开存储 → `st.add` → 打印结果。

use crate::args::ParsedArgs;
use crate::cli::open_storage;
use crate::config::Config;
use crate::date::{
    DateValue, local_midnight, parse_date_value, parse_duration, resolve_end,
};
use crate::model::{Task, TaskStatus};
use crate::source::resolve_source_name;
use crate::storage::Storage;
use anyhow::{Context, Result, bail};
use chrono::Local;

/// 执行 `add` 命令。`conf` 是全局配置（只读借用），`q` 是已解析的命令行参数。
pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    // 没有任务文字就无从创建，直接返回错误（bail!）。
    if q.text.is_empty() {
        bail!("task summary required");
    }

    // 目标源：命令行指定了 `source:` 就用它，否则用配置里的默认写入源。
    // `then_some(...)`：条件为真时返回 Some，否则 None（Option 类型）。
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    let single = match override_ {
        Some(ns) if ns.len() == 1 => ns[0].clone(),
        Some(_) => bail!("`add` accepts exactly one source (e.g. source:work)"),
        None => conf.write_source().to_string(),
    };
    // Resolve through `resolve_source_name` so `IcsDir` collection refs
    // (e.g. `remote/sorge`) map to a concrete virtual source.
    let resolved = resolve_source_name(&conf.sources, &single)
        .with_context(|| format!("resolve source `{single}`"))?;
    let src = match resolved.len() {
        1 => resolved.into_iter().next().unwrap(),
        0 => bail!("unknown write source `{single}`"),
        _ => bail!(
            "`add` to an IcsDir source needs a specific collection, e.g. source:{}/<collection>",
            single
        ),
    };

    // 新建任务对象。`&src.name` 是借用 String；`clone()` 复制一份，避免所有权问题。
    let mut task = Task::new(&src.name, q.text.clone());
    task.priority = q.priority;
    task.tags = q.tags.clone();
    task.description = q.description.clone();
    task.location = q.location.clone();
    task.rrule = q
        .repeat
        .as_deref()
        .map(crate::recurrence::normalize_recurrence)
        .transpose()?;
    if let Some(st) = q.status {
        task.status = st;
    }
    if task.rrule.is_some() && task.status.is_active() {
        task.status = TaskStatus::Recurring;
    }
    if let Some(rel) = &q.rel {
        // `rel:<id>` 关联父任务：先把对方 ID 解析成 UID，再存进 related_to。
        let targets = crate::cli::resolve_targets(conf, None, std::slice::from_ref(rel))?;
        task.related_to = targets.first().map(|(uid, _)| uid.clone());
    }

    // `from:` → 事件（event）；`due:` → 待办（todo），两者不能同时出现。
    if let Some(start_str) = &q.from {
        if q.due.is_some() {
            bail!("use either `from:` (event) or `due:` (todo), not both");
        }
        task.event = true;
        let start = parse_date_value(start_str)?; // `?`：解析失败就把错误抛给调用方
        // matches! 宏检查枚举值；Date-only 输入自动变成全天事件。
        task.allday = q.allday || matches!(start, DateValue::Date(_));
        // match 把“日期”和“具体时刻”统一成 DateTime：日期 → 当地午夜。
        task.dtstart = Some(match start {
            DateValue::Date(d) => local_midnight(d),
            DateValue::Time(dt) => dt,
        });
        if q.allday {
            let d = task.dtstart.unwrap().with_timezone(&Local).date_naive();
            task.dtstart = Some(local_midnight(d));
        }

        let dur = q.span.as_deref().map(parse_duration).transpose()?;
        if q.to.is_some() && dur.is_some() {
            bail!("use either `to:` or `for:`, not both");
        }
        if let Some(end_str) = &q.to {
            let end = parse_date_value(end_str)?;
            task.dtend = Some(resolve_end(task.dtstart.unwrap(), task.allday, end)?);
        } else if let Some(d) = dur {
            task.dtend = Some(task.dtstart.unwrap() + d);
        } else if !task.allday {
            let def = conf.date.default_event_duration.trim();
            if !def.is_empty() {
                task.dtend = Some(task.dtstart.unwrap() + parse_duration(def)?);
            }
        }

        if !task.allday
            && let (Some(s), Some(e)) = (task.dtstart, task.dtend)
            && e < s
        {
            bail!("to must be after from");
        }
    } else {
        // 没有 `from:` 就是 todo：`to:`/`for:`/`allday` 这些事件专属选项不允许出现。
        if q.to.is_some() {
            bail!("`to:` requires `from:` (use an event)");
        }
        if q.span.is_some() {
            bail!("`for:` requires `from:` (use an event)");
        }
        if q.allday {
            bail!("`allday` requires `from:` (use an event)");
        }
        if let Some(d) = &q.due {
            task.due = Some(*d);
            // Date-only `due` → all-day todo (ICS DUE;VALUE=DATE)。
            // 仅日期的到期日 → 全天待办（ICS 里存成 DUE;VALUE=DATE）。
            task.allday = task.allday || q.due_allday;
        }
    }

    if let Some(a) = &q.alert {
        // `alert:` 是提前提醒：解析成秒数，必须为正数。
        let lead = parse_duration(a)?;
        let secs = lead.num_seconds();
        if secs <= 0 {
            bail!("alert must be positive, got `{a}`");
        }
        task.alarm_before = Some(secs);
    }

    if let Some(w) = &q.wait {
        // `wait:` 需要以 due/start 为锚点，算出“什么时候开始显示”。
        let anchor = task
            .due
            .or(task.dtstart)
            .ok_or_else(|| anyhow::anyhow!("wait needs a date anchor: give `due:` (todo) or `start:` (event)"))?;
        task.wait = Some(crate::args::resolve_wait(w, anchor)?);
    }

    let mut st = open_storage(conf, &src)?;
    st.add(task)?; // 追加一行；`?` 把写入错误直接传给用户

    if q.from.is_some() {
        println!("added event to `{single}`");
    } else {
        println!("added task to `{single}`");
    }
    Ok(())
}
