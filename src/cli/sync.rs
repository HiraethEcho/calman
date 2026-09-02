//! `sync` 子命令：对数据源跑外部同步命令。
//! `sync` subcommand handler.
//!
//! 数据流：解析源 → 逐个检查是否配了 sync 命令 → 运行命令 → 更新状态文件(mtime) → 打印。

use crate::args::ParsedArgs;
use crate::cli::resolve_sources;
use crate::config::{Config, ContextKind};
use crate::storage::state::StateManager;
use crate::sync::executor::run_sync;
use anyhow::Result;

/// 执行 sync。只同步配置了 `sync` 命令的 source，没配的直接跳过。
pub fn run(conf: &Config, q: &ParsedArgs) -> Result<()> {
    let override_ = (!q.sources.is_empty()).then_some(q.sources.as_slice());
    let sources = resolve_sources(conf, override_, ContextKind::Sync)?;
    for src in sources {
        // Option + continue：没有 sync 配置的源不是错误，只是没东西可跑。
        let Some(cfg) = &src.sync else { continue };
        let loc = src.abs_location();
        run_sync(&loc, &src.name, cfg)?; // 真正的同步逻辑在 sync::executor
        // touch 状态文件，记录最近一次同步时间；? 传播状态读取/写入错误。
        let mut state = StateManager::open(&loc)?;
        state.touch()?;
        println!("synced `{}`", src.name);
    }
    Ok(())
}
