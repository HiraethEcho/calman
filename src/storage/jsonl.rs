//! JSONL storage backend (`tasks.jsonl`, atomic write via tmp+rename).
//!
//! Layout per DESIGN.md §2.2.A: one JSON object per line.

use super::{Storage, atomic_write};
use crate::model::Task;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// File name for the task collection within a source location.
pub const TASKS_FILE: &str = "tasks.jsonl";

/// Line-oriented JSON storage.
pub struct JsonlStorage {
    path: PathBuf,
    tasks: Vec<Task>,
}

impl JsonlStorage {
    /// Open (or create) the collection rooted at `location`.
    pub fn open(location: &Path) -> Result<Self> {
        fs::create_dir_all(location)
            .with_context(|| format!("create storage dir {}", location.display()))?;
        let path = location.join(TASKS_FILE);
        let tasks = Self::read_file(&path)?;
        Ok(JsonlStorage { path, tasks })
    }

    fn read_file(path: &Path) -> Result<Vec<Task>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content =
            fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let mut tasks = Vec::new();
        for (idx, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let task: Task = serde_json::from_str(line)
                .with_context(|| format!("parse line {} of {}", idx + 1, path.display()))?;
            tasks.push(task);
        }
        Ok(tasks)
    }

    /// Rewrite the full file atomically from cached tasks.
    fn persist(&self) -> Result<()> {
        let mut buf = String::new();
        for t in &self.tasks {
            buf.push_str(&serde_json::to_string(t)?);
            buf.push('\n');
        }
        atomic_write(&self.path, buf.as_bytes())
    }
}

impl Storage for JsonlStorage {
    fn list(&self) -> &[Task] {
        &self.tasks
    }

    fn add(&mut self, task: Task) -> Result<()> {
        let line = format!("{}\n", serde_json::to_string(&task)?);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        use std::io::Write;
        file.write_all(line.as_bytes())?;
        self.tasks.push(task);
        Ok(())
    }

    fn update<F>(&mut self, uid: &str, f: F) -> Result<Option<Task>>
    where
        F: FnOnce(&mut Task) -> Result<()>,
    {
        let mut found = false;
        for t in &mut self.tasks {
            if t.uid == uid {
                f(t)?;
                t.updated_at = chrono::Utc::now();
                found = true;
                break;
            }
        }
        if !found {
            return Ok(None);
        }
        self.persist()?;
        Ok(self.tasks.iter().find(|t| t.uid == uid).cloned())
    }

    fn remove(&mut self, uid: &str) -> Result<Option<Task>> {
        let pos = self.tasks.iter().position(|t| t.uid == uid);
        let Some(pos) = pos else {
            return Ok(None);
        };
        let removed = self.tasks.remove(pos);
        self.persist()?;
        Ok(Some(removed))
    }
}

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
