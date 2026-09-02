//! 报告子命令处理器（`calman` 的默认输出）。
//! Report subcommand handler (default output of `calman`).
//!
//! `ls`/`list`/`next`（裸 `calman` → `[defaults] default_report`，默认 `next`）。
//! 通过 report 引擎渲染：过滤器、排序、图标、颜色。

use crate::args::ParsedArgs;
use crate::cli::{Row, resolve_sources};
use crate::config::{Config, ContextKind};
use crate::filter::parse_expr;
use crate::report::{self, Report};
use anyhow::Result;

/// 执行列表命令。`report_name` 来自命令行（ls/list/next 或 rc 覆盖）。
///
/// 数据流：解析源 → 合并展开所有行 → 解析 CLI 过滤器 → 合并 report 过滤器 → 排序 → 渲染。
pub fn run(conf: &Config, q: &ParsedArgs, report_name: &str) -> Result<()> {
    // `then_some`：只有用户显式传了 `source:` 时才覆盖默认源。
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    let sources = resolve_sources(conf, override_, ContextKind::Cli)?;
    let rows = crate::cli::load_merged_expanded(conf, &sources)?;
    #[cfg(not(feature = "recur-expand"))]
    let rows: Vec<Row> = rows
        .into_iter()
        .filter(|r| r.task.recurrence_id.is_none())
        .collect();

    let cli_filter = parse_expr(&q.filter_tokens)?; // 命令行里的过滤词，如 +PENDING、+tag
    let mut report = Report::resolve(report_name, conf);
    report::apply_rc(&mut report, &q.rc_reports, report_name)?; // rc.report.* 覆盖
    let report_filter = if wants_include(&q.filter_tokens) {
        // 用户显式要求显示系列父任务/等待项：去掉 report 默认排除规则。
        // User explicitly asked for series parents/waiting items: drop the
        // default `-status:recurring` / `-WAITING` exclusions so they match.
        let f = report
            .filter
            .replace("-status:recurring", "")
            .replace("-WAITING", "");
        crate::filter::parse_expr_str(&f)?
    } else {
        report.filter()?
    };

    // 两层过滤都通过的行才显示；闭包 `|r| ...` 接收每一行并返回 bool。
    let mut selected: Vec<&Row> = rows
        .iter()
        .filter(|r| {
            cli_filter.matches_with(&r.task)
                && report_filter.matches_with(&r.task)
        })
        .collect();
    report::sort_rows(&mut selected, &report.sort);
    print!("{}", report::render(conf, &report, &selected));
    Ok(())
}

/// CLI 过滤器是否显式提到了默认隐藏的特性（`PARENT`/`status:recurring`/`WAITING`）？
/// 是则去掉 report 的默认排除，让用户的 `+...` 能匹配到。
/// Does the CLI filter explicitly mention hidden-by-default features
/// (`PARENT`/`status:recurring`/`WAITING`)? If so, drop the report's default
/// exclusion so the user's `+...` can match.
fn wants_include(tokens: &[String]) -> bool {
    tokens.iter().any(|t| {
        // any：只要有一个 token 命中关键词就返回 true。
        let l = t.to_ascii_lowercase();
        l.contains("parent")
            || l.contains("status:recurring")
            || l.contains("waiting")
    })
}
