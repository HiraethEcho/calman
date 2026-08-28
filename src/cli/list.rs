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
    let rows = crate::cli::load_merged(&sources)?;

    let cli_filter = parse_expr(&q.filter_tokens)?;
    let mut report = Report::resolve(report_name, conf);
    report::apply_rc(&mut report, &q.rc_reports, report_name)?;
    let report_filter = report.filter()?;

    let mut selected: Vec<&Row> = rows
        .iter()
        .filter(|r| cli_filter.matches(&r.task) && report_filter.matches(&r.task))
        .collect();
    report::sort_rows(&mut selected, &report.sort);
    print!("{}", report::render(conf, &report, &selected));
    Ok(())
}
