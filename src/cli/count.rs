//! `count` subcommand handler.

use crate::args::ParsedArgs;
use crate::cli::resolve_sources;
use crate::config::{Config, ContextKind};
use crate::filter::Filter;
use anyhow::Result;

pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    let sources = resolve_sources(conf, override_, ContextKind::Cli)?;
    let rows = crate::cli::load_merged(&sources)?;
    let filter = Filter::from_parsed(q);
    let n = rows.iter().filter(|r| filter.matches(&r.task)).count();
    println!("{n}");
    Ok(())
}
