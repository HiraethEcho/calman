//! `tui` 子命令（占位 stub）：启动终端界面，尚未实现（Phase 3）。
//! `tui` subcommand handler (stub).

use anyhow::Result;

/// 目前只打印提示并正常返回；真正的 TUI 在 Phase 3 实现。
pub fn run() -> Result<()> {
    eprintln!("tui not implemented yet (Phase 3)");
    Ok(())
}
