//! calman — minimalist, keyboard-driven task manager.
//! calman — 极简、键盘驱动的任务管理器 (task manager)。
//!
//! Taskwarrior-style CLI: free-form filters/attributes, default report = list.
//! Taskwarrior 风格命令行：自由格式过滤器/属性，默认报告为 list。
//!
//! 程序整体流程 (program flow)：
//! 读参数 (parse args) → 载入配置 (load config) → 解析自由格式参数 (parse free-form args)
//! → 分发到子命令 (dispatch to subcommand) → 打印结果 (print results)。
//! 具体实现见 `run()` 的逐步注释 (step-by-step comments in `run()`)。

// `mod` 声明模块 (module)：Rust 按文件拆分代码，`mod args;` 引入 src/args.rs。
// 之后通过 `args::xxx` 访问该模块里 pub 的项。
mod args;
mod cli;
mod config;
mod date;
mod filter;
mod model;
mod recurrence;
mod report;
mod source;
mod storage;
mod sync;

#[cfg(feature = "recur-expand")]
mod recur_expand;

use args::{Command, parse};
use clap::Parser;

// clap 的 derive 宏 (derive macro)：根据结构体字段自动生成命令行解析代码。
// derive 是 Rust 自动实现 trait 的机制；这里 Parser trait 提供 Cli::parse()。
#[derive(Parser)]
#[command(
    name = "calman",
    version,
    about = "Minimalist keyboard-driven task manager"
)]
struct Cli {
    /// Taskwarrior-style arguments (bare `calman` lists tasks).
    /// Taskwarrior 风格的自由参数（不带子命令直接运行 `calman` 时列出任务）。
    #[arg(
        value_name = "ARGS",
        allow_hyphen_values = true,
        trailing_var_arg = true
    )]
    args: Vec<String>,
}

/// 程序入口 (entry point)：调用 run()，出错则打印错误并以非零码退出。
fn main() {
    // run() 返回 `Result`：成功是 Ok，失败是 Err。
    // `if let Err(e)` 只在 Err 时进入分支，并把错误值绑定到 e。
    if let Err(e) = run() {
        // `{:#}` 打印完整错误链 (full error chain)。
        eprintln!("error: {e:#}");
        // std::process::exit(1)：立即结束进程，退出码 1 表示失败 (nonzero = failure)。
        std::process::exit(1);
    }
}

/// 核心流程 (core pipeline)，每一步都有注释：
/// 1. `Cli::parse()` — clap 解析命令行参数，失败会自动打印帮助并退出。
/// 2. `config::Config::load()?` — 载入配置文件；`?` 表示出错时立刻返回 Err。
/// 3. `set_day_bounds` — 按配置设定“日”的起止时刻，影响 today/明天 等日期解析。
/// 4. `parse(&cli.args)?` — 把自由格式参数解析成 `Command` 和过滤器 (filter)。
/// 5. `match q.cmd` — 根据命令分发到对应子命令 handler，handler 打印结果。
fn run() -> anyhow::Result<()> {
    // 1. clap 解析命令行参数 (parse command-line args)。
    let cli = Cli::parse();
    // 2. 载入配置；`?` 出错即提前返回 (early return on error)。
    let conf = config::Config::load()?;
    // 3. 设定“日”边界，影响 today/明天 等日期解析。
    crate::date::set_day_bounds(&conf.date.day_start, &conf.date.day_end);
    // 4. 自由参数 → Command/过滤器 (free-form args → command)。
    let q = parse(&cli.args)?;

    // `match` 是 Rust 的模式匹配 (pattern matching)：按 q.cmd 的值选择分支。
    match q.cmd {
        // 没有子命令 → 用默认报告列出任务 (default report, 默认 next)。
        None => cli::list::run(
            &conf,
            &q,
            conf.defaults.default_report.as_deref().unwrap_or("next"),
        ),
        // list：用 list 报告列出任务 (report list)。
        Some(Command::List) => {
            cli::list::run(&conf, &q, q.report_name.as_deref().unwrap_or("list"))
        }
        // add：添加新任务 (add task)。
        Some(Command::Add) => cli::add::run(&conf, &q),
        // done：把任务标记为完成 (mark done)。
        Some(Command::Done) => cli::done::run(&conf, &q),
        // delete：删除任务 (delete task)。
        Some(Command::Delete) => cli::delete::run(&conf, &q),
        // modify：修改任务字段 (modify task fields)。
        Some(Command::Modify) => cli::modify::run(&conf, &q),
        // count：统计匹配的任务数量 (count matching tasks)。
        Some(Command::Count) => cli::count::run(&conf, &q),
        // info：显示任务详情 (show task details)。
        Some(Command::Info) => cli::info::run(&conf, &q),
        // start：开始计时任务 (start timer)。
        Some(Command::Start) => cli::start::run(&conf, &q),
        // stop：停止计时任务 (stop timer)。
        Some(Command::Stop) => cli::stop::run(&conf, &q),
        // sync：执行同步 (run sync hooks)。
        Some(Command::Sync) => cli::sync::run(&conf, &q),
        // help：打印过滤器语法帮助 (filter help)。
        Some(Command::Help) => {
            cli::print_filter_help();
            // Ok(())：成功且无返回值；() 是 unit 类型 (unit type)。
            Ok(())
        }
        #[cfg(feature = "tui")]
        // tui（仅启用 tui feature 时编译）：启动终端界面 (launch TUI)。
        Some(Command::Tui) => cli::tui::run(),
    }
}
