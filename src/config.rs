//! Configuration loader/saver for `~/.config/calman/config.toml`.
//!
//! Layout follows DESIGN.md §1. On first run a default config is generated
//! and written if the file is absent. Contexts fall back to source lists.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Config directory name under the XDG config root.
pub const CONFIG_DIR: &str = "calman";
/// Config file name.
pub const CONFIG_FILE: &str = "config.toml";

/// Storage backend for a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Jsonl,
    Ics,
    /// Directory of ICS collections (Radicale/vdirsync layout).
    /// Auto-discovers subdirectories containing `.ics` files.
    #[serde(rename = "ics-dir")]
    IcsDir,
}

/// Optional per-source sync configuration (`pre_hook` → `cmd` → `post_hook`).
///
/// `cmd == None` marks the source as non-syncable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_hook: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_hook: Option<String>,
}

impl SyncConfig {
    pub fn is_syncable(&self) -> bool {
        self.cmd.is_some()
    }
}

/// A single data source (`[[source]]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub name: String,
    #[serde(rename = "type")]
    pub source_type: SourceType,
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<SyncConfig>,
}

impl Source {
    /// Absolute location with `~` expanded.
    pub fn abs_location(&self) -> PathBuf {
        expand_tilde(&self.location)
    }
}

/// `[defaults]` — the default write target and report for `add` / bare `calman`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    pub write_source: String,
    /// Name of the default report run by bare `calman` (default `next`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_report: Option<String>,
}

/// A single report column.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnCfg {
    pub field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<usize>,
    /// `relative | countdown | iso | truncate | date`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Date format for the `date` column when the row is an event (e.g. "%m/%d").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_format: Option<String>,
    /// Date format for the `date` column when the row is a todo: `relative`,
    /// `countdown`, `iso`, or a chrono strftime pattern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todo_format: Option<String>,
    /// Render a nerdfont glyph instead of text (status/type only).
    #[serde(default)]
    pub icon: bool,
    /// Per-column icon overrides (keyed by value, e.g. `completed = "✔"`).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub icons: HashMap<String, String>,
}

/// `[report.<name>]` — a user report definition.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportCfg {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Sort keys: `key+`, `key-`, trailing `/` inserts a break line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<ColumnCfg>,
}

/// `[icons]` — global nerdfont icon overrides for status/type.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconsCfg {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub status: HashMap<String, String>,
    #[serde(default, rename = "type", skip_serializing_if = "HashMap::is_empty")]
    pub r#type: HashMap<String, String>,
}

/// `[contexts]` — default source lists per context.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contexts {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cli: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sync: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tui: Vec<String>,
}

/// `[date]` — date-parsing settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateConfig {
    #[serde(default = "default_workweek_end")]
    pub workweek_end: String,
    #[serde(default = "default_week_start")]
    pub week_start: String,
    /// Time used when a `modify start:<date>` needs an hour (e.g. "09:00").
    #[serde(default = "default_start_time")]
    pub default_start_time: String,
    /// Fallback event length when neither `end:` nor `duration:` given (e.g. "1h").
    #[serde(default = "default_event_duration")]
    pub default_event_duration: String,
}

/// `[ui]` — TUI settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub vim_keys: bool,
    #[serde(default)]
    pub default_filter: String,
}

/// `[locale]` — UI language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocaleConfig {
    pub language: String,
}

/// Root config document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Extra TOML files to merge (paths relative to this file, `~/` ok).
    /// Merged first; values here take precedence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub contexts: Contexts,
    #[serde(default)]
    pub date: DateConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub locale: LocaleConfig,
    #[serde(default, rename = "source")]
    pub sources: Vec<Source>,
    #[serde(default, rename = "report")]
    pub reports: HashMap<String, ReportCfg>,
    #[serde(default)]
    pub icons: IconsCfg,
    /// `[theme]` palette (used by the Phase-3 TUI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeCfg>,
}

/// `[theme]` — named palette + taskwarrior-style color rules for reports.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeCfg {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `rule.precedence.color` — comma-separated rule order, first match wins.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "rule.precedence.color"
    )]
    pub precedence: Option<String>,
    /// `color.<rule> = "<fg> [on <bg>] [bold|underline|inverse|...]"`.
    /// Supported rules: deleted completed active overdue due.today due
    /// blocked blocking scheduled tagged uda.priority.L/M/H.
    #[serde(default, skip_serializing_if = "HashMap::is_empty", rename = "color")]
    pub colors: HashMap<String, String>,
}

fn default_workweek_end() -> String {
    "17:00".to_string()
}

fn default_week_start() -> String {
    "monday".to_string()
}

fn default_start_time() -> String {
    "09:00".to_string()
}

fn default_event_duration() -> String {
    "1h".to_string()
}

impl Default for DateConfig {
    fn default() -> Self {
        Self {
            workweek_end: default_workweek_end(),
            week_start: default_week_start(),
            default_start_time: default_start_time(),
            default_event_duration: default_event_duration(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "default".to_string(),
            vim_keys: true,
            default_filter: "all".to_string(),
        }
    }
}

impl Default for LocaleConfig {
    fn default() -> Self {
        Self {
            language: "en".to_string(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            include: Vec::new(),
            defaults: Defaults {
                write_source: "work".to_string(),
                default_report: None,
            },
            // Empty contexts → fall back to all (selected) sources per DESIGN §1.2.
            contexts: Contexts::default(),
            date: DateConfig::default(),
            ui: UiConfig::default(),
            locale: LocaleConfig::default(),
            sources: vec![Source {
                name: "work".to_string(),
                source_type: SourceType::Jsonl,
                location: "~/.local/share/calman/work/".to_string(),
                sync: None,
            }],
            reports: HashMap::new(),
            icons: IconsCfg::default(),
            theme: None,
        }
    }
}

impl Config {
    /// Load config from the default path, generating a default on first run.
    pub fn load() -> Result<Config> {
        let path = default_config_path()?;
        Self::load_from(&path)
    }

    /// Load from an explicit path, generating a default if absent.
    pub fn load_from(path: &Path) -> Result<Config> {
        if !path.exists() {
            let cfg = Config::default();
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create config dir {}", parent.display()))?;
            }
            cfg.save(path)?;
            return Ok(cfg);
        }
        let content =
            fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
        let mut value: toml::Value =
            toml::from_str(&content).with_context(|| format!("parse config {}", path.display()))?;

        // Merge `include` files (relative to this config's dir) beneath us:
        // included files fill missing keys; main-file values win.
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let includes = value
            .get("include")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for inc in includes {
            if let Some(rel) = inc.as_str() {
                let inc_path = resolve_include(parent, rel);
                let inc_text = fs::read_to_string(&inc_path)
                    .with_context(|| format!("read include {}", inc_path.display()))?;
                let inc_value: toml::Value = toml::from_str(&inc_text)
                    .with_context(|| format!("parse include {}", inc_path.display()))?;
                value = merge(value, inc_value);
            }
        }

        Config::deserialize(value).with_context(|| format!("parse config {}", path.display()))
    }

    /// Atomically persist the config (tmp file + rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string(self)?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, content).with_context(|| format!("write {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| format!("rename onto {}", path.display()))?;
        Ok(())
    }

    /// The default write source name for `add`.
    pub fn write_source(&self) -> &str {
        &self.defaults.write_source
    }

    /// Source list for a named context, with fallback per DESIGN.md §1.2.
    pub fn context_sources(&self, context: ContextKind) -> Vec<String> {
        let names = self
            .sources
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>();
        match context {
            ContextKind::Cli => {
                if self.contexts.cli.is_empty() {
                    names
                } else {
                    self.contexts.cli.clone()
                }
            }
            ContextKind::Sync => {
                if self.contexts.sync.is_empty() {
                    self.sources
                        .iter()
                        .filter(|s| s.sync.as_ref().is_some_and(|sc| sc.is_syncable()))
                        .map(|s| s.name.clone())
                        .collect()
                } else {
                    self.contexts.sync.clone()
                }
            }
            ContextKind::Tui => {
                if self.contexts.tui.is_empty() {
                    names
                } else {
                    self.contexts.tui.clone()
                }
            }
        }
    }

    /// Look up a source by name.
    pub fn source(&self, name: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.name == name)
    }
}

/// Which default source list a command uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextKind {
    Cli,
    Sync,
    #[allow(dead_code)] // used by Phase-3 TUI
    Tui,
}

/// Expand a leading `~` to `$HOME`. Leaves other paths untouched.
pub fn expand_tilde(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(raw)
}

/// Resolve the default config path (XDG or `$HOME/.config`).
pub fn default_config_path() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let home = std::env::var_os("HOME").context("HOME not set")?;
            PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join(CONFIG_DIR).join(CONFIG_FILE))
}

/// Resolve an include path: absolute, `~/`-expanded, or relative to the
/// config file's directory.
fn resolve_include(base: &Path, raw: &str) -> PathBuf {
    let p = if raw.starts_with('/') {
        PathBuf::from(raw)
    } else {
        base.join(raw)
    };
    expand_tilde(p.to_str().unwrap_or(raw))
}

/// Merge `inc` into `main`: tables recurse, scalars/lists in `main` win,
/// keys missing from `main` are taken from `inc`.
fn merge(main: toml::Value, inc: toml::Value) -> toml::Value {
    match (main, inc) {
        (toml::Value::Table(mut m), toml::Value::Table(i)) => {
            for (k, iv) in i {
                match m.get_mut(&k) {
                    Some(mv) if mv.is_table() && iv.is_table() => {
                        *mv = merge(mv.clone(), iv);
                    }
                    Some(_) => {} // main wins for scalars/lists
                    None => {
                        m.insert(k, iv);
                    }
                }
            }
            toml::Value::Table(m)
        }
        (m, _) => m,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn cfg() -> Config {
        Config {
            include: Vec::new(),
            defaults: Defaults {
                write_source: "work".into(),
                default_report: None,
            },
            contexts: Contexts {
                cli: vec!["work".into(), "personal".into()],
                sync: vec!["work".into()],
                tui: Vec::new(),
            },
            date: DateConfig::default(),
            ui: UiConfig::default(),
            locale: LocaleConfig::default(),
            reports: HashMap::new(),
            icons: IconsCfg::default(),
            theme: None,
            sources: vec![
                Source {
                    name: "work".into(),
                    source_type: SourceType::Jsonl,
                    location: "~/.local/share/calman/work/".into(),
                    sync: None,
                },
                Source {
                    name: "personal".into(),
                    source_type: SourceType::Jsonl,
                    location: "~/.local/share/calman/personal/".into(),
                    sync: Some(SyncConfig {
                        pre_hook: Some("git pull".into()),
                        cmd: Some("git push".into()),
                        post_hook: None,
                    }),
                },
            ],
        }
    }

    #[test]
    fn generates_default_when_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.write_source(), "work");
        assert!(path.exists());
    }

    #[test]
    fn roundtrip_serialize() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let c = cfg();
        c.save(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded, c);
    }

    #[test]
    fn serializes_source_table_name() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        cfg().save(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        // DESIGN §1 uses `[[source]]`, not `[[sources]]`.
        assert!(text.contains("[[source]]"));
        assert!(!text.contains("[[sources]]"));
    }

    #[test]
    fn cli_context_falls_back_to_all_sources() {
        let c = cfg();
        // cli is explicitly set
        assert_eq!(
            c.context_sources(ContextKind::Cli),
            vec!["work", "personal"]
        );
        // tui is empty → all sources
        assert_eq!(
            c.context_sources(ContextKind::Tui),
            vec!["work", "personal"]
        );
    }

    #[test]
    fn sync_context_filters_syncable() {
        let mut c = cfg();
        c.contexts.sync = Vec::new();
        // only `personal` has a cmd → syncable
        assert_eq!(c.context_sources(ContextKind::Sync), vec!["personal"]);
    }

    #[test]
    fn tilde_expansion() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_tilde("~/x/y"), PathBuf::from(home).join("x/y"));
        assert_eq!(expand_tilde("/abs/path"), PathBuf::from("/abs/path"));
    }

    #[test]
    fn include_files_are_merged() {
        let dir = tempdir().unwrap();
        let main = dir.path().join("config.toml");
        fs::write(
            &main,
            r#"include = ["report.toml", "theme.toml"]
[defaults]
write_source = "work"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("report.toml"),
            r#"[report.next]
filter = "status:active"
sort = ["due+"]
columns = [
  { field = "id", label = "ID" },
]
[icons.status]
pending = "○"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("theme.toml"),
            r#"[ui]
theme = "dark"
[theme]
name = "dark"
"rule.precedence.color" = "completed,overdue"
[theme.color]
completed = "gray10 on gray2"
overdue = "inverse"
"#,
        )
        .unwrap();

        let cfg = Config::load_from(&main).unwrap();
        assert!(cfg.reports.contains_key("next"));
        assert_eq!(
            cfg.icons.status.get("pending").map(|s| s.as_str()),
            Some("○")
        );
        assert_eq!(cfg.ui.theme, "dark");
        let theme = cfg.theme.unwrap();
        assert_eq!(theme.precedence.as_deref(), Some("completed,overdue"));
        assert_eq!(
            theme.colors.get("completed").map(|s| s.as_str()),
            Some("gray10 on gray2")
        );
    }
}
