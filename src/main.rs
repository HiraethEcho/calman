//! calman — minimalist, keyboard-driven task manager.
//!
//! Taskwarrior-style CLI: free-form filters/attributes, default report = list.

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

#[derive(Parser)]
#[command(
    name = "calman",
    version,
    about = "Minimalist keyboard-driven task manager"
)]
struct Cli {
    /// Taskwarrior-style arguments (bare `calman` lists tasks).
    #[arg(
        value_name = "ARGS",
        allow_hyphen_values = true,
        trailing_var_arg = true
    )]
    args: Vec<String>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let conf = config::Config::load()?;
    crate::date::set_day_bounds(&conf.date.day_start, &conf.date.day_end);
    let q = parse(&cli.args)?;

    match q.cmd {
        None => cli::list::run(
            &conf,
            &q,
            conf.defaults.default_report.as_deref().unwrap_or("next"),
        ),
        Some(Command::List) => {
            cli::list::run(&conf, &q, q.report_name.as_deref().unwrap_or("list"))
        }
        Some(Command::Add) => cli::add::run(&conf, &q),
        Some(Command::Done) => cli::done::run(&conf, &q),
        Some(Command::Delete) => cli::delete::run(&conf, &q),
        Some(Command::Modify) => cli::modify::run(&conf, &q),
        Some(Command::Count) => cli::count::run(&conf, &q),
        Some(Command::Info) => cli::info::run(&conf, &q),
        Some(Command::Start) => cli::start::run(&conf, &q),
        Some(Command::Stop) => cli::stop::run(&conf, &q),
        Some(Command::Sync) => cli::sync::run(&conf, &q),
        Some(Command::Help) => {
            cli::print_filter_help();
            Ok(())
        }
        #[cfg(feature = "tui")]
        Some(Command::Tui) => cli::tui::run(),
    }
}
