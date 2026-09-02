//! 配置加载/保存模块：读写 `~/.config/calman/config.toml`（Configuration loader/saver）。
//!
//! 配置用 TOML 编写，经 serde 自动映射到 Rust 结构体（serde 负责反序列化/序列化）。
//! 配置支持两档：用户主配置 `config.toml` 可 `include` 其他 TOML 文件并合并；
//! 首次运行无配置文件时，自动生成并写入默认配置（default config）。
//! 上下文（contexts）为空时回退到默认源列表（context fallback）。
//! 本模块还负责 `~` 展开（tilde expansion）与包含文件解析（include-file resolution）。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use chrono_tz::Tz;

/// XDG 配置根目录下的子目录名（Config directory name）。
/// 最终路径形如 `~/.config/calman/`。
pub const CONFIG_DIR: &str = "calman";
/// 配置文件名称（Config file name）。
pub const CONFIG_FILE: &str = "config.toml";

/// 数据源存储后端（Storage backend for a source）。
///
/// serde 会把枚举值按小写字符串读写（`#[serde(rename_all = "lowercase")]`），
/// 所以 TOML 里写 `type = "jsonl"`、`type = "ics"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    /// JSONL 文件：每行一条 JSON 任务记录（line-delimited JSON）。
    Jsonl,
    /// 单个 ICS 日历文件（iCalendar 格式）。
    Ics,
    /// 目录型 ICS 集合（Radicale/vdirsync 布局）：自动发现含 `.ics` 的子目录。
    ///
    /// 通过 `#[serde(rename = "ics-dir")]` 让 TOML 中的 `"ics-dir"` 映射到此变体。
    #[serde(rename = "ics-dir")]
    IcsDir,
}

/// 可选的数据源同步配置（Optional per-source sync configuration）。
///
/// 同步流程：`pre_hook`（同步前命令）→ `cmd`（同步命令）→ `post_hook`（同步后命令）。
/// `cmd == None` 表示该源不可同步（non-syncable）。
/// `Option<String>`：有值（Some）或无值（None）；serde 的
/// `skip_serializing_if = "Option::is_none"` 让空字段在写出 TOML 时省略。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncConfig {
    /// 同步前执行的命令（hook，如 `git pull`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_hook: Option<String>,
    /// 实际同步命令（如 `git push`）；为 `None` 时不可同步。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<String>,
    /// 同步后执行的命令（如 `git push --tags`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_hook: Option<String>,
}

impl SyncConfig {
    /// 是否可同步：存在 `cmd`（同步命令）即可。
    pub fn is_syncable(&self) -> bool {
        self.cmd.is_some()
    }
}

/// 单个数据源（`[[source]]` 表，A single data source）。
///
/// TOML 中 `[[source]]` 数组的每一项代表一个数据源（如 `work`、`personal`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// 源名称（source name），供命令引用，如 `work`。
    pub name: String,
    /// 存储后端类型；`#[serde(rename = "type")]` 把 TOML 的 `type` 键映射到此字段。
    #[serde(rename = "type")]
    pub source_type: SourceType,
    /// 存储位置：路径或目录（支持 `~/` 开头的家目录路径）。
    pub location: String,
    /// 可选同步配置；`None` 表示不参与同步。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<SyncConfig>,
}

impl Source {
    /// 返回绝对路径：把开头的 `~` 展开为 `$HOME`（tilde expansion）。
    pub fn abs_location(&self) -> PathBuf {
        expand_tilde(&self.location)
    }
}

/// `[defaults]` 段：`add` 命令与裸 `calman` 的默认写入源和报告（default write target/report）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    /// 默认写入源（write source），即 `add` 把任务写到哪个源。
    /// `#[serde(default = "default_write_source")]`：TOML 缺失时调用该函数取默认值 `"work"`。
    #[serde(default = "default_write_source")]
    pub write_source: String,
    /// 裸 `calman` 默认运行的报告名（默认 `next`）；`None` 表示未设置。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_report: Option<String>,
    /// `list`/`next` 中每个重复系列（recurring series）展开的未来发生次数。
    /// `0` = 在时间窗口内展开全部未来事件。
    #[serde(default = "default_recur_expand_count")]
    pub recur_expand_count: usize,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            write_source: "work".to_string(),
            default_report: None,
            recur_expand_count: default_recur_expand_count(),
        }
    }
}

/// serde 默认值函数：`default = "default_write_source"` 指向此函数。
fn default_write_source() -> String {
    "work".to_string()
}

/// serde 默认值函数：未配置 `recur_expand_count` 时展开 1 次。
fn default_recur_expand_count() -> usize {
    1
}

/// 报告中的一列（A single report column）。
///
/// 每个字段都可选，缺失时使用 serde 默认值：
/// `#[serde(default)]` 表示取 Rust 类型的 `Default`（`None`/空容器/false）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnCfg {
    /// 列对应的任务字段名（如 `id`、`due`、`status`）。
    pub field: String,
    /// 列标题（label）；`None` 时显示默认标题。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// 列宽（字符数）；`None` 表示自动宽度。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<usize>,
    /// 日期/文本格式化方式：`relative | countdown | iso | truncate | date`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// `date` 列在事件（event）行的日期格式，如 `"%m/%d"`（chrono 格式串）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_format: Option<String>,
    /// `date` 列在待办（todo）行的日期格式：`relative`、`countdown`、`iso` 或 chrono 格式串。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todo_format: Option<String>,
    /// 是否用 nerdfont 图标代替文本渲染（仅 status/type 列）。
    #[serde(default)]
    pub icon: bool,
    /// 按值覆盖列图标（keyed by value，如 `completed = "✔"`）。
    /// `HashMap<String, String>` 是键值对容器；`skip_serializing_if = "HashMap::is_empty"` 空表不写出。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub icons: HashMap<String, String>,
}

/// `[report.<name>]` 段：用户自定义报告（user report definition）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportCfg {
    /// 报告过滤表达式（filter expression，复用 CLI 的过滤语法）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// 排序键：`key+` 升序、`key-` 降序；末尾 `/` 表示插入分隔行。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<String>,
    /// 报告列定义列表（columns）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<ColumnCfg>,
}

/// `[icons]` 段：按任务类型覆盖全局 nerdfont 图标。
///
/// `[icons.todo]` / `[icons.event]` 把状态键映射到字形：
/// `pending`、`in-progress`、`completed`、`cancelled`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconsCfg {
    /// 待办（todo）状态的图标映射。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub todo: HashMap<String, String>,
    /// 事件（event）状态的图标映射。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub event: HashMap<String, String>,
}

/// `[contexts]` 段：每个上下文（context）默认使用的数据源列表。
///
/// 上下文决定命令默认操作哪些源；列表为空时回退到全部源（fallback）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contexts {
    /// CLI 上下文默认源名列表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cli: Vec<String>,
    /// 同步上下文默认源名列表（仅列出可同步的源）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sync: Vec<String>,
    /// TUI 上下文默认源名列表（Phase-3 使用）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tui: Vec<String>,
}

/// `[date]` 段：日期解析设置（date-parsing settings）。
///
/// 这些字符串默认值由 `#[serde(default = "函数名")]` 提供。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateConfig {
    /// 命名边界（`sod`/`sow`/`som`/…）的“日始”时间，格式 `"HH:MM:SS"`。
    #[serde(default = "default_day_start")]
    pub day_start: String,
    /// 命名边界（`eod`/`eow`/`eom`/…）的“日终”时间，格式 `"HH:MM:SS"`。
    #[serde(default = "default_day_end")]
    pub day_end: String,
    /// 未给 `end:` 或 `duration:` 时的事件默认时长。
    /// 空字符串表示瞬时事件（只有 DTSTART，无 DTEND）。
    #[serde(default = "default_event_duration")]
    pub default_event_duration: String,
    /// 把带时间的 `DTSTART`/`DTEND`/`DUE` 序列化到 ICS 时使用的 IANA 时区
    /// （带 `TZID`，iOS 风格本地墙钟时间）。省略时自动探测系统时区。
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

/// 根配置文档（Root config document）：对应整个 `config.toml` 文件。
///
/// 每个字段都有 `#[serde(default)]`，所以 TOML 里缺哪个段都能用默认值兜底。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// 要合并的额外 TOML 文件（路径相对本文件，`~/` 也允许）。
    /// 这些文件先合并（merged first），本文件的值优先（values here take precedence）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    /// `[defaults]` 段。
    #[serde(default)]
    pub defaults: Defaults,
    /// `[contexts]` 段。
    #[serde(default)]
    pub contexts: Contexts,
    /// `[date]` 段。
    #[serde(default)]
    pub date: DateConfig,
    /// `[[source]]` 数组；`#[serde(rename = "source")]` 让 TOML 键 `source` 映射到此字段。
    #[serde(default, rename = "source")]
    pub sources: Vec<Source>,
    /// `[report]` 表：报告名 → 报告配置。`HashMap<String, ReportCfg>` 由 TOML 表驱动。
    #[serde(default, rename = "report")]
    pub reports: HashMap<String, ReportCfg>,
    /// `[icons]` 段。
    #[serde(default)]
    pub icons: IconsCfg,
    /// `[colorscheme]` 段：报告行配色（优先级顺序 + 每条规则的样式）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colorscheme: Option<ColorSchemeCfg>,
    /// `[tui]` 段：Phase-3 TUI 设置（预留）。
    #[serde(default)]
    pub tui: TuiConfig,
}

/// `[tui]` 段：TUI 设置（Phase-3，预留）。`default_filter` 是 TUI 默认视图过滤器
/// （"todo" | "event" | "all"）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuiConfig {
    /// 是否启用 vim 风格按键（默认 `true`）。
    #[serde(default)]
    pub vim_keys: bool,
    /// 默认视图过滤条件字符串。
    #[serde(default)]
    pub default_filter: String,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            vim_keys: true,
            default_filter: String::new(),
        }
    }
}

/// `[colorscheme]` 段：报告行级配色（report row-level coloring）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorSchemeCfg {
    /// 规则优先级（第一个匹配者生效，first match wins）。
    /// 支持的规则键：completed cancelled overdue today due blocked blocking
    /// tagged priority.L priority.M priority.H（`scheduled` 已被移除）。
    /// Rule precedence (first match wins). Supported rule keys:
    /// completed cancelled overdue today due blocked blocking
    /// tagged priority.L priority.M priority.H.
    #[serde(default)]
    pub priority: Vec<String>,
    /// 命名颜色 → hex（或任意 CSS 风格颜色值），供 `fg`/`bg` 引用。
    #[serde(default)]
    pub palette: HashMap<String, String>,
    /// 每条规则的样式表，如 `[colorscheme.rules] overdue = {inverse = true}`。
    #[serde(default)]
    pub rules: HashMap<String, RuleStyle>,
}

/// 单个颜色规则的样式（A single color rule's style）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleStyle {
    /// 前景色（foreground color）。
    #[serde(default)]
    pub fg: Option<String>,
    /// 背景色（background color）。
    #[serde(default)]
    pub bg: Option<String>,
    /// 加粗。
    #[serde(default)]
    pub bold: bool,
    /// 斜体。
    #[serde(default)]
    pub italic: bool,
    /// 下划线。
    #[serde(default)]
    pub underline: bool,
    /// 暗色（dim）。
    #[serde(default)]
    pub dim: bool,
    /// 反色（inverse）。
    #[serde(default)]
    pub inverse: bool,
}

/// serde 默认值：日始为 `00:00:00`。
fn default_day_start() -> String {
    "00:00:00".to_string()
}

/// serde 默认值：日终为 `23:59:59`。
fn default_day_end() -> String {
    "23:59:59".to_string()
}

/// serde 默认值：事件默认时长为空（表示瞬时事件）。
fn default_event_duration() -> String {
    String::new()
}

/// serde 默认值：尽力探测系统 IANA 时区，失败则回退 UTC（best-effort）。
fn default_timezone() -> String {
    // 先看环境变量 `TZ`（用户显式指定时区）→ 直接用。
    // `if let Ok(v) = ... && !v.is_empty()` 是 Rust 的 let-chain：条件成立才进入分支。
    if let Ok(v) = std::env::var("TZ") && !v.is_empty() {
        return v;
    }
    // 再读 `/etc/localtime` 符号链接，从中提取 `zoneinfo/` 后的时区名（如 `Asia/Shanghai`）。
    // `to_string_lossy()`：把非 UTF-8 路径安全地转成字符串（lossy = 有损但不会 panic）。
    if let Ok(link) = std::fs::read_link("/etc/localtime")
        && let Some(name) = link.to_string_lossy().split("zoneinfo/").nth(1)
    {
        return name.to_string();
    }
    // 都失败 → 用 UTC 兜底。
    "UTC".to_string()
}

impl DateConfig {
    /// 把配置的时区字符串解析为 `Tz`；未知时区回退 UTC。
    ///
    /// `self.timezone.parse::<Tz>()` 返回 `Result`，`unwrap_or` 取成功值，失败时用默认值。
    pub fn tz(&self) -> Tz {
        self.timezone.parse::<Tz>().unwrap_or(Tz::UTC)
    }
}

impl Default for DateConfig {
    fn default() -> Self {
        Self {
            day_start: default_day_start(),
            day_end: default_day_end(),
            default_event_duration: default_event_duration(),
            timezone: default_timezone(),
        }
    }
}

impl Default for Config {
    // 无配置时的默认值：一个名为 `work` 的 JSONL 源，空上下文 → 回退到该源。
    fn default() -> Self {
        Config {
            include: Vec::new(),
            defaults: Defaults {
                write_source: "work".to_string(),
                default_report: None,
                recur_expand_count: default_recur_expand_count(),
            },
            // 空 contexts（CLI/Sync/TUI 均未配置）→ 回退：CLI/TUI 用全部源，Sync 用可同步的源。
            contexts: Contexts::default(),
            date: DateConfig::default(),
            sources: vec![Source {
                name: "work".to_string(),
                source_type: SourceType::Jsonl,
                location: "~/.local/share/calman/work/".to_string(),
                sync: None,
            }],
            reports: HashMap::new(),
            icons: IconsCfg::default(),
            colorscheme: None,
            tui: TuiConfig::default(),
        }
    }
}

impl Config {
    /// 从默认路径加载配置；文件不存在时先生成默认配置（first-run default）。
    ///
    /// `?` 运算符：若 `Result` 是 `Err`，立即从当前函数返回该错误。
    pub fn load() -> Result<Config> {
        let path = default_config_path()?;
        Self::load_from(&path)
    }

    /// 从显式路径加载配置；文件不存在时生成默认配置。
    ///
    /// 加载流程：读 TOML 文本 → 解析为 `toml::Value`（通用 TOML 值树）→
    /// 合并 `include` 文件 → `Config::deserialize` 转成强类型结构体。
    pub fn load_from(path: &Path) -> Result<Config> {
        if !path.exists() {
            // 无配置文件：生成默认配置，先建父目录再保存（save 用原子写）。
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
        // 先解析成 `toml::Value`，因为合并 include 需要在通用值层面操作。
        let mut value: toml::Value =
            toml::from_str(&content).with_context(|| format!("parse config {}", path.display()))?;

        // 合并 `include` 文件（相对本配置文件目录解析）：
        // include 文件填缺失的键，主文件里已有的值优先（main wins）。
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
                // 把 include 值合并进主值（主值优先）。
                value = merge(value, inc_value);
            }
        }

        // `Config::deserialize`：serde 把 `toml::Value` 反序列化为 `Config` 结构体。
        Config::deserialize(value).with_context(|| format!("parse config {}", path.display()))
    }

    /// 原子保存配置（atomic save）：先写临时文件再 rename，避免写一半损坏文件。
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string(self)?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, content).with_context(|| format!("write {}", tmp.display()))?;
        // rename 在同一文件系统内是原子操作：要么旧文件，要么新文件，不会出现中间态。
        fs::rename(&tmp, path).with_context(|| format!("rename onto {}", path.display()))?;
        Ok(())
    }

    /// 返回 `add` 命令的默认写入源名称（default write source）。
    pub fn write_source(&self) -> &str {
        &self.defaults.write_source
    }

    /// 返回某上下文（context）的源名列表；对应上下文列表为空时回退。
    ///
    /// 回退规则：CLI/TUI 空 → 全部源；Sync 空 → 仅可同步的源（syncable）。
    pub fn context_sources(&self, context: ContextKind) -> Vec<String> {
        let names = self
            .sources
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>();
        match context {
            ContextKind::Cli => {
                if self.contexts.cli.is_empty() {
                    // 回退：CLI 未配置 → 使用全部源。
                    names
                } else {
                    self.contexts.cli.clone()
                }
            }
            ContextKind::Sync => {
                if self.contexts.sync.is_empty() {
                    // 回退：Sync 未配置 → 只选可同步的源。
                    // `.filter(...)` 是迭代器过滤；`.is_some_and(...)`：Option 为 Some 且闭包为真。
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
                    // 回退：TUI 未配置 → 使用全部源。
                    names
                } else {
                    self.contexts.tui.clone()
                }
            }
        }
    }
}

/// 命令使用哪个默认源列表（Which default source list a command uses）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextKind {
    /// CLI 命令（如 `calman list`）。
    Cli,
    /// 同步命令（`calman sync`）。
    Sync,
    /// TUI（Phase-3 使用，目前代码未引用故允许 dead_code）。
    #[allow(dead_code)] // used by Phase-3 TUI
    Tui,
}

/// 把开头的 `~` 展开为 `$HOME`；其他路径原样返回（Expand a leading `~` to `$HOME`）。
///
/// `strip_prefix("~/")` 返回 `Option<&str>`：匹配则返回剩余部分。
/// `PathBuf::join` 用平台正确的分隔符拼接路径。
pub fn expand_tilde(raw: &str) -> PathBuf {
    // let-chain：`~` 前缀存在 且 环境变量 HOME 存在，两个条件都满足才展开。
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(raw)
}

/// 解析默认配置路径：优先 XDG_CONFIG_HOME，否则 `$HOME/.config`（XDG base directory 规范）。
pub fn default_config_path() -> Result<PathBuf> {
    // `match` 的守卫（guard）：仅当 `Some(dir)` 且非空时使用 XDG 目录。
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            // `.context("HOME not set")`：给 anyhow 错误补充说明，便于排查。
            let home = std::env::var_os("HOME").context("HOME not set")?;
            PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join(CONFIG_DIR).join(CONFIG_FILE))
}

/// 解析 include 文件路径：绝对路径、`~/` 展开、或相对配置文件目录。
fn resolve_include(base: &Path, raw: &str) -> PathBuf {
    // 绝对路径（以 `/` 开头）直接用；否则拼到主配置所在目录下。
    let p = if raw.starts_with('/') {
        PathBuf::from(raw)
    } else {
        base.join(raw)
    };
    // `to_str().unwrap_or(raw)`：Path 转字符串，失败（非 UTF-8）时退回原始字符串。
    expand_tilde(p.to_str().unwrap_or(raw))
}

/// 把 `inc` 合并进 `main`：表（table）递归合并；标量/列表以 `main` 为准；
/// `main` 缺失的键从 `inc` 取（Merge `inc` into `main`）。
fn merge(main: toml::Value, inc: toml::Value) -> toml::Value {
    match (main, inc) {
        // 双方都是表（TOML table，即 `{...}`）才需要逐键合并。
        (toml::Value::Table(mut m), toml::Value::Table(i)) => {
            for (k, iv) in i {
                match m.get_mut(&k) {
                    // 两边都是表 → 递归合并（deep merge），例如 `[report.next]` 下的嵌套。
                    Some(mv) if mv.is_table() && iv.is_table() => {
                        *mv = merge(mv.clone(), iv);
                    }
                    // 主文件已有非表值（标量/列表）→ 主文件优先，跳过 include 的值。
                    Some(_) => {} // main wins for scalars/lists
                    // 主文件没有该键 → 从 include 补进来。
                    None => {
                        m.insert(k, iv);
                    }
                }
            }
            toml::Value::Table(m)
        }
        // 任一侧不是表 → 直接返回主值（main wins）。
        (m, _) => m,
    }
}

#[cfg(test)]
mod tests {
    // 配置模块单元测试（unit tests）：验证默认值、序列化往返、include 合并、~ 展开等。
    use super::*;
    use tempfile::tempdir;

    fn cfg() -> Config {
        Config {
            include: Vec::new(),
            defaults: Defaults {
                write_source: "work".into(),
                default_report: None,
                recur_expand_count: 1,
            },
            contexts: Contexts {
                cli: vec!["work".into(), "personal".into()],
                sync: vec!["work".into()],
                tui: Vec::new(),
            },
            date: DateConfig::default(),
            reports: HashMap::new(),
            icons: IconsCfg::default(),
            colorscheme: None,
            tui: TuiConfig::default(),
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
    fn defaults_missing_write_source_falls_back_to_work() {
        // A user config with sources but no [defaults] table must still resolve
        // write_source; otherwise `add` breaks with "unknown write source"".
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[[source]]\nname = \"main\"\ntype = \"jsonl\"\nlocation = \"~/.local/share/calman/main/\"\n",
        )
        .unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.write_source(), "work");
        // Empty [defaults] table likewise.
        let path2 = dir.path().join("config2.toml");
        std::fs::write(&path2, "[defaults]\n").unwrap();
        let cfg2 = Config::load_from(&path2).unwrap();
        assert_eq!(cfg2.write_source(), "work");
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
            r#"include = ["report.toml", "colorscheme.toml"]
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
[icons.todo]
pending = "○"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("colorscheme.toml"),
            r#"[colorscheme]
priority = ["completed", "overdue"]
[colorscheme.rules]
completed = {fg="gray10", bg="gray2"}
overdue = {inverse=true}
"#,
        )
        .unwrap();

        let cfg = Config::load_from(&main).unwrap();
        assert!(cfg.reports.contains_key("next"));
        assert_eq!(cfg.icons.todo.get("pending").map(|s| s.as_str()), Some("○"));
        let cs = cfg.colorscheme.unwrap();
        assert_eq!(cs.priority, vec!["completed", "overdue"]);
        assert_eq!(
            cs.rules.get("completed").and_then(|r| r.fg.as_deref()),
            Some("gray10")
        );
        assert_eq!(cs.rules.get("overdue").map(|r| r.inverse), Some(true));
    }
}
