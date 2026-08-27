//! Storage trait + JSONL/ICS backends and state manager.

pub mod state;

#[cfg(feature = "storage-ics")]
pub mod ics;
#[cfg(feature = "storage-jsonl")]
pub mod jsonl;

#[cfg(not(any(feature = "storage-jsonl", feature = "storage-ics")))]
compile_error!("at least one of `storage-jsonl` or `storage-ics` features is required");

use crate::model::Task;
use anyhow::Result;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Common storage interface shared by all backends.
pub trait Storage {
    /// All tasks currently held (borrowed).
    fn list(&self) -> &[Task];

    /// Append a new task.
    fn add(&mut self, task: Task) -> Result<()>;

    /// Mutate a task by UID. Returns the updated task, or `None` if missing.
    /// The mutation closure may fail (validation) — error aborts the write.
    fn update<F>(&mut self, uid: &str, f: F) -> Result<Option<Task>>
    where
        F: FnOnce(&mut Task) -> Result<()>;

    /// Delete a task by UID. Returns the removed task, or `None` if missing.
    fn remove(&mut self, uid: &str) -> Result<Option<Task>>;
}

/// Atomic write of `contents` to `path` via a temp file + rename.
pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(contents)?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// A concrete storage backend chosen at runtime by source type.
pub enum Store {
    #[cfg(feature = "storage-jsonl")]
    Jsonl(jsonl::JsonlStorage),
    #[cfg(feature = "storage-ics")]
    Ics(ics::IcsStorage),
}

impl Storage for Store {
    fn list(&self) -> &[Task] {
        match self {
            #[cfg(feature = "storage-jsonl")]
            Store::Jsonl(s) => s.list(),
            #[cfg(feature = "storage-ics")]
            Store::Ics(s) => s.list(),
        }
    }

    fn add(&mut self, task: Task) -> Result<()> {
        match self {
            #[cfg(feature = "storage-jsonl")]
            Store::Jsonl(s) => s.add(task),
            #[cfg(feature = "storage-ics")]
            Store::Ics(s) => s.add(task),
        }
    }

    fn update<F>(&mut self, uid: &str, f: F) -> Result<Option<Task>>
    where
        F: FnOnce(&mut Task) -> Result<()>,
    {
        match self {
            #[cfg(feature = "storage-jsonl")]
            Store::Jsonl(s) => s.update(uid, f),
            #[cfg(feature = "storage-ics")]
            Store::Ics(s) => s.update(uid, f),
        }
    }

    fn remove(&mut self, uid: &str) -> Result<Option<Task>> {
        match self {
            #[cfg(feature = "storage-jsonl")]
            Store::Jsonl(s) => s.remove(uid),
            #[cfg(feature = "storage-ics")]
            Store::Ics(s) => s.remove(uid),
        }
    }
}
