//! Subcommand handlers and shared CLI helpers.

pub mod add;
pub mod count;
pub mod delete;
pub mod done;
pub mod list;
pub mod modify;
pub mod sync;
#[cfg(feature = "tui")]
pub mod tui;

use crate::config::{Config, ContextKind, Source, SourceType};
use crate::model::Task;
use crate::source;
#[cfg(feature = "storage-ics")]
use crate::storage::ics::IcsStorage;
#[cfg(feature = "storage-jsonl")]
use crate::storage::jsonl::JsonlStorage;
use crate::storage::{Storage, Store};
use anyhow::{Result, bail};

/// One merged row across selected sources, with a dynamic short ID.
pub struct Row {
    pub id: usize,
    pub source: String,
    pub task: Task,
}

/// Open the storage backend for a source.
pub fn open_storage(src: &Source) -> Result<Store> {
    let loc = src.abs_location();
    Ok(match src.source_type {
        #[cfg(feature = "storage-jsonl")]
        SourceType::Jsonl => Store::Jsonl(JsonlStorage::open(&loc)?),
        #[cfg(not(feature = "storage-jsonl"))]
        SourceType::Jsonl => {
            bail!("this build was compiled without the `storage-jsonl` feature")
        }
        #[cfg(feature = "storage-ics")]
        SourceType::Ics => Store::Ics(IcsStorage::open(&loc)?),
        #[cfg(not(feature = "storage-ics"))]
        SourceType::Ics => {
            bail!("this build was compiled without the `storage-ics` feature")
        }
        SourceType::IcsDir => {
            bail!(
                "IcsDir source `{}` must be resolved to a collection before opening storage",
                src.name
            )
        }
    })
}

/// Resolve the effective source list: `--source` override > context defaults.
///
/// Handles `IcsDir` expansion:
/// - `source:name/collection` → single virtual `Ics` source for that collection
/// - `source:name` (IcsDir) → expand all discovered collections
/// - `source:name` (regular) → use directly
pub fn resolve_sources(
    conf: &Config,
    override_: Option<&[String]>,
    ctx: ContextKind,
) -> Result<Vec<Source>> {
    let names: Vec<String> = match override_ {
        Some(ns) => ns.to_vec(),
        None => conf.context_sources(ctx),
    };
    let mut out = Vec::new();
    for name in names {
        let resolved = source::resolve_source_name(&conf.sources, &name)?;
        out.extend(resolved);
    }
    if out.is_empty() {
        bail!("no sources selected");
    }
    Ok(out)
}

/// Merge all tasks from `sources` into `Row`s with sequential short IDs.
///
/// Taskwarrior-style numbering: the **oldest** task gets ID 1. Rows are
/// ordered by `created_at` (ties broken by UID) before IDs are assigned, so
/// IDs stay stable across sessions regardless of storage iteration order.
pub fn load_merged(sources: &[Source]) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for src in sources {
        let st = open_storage(src)?;
        for t in st.list() {
            let mut t = t.clone();
            t.source = src.name.clone();
            rows.push(Row {
                id: 0,
                source: src.name.clone(),
                task: t,
            });
        }
    }
    rows.sort_by(|a, b| {
        a.task
            .created_at
            .cmp(&b.task.created_at)
            .then_with(|| a.task.uid.cmp(&b.task.uid))
    });
    for (i, r) in rows.iter_mut().enumerate() {
        r.id = i + 1;
    }
    Ok(rows)
}

/// Resolve ID arguments to `(uid, source_name)` pairs.
///
/// Numeric tokens are short IDs into the merged list; `--uid` tokens are
/// matched directly against stored UIDs. Mixed forms are allowed.
pub fn resolve_targets(
    conf: &Config,
    override_: Option<&[String]>,
    ids: &[String],
) -> Result<Vec<(String, String)>> {
    let sources = resolve_sources(conf, override_, ContextKind::Cli)?;
    let rows = load_merged(&sources)?;
    let mut out = Vec::new();
    for id in ids {
        if let Ok(n) = id.parse::<usize>() {
            let row = rows
                .get(n.checked_sub(1).unwrap_or(usize::MAX))
                .ok_or_else(|| anyhow::anyhow!("no task with ID `{id}`"))?;
            out.push((row.task.uid.clone(), row.source.clone()));
        } else {
            let found = rows
                .iter()
                .find(|r| r.task.uid == *id)
                .ok_or_else(|| anyhow::anyhow!("no task with UID `{id}`"))?;
            out.push((found.task.uid.clone(), found.source.clone()));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SourceType;
    use tempfile::tempdir;

    fn source(dir: &std::path::Path, name: &str) -> Source {
        Source {
            name: name.into(),
            source_type: SourceType::Jsonl,
            location: dir.display().to_string(),
            sync: None,
        }
    }

    #[test]
    fn ids_assigned_oldest_first() {
        let dir = tempdir().unwrap();
        let mut st = crate::storage::jsonl::JsonlStorage::open(dir.path()).unwrap();
        let mut first = Task::new("work", "newer");
        let mut second = Task::new("work", "older");
        // older created earlier: rewind its timestamp manually
        second.created_at = first.created_at - chrono::Duration::days(1);
        st.add(second).unwrap();
        st.add(first).unwrap();
        drop(st);

        let rows = load_merged(&[source(dir.path(), "work")]).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, 1);
        assert_eq!(rows[0].task.summary, "older");
        assert_eq!(rows[1].id, 2);
        assert_eq!(rows[1].task.summary, "newer");
    }
}
