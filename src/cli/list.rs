//! `list` subcommand handler (default report).

use crate::args::ParsedArgs;
use crate::cli::{Row, resolve_sources};
use crate::config::{Config, ContextKind};
use crate::filter::Filter;
use anyhow::Result;
use chrono::Local;

pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    let sources = resolve_sources(conf, override_, ContextKind::Cli)?;
    let rows = crate::cli::load_merged(&sources)?;
    let filter = Filter::from_parsed(q);
    let rows: Vec<&Row> = rows.iter().filter(|r| filter.matches(&r.task)).collect();
    print_table(&rows);
    Ok(())
}

fn print_table(rows: &[&Row]) {
    if rows.is_empty() {
        println!("(no tasks)");
        return;
    }
    for r in rows {
        let t = &r.task;
        let status = format!("{:?}", t.status).to_lowercase();
        let prio = t.priority.map(|p| p.to_string()).unwrap_or_default();
        let due = if t.is_event() {
            t.dtstart
                .map(|d| d.with_timezone(&Local).format("%Y-%m-%d").to_string())
                .unwrap_or_default()
        } else {
            t.due
                .map(|d| d.with_timezone(&Local).format("%Y-%m-%d").to_string())
                .unwrap_or_default()
        };
        let tags = t.tags.join(",");
        println!(
            "{:<4} {:<12} {:<3} {:<10} {:<20} {}",
            r.id, status, prio, due, tags, t.summary
        );
    }
}
