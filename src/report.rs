//! Report engine: config/builtin report definitions, column rendering,
//! nerdfont icons and global row-level color rules.

use crate::args::RcReport;
use crate::cli::Row;
use crate::config::{ColumnCfg, Config};
use crate::filter::{Expr, parse_expr_str};
use crate::model::TaskStatus;
use anyhow::{Result, bail};
use chrono::{DateTime, Local, Utc};
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
    pub fn resolve(name: &str, conf: &Config) -> Report {
        match conf.reports.get(name) {
            Some(cfg) => from_config(cfg),
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
                    if matches!(c.field.as_str(), "date" | "due") && c.format.is_none() {
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
/// (including today), the type/status columns are merged (event = calendar
/// icon), and DUE is relabelled DATE (event → plain date, todo → relative).
fn builtin(name: &str) -> Report {
    let (filter, sort, columns) = match name {
        "ls" => (
            "type:todo status:active or type:event due.after:sod".to_string(),
            vec!["due+", "created+"],
            vec![
                col("id", "ID", Some(4), None, false),
                col("status", "ST", None, None, true),
                date_col("DATE", "relative"),
                col("summary", "SUMMARY", None, None, false),
            ],
        ),
        "list" => (
            "type:todo -status:completed -status:cancelled or type:event due.after:sod".to_string(),
            vec!["status-", "pri-", "due+"],
            vec![
                col("id", "ID", Some(4), None, false),
                col("status", "STATUS", None, None, true),
                col("pri", "PRI", None, None, false),
                date_col("DATE", "relative"),
                col("tags", "TAGS", None, None, false),
                col("summary", "SUMMARY", None, None, false),
                col("desc", "DESC", None, None, false),
            ],
        ),
        "next" => (
            "type:todo status:active or type:event due.after:sod".to_string(),
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
        let mut w = c.label.chars().count();
        for row in &cells {
            w = w.max(row[ci].chars().count());
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
        // Merged type/status: events show a calendar glyph, todos show their
        // status (feature.md `more on report`).
        "status" => {
            if t.is_event() {
                return if c.icon {
                    icon(conf, "type", "event", c)
                } else {
                    "event".to_string()
                };
            }
            let st = status_txt(t.status);
            if c.icon {
                icon(conf, "status", st, c)
            } else {
                st.to_string()
            }
        }
        "type" => {
            let ty = if t.is_event() { "event" } else { "todo" };
            if c.icon {
                icon(conf, "type", ty, c)
            } else {
                ty.to_string()
            }
        }
        "summary" => maybe_truncate(t.summary.as_str(), c),
        "desc" => maybe_truncate(t.description.as_deref().unwrap_or(""), c),
        "tags" => t.tags.join(","),
        "date" => date_str(r, c),
        // Backwards-compatible alias: `due` == `date`.
        "due" => date_str(r, c),
        "pri" => pri_str(t.priority),
        "source" => t.source.clone(),
        _ => String::new(),
    }
}

fn maybe_truncate(s: &str, c: &Column) -> String {
    if c.format.as_deref() == Some("truncate") {
        let max = c.width.unwrap_or(30);
        let n = s.chars().count();
        if n > max {
            let cut: String = s.chars().take(max.saturating_sub(1)).collect();
            return format!("{cut}…");
        }
    }
    s.to_string()
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
    match c.todo_format.as_deref().or(c.format.as_deref()) {
        Some("relative") => relative(&local, &Local::now()),
        Some("countdown") => countdown(&local, &Local::now()),
        Some(f) => fmt_date(&local, f),
        None => local.format("%Y-%m-%d").to_string(),
    }
}

/// Format a date: `iso`/`date` keywords or any chrono strftime pattern.
fn fmt_date(dt: &DateTime<Local>, spec: &str) -> String {
    match spec {
        "iso" => dt.format("%Y-%m-%d").to_string(),
        "date" => dt.format("%m/%d").to_string(),
        s if s.contains('%') => dt.format(s).to_string(),
        s => dt.format(s).to_string(),
    }
}

fn pri_str(p: Option<u8>) -> String {
    match p {
        Some(9) => "H".into(),
        Some(5) => "M".into(),
        Some(1) => "L".into(),
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
        TaskStatus::Completed => "completed",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn icon(conf: &Config, field: &str, val: &str, c: &Column) -> String {
    if let Some(g) = c.icons.get(val) {
        return g.clone();
    }
    match field {
        "status" => {
            if let Some(g) = conf.icons.status.get(val) {
                return g.clone();
            }
        }
        "type" => {
            if let Some(g) = conf.icons.r#type.get(val) {
                return g.clone();
            }
        }
        _ => {}
    }
    builtin_icon(field, val).unwrap_or_else(|| val.to_string())
}

fn builtin_icon(field: &str, val: &str) -> Option<String> {
    let s = match field {
        "status" => match val {
            "pending" => "○",
            "in-progress" => "●",
            "completed" => "✓",
            "cancelled" => "✕",
            _ => return None,
        },
        "type" => match val {
            "todo" => "󰄰",
            "event" => "󰃭",
            _ => return None,
        },
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
        "due" | "date" => SVal::Num(
            if t.is_event() {
                t.dtstart.map(|d| d.timestamp())
            } else {
                t.due.map(|d| d.timestamp())
            }
            .unwrap_or(i64::MAX),
        ),
        "pri" => SVal::Num(t.priority.unwrap_or(0) as i64),
        "status" => SVal::Num(match t.status {
            TaskStatus::Pending => 0,
            TaskStatus::InProgress => 1,
            TaskStatus::Completed => 2,
            TaskStatus::Cancelled => 3,
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
    let n = s.chars().count();
    if n >= w {
        return s.to_string();
    }
    format!("{s}{}", " ".repeat(w - n))
}

/// First matching theme color rule (per `rule.precedence.color`) wraps the
/// whole row in ANSI codes.
fn colorize(conf: &Config, parents: &HashSet<&str>, r: &Row, line: &str) -> String {
    let Some(theme) = conf.theme.as_ref() else {
        return line.to_string();
    };
    let Some(prec) = theme.precedence.as_deref() else {
        return line.to_string();
    };
    for token in prec.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        for (key, spec) in &theme.colors {
            if key == token || (token == "uda." && key.starts_with("uda.")) {
                if rule_matches(key, parents, r) {
                    return wrap_style(&parse_style(spec), line);
                }
            }
        }
    }
    line.to_string()
}

/// Taskwarrior-style rule semantics.
///
/// Note: per the reference `custom.theme`, `blocked` = parent todo (uid is
/// referenced by another task's `related_to`), `blocking` = sub todo (has a
/// parent via `related_to`).
fn rule_matches(key: &str, parents: &HashSet<&str>, r: &Row) -> bool {
    let t = &r.task;
    match key {
        "deleted" => false, // calman hard-deletes; no deleted state
        "completed" => t.status == TaskStatus::Completed,
        "active" => t.status == TaskStatus::InProgress,
        "overdue" => t.due.is_some_and(|d| d < Utc::now()) && !t.status.is_done(),
        "due.today" => t
            .due
            .is_some_and(|d| d.with_timezone(&Local).date_naive() == Local::now().date_naive()),
        "due" => t.due.is_some(),
        "blocked" => parents.contains(t.uid.as_str()),
        "blocking" => t.related_to.is_some(),
        "scheduled" => t.is_event(),
        "tagged" => !t.tags.is_empty(),
        k if k.starts_with("uda.priority.") => {
            let lvl = k.rsplit('.').next().unwrap_or("");
            t.priority
                == Some(match lvl {
                    "H" => 9,
                    "M" => 5,
                    "L" => 1,
                    _ => return false,
                })
        }
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
}

fn parse_style(s: &str) -> Style {
    let mut st = Style::default();
    let mut fg_side = true;
    for tok in s.split_whitespace() {
        if tok.eq_ignore_ascii_case("on") {
            fg_side = false;
            continue;
        }
        match tok.to_ascii_lowercase().as_str() {
            "bold" => st.bold = true,
            "dim" => st.dim = true,
            "italic" => st.italic = true,
            "underline" => st.underline = true,
            "inverse" => st.inverse = true,
            _ => {
                if let Some(c) = color_code(tok) {
                    if fg_side {
                        st.fg = Some(c);
                    } else {
                        st.bg = Some(c);
                    }
                }
            }
        }
    }
    st
}

fn color_code(name: &str) -> Option<Code> {
    let n = name.to_ascii_lowercase();
    if let Some(g) = n.strip_prefix("gray") {
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
    if let Some(Code::Named(n)) = st.fg {
        codes.push(n.to_string());
    }
    if let Some(Code::Gray(g)) = st.fg {
        codes.push(format!("38;5;{}", 232 + g));
    }
    if let Some(Code::Named(n)) = st.bg {
        codes.push((n + 10).to_string());
    }
    if let Some(Code::Gray(g)) = st.bg {
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
    fn parses_tw_style_strings() {
        let st = parse_style("gray10 on gray2");
        assert!(matches!(st.fg, Some(Code::Gray(10))));
        assert!(matches!(st.bg, Some(Code::Gray(2))));
        let st2 = parse_style("gray21 bold");
        assert!(st2.bold);
        assert!(matches!(st2.fg, Some(Code::Gray(21))));
        assert!(st2.bg.is_none());
        assert!(parse_style("inverse").inverse);
    }

    #[test]
    fn theme_colorizes_row_by_precedence() {
        let mut conf = Config::default();
        conf.theme = Some(crate::config::ThemeCfg {
            name: Some("t".into()),
            precedence: Some("completed,overdue".into()),
            colors: HashMap::from([
                ("completed".to_string(), "gray10 on gray2".to_string()),
                ("overdue".to_string(), "inverse".to_string()),
            ]),
        });
        let mut t = crate::model::Task::new("work", "done");
        t.status = crate::model::TaskStatus::Completed;
        let r = Row {
            id: 1,
            source: "work".into(),
            task: t,
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
    fn event_renders_calendar_in_status_and_todo_relative_in_date() {
        let conf = Config::default();
        let report = Report::resolve("next", &conf);
        let mut ev = crate::model::Task::new("work", "meet");
        ev.dtstart = Some(chrono::Utc::now() + chrono::Duration::days(1));
        let r = Row {
            id: 1,
            source: "work".into(),
            task: ev,
        };
        let cells: Vec<String> = report.columns.iter().map(|c| cell(&conf, &r, c)).collect();
        // status col → event calendar glyph (nerdfont), date col → plain MM/DD
        assert_eq!(cells[1], "󰃭");
        assert_eq!(cells[2].len(), 5); // MM/DD
    }
}
