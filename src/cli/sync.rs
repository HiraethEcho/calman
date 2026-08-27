//! `sync` subcommand handler.

use crate::args::ParsedArgs;
use crate::cli::resolve_sources;
use crate::config::{Config, ContextKind};
use crate::storage::state::StateManager;
use crate::sync::executor::run_sync;
use anyhow::Result;

pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    let sources = resolve_sources(conf, override_, ContextKind::Sync)?;
    for src in sources {
        let Some(cfg) = &src.sync else { continue };
        let loc = src.abs_location();
        run_sync(&loc, &src.name, cfg)?;
        let mut state = StateManager::open(&loc)?;
        state.touch()?;
        println!("synced `{}`", src.name);
    }
    Ok(())
}
