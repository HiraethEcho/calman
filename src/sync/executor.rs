//! 同步执行器：运行用户配置的外部命令链 `pre_hook` → `cmd` → `post_hook`，并管理锁文件。
//! Sync executor: runs `pre_hook` → `cmd` → `post_hook`; lock + short-circuit.
//!
//! 完整工作流（自包含描述，不依赖外部文档）：
//! Full flow (self-contained, no external doc needed):
//! 1. 若 `.sync.lock` 已存在则跳过（防止并发同步互相冲突）；
//!    skip if `.sync.lock` exists (prevents concurrent syncs);
//! 2. 原子创建锁文件；atomically create the lock file;
//! 3. 依次运行三个钩子（每个钩子是一条 shell 命令）；run the hooks sequentially;
//! 4. 任一命令返回非零退出码就中止整条链；abort on first non-zero exit;
//! 5. 最后删除锁文件，由调用方负责更新状态。remove lock, caller updates state.
//!
//! Rust 概念：本模块大量使用 `Result`。`?` 运算符表示“若出错则立即把错误向上返回”，
//! 类似其它语言的异常，但更显式——错误会一路传到调用方。
//! Rust concept: `Result` + `?` propagate errors explicitly instead of exceptions.

use crate::config::SyncConfig;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;
use std::process::Command;

/// 锁文件名，创建在数据源目录内，表示一次同步正在进行。
/// Lock file name created inside a source location during sync.
pub const LOCK_FILE: &str = ".sync.lock";

/// 超过该秒数的锁视为“陈旧”——多半是上一次进程崩溃留下的，可以安全接管。
/// A lock older than this (seconds) is assumed stale (crashed run).
const STALE_LOCK_SECS: u64 = 600;

/// 为单个数据源执行配置好的同步链。
/// Run the configured sync chain for one source.
///
/// `{location}` 与 `{name}` 占位符会在执行前被替换；子进程的工作目录设为 `location`。
/// `{location}` and `{name}` placeholders are substituted before execution;
/// the working directory is set to `location`.
///
/// 类型小提示：`location: &Path` 与 `sync: &SyncConfig` 都是“借用”（不拥有数据）；
/// 返回值 `Result<()>` 成功时不含数据，失败时携带可打印的错误（anyhow 的 `Result`）。
pub fn run_sync(location: &Path, name: &str, sync: &SyncConfig) -> Result<()> {
    // 先确保目录存在（`create_dir_all` 会递归创建）。`with_context` 给底层错误补上
    // “在哪一步失败”的上下文，`?` 让它出错时直接返回。
    // Ensure the directory exists; `with_context` adds context, `?` returns on error.
    fs::create_dir_all(location).with_context(|| format!("create dir {}", location.display()))?;
    let lock = location.join(LOCK_FILE);
    // 原子创建锁文件：`create_new(true)` 在文件已存在时会失败，从而保证
    // 两个并发的同步进程不可能同时拿到锁。同时做“陈旧锁恢复”：
    // 若锁存活时间超过了同步时长，多半是崩溃遗留，可以接管。
    // Atomic lock creation (`create_new`) guarantees mutual exclusion; a lock that
    // outlives the sync duration is likely a crashed run left behind — take it over.
    match fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // 陈旧判断：读取锁文件的修改时间，看它是否超过 STALE_LOCK_SECS。
            // 这里是一串方法链（`and_then`/`map`/`unwrap_or`），每一步都可能失败；
            // 任何一步失败都保守地当作“未过期”（不可接管），避免误删正在运行的锁。
            // Stale check via method chaining; any failure safely defaults to "fresh".
            let stale = fs::metadata(&lock)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.elapsed().ok())
                .map(|d| d.as_secs() > STALE_LOCK_SECS)
                .unwrap_or(false);
            if !stale {
                bail!(
                    "sync already in progress for source `{name}` ({})",
                    lock.display()
                );
            }
            // 删除陈旧锁后重新创建空锁。`let _ =` 表示“忽略删除失败的返回值”。
            // Remove the stale lock then recreate an empty one; `let _ =` ignores failure.
            eprintln!("warning: removing stale sync lock {}", lock.display());
            let _ = fs::remove_file(&lock);
            fs::write(&lock, "")?;
        }
        Err(e) => return Err(e).with_context(|| format!("create lock {}", lock.display())),
    };

    // 用一个“立即调用的闭包”包住钩子执行逻辑：无论内部成功还是出错，
    // 下面的锁清理代码都一定会执行（这正是闭包的价值）。
    // An immediately-invoked closure isolates hook logic so lock cleanup always runs.
    let result = (|| -> Result<()> {
        // 三个钩子的（标签, 可选命令）数组。`&Option<String>` 表示“可能没有命令”；
        // 后面的 `if let Some(cmd)` 用来解包可选值。
        // Array of (label, optional command); `if let Some(cmd)` unwraps the Option.
        let hooks: [(&str, &Option<String>); 3] = [
            ("pre_hook", &sync.pre_hook),
            ("cmd", &sync.cmd),
            ("post_hook", &sync.post_hook),
        ];
        for (label, hook) in hooks {
            if let Some(cmd) = hook {
                run_command(location, name, label, cmd)?;
            }
        }
        Ok(())
    })();

    let _ = fs::remove_file(&lock);
    result
}

/// 在 `sh -c` 中运行一条钩子命令，并把 `{location}`/`{name}` 安全地替换成实际值。
/// Run one hook command via `sh -c`, substituting `{location}`/`{name}` safely.
///
/// Rust 概念：`Command::new("sh").arg("-c")` 让 shell 解释整条命令字符串；
/// `.current_dir(location)` 设定子进程的工作目录；`.status()` 等待子进程结束并返回退出状态。
fn run_command(location: &Path, name: &str, label: &str, cmd: &str) -> Result<()> {
    // 用单引号包裹占位符值，防止路径/名称里的空格或 shell 元字符被 shell 二次解析。
    // Quote substitutions so paths/names with shell metacharacters or spaces
    // are passed verbatim rather than re-parsed by the shell.
    let expanded = cmd
        .replace("{location}", &shell_quote(&location.display().to_string()))
        .replace("{name}", &shell_quote(name));
    let status = Command::new("sh")
        .arg("-c")
        .arg(&expanded)
        .current_dir(location)
        .status()
        .with_context(|| format!("failed to spawn `{label}`"))?;
    // 退出码非 0 → 用 `bail!` 宏构造错误并返回，整条同步链在此中止。
    // Non-zero exit → `bail!` returns an error and the sync chain aborts.
    if !status.success() {
        bail!("`{label}` exited with status {status}");
    }
    Ok(())
}

/// 把字符串用单引号包裹，供 shell 安全插值；内部单引号用 `'\''` 惯用法转义。
/// Wrap a string in single quotes for safe shell interpolation, escaping any
/// embedded single quotes via the `'\''` idiom.
///
/// 例如 `it's` → `'it'\''s'`，shell 会把它还原成一个参数。
/// Example: `it's` → `'it'\''s'`, which the shell treats as one argument.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
// 测试模块：仅覆盖外部命令行为与锁逻辑，学习时可以先跳过。
// Tests only; safe to skip while learning.
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn placeholders_substituted_and_cwd() {
        let dir = tempdir().unwrap();
        let sync = SyncConfig {
            pre_hook: Some("echo \"{name}\" \"{location}\" > placed.txt".into()),
            cmd: None,
            post_hook: None,
        };
        run_sync(dir.path(), "work", &sync).unwrap();
        let content = fs::read_to_string(dir.path().join("placed.txt")).unwrap();
        assert!(content.contains("work"));
        assert!(content.contains(&dir.path().display().to_string()));
    }

    #[test]
    fn aborts_on_nonzero_and_releases_lock() {
        let dir = tempdir().unwrap();
        let sync = SyncConfig {
            pre_hook: Some("exit 3".into()),
            cmd: Some("echo should-not-run > boom.txt".into()),
            post_hook: None,
        };
        assert!(run_sync(dir.path(), "work", &sync).is_err());
        assert!(!dir.path().join(LOCK_FILE).exists());
        assert!(!dir.path().join("boom.txt").exists());
    }

    #[test]
    fn skips_when_lock_present() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(LOCK_FILE), "").unwrap();
        let sync = SyncConfig::default();
        assert!(run_sync(dir.path(), "work", &sync).is_err());
    }
}
