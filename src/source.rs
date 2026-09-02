//! 数据源发现与引用解析模块：处理 `IcsDir` 类型的数据源（Source discovery and reference resolution）。
//!
//! `IcsDir` 源指向一个根目录（如 Radicale/vdirsync 的同步根目录）。
//! 程序递归扫描该目录下的 `.ics` 文件来自动发现集合（collections）；
//! 每个含 `.ics` 的目录成为一个“虚拟源”，引用形式为 `<source_name>/<collection_path>`。
//!
//! 引用格式：`source:<name>/<collection>`（路径风格）。
//! 集合名含空格时用引号包裹：`source:<name>/"My Collection"`。
//!
//! 本模块与 `crate::config::Source` 配合：`SourceType::IcsDir` 展开为多个
//! `SourceType::Ics` 虚拟源，供上层按名称解析到具体目录。

use crate::config::{Source, SourceType};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// `IcsDir` 源中发现的一个集合（A discovered collection within an `IcsDir` source）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredCollection {
    /// 组合源名：`<source_name>/<relative_path>`（如 `personal/calendars`）。
    pub name: String,
    /// 集合目录的绝对路径（absolute path）。
    pub path: PathBuf,
}

/// 递归发现源根目录下所有包含 `.ics` 文件的目录（Recursively discover collections）。
///
/// 只要目录里至少有一个 `.ics` 文件，就成为一个集合；
/// 集合名是相对源根目录的路径。
/// `Result<T, E>`：成功返回集合列表，失败返回错误（anyhow 便于加上下文）。
pub fn discover(source: &Source) -> Result<Vec<DiscoveredCollection>> {
    let root = source.abs_location();
    // 根路径必须是目录；`bail!` 直接返回错误并附带信息。
    if !root.is_dir() {
        anyhow::bail!(
            "IcsDir source '{}' path is not a directory: {}",
            source.name,
            root.display()
        );
    }

    let mut collections = Vec::new();
    // 把可变引用 `&mut collections` 传下去，递归过程中不断追加结果（borrow 传递）。
    discover_recursive(&root, &root, &source.name, &mut collections)?;
    Ok(collections)
}

/// 递归扫描的辅助函数（private helper）。
///
/// `root` 是源根目录，`current` 是当前正在扫描的目录；函数先看当前目录
/// 是否有 `.ics` 文件，再递归进入子目录。`out` 是输出缓冲区（mutable borrow）。
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

        // 目录 → 递归深入；`.ics` 文件 → 标记当前目录含 ICS。
        if path.is_dir() {
            discover_recursive(root, &path, source_name, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("ics") {
            has_ics = true;
        }
    }

    // 当前目录含 `.ics` 才登记为一个集合。
    if has_ics {
        // 去掉根目录前缀得到相对路径；`strip_prefix` 返回 Option，失败用 `unwrap_or(current)`。
        let relative = current
            .strip_prefix(root)
            .unwrap_or(current)
            .to_string_lossy()
            .to_string();

        // 相对路径为空 → 集合名就是源名本身；否则 `源名/相对路径`。
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

/// 把源引用（如 `personal/calendars`）解析为 `(源名, 集合路径)`。
///
/// 支持带引号的集合名以容纳空格：`personal/"My Collection"`。
/// 若引用中没有 `/` 分隔符，返回 `None`。
/// `Option<T>`：有结果 → `Some`，无法解析 → `None`；`?` 遇到 `None` 直接返回 `None`。
pub fn parse_source_ref(name: &str) -> Option<(String, String)> {
    // 找第一个 `/`；找不到则 `?` 直接返回 None。
    let slash_pos = name.find('/')?;
    let source_name = name[..slash_pos].to_string();
    let collection_part = &name[slash_pos + 1..];

    // 源名或集合路径为空（如 `/x` 或 `x/`）→ 非法引用。
    if source_name.is_empty() || collection_part.is_empty() {
        return None;
    }

    // 处理带引号的集合名：去掉首尾引号，只取引号内文本。
    let collection = if let Some(inner) = collection_part.strip_prefix('"') {
        // 找结束引号；没有结束引号视为非法（`?` 返回 None）。
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

/// 把组合源名（如 `personal/calendars`）解析为目录路径。
///
/// 找到对应的 `IcsDir` 源 → 发现其集合 → 返回匹配集合的目录路径。
/// 返回 `Option<PathBuf>`：找不到匹配集合时是 `None`（不是错误）。
pub fn resolve_collection_path(source: &Source, collection: &str) -> Result<Option<PathBuf>> {
    let collections = discover(source)?;
    // 完整名称是 `源名/集合路径`，与 DiscoveredCollection.name 对齐。
    let full_name = format!("{}/{}", source.name, collection);
    // `.find(...)` 是迭代器查找；闭包 `|c| c.name == full_name` 判断是否匹配。
    // `.map(...)` 把 `Option<DiscoveredCollection>` 转成 `Option<PathBuf>`。
    Ok(collections
        .into_iter()
        .find(|c| c.name == full_name)
        .map(|c| c.path))
}

/// 把 `IcsDir` 源展开为每个集合对应的虚拟 `Ics` 源（Expand an `IcsDir` source）。
///
/// 每个虚拟源：名字 = 集合名，类型 = `Ics`，位置 = 集合目录绝对路径；
/// 同步配置继承自父 `IcsDir` 源。
pub fn expand_ics_dir(source: &Source) -> Result<Vec<Source>> {
    let collections = discover(source)?;
    Ok(collections
        .into_iter()
        // `.map` 用闭包把每个集合转换为一个虚拟 `Source`。
        .map(|c| Source {
            name: c.name,
            source_type: SourceType::Ics,
            location: c.path.to_string_lossy().to_string(),
            // 同步配置从父源克隆（clone），每个虚拟源共享同一套 hook。
            sync: source.sync.clone(),
        })
        .collect())
}

/// 解析可能为组合引用（composite reference）的源名，返回一个或多个实际源。
///
/// - 名字含 `/` 且匹配某个 `IcsDir` 集合 → 单个虚拟 `Ics` 源
/// - 名字匹配某个 `IcsDir` 源 → 展开为全部集合
/// - 名字匹配普通源 → 原样返回
pub fn resolve_source_name(sources: &[Source], name: &str) -> Result<Vec<Source>> {
    // 先尝试按组合引用解析：`源名/集合路径`。
    if let Some((source_name, collection)) = parse_source_ref(name) {
        // let-chain 多条件：找到父 IcsDir 源 → 父是 IcsDir → 集合路径能解析。
        if let Some(parent) = sources.iter().find(|s| s.name == source_name)
            && parent.source_type == SourceType::IcsDir
            && let Some(path) = resolve_collection_path(parent, &collection)?
        {
            // 构造一个指向该集合目录的虚拟 Ics 源。
            return Ok(vec![Source {
                name: name.to_string(),
                source_type: SourceType::Ics,
                location: path.to_string_lossy().to_string(),
                sync: parent.sync.clone(),
            }]);
        }
        // 组合引用解析失败 → 报“未知源”错误。
        anyhow::bail!("unknown source `{name}`");
    }

    // 非组合引用：直接按名字查源。
    if let Some(src) = sources.iter().find(|s| s.name == *name) {
        // `IcsDir` → 展开全部集合；普通源 → 原样返回（克隆）。
        if src.source_type == SourceType::IcsDir {
            expand_ics_dir(src)
        } else {
            Ok(vec![src.clone()])
        }
    } else {
        anyhow::bail!("unknown source `{name}`");
    }
}

#[cfg(test)]
mod tests {
    // 数据源模块单元测试：验证目录发现、引用解析、组合引用与 IcsDir 展开。
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
