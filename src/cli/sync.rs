//! `sync` subcommand handler.

use crate::args::ParsedArgs;
use crate::cli::resolve_sources_exact;
use crate::config::{Config, ContextKind, SourceType};
use crate::storage::state::StateManager;
use crate::sync::executor::run_sync;
use anyhow::Result;

pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    // Resolve without `IcsDir` expansion: one run per configured source, at the
    // source root. Expanding would repeat the same command once per collection.
    let sources = resolve_sources_exact(conf, override_, ContextKind::Sync)?;
    for src in sources {
        let Some(cfg) = &src.sync else { continue };
        let loc = src.abs_location();
        run_sync(&loc, &src.name, cfg)?;
        // An `ics-dir` root holds collections, not items: metadata such as
        // `uid_counter`/`last_modified` has nothing to record there.
        if src.source_type != SourceType::IcsDir {
            let mut state = StateManager::open(&loc)?;
            state.touch()?;
        }
        println!("synced `{}`", src.name);
    }
    Ok(())
}
