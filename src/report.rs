//! Report engine: builtin ls/list/next reports, merging TODO/EVENT rows,
//! STATUS and DATE columns, `rc.report.*` overrides, sorting, nerdfont icons, colors.
//! 报表引擎：内置 ls/list/next 报表、合并 TODO/EVENT 行、STATUS 与 DATE 列、
//! `rc.report.*` 覆盖、排序、nerdfont 图标与颜色。

use crate::args::RcReport;
use crate::cli::Row;
use crate::config::{ColumnCfg, Config};
use crate::filter::{Expr, parse_expr_str};
use crate::model::TaskStatus;
use anyhow::{Result, bail};
use chrono::{DateTime, Local};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// A single rendered report column.
/// 单个渲染出的报表列：字段名、表头标签、宽度、格式、图标配置等。
#[derive(Debug, Clone)]
pub struct Column {
    pub field: String,
    pub label: String,
    pub width: Option<usize>,
    pub format: Option<String>,
    pub icon: bool,
    pub icons: HashMap<String, String>,
    pub event_format: Option<String>,
    pub todo_format: Option<String>,
}

/// A sort key parsed from `key+` / `key-` / `key+/` (trailing `/` = break).
/// 排序键解析结果：`due+` 升序、`due-` 降序，末尾 `/` 表示“该键值变化时插入分隔空行”。
#[derive(Debug, Clone)]
pub struct SortKey {
    pub field: String,
    pub asc: bool,
    pub brk: bool,
}

/// A resolved report (config overrides builtin).
/// 解析后的报表：过滤器、排序键、列定义；配置优先，缺省回退到内置报表。
#[derive(Debug, Clone)]
pub struct Report {
    pub filter: String,
    pub sort: Vec<SortKey>,
    pub columns: Vec<Column>,
}

/// Comparable value for sorting (numeric preferred over string).
/// 可比较的排序值：数值优先于字符串（混合类型时数字排前面）。
#[derive(Debug, Clone, PartialEq, PartialOrd)]
enum SVal {
    Str(String),
    Num(i64),
}

impl Report {
    /// Resolve a report by name: config `[report.<name>]` wins, else builtin.
    /// A config that omits `filter` inherits the builtin report's filter, so
    /// partial overrides (columns/sort only) keep hiding completed/cancelled/
    /// recurring parents as the defaults do.
    /// 按名称解析报表：配置 `[report.<name>]` 优先，否则用内置报表。
    /// 若配置省略 `filter`，则继承内置过滤条件，保证只改列/排序时仍会隐藏已完成等任务。
    pub fn resolve(name: &str, conf: &Config) -> Report {
        match conf.reports.get(name) {
            Some(cfg) => {
                let mut r = from_config(cfg);
                if cfg.filter.is_none() {
                    r.filter = builtin(name).filter;
                }
                r
            }
            None => builtin(name),
        }
    }

    /// The report's default filter (parsed to a `Filter`).
    /// 将报表默认过滤字符串解析为表达式树 `Expr`；空字符串返回默认过滤器。
    pub fn filter(&self) -> Result<Expr> {
        if self.filter.trim().is_empty() {
            return Ok(Expr::Atom(crate::filter::Filter::default()));
        }
        parse_expr_str(&self.filter)
    }
}

/// Convert a config report definition into a `Report`.
/// 把配置文件里的 `[report.<name>]` 定义转换成 `Report`（排序键、列逐个解析）。
fn from_config(cfg: &crate::config::ReportCfg) -> Report {
    Report {
        filter: cfg.filter.clone().unwrap_or_default(),
        sort: cfg.sort.iter().map(|s| parse_sort(s)).collect(),
        columns: cfg.columns.iter().map(from_column).collect(),
    }
}

/// Convert one column config into a `Column`; label defaults to uppercase field.
/// 单列配置 → `Column`；未写 label 时默认用字段名大写作为表头。
fn from_column(c: &ColumnCfg) -> Column {
    let label = c.label.clone().unwrap_or_else(|| c.field.to_uppercase());
    Column {
        field: c.field.clone(),
        label,
        width: c.width,
        format: c.format.clone(),
        icon: c.icon,
        icons: c.icons.clone(),
        event_format: c.event_format.clone(),
        todo_format: c.todo_format.clone(),
    }
}

/// Apply Taskwarrior-style `rc.report.<name>.<key>=<value>` overrides.
///
/// Supported keys: `columns`, `labels`, `filter`, `sort`. `columns` accepts
/// `field` or `field.format` tokens (comma-separated).
/// 应用 Taskwarrior 风格命令行覆盖：支持 `columns`/`labels`/`filter`/`sort`。
/// `columns` 接受逗号分隔的 `field` 或 `field.format` token（如 `date.relative`）。
pub fn apply_rc(report: &mut Report, rcs: &[RcReport], name: &str) -> Result<()> {
    for rc in rcs {
        if !rc.name.eq_ignore_ascii_case(name) {
            continue;
        }
        match rc.key.as_str() {
            "columns" => {
                let mut cols = Vec::new();
                for part in rc.value.split(',') {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    let (field, fmt) = match part.split_once('.') {
                        Some((f, fm)) if !fm.is_empty() => (f, Some(fm)),
                        _ => (part, None),
                    };
                    let mut c = col(field, &field.to_uppercase(), None, None, false);
                    c.format = fmt.map(|s| s.to_string());
                    // `date`/`due` columns keep feature defaults unless the
                    // rc override explicitly sets a format (see `date_col`).
                    // 日期类列保持默认格式；只有 rc 显式给了 format 才覆盖。
                    if matches!(c.field.as_str(), "date" | "due" | "start" | "from") && c.format.is_none() {
                        c.todo_format = Some("relative".to_string());
                    }
                    cols.push(c);
                }
                if cols.is_empty() {
                    bail!("rc.report.{name}.columns is empty");
                }
                report.columns = cols;
            }
            "labels" => {
                for (i, label) in rc.value.split(',').enumerate() {
                    let label = label.trim();
                    if label.is_empty() {
                        continue;
                    }
                    if i < report.columns.len() {
                        report.columns[i].label = label.to_string();
                    }
                }
            }
            "filter" => report.filter = rc.value.clone(),
            "sort" => {
                report.sort = rc
                    .value
                    .split(',')
                    .filter(|s| !s.trim().is_empty())
                    .map(parse_sort)
                    .collect();
            }
            other => bail!("unsupported rc.report key `{other}`"),
        }
    }
    Ok(())
}

/// Parse one sort token: `key+`/`key-` for direction, trailing `/` for break.
/// 解析单个排序键：`+`/`-` 表示升降序，尾部 `/` 表示按该键分组并插入分隔线。
fn parse_sort(s: &str) -> SortKey {
    let s = s.trim();
    let (body, brk) = match s.strip_suffix('/') {
        Some(b) => (b, true),
        None => (s, false),
    };
    let (field, asc) = match body.strip_suffix('+') {
        Some(f) => (f.to_string(), true),
        None => match body.strip_suffix('-') {
            Some(f) => (f.to_string(), false),
            None => (body.to_string(), true),
        },
    };
    SortKey { field, asc, brk }
}

/// Build a plain column (no date-specific defaults).
/// 构造普通列：不含日期相关的默认格式。
fn col(field: &str, label: &str, width: Option<usize>, format: Option<&str>, icon: bool) -> Column {
    Column {
        field: field.to_string(),
        label: label.to_string(),
        width,
        format: format.map(|s| s.to_string()),
        icon,
        icons: HashMap::new(),
        event_format: None,
        todo_format: None,
    }
}

/// Build a unified date column with a per-kind todo format.
/// 构造统一 DATE 列：todo 用 relative 等格式，event 另有默认格式。
fn date_col(label: &str, todo_format: &str) -> Column {
    Column {
        field: "date".to_string(),
        label: label.to_string(),
        width: None,
        format: None,
        icon: false,
        icons: HashMap::new(),
        event_format: None,
        todo_format: Some(todo_format.to_string()),
    }
}

/// Builtin `ls` / `list` / `next` reports (overridable via config).
///
/// Per feature.md `more on report`: only future events are shown by default
/// (including today), the type/status columns are merged (both render a
/// status glyph from `[icons.todo]` / `[icons.event]`), and DUE is relabelled
/// DATE (event → plain date, todo → relative).
/// 内置 ls/list/next 报表（可被配置覆盖）。默认只显示未来事件（含今天）；
/// type/status 合并为状态列（图标来自 `[icons.todo]`/`[icons.event]`）；
/// DUE 改名为 DATE（event 显示日期，todo 显示相对时间）。
fn builtin(name: &str) -> Report {
    let (filter, sort, columns) = match name {
        "ls" => (
            "type:todo status:active -status:recurring -WAITING or type:event due.after:sod -status:recurring -WAITING"
                .to_string(),
            vec!["due+", "created+"],
            vec![
                col("id", "ID", Some(4), None, false),
                col("status", "ST", None, None, true),
                date_col("DATE", "relative"),
                col("summary", "SUMMARY", None, None, false),
            ],
        ),
        "list" => (
            "type:todo -status:completed -status:cancelled -status:recurring -WAITING or type:event due.after:sod -status:recurring -WAITING"
                .to_string(),
            vec!["status-", "pri-", "due+"],
            vec![
                col("id", "ID", Some(4), None, false),
                col("status", "STATUS", None, None, true),
                col("pri", "PRI", None, None, false),
                date_col("DATE", "relative"),
                col("tags", "TAGS", None, None, false),
                col("recur", "RECUR", None, None, false),
                col("summary", "SUMMARY", None, None, false),
                col("desc", "DESC", Some(40), Some("truncate"), false),
            ],
        ),
        "next" => (
            "type:todo status:active -status:recurring -WAITING or type:event due.after:sod -status:recurring -WAITING"
                .to_string(),
            vec!["due+", "pri-"],
            vec![
                col("id", "ID", Some(4), None, false),
                col("status", "ST", None, None, true),
                date_col("DATE", "relative"),
                col("summary", "SUMMARY", None, None, false),
            ],
        ),
        _ => (
            String::new(),
            Vec::new(),
            vec![
                col("id", "ID", Some(4), None, false),
                col("summary", "SUMMARY", None, None, false),
            ],
        ),
    };
    Report {
        filter,
        sort: sort.iter().map(|s| parse_sort(s)).collect(),
        columns,
    }
}

/// Sort a slice of borrowed rows in place by the report's sort keys.
/// 按报表排序键原地排序；入参是借用切片 `&[&Row]`，不转移数据所有权。
pub fn sort_rows(rows: &mut [&Row], keys: &[SortKey]) {
    rows.sort_by(|a, b| {
        for k in keys {
            let ord = cmp_sval(&sort_val(a, &k.field), &sort_val(b, &k.field));
            if ord != Ordering::Equal {
                return if k.asc { ord } else { ord.reverse() };
            }
        }
        Ordering::Equal
    });
}

/// Render a report over selected rows to a string (table + colors + breaks).
/// 把选中行渲染成表格字符串：计算列宽 → 表头 → 逐行处理分组空行与颜色。
pub fn render(conf: &Config, report: &Report, rows: &[&Row]) -> String {
    if rows.is_empty() {
        return "(no tasks)\n".to_string();
    }
    let ncol = report.columns.len();
    let mut widths = vec![0usize; ncol];
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| report.columns.iter().map(|c| cell(conf, r, c)).collect())
        .collect();
    // UIDs referenced as parents (for the `blocked` color rule).
    // 收集被其他任务引用的父任务 UID，供 `blocked` 颜色规则判断。
    let parents: HashSet<&str> = rows
        .iter()
        .filter_map(|r| r.task.related_to.as_deref())
        .collect();

    // Column width = max(header, all cell display widths, configured min).
    // 列宽 = max(表头宽度, 所有单元格宽度, 配置最小宽度)。
    // UnicodeWidthStr::width 按终端显示宽度计：CJK 汉字占 2 列，保证中文对齐。
    for (ci, c) in report.columns.iter().enumerate() {
        let mut w = unicode_width::UnicodeWidthStr::width(c.label.as_str());
        for row in &cells {
            w = w.max(unicode_width::UnicodeWidthStr::width(row[ci].as_str()));
        }
        if let Some(mw) = c.width {
            w = w.max(mw);
        }
        widths[ci] = w;
    }

    let mut out = String::new();
    let header: Vec<String> = report
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| pad(&c.label, widths[i]))
        .collect();
    out.push_str(&header.join("  "));
    out.push('\n');

    let mut prev: Vec<Option<SVal>> = vec![None; report.sort.len()];
    for (ri, row_cells) in cells.iter().enumerate() {
        let r = rows[ri];
        // Break keys: insert blank line when the sort value changes vs previous row.
        // Option 链：as_ref().is_some_and() 比较“上一个值”，值变化时插入空行分组。
        for (ki, k) in report.sort.iter().enumerate() {
            if !k.brk {
                continue;
            }
            let v = Some(sort_val(r, &k.field));
            if prev[ki].as_ref().is_some_and(|p| p != v.as_ref().unwrap()) {
                out.push('\n');
            }
            prev[ki] = v;
        }
        let line = row_cells
            .iter()
            .enumerate()
            .map(|(i, c)| pad(c, widths[i]))
            .collect::<Vec<_>>()
            .join("  ");
        out.push_str(&colorize(conf, &parents, r, &line));
        out.push('\n');
    }
    out
}

/// Render one cell for a row/column.
/// 渲染单个单元格：按字段名 match 分派到不同取值逻辑。
fn cell(conf: &Config, r: &Row, c: &Column) -> String {
    let t = &r.task;
    match c.field.as_str() {
        "id" => r.id.to_string(),
        // Merged type/status: both kinds show a status glyph; the per-kind
        // global tables `[icons.todo]` / `[icons.event]` pick the glyph.
        // 合并 type/status：TODO 与 EVENT 都显示状态图标，图标来自各自类型表。
        "status" => {
        // Overdue 行把 `overdue` 当作状态字（可通过 `[icons.todo].overdue`
        // 或列的 `icons` 自定义）；日期列则只显示纯日期，避免重复标记。
        // Overdue rows show `overdue` as the status glyph (customisable
        // via `[icons.todo].overdue` / column `icons`); the date column
        // then just shows the plain date instead.
        let st = if crate::filter::is_overdue(t) {
            "overdue"
        } else {
            status_txt(t.status)
        };
            let kind = if t.is_event() { "event" } else { "todo" };
            if c.icon {
                icon(conf, kind, st, c)
            } else {
                st.to_string()
            }
        }
        "type" => {
            let ty = if t.is_event() { "event" } else { "todo" };
            if c.icon {
                icon_type(c, ty)
            } else {
                ty.to_string()
            }
        }
        "summary" => maybe_truncate(t.summary.as_str(), c),
        "desc" => maybe_truncate(t.description.as_deref().unwrap_or(""), c),
        "tags" => t.tags.join(","),
        // Unified date column; `due`/`start`/`from` are aliases of `date`.
        // 统一日期列：`due`/`start`/`from` 都是 `date` 的别名，走同一渲染逻辑。
        "date" | "due" | "start" | "from" => date_str(r, c),
        "pri" => pri_str(t.priority),
        "source" => t.source.clone(),
        "recur" | "recurrence" => t
            .rrule
            .as_deref()
            .map(crate::recurrence::rrule_period)
            .unwrap_or_default(),
        // Option 链：as_deref() 把 &Option<String> 变成 Option<&str>，map 转周期文本。
        _ => String::new(),
    }
}

/// Truncate a cell to the configured width (display width, not chars).
/// 按显示宽度截断：CJK 汉字算 2 列，保留完整字符并给省略号留 1 格。
fn maybe_truncate(s: &str, c: &Column) -> String {
    let s = s.replace('\n', " ");
    if c.format.as_deref() == Some("truncate") {
        let max = c.width.unwrap_or(30);
        // Truncate by terminal display width, keeping whole chars.
        // 逐字符累加显示宽度，宽度超限时截断并补省略号。
        let mut w = 0;
        let mut cut = String::new();
        for ch in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw + 1 > max {
                // Reserve one cell for the ellipsis.
                // 预留 1 格给省略号，避免末尾字符被挤出去。
                cut.push('…');
                return cut;
            }
            cut.push(ch);
            w += cw;
        }
        s
    } else {
        s
    }
}

/// Format a row's date column (event vs todo, all-day vs timestamp).
/// 格式化日期列：event 默认 MM/DD；todo 默认相对时间；全天任务按日历日处理。
fn date_str(r: &Row, c: &Column) -> String {
    let dt = crate::filter::task_date(&r.task);
    let Some(dt) = dt else {
        return String::new();
    };
    // let-else 语法：没有日期就提前返回空字符串，有日期则继续。
    let local = dt.with_timezone(&Local);
    if r.task.is_event() {
        // Event: plain date by default, customisable via `event_format`.
        // event 默认显示普通日期 MM/DD；可用 event_format 自定义格式。
        return match c.event_format.as_deref() {
            Some(f) => fmt_date(&local, f),
            None => local.format("%m/%d").to_string(),
        };
    }
    if r.task.allday {
        // All-day todos: compare by calendar day so the display agrees with
        // the `+OVERDUE` filter (date-only due is overdue the day after).
        // 全天 todo 按日历日比较：显示与 `+OVERDUE` 过滤一致（次日才算逾期）。
        return allday_date_str(c, &local);
    }
    match c.todo_format.as_deref().or(c.format.as_deref()) {
        Some("relative") => {
            // 过去的日期直接显示成纯日期（例如 MM/DD），不再显示 relative 文案；
            // OVERDUE 标记由 STATUS 列负责。循环模板行（`is_overdue` 特意排除）
            // 也会走到这里，所以也用纯日期。
            // Any past date shows as a plain date (works for recurring master
            // rows too, which `is_overdue` deliberately excludes); the status
            // column carries the overdue marker for real instances.
            let now = Local::now();
            if local < now {
                local.format("%m/%d").to_string()
            } else {
                relative(&local, &now)
            }
        }
        Some("countdown") => {
            // 同上：倒计时只对“还没到”的时刻有意义，已过去就显示纯日期。
            // Same rule: countdown only makes sense for future moments;
            // past dates fall back to a plain date.
            let now = Local::now();
            if local < now {
                local.format("%m/%d").to_string()
            } else {
                countdown(&local, &now)
            }
        }
        // Option::or 链：todo_format 优先，缺失时回退到通用 format。
        Some(f) => fmt_date(&local, f),
        None => local.format("%Y-%m-%d").to_string(),
    }
}

/// All-day date column: day-granular relative/countdown, else formatted date.
/// 全天任务的日期列：按天粒度显示 relative/countdown，否则输出格式化日期。
fn allday_date_str(c: &Column, local: &DateTime<Local>) -> String {
    let day = local.date_naive();
    let today = Local::now().date_naive();
    let days = day.signed_duration_since(today).num_days();
    // 按日历日差显示：负数 → overdue，0 → today，正数 → N d。
    match c.todo_format.as_deref().or(c.format.as_deref()) {
        Some("relative") | Some("countdown") => {
            if days < 0 {
                // 已过期：显示纯日期（例如 MM/DD），状态列负责 overdue 标记。
                // Overdue: plain date; the status column carries the marker.
                local.format("%m/%d").to_string()
            } else if days == 0 {
                "today".to_string()
            } else {
                format!("{days}d")
            }
        }
        Some(f) => fmt_date(local, f),
        None => local.format("%Y-%m-%d").to_string(),
    }
}

/// Format a date: `iso`/`date` keywords or any chrono strftime pattern.
/// 日期格式化：`iso`/`date` 是内置简写，其他字符串直接当 chrono 格式模板。
fn fmt_date(dt: &DateTime<Local>, spec: &str) -> String {
    match spec {
        "iso" => dt.format("%Y-%m-%d").to_string(),
        "date" => dt.format("%m/%d").to_string(),
        s => dt.format(s).to_string(),
    }
}

/// Priority → single letter (H/M/L) or raw number.
/// 优先级映射：1→H、5→M、9→L（数字越小优先级越高），其他数值原样显示，None 显示空。
fn pri_str(p: Option<u8>) -> String {
    match p {
        Some(1) => "H".into(),
        Some(5) => "M".into(),
        Some(9) => "L".into(),
        Some(n) => n.to_string(),
        None => String::new(),
    }
}

/// Human-friendly relative time: overdue / N m / N h / N d / N w / N mo / N y.
/// 相对时间：过去显示 overdue，未来按分钟/小时/天/周/月/年取整。
fn relative(dt: &DateTime<Local>, now: &DateTime<Local>) -> String {
    let diff = *dt - *now;
    use chrono::Duration;
    let secs = diff.num_seconds();
    // chrono::Duration 可比较、可取秒数；阈值逐级放大，单位从分钟到年。
    if diff < Duration::zero() {
        return "overdue".to_string();
    }
    if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else if secs < 604_800 {
        format!("{}d", secs / 86_400)
    } else if secs < 2_592_000 {
        format!("{}w", secs / 604_800)
    } else if secs < 31_536_000 {
        format!("{}m", secs / 2_592_000)
    } else {
        format!("{}y", secs / 31_536_000)
    }
}

/// Countdown: `D d H h` or `H h MM m`; overdue when in the past.
/// 倒计时：整天以上显示 `D d H h`，不足一天显示 `H h MM m`；过去显示 overdue。
fn countdown(dt: &DateTime<Local>, now: &DateTime<Local>) -> String {
    let diff = *dt - *now;
    use chrono::Duration;
    if diff < Duration::zero() {
        return "overdue".to_string();
    }
    let secs = diff.num_seconds();
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3600;
    if d > 0 {
        format!("{d}d {h}h")
    } else {
        format!("{h}h {:02}m", (secs % 3600) / 60)
    }
}

/// TaskStatus enum → stable lowercase string (used for icons/filters).
/// 枚举转字符串：match 穷举所有状态，返回静态字符串 `&'static str`。
fn status_txt(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in-progress",
        TaskStatus::Recurring => "recurring",
        TaskStatus::Completed => "completed",
        TaskStatus::Cancelled => "cancelled",
    }
}

/// Resolve status glyph: column-specific icons → per-kind global icons → builtin.
/// 图标回退链：列自定义 icons → [icons.todo]/[icons.event] → 内置图标 → 原文本。
fn icon(conf: &Config, kind: &str, val: &str, c: &Column) -> String {
    if let Some(g) = c.icons.get(val) {
        return g.clone();
    }
    let map = if kind == "event" {
        &conf.icons.event
    } else {
        &conf.icons.todo
    };
    if let Some(g) = map.get(val) {
        return g.clone();
    }
    builtin_status_icon(kind, val).unwrap_or_else(|| val.to_string())
}

/// Type column icon (kept for user-defined `field = "type"` columns).
/// 类型列图标（保留给用户自定义的 `field = "type"` 列使用）。
fn icon_type(c: &Column, ty: &str) -> String {
    if let Some(g) = c.icons.get(ty) {
        return g.clone();
    }
    builtin_type_icon(ty).unwrap_or_else(|| ty.to_string())
}

/// Default status glyph. Events are calendar items: any non-cancelled status
/// renders the calendar glyph; only `cancelled` is distinct.
/// 内置状态图标：event 非 cancelled 都用日历图标，cancelled 用 ✕。
fn builtin_status_icon(kind: &str, val: &str) -> Option<String> {
    if kind == "event" {
        return Some(if val == "cancelled" { "✕" } else { "󰃭" }.to_string());
    }
    let s = match val {
        "pending" => "○",
        "in-progress" => "●",
        "recurring" => "⟳",
        "completed" => "✓",
        "cancelled" => "✕",
        // 逾期状态的内置默认图标：叹号，表示“已过期”。
        // Builtin default glyph for overdue status: `!` (configurable via icons).
        "overdue" => "!",
        _ => return None,
    };
    Some(s.to_string())
}

/// Built-in glyphs for type columns (`todo` / `event`).
/// 内置类型图标：todo 用待办图标，event 用日历图标。
fn builtin_type_icon(val: &str) -> Option<String> {
    let s = match val {
        "todo" => "󰄰",
        "event" => "󰃭",
        _ => return None,
    };
    Some(s.to_string())
}

/// Extract a comparable value for one sort field.
/// 提取排序值：日期/优先级转数字，文本字段转字符串；缺失日期用 i64::MAX 沉底。
fn sort_val(r: &Row, field: &str) -> SVal {
    let t = &r.task;
    match field {
        "id" => SVal::Num(r.id as i64),
        "created" => SVal::Num(t.created_at.timestamp()),
        "updated" => SVal::Num(t.updated_at.timestamp()),
        "due" | "date" | "start" | "from" => SVal::Num(
            if t.is_event() {
                t.dtstart.map(|d| d.timestamp())
            } else {
                t.due.map(|d| d.timestamp())
            }
            .unwrap_or(i64::MAX),
        ),
        // 优先级排序值：H=1 < M=5 < 无优先级=7 < L=9（数字越小越靠前）。
        // Sort value: H=1 < M=5 < no-priority=7 < L=9 (lower sorts first).
        "pri" => SVal::Num(t.priority.unwrap_or(7) as i64),
        "recur" | "recurrence" => SVal::Str(
            t.rrule
                .as_deref()
                .map(crate::recurrence::rrule_period)
                .unwrap_or_default(),
        ),
        "status" => SVal::Num(match t.status {
            // 状态映射为数字：pending(0) … cancelled(4)，让排序稳定可预期。
            TaskStatus::Pending => 0,
            TaskStatus::InProgress => 1,
            TaskStatus::Recurring => 2,
            TaskStatus::Completed => 3,
            TaskStatus::Cancelled => 4,
        }),
        "type" => SVal::Str(if t.is_event() {
            "event".into()
        } else {
            "todo".into()
        }),
        "summary" => SVal::Str(t.summary.to_lowercase()),
        // 排序不区分大小写：先 to_lowercase() 再比较。
        _ => SVal::Str(String::new()),
    }
}

/// Compare two sort values; numbers sort before strings.
/// 比较两个排序值：数值 < 字符串（混合类型时数字排在前面）。
fn cmp_sval(a: &SVal, b: &SVal) -> Ordering {
    match (a, b) {
        (SVal::Num(x), SVal::Num(y)) => x.cmp(y),
        (SVal::Str(x), SVal::Str(y)) => x.cmp(y),
        (SVal::Num(_), SVal::Str(_)) => Ordering::Less,
        (SVal::Str(_), SVal::Num(_)) => Ordering::Greater,
    }
}

/// Right-pad a string to display width `w`.
/// 把字符串右补齐到显示宽度 `w`；等价于 `format!("{s:>w$}")` 但按显示宽度而非字符数。
fn pad(s: &str, w: usize) -> String {
    // Pad by terminal display width, not char count (CJK renders 2 cells).
    // 按终端显示宽度补齐，而不是按字符数：CJK 汉字占 2 格。
    // format! 支持字符串内插：{s} 输出原值，再拼接空格到目标宽度。
    let n = unicode_width::UnicodeWidthStr::width(s);
    if n >= w {
        return s.to_string();
    }
    format!("{s}{}", " ".repeat(w - n))
}

/// First matching `[colorscheme]` rule (in `priority` order) wraps the row.
/// Rules evaluated in precedence order; first hit wins.
/// 按 `priority` 顺序找第一条匹配的 `[colorscheme]` 规则并着色；先命中者优先。
fn colorize(conf: &Config, parents: &HashSet<&str>, r: &Row, line: &str) -> String {
    let Some(cs) = conf.colorscheme.as_ref() else {
        return line.to_string();
    };
    let order: Vec<&str> = if cs.priority.is_empty() {
        DEFAULT_PRIORITY.to_vec()
    } else {
        cs.priority.iter().map(String::as_str).collect()
    };
    for rule in order {
        if let Some(spec) = cs.rules.get(rule)
            && rule_matches(rule, parents, r)
        {
            return wrap_style(&rule_style_to_style(spec, &cs.palette), line);
        }
    }
    line.to_string()
}

/// Default rule precedence when `[colorscheme].priority` is empty.
/// 未配置 priority 时的默认规则顺序：已完成、取消、逾期、今天、优先级等。
const DEFAULT_PRIORITY: &[&str] = &[
    "completed", "cancelled", "overdue", "today", "due",
    "priority.H", "priority.M", "priority.L",
    "tagged", "blocked", "blocking",
];

/// Convert config RuleStyle → ANSI Style; resolve palette/color names.
/// 配置样式 → 内部 Style：颜色名经调色板解析成具体色值。
fn rule_style_to_style(rs: &crate::config::RuleStyle, palette: &HashMap<String, String>) -> Style {
    Style {
        bold: rs.bold,
        dim: rs.dim,
        italic: rs.italic,
        underline: rs.underline,
        inverse: rs.inverse,
        fg: rs.fg.as_deref().and_then(|c| color_code(c, palette)),
        bg: rs.bg.as_deref().and_then(|c| color_code(c, palette)),
    }
}

/// Taskwarrior-style rule semantics.
///
/// Note: per the `[colorscheme]` rules, `blocked` = parent todo (uid is
/// referenced by another task's `related_to`), `blocking` = sub todo (has a
/// parent via `related_to`).
/// Taskwarrior 风格规则语义：`blocked` 是被引用的父任务，`blocking` 是带父任务
/// `related_to` 的子任务。
fn rule_matches(key: &str, parents: &HashSet<&str>, r: &Row) -> bool {
    let t = &r.task;
    match key {
        "deleted" => false, // calman hard-deletes; no deleted state
        // calman 是硬删除，没有 deleted 状态，因此该规则永远不匹配。
        "completed" => t.status == TaskStatus::Completed,
        // 过期规则：仅 VTODO 且 due 的本地日历日早于今天，且未完成。
        // Overdue rule: VTODO only — due day before today and not done.
        "overdue" => !t.is_event() && t.due.is_some_and(|d| {
            d.with_timezone(&Local).date_naive() < Local::now().date_naive()
        }) && !t.status.is_done(),
        "today" => crate::filter::task_date(t).is_some_and(|d| {
            d.with_timezone(&Local).date_naive() == Local::now().date_naive()
        }),
        "due" => crate::filter::task_date(t).is_some(),
        "cancelled" => t.status == TaskStatus::Cancelled,
        "blocked" => parents.contains(t.uid.as_str()),
        "blocking" => t.related_to.is_some(),
        "tagged" => !t.tags.is_empty(),
        "priority.L" => t.priority == Some(9),
        "priority.M" => t.priority == Some(5),
        "priority.H" => t.priority == Some(1),
        _ => false,
    }
}

/// Parsed ANSI style: `fg [on bg] [bold|dim|italic|underline|inverse]`.
/// 解析后的 ANSI 样式：前景/背景色 + 粗体等修饰符。
#[derive(Default)]
struct Style {
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
    fg: Option<Code>,
    bg: Option<Code>,
}

/// ANSI color code: named 30-97, 256-color gray, or 24-bit RGB hex.
/// ANSI 颜色代码：命名色、256 色灰度、或 24 位 RGB 十六进制。
#[derive(Clone, Copy)]
enum Code {
    Named(u8),
    Gray(u8),
    Hex(u8, u8, u8),
}


/// Parse a color token: `#rrggbb`, palette alias, `grayN` (0-23), named color.
/// 解析颜色 token：`#rrggbb` 十六进制、调色板别名、`grayN` 灰度、命名色；失败返回 None。
fn color_code(name: &str, palette: &HashMap<String, String>) -> Option<Code> {
    let n = name.trim().to_ascii_lowercase();
    if let Some(hex) = n.strip_prefix('#') {
        // strip_prefix 用 if let 解构 Option；`?` 让解析失败直接返回 None。
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Code::Hex(r, g, b));
        }
        return None;
    }
    if let Some(v) = palette.get(&n) {
        return color_code(v, palette);
    }
    // HashMap::get 返回 Option<&String>；别名可递归解析（调色板引用调色板）。
    if let Some(g) = n.strip_prefix("gray").or_else(|| n.strip_prefix("grey")) {
        // or_else 闭包：兼容 gray/grey 前缀；后接 0-23 的灰度编号。
        let v: u8 = g.parse().ok()?;
        return (v <= 23).then_some(Code::Gray(v));
    }
    Some(Code::Named(match n.as_str() {
        "black" => 30,
        "red" => 31,
        "green" => 32,
        "yellow" => 33,
        "blue" => 34,
        "magenta" => 35,
        "cyan" => 36,
        "white" => 37,
        "bright-black" | "gray" | "grey" => 90,
        "bright-red" => 91,
        "bright-green" => 92,
        "bright-yellow" => 93,
        "bright-blue" => 94,
        "bright-magenta" => 95,
        "bright-cyan" => 96,
        "bright-white" => 97,
        _ => return None,
    }))
}

/// Wrap text in ANSI SGR codes: `ESC[<codes>m ... ESC[0m`.
/// 生成 ANSI 转义序列：空样式直接返回原文，不加任何控制字符。
fn wrap_style(st: &Style, text: &str) -> String {
    let mut codes: Vec<String> = Vec::new();
    if st.bold {
        codes.push("1".into());
    }
    if st.dim {
        codes.push("2".into());
    }
    if st.italic {
        codes.push("3".into());
    }
    if st.underline {
        codes.push("4".into());
    }
    if st.inverse {
        codes.push("7".into());
    }
    if let Some(Code::Hex(r, g, b)) = st.fg {
        codes.push(format!("38;2;{r};{g};{b}"));
    } else if let Some(Code::Named(n)) = st.fg {
        codes.push(n.to_string());
    } else if let Some(Code::Gray(g)) = st.fg {
        codes.push(format!("38;5;{}", 232 + g));
    }
    if let Some(Code::Hex(r, g, b)) = st.bg {
        codes.push(format!("48;2;{r};{g};{b}"));
    } else if let Some(Code::Named(n)) = st.bg {
        // 前景色 30-37，背景色 = 前景 + 10（30→40, 31→41 …）。
        codes.push((n + 10).to_string());
    } else if let Some(Code::Gray(g)) = st.bg {
        codes.push(format!("48;5;{}", 232 + g));
    }
    if codes.is_empty() {
        // 没有样式码时不加转义，保持输出纯净。
        return text.to_string();
    }
    format!("\x1b[{}m{}\x1b[0m", codes.join(";"), text)
}

// Tests: unit tests for sort/render/colorscheme/rc overrides (kept untouched).
// 测试模块：排序、渲染、配色、rc 覆盖的单元测试（保持原样未改动）。
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn row(id: usize, summary: &str) -> Row {
        Row {
            id,
            source: "work".into(),
            task: crate::model::Task::new("work", summary),
            occ: None,
        }
    }

    #[test]
    fn sorts_by_key() {
        let a = row(1, "a");
        let b = row(2, "b");
        let c = row(3, "c");
        let mut rows: Vec<&Row> = vec![&b, &a, &c];
        sort_rows(&mut rows, &[parse_sort("id+")]);
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn priority_sort_places_no_priority_between_m_and_l() {
        let mut h = row(1, "h");
        h.task.priority = Some(1);
        let mut m = row(2, "m");
        m.task.priority = Some(5);
        let none = row(3, "none"); // no priority
        let mut l = row(4, "l");
        l.task.priority = Some(9);

        // asc: H(1) < M(5) < none(7) < L(9)
        let mut rows: Vec<&Row> = vec![&l, &none, &m, &h];
        sort_rows(&mut rows, &[parse_sort("pri+")]);
        assert_eq!(
            rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );

        // desc: L(9) > none(7) > M(5) > H(1)
        sort_rows(&mut rows, &[parse_sort("pri-")]);
        assert_eq!(
            rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![4, 3, 2, 1]
        );
    }

    #[test]
    fn renders_header_and_rows() {
        let conf = Config::default();
        let report = Report::resolve("ls", &conf);
        let one = row(1, "buy milk");
        let rows: Vec<&Row> = vec![&one];
        let out = render(&conf, &report, &rows);
        assert!(out.contains("ID"));
        assert!(out.contains("1"));
        assert!(out.contains("buy milk"));
    }

    #[test]
    fn overdue_row_shows_date_in_date_col_and_marker_in_status() {
        use chrono::{Duration, Utc};
        let conf = Config::default();
        let mut r = row(1, "missed");
        r.task.due = Some(Utc::now() - Duration::hours(5));
        let date_col = Column {
            field: "date".into(),
            label: "DATE".into(),
            width: None,
            format: None,
            icon: false,
            icons: Default::default(),
            event_format: None,
            todo_format: Some("relative".into()),
        };
        let status_col = Column {
            field: "status".into(),
            label: "ST".into(),
            width: None,
            format: None,
            icon: false,
            icons: Default::default(),
            event_format: None,
            todo_format: None,
        };
        let d = cell(&conf, &r, &date_col);
        assert!(!d.contains("overdue"));
        assert_eq!(d.len(), 5); // MM/DD
        let s = cell(&conf, &r, &status_col);
        assert_eq!(s, "overdue");
        // Icon form uses the builtin marker and is overridable per column.
        let mut icon_col = status_col.clone();
        icon_col.icon = true;
        assert_eq!(cell(&conf, &r, &icon_col), "!");
        icon_col.icons.insert("overdue".into(), "⚠".into());
        assert_eq!(cell(&conf, &r, &icon_col), "⚠");
    }

    #[test]
    fn config_override_without_filter_inherits_builtin_filter() {
        use crate::config::{ColumnCfg, ReportCfg};
        let mut conf = Config::default();
        conf.reports.insert(
            "list".to_string(),
            ReportCfg {
                filter: None,
                sort: Vec::new(),
                columns: vec![ColumnCfg {
                    field: "id".into(),
                    label: Some("ID".into()),
                    width: None,
                    format: None,
                    icon: false,
                    icons: Default::default(),
                    event_format: None,
                    todo_format: None,
                }],
            },
        );
        let report = Report::resolve("list", &conf);
        assert!(report.filter.contains("-status:recurring"));
        assert_eq!(report.columns.len(), 1); // columns override still applies
    }

    #[test]
    fn resolves_color_codes_and_hex() {
        let palette = HashMap::from([("blue".to_string(), "#1e90ff".to_string())]);
        assert!(matches!(color_code("gray10", &palette), Some(Code::Gray(10))));
        assert!(matches!(
            color_code("blue", &palette),
            Some(Code::Hex(0x1e, 0x90, 0xff))
        ));
        assert!(matches!(
            color_code("#00ff00", &palette),
            Some(Code::Hex(0, 0xff, 0))
        ));
        assert!(matches!(color_code("red", &palette), Some(Code::Named(31))));
    }

    #[test]
    fn colorscheme_colorizes_row_by_precedence() {
        let conf = Config {
            colorscheme: Some(crate::config::ColorSchemeCfg {
                priority: vec!["completed".into(), "overdue".into()],
                palette: HashMap::new(),
                rules: HashMap::from([
                    (
                        "completed".to_string(),
                        crate::config::RuleStyle {
                            fg: Some("gray10".into()),
                            bg: Some("gray2".into()),
                            ..Default::default()
                        },
                    ),
                    (
                        "overdue".to_string(),
                        crate::config::RuleStyle {
                            inverse: true,
                            ..Default::default()
                        },
                    ),
                ]),
            }),
            ..Config::default()
        };
        let mut t = crate::model::Task::new("work", "done");
        t.status = crate::model::TaskStatus::Completed;
        let r = Row {
            id: 1,
            source: "work".into(),
            task: t,
            occ: None,
        };
        let parents = HashSet::new();
        let line = colorize(&conf, &parents, &r, "row");
        assert!(line.starts_with("\x1b[38;5;242;48;5;234m")); // gray10 on gray2
        assert!(line.ends_with("\x1b[0m"));
    }

    #[test]
    fn apply_rc_overrides_columns_and_labels() {
        let conf = Config::default();
        let mut report = Report::resolve("next", &conf);
        let rcs = vec![
            RcReport {
                name: "next".into(),
                key: "columns".into(),
                value: "id,date,summary".into(),
            },
            RcReport {
                name: "next".into(),
                key: "labels".into(),
                value: "ID,DATE,TASK".into(),
            },
        ];
        apply_rc(&mut report, &rcs, "next").unwrap();
        assert_eq!(report.columns.len(), 3);
        assert_eq!(report.columns[0].field, "id");
        assert_eq!(report.columns[1].label, "DATE");
        assert_eq!(report.columns[2].label, "TASK");
        assert_eq!(report.columns[1].field, "date");
    }

    #[test]
    fn unsupported_rc_key_errors() {
        let conf = Config::default();
        let mut report = Report::resolve("next", &conf);
        let rcs = vec![RcReport {
            name: "next".into(),
            key: "bogus".into(),
            value: "x".into(),
        }];
        assert!(apply_rc(&mut report, &rcs, "next").is_err());
    }

    #[test]
    fn default_report_labels_are_date_and_merged_status() {
        let conf = Config::default();
        let report = Report::resolve("next", &conf);
        assert!(report.columns.iter().any(|c| c.label == "DATE"));
        assert!(report.columns.iter().any(|c| c.field == "status"));
        assert!(report.columns.iter().all(|c| c.field != "type"));
    }

    #[test]
    fn event_uses_status_glyph_and_todo_relative_in_date() {
        let conf = Config::default();
        let report = Report::resolve("next", &conf);
        let mut ev = crate::model::Task::new("work", "meet");
        ev.dtstart = Some(chrono::Utc::now() + chrono::Duration::days(1));
        let r = Row {
            id: 1,
            source: "work".into(),
            task: ev,
            occ: None,
        };
        let cells: Vec<String> = report.columns.iter().map(|c| cell(&conf, &r, c)).collect();
        // status col → event calendar glyph (built-in default for events),
        // date col → plain MM/DD
        assert_eq!(cells[1], "󰃭");
        assert_eq!(cells[2].len(), 5); // MM/DD
    }

    #[test]
    fn per_kind_icon_tables_apply() {
        let mut conf = Config::default();
        conf.icons.todo.insert("pending".into(), "T".into());
        conf.icons.event.insert("pending".into(), "E".into());
        let report = Report::resolve("next", &conf);
        let mut ev = crate::model::Task::new("work", "meet");
        ev.dtstart = Some(chrono::Utc::now() + chrono::Duration::days(1));
        let r = Row {
            id: 1,
            source: "work".into(),
            task: ev,
            occ: None,
        };
        let ev_cells: Vec<String> = report.columns.iter().map(|c| cell(&conf, &r, c)).collect();
        assert_eq!(ev_cells[1], "E");

        let t = Row {
            id: 2,
            source: "work".into(),
            task: crate::model::Task::new("work", "chore"),
            occ: None,
        };
        let t_cells: Vec<String> = report.columns.iter().map(|c| cell(&conf, &t, c)).collect();
        assert_eq!(t_cells[1], "T");
    }
}
