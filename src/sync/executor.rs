//! Sync executor: runs `pre_hook` → `cmd` → `post_hook`; lock + short-circuit.
//!
//! Flow per DESIGN.md §3.1: skip if `.sync.lock` exists, create lock, run hooks
//! sequentially, abort on first non-zero exit, remove lock, caller updates state.

use crate::config::SyncConfig;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Lock file name created inside a source location during sync.
pub const LOCK_FILE: &str = ".sync.lock";

/// A lock older than this (seconds) is assumed stale (crashed run).
const STALE_LOCK_SECS: u64 = 600;

/// Run the configured sync chain for one source.
///
/// `{location}` and `{name}` placeholders are substituted before execution;
/// the working directory is set to `location`.
pub fn run_sync(location: &Path, name: &str, sync: &SyncConfig) -> Result<()> {
    fs::create_dir_all(location).with_context(|| format!("create dir {}", location.display()))?;
    let lock = location.join(LOCK_FILE);
    // Atomic lock creation + stale-lock recovery: if the lock survives past
    // the sync duration, a crashed run left it behind — take it over.
    match fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
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
            eprintln!("warning: removing stale sync lock {}", lock.display());
            let _ = fs::remove_file(&lock);
            fs::write(&lock, "")?;
        }
        Err(e) => return Err(e).with_context(|| format!("create lock {}", lock.display())),
    };

    let result = (|| -> Result<()> {
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

fn run_command(location: &Path, name: &str, label: &str, cmd: &str) -> Result<()> {
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
    if !status.success() {
        bail!("`{label}` exited with status {status}");
    }
    Ok(())
}

/// Wrap a string in single quotes for safe shell interpolation, escaping any
/// embedded single quotes via the `'\''` idiom.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
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
