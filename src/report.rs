//! Report engine: config/builtin report definitions, column rendering,
//! nerdfont icons and global row-level color rules.

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
#[derive(Debug, Clone)]
pub struct SortKey {
    pub field: String,
    pub asc: bool,
    pub brk: bool,
}

/// A resolved report (config overrides builtin).
#[derive(Debug, Clone)]
pub struct Report {
    pub filter: String,
    pub sort: Vec<SortKey>,
    pub columns: Vec<Column>,
}

/// Comparable value for sorting (numeric preferred over string).
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
    pub fn filter(&self) -> Result<Expr> {
        if self.filter.trim().is_empty() {
            return Ok(Expr::Atom(crate::filter::Filter::default()));
        }
        parse_expr_str(&self.filter)
    }
}

fn from_config(cfg: &crate::config::ReportCfg) -> Report {
    Report {
        filter: cfg.filter.clone().unwrap_or_default(),
        sort: cfg.sort.iter().map(|s| parse_sort(s)).collect(),
        columns: cfg.columns.iter().map(from_column).collect(),
    }
}

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
    let parents: HashSet<&str> = rows
        .iter()
        .filter_map(|r| r.task.related_to.as_deref())
        .collect();

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

fn cell(conf: &Config, r: &Row, c: &Column) -> String {
    let t = &r.task;
    match c.field.as_str() {
        "id" => r.id.to_string(),
        // Merged type/status: both kinds show a status glyph; the per-kind
        // global tables `[icons.todo]` / `[icons.event]` pick the glyph.
        "status" => {
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
        "date" | "due" | "start" | "from" => date_str(r, c),
        "pri" => pri_str(t.priority),
        "source" => t.source.clone(),
        "recur" | "recurrence" => t
            .rrule
            .as_deref()
            .map(crate::recurrence::rrule_period)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn maybe_truncate(s: &str, c: &Column) -> String {
    let s = s.replace('\n', " ");
    if c.format.as_deref() == Some("truncate") {
        let max = c.width.unwrap_or(30);
        // Truncate by terminal display width, keeping whole chars.
        let mut w = 0;
        let mut cut = String::new();
        for ch in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw + 1 > max {
                // Reserve one cell for the ellipsis.
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

fn date_str(r: &Row, c: &Column) -> String {
    let dt = crate::filter::task_date(&r.task);
    let Some(dt) = dt else {
        return String::new();
    };
    let local = dt.with_timezone(&Local);
    if r.task.is_event() {
        // Event: plain date by default, customisable via `event_format`.
        return match c.event_format.as_deref() {
            Some(f) => fmt_date(&local, f),
            None => local.format("%m/%d").to_string(),
        };
    }
    if r.task.allday {
        // All-day todos: compare by calendar day so the display agrees with
        // the `+OVERDUE` filter (date-only due is overdue the day after).
        return allday_date_str(c, &local);
    }
    match c.todo_format.as_deref().or(c.format.as_deref()) {
        Some("relative") => {
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
            let now = Local::now();
            if local < now {
                local.format("%m/%d").to_string()
            } else {
                countdown(&local, &now)
            }
        }
        Some(f) => fmt_date(&local, f),
        None => local.format("%Y-%m-%d").to_string(),
    }
}

/// All-day date column: day-granular relative/countdown, else formatted date.
fn allday_date_str(c: &Column, local: &DateTime<Local>) -> String {
    let day = local.date_naive();
    let today = Local::now().date_naive();
    let days = day.signed_duration_since(today).num_days();
    match c.todo_format.as_deref().or(c.format.as_deref()) {
        Some("relative") | Some("countdown") => {
            if days < 0 {
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
fn fmt_date(dt: &DateTime<Local>, spec: &str) -> String {
    match spec {
        "iso" => dt.format("%Y-%m-%d").to_string(),
        "date" => dt.format("%m/%d").to_string(),
        s => dt.format(s).to_string(),
    }
}

fn pri_str(p: Option<u8>) -> String {
    match p {
        Some(1) => "H".into(),
        Some(5) => "M".into(),
        Some(9) => "L".into(),
        Some(n) => n.to_string(),
        None => String::new(),
    }
}

fn relative(dt: &DateTime<Local>, now: &DateTime<Local>) -> String {
    let diff = *dt - *now;
    use chrono::Duration;
    let secs = diff.num_seconds();
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

fn status_txt(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in-progress",
        TaskStatus::Recurring => "recurring",
        TaskStatus::Completed => "completed",
        TaskStatus::Cancelled => "cancelled",
    }
}

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
fn icon_type(c: &Column, ty: &str) -> String {
    if let Some(g) = c.icons.get(ty) {
        return g.clone();
    }
    builtin_type_icon(ty).unwrap_or_else(|| ty.to_string())
}

/// Default status glyph. Events are calendar items: any non-cancelled status
/// renders the calendar glyph; only `cancelled` is distinct.
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
        "overdue" => "!",
        _ => return None,
    };
    Some(s.to_string())
}

fn builtin_type_icon(val: &str) -> Option<String> {
    let s = match val {
        "todo" => "󰄰",
        "event" => "󰃭",
        _ => return None,
    };
    Some(s.to_string())
}

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
        "pri" => SVal::Num(t.priority.unwrap_or(0) as i64),
        "recur" | "recurrence" => SVal::Str(
            t.rrule
                .as_deref()
                .map(crate::recurrence::rrule_period)
                .unwrap_or_default(),
        ),
        "status" => SVal::Num(match t.status {
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
        _ => SVal::Str(String::new()),
    }
}

fn cmp_sval(a: &SVal, b: &SVal) -> Ordering {
    match (a, b) {
        (SVal::Num(x), SVal::Num(y)) => x.cmp(y),
        (SVal::Str(x), SVal::Str(y)) => x.cmp(y),
        (SVal::Num(_), SVal::Str(_)) => Ordering::Less,
        (SVal::Str(_), SVal::Num(_)) => Ordering::Greater,
    }
}

fn pad(s: &str, w: usize) -> String {
    // Pad by terminal display width, not char count (CJK renders 2 cells).
    let n = unicode_width::UnicodeWidthStr::width(s);
    if n >= w {
        return s.to_string();
    }
    format!("{s}{}", " ".repeat(w - n))
}

/// First matching `[colorscheme]` rule (in `priority` order) wraps the row.
/// Rules evaluated in precedence order; first hit wins.
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
const DEFAULT_PRIORITY: &[&str] = &[
    "completed", "cancelled", "overdue", "today", "due",
    "priority.H", "priority.M", "priority.L",
    "tagged", "blocked", "blocking",
];

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
fn rule_matches(key: &str, parents: &HashSet<&str>, r: &Row) -> bool {
    let t = &r.task;
    match key {
        "deleted" => false, // calman hard-deletes; no deleted state
        "completed" => t.status == TaskStatus::Completed,
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

#[derive(Clone, Copy)]
enum Code {
    Named(u8),
    Gray(u8),
    Hex(u8, u8, u8),
}


fn color_code(name: &str, palette: &HashMap<String, String>) -> Option<Code> {
    let n = name.trim().to_ascii_lowercase();
    if let Some(hex) = n.strip_prefix('#') {
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
    if let Some(g) = n.strip_prefix("gray").or_else(|| n.strip_prefix("grey")) {
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
        codes.push((n + 10).to_string());
    } else if let Some(Code::Gray(g)) = st.bg {
        codes.push(format!("48;5;{}", 232 + g));
    }
    if codes.is_empty() {
        return text.to_string();
    }
    format!("\x1b[{}m{}\x1b[0m", codes.join(";"), text)
}

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
