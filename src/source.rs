//! Source discovery and reference resolution for `IcsDir` sources.
//!
//! An `IcsDir` source points to a root directory (e.g. a Radicale/vdirsync
//! sync root). Collections are auto-discovered by recursively scanning for
//! `.ics` files. Each collection directory becomes a virtual source referenced
//! as `<source_name>/<collection_path>`.
//!
//! Reference format: `source:<name>/<collection>` (path-like).
//! Spaces in collection names use quotes: `source:<name>/"My Collection"`.

use crate::config::{Source, SourceType};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// A discovered collection within an `IcsDir` source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredCollection {
    /// Composite source name: `<source_name>/<relative_path>`.
    pub name: String,
    /// Absolute path to the collection directory.
    pub path: PathBuf,
}

/// Recursively discover all `.ics`-containing directories under a source root.
///
/// Each directory that contains at least one `.ics` file becomes a collection.
/// The collection name is the relative path from the source root.
pub fn discover(source: &Source) -> Result<Vec<DiscoveredCollection>> {
    let root = source.abs_location();
    if !root.is_dir() {
        anyhow::bail!(
            "IcsDir source '{}' path is not a directory: {}",
            source.name,
            root.display()
        );
    }

    let mut collections = Vec::new();
    discover_recursive(&root, &root, &source.name, &mut collections)?;
    Ok(collections)
}

fn discover_recursive(
    root: &Path,
    current: &Path,
    source_name: &str,
    out: &mut Vec<DiscoveredCollection>,
) -> Result<()> {
    let mut has_ics = false;

    let entries =
        fs::read_dir(current).with_context(|| format!("read dir {}", current.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            discover_recursive(root, &path, source_name, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("ics") {
            has_ics = true;
        }
    }

    if has_ics {
        let relative = current
            .strip_prefix(root)
            .unwrap_or(current)
            .to_string_lossy()
            .to_string();

        let name = if relative.is_empty() {
            source_name.to_string()
        } else {
            format!("{source_name}/{relative}")
        };

        out.push(DiscoveredCollection {
            name,
            path: current.to_path_buf(),
        });
    }

    Ok(())
}

/// Parse a source reference like `personal/calendars` into `(source_name, collection_path)`.
///
/// Supports quoted collection names for spaces: `personal/"My Collection"`.
/// Returns `None` if the reference doesn't contain a `/` separator.
pub fn parse_source_ref(name: &str) -> Option<(String, String)> {
    let slash_pos = name.find('/')?;
    let source_name = name[..slash_pos].to_string();
    let collection_part = &name[slash_pos + 1..];

    if source_name.is_empty() || collection_part.is_empty() {
        return None;
    }

    // Handle quoted collection names
    let collection = if let Some(inner) = collection_part.strip_prefix('"') {
        let end = inner.find('"')?;
        collection_part[1..end + 1].to_string()
    } else {
        collection_part.to_string()
    };

    if collection.is_empty() {
        return None;
    }

    Some((source_name, collection))
}

/// Resolve a composite source name (e.g. `personal/calendars`) to a directory path.
///
/// Finds the `IcsDir` source, discovers its collections, and returns the path
/// for the matching collection.
pub fn resolve_collection_path(source: &Source, collection: &str) -> Result<Option<PathBuf>> {
    let collections = discover(source)?;
    let full_name = format!("{}/{}", source.name, collection);
    Ok(collections
        .into_iter()
        .find(|c| c.name == full_name)
        .map(|c| c.path))
}

/// Expand an `IcsDir` source into virtual `Ics` sources for each discovered collection.
pub fn expand_ics_dir(source: &Source) -> Result<Vec<Source>> {
    let collections = discover(source)?;
    Ok(collections
        .into_iter()
        .map(|c| Source {
            name: c.name,
            source_type: SourceType::Ics,
            location: c.path.to_string_lossy().to_string(),
            sync: source.sync.clone(),
        })
        .collect())
}

/// Resolve a source name to exactly one `Source`, **without** expanding `IcsDir`.
///
/// - If `name` contains `/` and matches an `IcsDir` collection → that virtual
///   `Ics` source (inheriting the parent's `sync`).
/// - If `name` matches an `IcsDir` source → the `IcsDir` source itself.
/// - If `name` matches a regular source → that source.
///
/// Use this for operations that act on a configured source as a whole (e.g.
/// `sync`), where expanding into one run per collection would repeat the same
/// command.
pub fn resolve_source_name_single(sources: &[Source], name: &str) -> Result<Source> {
    // Composite reference (`personal/calendars`)
    if let Some((source_name, collection)) = parse_source_ref(name) {
        if let Some(parent) = sources.iter().find(|s| s.name == source_name)
            && parent.source_type == SourceType::IcsDir
            && let Some(path) = resolve_collection_path(parent, &collection)?
        {
            return Ok(Source {
                name: name.to_string(),
                source_type: SourceType::Ics,
                location: path.to_string_lossy().to_string(),
                sync: parent.sync.clone(),
            });
        }
        anyhow::bail!("unknown source `{name}`");
    }

    sources
        .iter()
        .find(|s| s.name == *name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("unknown source `{name}`"))
}

/// Resolve a source name to one or more concrete sources.
///
/// - If `name` contains `/` and matches an `IcsDir` collection → single virtual source
/// - If `name` matches an `IcsDir` source → expand all collections
/// - If `name` matches a regular source → return as-is
pub fn resolve_source_name(sources: &[Source], name: &str) -> Result<Vec<Source>> {
    let src = resolve_source_name_single(sources, name)?;
    if src.source_type == SourceType::IcsDir {
        expand_ics_dir(&src)
    } else {
        Ok(vec![src])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SyncConfig;
    use tempfile::tempdir;

    fn make_ics_dir_source(name: &str, path: &str) -> Source {
        Source {
            name: name.to_string(),
            source_type: SourceType::IcsDir,
            location: path.to_string(),
            sync: None,
        }
    }

    #[test]
    fn discover_finds_collections() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("calendars")).unwrap();
        fs::write(root.join("calendars/event1.ics"), "BEGIN:VCALENDAR...").unwrap();

        fs::create_dir_all(root.join("contacts")).unwrap();
        fs::write(root.join("contacts/card1.ics"), "BEGIN:VCARD...").unwrap();

        // Nested collection
        fs::create_dir_all(root.join("work/projects")).unwrap();
        fs::write(root.join("work/projects/task1.ics"), "BEGIN:VCALENDAR...").unwrap();

        // Directory without .ics files (should not be discovered)
        fs::create_dir_all(root.join("empty")).unwrap();

        let source = make_ics_dir_source("personal", root.to_str().unwrap());
        let collections = discover(&source).unwrap();

        assert_eq!(collections.len(), 3);

        let names: Vec<&str> = collections.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"personal/calendars"));
        assert!(names.contains(&"personal/contacts"));
        assert!(names.contains(&"personal/work/projects"));
    }

    #[test]
    fn discover_empty_dir_returns_empty() {
        let dir = tempdir().unwrap();
        let source = make_ics_dir_source("empty", dir.path().to_str().unwrap());
        let collections = discover(&source).unwrap();
        assert!(collections.is_empty());
    }

    #[test]
    fn discover_nonexistent_dir_errors() {
        let source = make_ics_dir_source("bad", "/nonexistent/path");
        assert!(discover(&source).is_err());
    }

    #[test]
    fn parse_source_ref_basic() {
        assert_eq!(
            parse_source_ref("personal/calendars"),
            Some(("personal".to_string(), "calendars".to_string()))
        );
    }

    #[test]
    fn parse_source_ref_nested() {
        assert_eq!(
            parse_source_ref("personal/work/projects"),
            Some(("personal".to_string(), "work/projects".to_string()))
        );
    }

    #[test]
    fn parse_source_ref_quoted() {
        assert_eq!(
            parse_source_ref("personal/\"My Collection\""),
            Some(("personal".to_string(), "My Collection".to_string()))
        );
    }

    #[test]
    fn parse_source_ref_no_slash() {
        assert_eq!(parse_source_ref("work"), None);
    }

    #[test]
    fn parse_source_ref_empty_parts() {
        assert_eq!(parse_source_ref("/calendars"), None);
        assert_eq!(parse_source_ref("personal/"), None);
    }

    #[test]
    fn resolve_source_name_composite() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("calendars")).unwrap();
        fs::write(root.join("calendars/event1.ics"), "BEGIN:VCALENDAR...").unwrap();

        let source = make_ics_dir_source("personal", root.to_str().unwrap());
        let sources = vec![source];

        let resolved = resolve_source_name(&sources, "personal/calendars").unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "personal/calendars");
        assert_eq!(resolved[0].source_type, SourceType::Ics);
    }

    #[test]
    fn resolve_source_name_expands_ics_dir() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("calendars")).unwrap();
        fs::write(root.join("calendars/event1.ics"), "BEGIN:VCALENDAR...").unwrap();
        fs::create_dir_all(root.join("tasks")).unwrap();
        fs::write(root.join("tasks/task1.ics"), "BEGIN:VCALENDAR...").unwrap();

        let source = make_ics_dir_source("personal", root.to_str().unwrap());
        let sources = vec![source];

        let resolved = resolve_source_name(&sources, "personal").unwrap();
        assert_eq!(resolved.len(), 2);
        let names: Vec<&str> = resolved.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"personal/calendars"));
        assert!(names.contains(&"personal/tasks"));
    }

    #[test]
    fn resolve_source_name_regular_source() {
        let source = Source {
            name: "work".to_string(),
            source_type: SourceType::Jsonl,
            location: "/tmp/work".to_string(),
            sync: None,
        };
        let sources = vec![source.clone()];

        let resolved = resolve_source_name(&sources, "work").unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "work");
    }

    #[test]
    fn resolve_source_name_single_keeps_ics_dir() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("calendars")).unwrap();
        fs::write(root.join("calendars/event1.ics"), "BEGIN:VCALENDAR...").unwrap();
        fs::create_dir_all(root.join("tasks")).unwrap();
        fs::write(root.join("tasks/task1.ics"), "BEGIN:VCALENDAR...").unwrap();

        let source = make_ics_dir_source("personal", root.to_str().unwrap());
        let sources = vec![source.clone()];

        let resolved = resolve_source_name_single(&sources, "personal").unwrap();
        assert_eq!(resolved.name, "personal");
        assert_eq!(resolved.source_type, SourceType::IcsDir);
        assert_eq!(resolved.location, source.location);
    }

    #[test]
    fn resolve_source_name_single_composite_stays_one() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("calendars")).unwrap();
        fs::write(root.join("calendars/event1.ics"), "BEGIN:VCALENDAR...").unwrap();

        let source = make_ics_dir_source("personal", root.to_str().unwrap());
        let sources = vec![source];

        let resolved = resolve_source_name_single(&sources, "personal/calendars").unwrap();
        assert_eq!(resolved.name, "personal/calendars");
        assert_eq!(resolved.source_type, SourceType::Ics);
    }

    #[test]
    fn expand_ics_dir_inherits_sync() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        fs::create_dir_all(root.join("cal")).unwrap();
        fs::write(root.join("cal/a.ics"), "BEGIN:VCALENDAR...").unwrap();

        let source = Source {
            name: "rad".to_string(),
            source_type: SourceType::IcsDir,
            location: root.to_str().unwrap().to_string(),
            sync: Some(SyncConfig {
                pre_hook: Some("git pull".into()),
                cmd: Some("git push".into()),
                post_hook: None,
            }),
        };

        let expanded = expand_ics_dir(&source).unwrap();
        assert_eq!(expanded.len(), 1);
        assert!(expanded[0].sync.is_some());
        assert_eq!(
            expanded[0].sync.as_ref().unwrap().cmd,
            Some("git push".into())
        );
    }
}
