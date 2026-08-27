//! Configuration loader/saver for `~/.config/calman/config.toml`.
//!
//! Layout follows DESIGN.md §1. On first run a default config is generated
//! and written if the file is absent. Contexts fall back to source lists.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
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

/// `[defaults]` — the default write target for `add`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    pub write_source: String,
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
    pub theme: String,
    pub vim_keys: bool,
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
            defaults: Defaults {
                write_source: "work".to_string(),
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
        toml::from_str(&content).with_context(|| format!("parse config {}", path.display()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn cfg() -> Config {
        Config {
            defaults: Defaults {
                write_source: "work".into(),
            },
            contexts: Contexts {
                cli: vec!["work".into(), "personal".into()],
                sync: vec!["work".into()],
                tui: Vec::new(),
            },
            date: DateConfig::default(),
            ui: UiConfig::default(),
            locale: LocaleConfig::default(),
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
}
