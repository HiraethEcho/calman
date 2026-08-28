//! Report subcommand handler (default output of `calman`).
//!
//! `ls`/`list`/`next` (bare `calman` → `[defaults] default_report`, default
//! `next`). Renders via `report` engine with filters, sort, icons, colors.

use crate::args::ParsedArgs;
use crate::cli::{Row, resolve_sources};
use crate::config::{Config, ContextKind};
use crate::filter::parse_expr;
use crate::report::{self, Report};
use anyhow::Result;

pub fn run(conf: &Config, q: &ParsedArgs, report_name: &str) -> Result<()> {
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    let sources = resolve_sources(conf, override_, ContextKind::Cli)?;
    let rows = crate::cli::load_merged_expanded(conf, &sources)?;
    #[cfg(not(feature = "recur-expand"))]
    let rows: Vec<Row> = rows
        .into_iter()
        .filter(|r| r.task.recurrence_id.is_none())
        .collect();

    let cli_filter = parse_expr(&q.filter_tokens)?;
    let mut report = Report::resolve(report_name, conf);
    report::apply_rc(&mut report, &q.rc_reports, report_name)?;
    let report_filter = if wants_recurring(&q.filter_tokens) {
        // User explicitly asked for series parents/occurrences: drop the
        // default `-status:recurring` exclusion so `+PARENT` etc. can match.
        let f = report.filter.replace("-status:recurring", "");
        crate::filter::parse_expr_str(&f)?
    } else {
        report.filter()?
    };

    let mut selected: Vec<&Row> = rows
        .iter()
        .filter(|r| {
            cli_filter.matches_with(&r.task, conf.date.due_date_overdue_today)
                && report_filter.matches_with(&r.task, conf.date.due_date_overdue_today)
        })
        .collect();
    report::sort_rows(&mut selected, &report.sort);
    print!("{}", report::render(conf, &report, &selected));
    Ok(())
}

/// Does the CLI filter explicitly mention recurring series (`PARENT`/`status:recurring`)?
fn wants_recurring(tokens: &[String]) -> bool {
    tokens.iter().any(|t| {
        let l = t.to_ascii_lowercase();
        l.contains("parent") || l.contains("status:recurring")
    })
}
