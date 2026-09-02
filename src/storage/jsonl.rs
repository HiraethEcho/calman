//! JSONL 存储后端：任务保存在 `tasks.jsonl`，每行一个 JSON 对象。
//! JSONL storage backend: tasks live in `tasks.jsonl`, one JSON object per line.
//!
//! 写入采用原子替换（临时文件 + rename），崩溃时不会留下损坏的文件。
//! Writes use atomic replacement (temp file + rename) so a crash never leaves a half-written file.
//!
//! 数据布局 / Layout: 每一行是一个独立的 JSON 对象（一个任务），
//! 空行会被忽略，读取时逐行解析。one JSON object per line, blank lines skipped.

use super::{Storage, atomic_write};
use crate::model::Task;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// 源目录内任务集合的文件名。
/// File name for the task collection within a source location.
pub const TASKS_FILE: &str = "tasks.jsonl";

/// 基于 JSONL 的行式存储：内部持有文件路径和全部任务的内存副本。
/// Line-oriented JSON storage: holds the file path plus an in-memory copy of all tasks.
///
/// 每次修改都重写整个文件（见 `persist`），保证内存状态与磁盘一致。
pub struct JsonlStorage {
    path: PathBuf,   // 数据文件路径 / path to the data file
    tasks: Vec<Task>, // 内存中的任务列表 / in-memory task list
}

impl JsonlStorage {
    /// 打开（若不存在则创建）以 `location` 为根目录的任务集合。
    /// Open (or create) the collection rooted at `location`.
    pub fn open(location: &Path) -> Result<Self> {
        fs::create_dir_all(location)
            .with_context(|| format!("create storage dir {}", location.display()))?;
        let path = location.join(TASKS_FILE);
        let tasks = Self::read_file(&path)?; // 启动时把整个文件读进内存，之后操作都基于它
        Ok(JsonlStorage { path, tasks })
    }

    /// 从文件逐行解析任务列表；文件不存在时返回空列表。
    /// Read and parse every task line; returns an empty list if the file is missing.
    fn read_file(path: &Path) -> Result<Vec<Task>> {
        if !path.exists() {
            return Ok(Vec::new()); // 首次运行：还没有文件，视为空集合
        }
        let content =
            fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let mut tasks = Vec::new();
        for (idx, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue; // 跳过空行
            }
            // 每行单独反序列化；出错时报出“第几行”，便于定位坏数据。
            let task: Task = serde_json::from_str(line)
                .with_context(|| format!("parse line {} of {}", idx + 1, path.display()))?;
            tasks.push(task);
        }
        Ok(tasks)
    }

    /// 把内存中的全部任务序列化后，用原子写入重写整个文件。
    /// Rewrite the full file atomically from the cached tasks.
    /// 因为这里用的是 `&self`（只读借用），重写只发生在 `add`/`update`/`remove` 修改之后。
    fn persist(&self) -> Result<()> {
        let mut buf = String::new();
        for t in &self.tasks {
            buf.push_str(&serde_json::to_string(t)?); // 把任务序列化成一行 JSON
            buf.push('\n'); // JSONL 以换行分隔
        }
        atomic_write(&self.path, buf.as_bytes()) // 一次性原子替换整个文件
    }
}

impl Storage for JsonlStorage {
    /// 返回内存中的任务切片（借用，不复制）。
    /// Return a borrowed slice of the in-memory tasks.
    fn list(&self) -> &[Task] {
        &self.tasks
    }

    /// 追加新任务：把任务追加到文件末尾，再同步进内存列表。
    /// Append a task: write its line to the end of the file, then mirror it in memory.
    /// 这里 `&mut self` 是因为需要修改 `self.tasks`。
    fn add(&mut self, task: Task) -> Result<()> {
        let line = format!("{}\n", serde_json::to_string(&task)?);
        // 追加模式打开文件：`.create(true)` 不存在就创建，`.append(true)` 从末尾写。
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        use std::io::Write;
        file.write_all(line.as_bytes())?; // 逐行追加，不重写整个文件，比 update/remove 更快
        self.tasks.push(task); // 同步内存副本
        Ok(())
    }

    /// 按 UID 修改任务：找到后调用闭包 `f` 改字段，再刷新 `updated_at` 并重写文件。
    /// Mutate a task by UID: find it, run closure `f`, refresh `updated_at`, rewrite the file.
    fn update<F>(&mut self, uid: &str, f: F) -> Result<Option<Task>>
    where
        F: FnOnce(&mut Task) -> Result<()>,
    {
        let mut found = false;
        for t in &mut self.tasks {
            // `&mut self.tasks` 才能拿到 `&mut Task` 交给闭包修改。
            if t.uid == uid {
                f(t)?; // 调用闭包；若返回 Err，`?` 立即中止整个写入（不落盘）
                t.updated_at = chrono::Utc::now(); // 记录修改时间
                found = true;
                break;
            }
        }
        if !found {
            return Ok(None); // UID 不存在：不写文件，直接返回 None
        }
        self.persist()?; // 修改后必须把新状态原子写回磁盘
        Ok(self.tasks.iter().find(|t| t.uid == uid).cloned())
    }

    /// 按 UID 删除任务：从内存列表中移除后重写文件。
    /// Delete a task by UID: remove it from the in-memory list, then rewrite the file.
    fn remove(&mut self, uid: &str) -> Result<Option<Task>> {
        let pos = self.tasks.iter().position(|t| t.uid == uid);
        // Rust 的 let-else 语法：若 `pos` 是 None（没找到），提前 return None。
        let Some(pos) = pos else {
            return Ok(None);
        };
        let removed = self.tasks.remove(pos); // Vec::remove 返回被移除的元素
        self.persist()?;
        Ok(Some(removed))
    }
}

// 测试模块：仅在 `cargo test` 时编译；本任务未改动其中的测试代码。
// Test module: compiled only under `cargo test`; the tests themselves were left untouched.
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn store(dir: &std::path::Path) -> JsonlStorage {
        JsonlStorage::open(dir).unwrap()
    }

    #[test]
    fn add_append_and_read() {
        let dir = tempdir().unwrap();
        let mut s = store(dir.path());
        s.add(Task::new("work", "one")).unwrap();
        drop(s);

        // reopening reads persisted state
        let s2 = JsonlStorage::open(dir.path()).unwrap();
        assert_eq!(s2.list().len(), 1);
        assert_eq!(s2.list()[0].summary, "one");
    }

    #[test]
    fn update_task_by_uid() {
        let dir = tempdir().unwrap();
        let mut s = store(dir.path());
        let t = Task::new("work", "a");
        let uid = t.uid.clone();
        s.add(t).unwrap();

        let updated = s
            .update(&uid, |t| {
                t.status = crate::model::TaskStatus::Completed;
                Ok(())
            })
            .unwrap();
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().status, crate::model::TaskStatus::Completed);
        assert_eq!(s.list()[0].status, crate::model::TaskStatus::Completed);
    }

    #[test]
    fn remove_task_by_uid() {
        let dir = tempdir().unwrap();
        let mut s = store(dir.path());
        let t = Task::new("work", "gone");
        let uid = t.uid.clone();
        s.add(t).unwrap();

        assert!(s.remove(&uid).unwrap().is_some());
        assert!(s.remove(&uid).unwrap().is_none());
        assert!(s.list().is_empty());
    }

    #[test]
    fn missing_update_returns_none() {
        let dir = tempdir().unwrap();
        let mut s = store(dir.path());
        assert!(s.update("nope", |_| Ok(())).unwrap().is_none());
    }
}
