//! `count` 子命令：只打印匹配数量。
//! `count` subcommand handler.
//!
//! 数据流：解析源 → 合并任务 → 应用过滤器 → 数个数 → 打印数字。

use crate::args::ParsedArgs;
use crate::cli::resolve_sources;
use crate::config::{Config, ContextKind};
use crate::filter::parse_expr;
use anyhow::Result;

/// 执行 count：统计满足过滤条件的任务条数。
///
/// `q.filter_tokens` 是命令行里的过滤词；`parse_expr` 把它们编译成可复用的过滤器。
pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    // 与 list 相同的源解析逻辑：显式 source 覆盖默认源。
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    let sources = resolve_sources(conf, override_, ContextKind::Cli)?;
    let rows = crate::cli::load_merged(conf, &sources)?;
    #[cfg(not(feature = "recur-expand"))]
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|r| r.task.recurrence_id.is_none())
        .collect();
    let filter = parse_expr(&q.filter_tokens)?;
    // 迭代器链：filter 逐个判断，count() 数出通过的数量。
    let n = rows
        .iter()
        .filter(|r| filter.matches_with(&r.task))
        .count();
    println!("{n}");
    Ok(())
}
