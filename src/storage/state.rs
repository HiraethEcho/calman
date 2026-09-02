//! 状态管理器：读写 `.calman-state.json` 元数据文件。
//! State manager: reads and writes the `.calman-state.json` metadata file.
//!
//! 文件内容 / Contents:
//! - `version`：状态文件格式版本，用于将来兼容旧版本。
//! - `uid_counter`：已分配的任务 UID 计数器，用来生成不重复的短 ID。
//! - `last_modified`：上次修改时间。
//!
//! 该文件与任务数据（`tasks.jsonl`）分开存放，仅记录“元数据/状态”。

use super::atomic_write;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 源目录内元数据文件的文件名（以点开头，Linux 下默认隐藏）。
/// Metadata file name within a source location.
pub const STATE_FILE: &str = ".calman-state.json";

/// 状态文件的内存表示，与磁盘 JSON 一一对应。
/// In-memory representation of the state file, mirroring the on-disk JSON.
///
/// `#[derive(...)]` 让 Rust 自动实现这些 trait：`Debug` 便于打印调试、
/// `Clone` 可复制、`PartialEq` 可比较、`Serialize`/`Deserialize` 由 serde 负责 JSON 互转。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// 状态文件格式版本号 / format version of the state file
    pub version: u32,
    /// 已分配的 UID 计数器，用于生成唯一短 ID / counter of assigned UIDs for unique short IDs
    pub uid_counter: u64,
    /// 上次修改时间 / time of the last modification
    pub last_modified: DateTime<Utc>,
}

/// 新状态文件使用的默认值。
/// Default values used for a brand-new state file.
impl Default for State {
    fn default() -> Self {
        State {
            version: 1,
            uid_counter: 0,          // 从 0 开始，首次分配后再递增
            last_modified: Utc::now(), // 创建时的当前时间
        }
    }
}

/// 读写某个源目录的 `.calman-state.json`。
/// Reads and writes `.calman-state.json` for a source location.
pub struct StateManager {
    path: PathBuf, // 状态文件路径 / path to the state file
    state: State,  // 内存中的状态副本 / in-memory copy of the state
}

impl StateManager {
    /// 打开状态文件：存在则解析，不存在则用默认值新建。
    /// Open the state file: parse it if it exists, otherwise fall back to defaults.
    pub fn open(location: &Path) -> Result<Self> {
        fs::create_dir_all(location)
            .with_context(|| format!("create storage dir {}", location.display()))?;
        let path = location.join(STATE_FILE);
        let state = if path.exists() {
            // 读取并反序列化；`.with_context` 为错误补充“哪个文件”的信息，便于排查。
            let content =
                fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
            serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))?
        } else {
            State::default() // 首次运行：没有文件，就用默认状态
        };
        Ok(StateManager { path, state })
    }

    /// 记录一次存储修改（递增 UID 计数器、刷新时间），并原子写回磁盘。
    /// Record a storage modification (bump the UID counter, refresh the time) and persist atomically.
    /// 因为 `&mut self` 会修改 `self.state`，所以必须用可变借用。
    pub fn touch(&mut self) -> Result<()> {
        self.state.uid_counter += 1; // 每新增一个任务，计数器 +1，得到唯一 UID
        self.state.last_modified = Utc::now();
        let json = serde_json::to_string(&self.state)?; // 序列化成一行 JSON
        atomic_write(&self.path, json.as_bytes()) // 原子替换，避免写坏状态文件
    }
}

// 测试模块：仅在 `cargo test` 时编译；本任务未改动其中的测试代码。
// Test module: compiled only under `cargo test`; the tests themselves were left untouched.
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_default_and_roundtrips() {
        let dir = tempdir().unwrap();
        let mut m = StateManager::open(dir.path()).unwrap();
        assert_eq!(m.state.version, 1);
        assert_eq!(m.state.uid_counter, 0);
        m.touch().unwrap();
        drop(m);

        let m2 = StateManager::open(dir.path()).unwrap();
        assert_eq!(m2.state.uid_counter, 1);
    }
}
