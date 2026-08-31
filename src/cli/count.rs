//! `count` subcommand handler.

use crate::args::ParsedArgs;
use crate::cli::resolve_sources;
use crate::config::{Config, ContextKind};
use crate::filter::parse_expr;
use anyhow::Result;

pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    let sources = resolve_sources(conf, override_, ContextKind::Cli)?;
    let rows = crate::cli::load_merged(conf, &sources)?;
    #[cfg(not(feature = "recur-expand"))]
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|r| r.task.recurrence_id.is_none())
        .collect();
    let filter = parse_expr(&q.filter_tokens)?;
    let n = rows
        .iter()
        .filter(|r| filter.matches_with(&r.task))
        .count();
    println!("{n}");
    Ok(())
}
