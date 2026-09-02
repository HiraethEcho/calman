//! Core data model: `Task` struct and `TaskStatus` enum.
//!
//! Layout: a task carrying `dtstart` is treated as an *event*; otherwise it is
//! a *todo*.
//! 中文: 本模块是核心数据模型，定义任务的数据结构和生命周期状态。
//! 布局规则：带 `dtstart` 的任务是“事件”(event)，否则是“待办”(todo)。
//! Rust 概念：`struct` 是数据容器，`enum` 是“多选一”的类型。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Lifecycle state of a task.
/// 中文: 任务的生命周期状态。`#[derive(...)]` 让编译器自动生成调试输出、
/// 克隆、比较、序列化等代码；`#[serde(rename_all = "kebab-case")]` 表示
/// 枚举值序列化为 `kebab-case` 字符串（如 `in-progress`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    /// 待办：还没开始。
    Pending,
    /// 进行中。
    InProgress,
    /// 重复模板：自身不直接完成，而是按规则展开成多个实例。
    Recurring,
    /// 已完成。
    Completed,
    /// 已取消。
    Cancelled,
}

impl TaskStatus {
    /// `Completed` and `Cancelled` count as done.
    /// 中文: 是否已结束。`matches!` 是模式匹配宏：检查枚举是否属于列出的变体。
    /// Rust 概念：`&self` 表示只读借用方法，不拥有数据。
    pub fn is_done(&self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }

    /// `Pending` and `InProgress` are active (recurring is its own state).
    /// 中文: 是否活跃：未结束且不是重复模板。
    pub fn is_active(&self) -> bool {
        !self.is_done() && !self.is_recurring()
    }

    /// A recurring template is its own state.
    /// 中文: 是否是重复模板（模板本身不直接完成，而是按规则展开）。
    pub fn is_recurring(&self) -> bool {
        matches!(self, Self::Recurring)
    }
}

/// Map a priority token (`high`/`medium`/`low` or 0-9) to a level.
/// 中文: 把优先级文字（`high`/`medium`/`low` 或 0-9）映射成数字等级。
/// 等级越小优先级越高：`high`→1、`medium`→5、`low`→9（排序时 H 在最前）。
/// 返回 `Option<u8>`：无法识别时返回 `None`。
pub fn priority_from_str(s: &str) -> Option<u8> {
    // 先转成小写再匹配，让 `HIGH` 和 `high` 等价；`_` 分支尝试把数字字符串解析成 u8。
    match s.to_ascii_lowercase().as_str() {
        "high" | "h" => Some(1),
        "medium" | "m" => Some(5),
        "low" | "l" => Some(9),
        _ => s.parse().ok(),
    }
}

/// A unified task/event record.
/// 中文: 统一的任务/事件记录。同一个结构同时表示待办(VTODO)和事件(VEVENT)，
/// 用 `Option<T>` 表示“可能有值也可能没有”的字段。
/// Rust 概念：`String` 是拥有所有权的文本；`Vec<String>` 是字符串列表；
/// `Option<DateTime<Utc>>` 表示可选的时间点。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    // Identity and ownership. 中文: 身份与归属 — uid 是全局唯一 ID；source 是数据来源名。
    pub uid: String,
    pub source: String,

    // Core fields. 中文: 核心字段 — summary 摘要；description 详细描述；
    // status 状态；priority 优先级；tags 标签。
    pub summary: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub priority: Option<u8>,
    pub tags: Vec<String>,

    // VTODO (tasks). 中文: 待办(task)专用字段。
    /// 截止时间（UTC）。
    pub due: Option<DateTime<Utc>>,
    /// 完成百分比，0-100。
    pub percent_complete: Option<u8>,
    /// 完成时刻（UTC）。
    pub completed_at: Option<DateTime<Utc>>,

    // VEVENT (events). 中文: 事件(event)专用字段。
    /// 开始时间（UTC）。待办通常没有此字段。
    pub dtstart: Option<DateTime<Utc>>,
    /// 结束时间（UTC）。
    pub dtend: Option<DateTime<Utc>>,
    /// 重复规则（RFC 5545 RRULE 字符串）。
    pub rrule: Option<String>,
    /// 地点。
    pub location: Option<String>,
    /// VEVENT/VTODO source component. `true` = VEVENT (dtstart required),
    /// `false` = VTODO. Stored explicitly so a VTODO that carries a non-standard
    /// DTSTART is still a todo; missing in legacy JSONL → inferred from dtstart.
    /// 中文: 来源组件类型。显式记录是事件还是待办，避免仅靠 `dtstart` 误判；
    /// 旧数据缺少该字段时，根据 `dtstart` 推断。`#[serde(default)]` 让旧数据
    /// 缺少字段时使用默认值，保证向后兼容。
    #[serde(default)]
    pub event: bool,
    /// All-day event: `.ics` renders `VALUE=DATE` (date-only, no timezone).
    /// 中文: 是否全天事件。全天事件在 ICS 里只写日期，不带时区。
    #[serde(default)]
    pub allday: bool,
    /// Seconds before start for a VALARM reminder (`alert:15min` → 900).
    /// 中文: 提醒：在开始前多少秒触发闹钟（例如 `alert:15min` 存为 900）。
    #[serde(default)]
    pub alarm_before: Option<i64>,

    // Relations. 中文: 关系字段 — 用于任务/事件之间的关联。
    /// 父任务/事件的 ID（用于子任务或覆盖实例）。
    pub related_to: Option<String>,
    /// Taskwarrior-style wait: offset seconds relative to the item's date
    /// (due for todo, dtstart for event). Positive = after, negative = before.
    /// Hidden from reports while `date + wait > now`; per-occurrence for
    /// recurring series. Stored in ICS as `X-CALMAN-WAIT-OFFSET`.
    /// 中文: 等待时间：相对截止/开始时间的偏移秒数。正数=之后可见，负数=之前；
    /// 在“日期 + wait”还没到当前时间之前，报告里会隐藏该任务。
    #[serde(default)]
    pub wait: Option<i64>,
    /// Recurrence exceptions (RFC 5545).
    /// `exdates`: occurrence original `DTSTART`s excluded from the series (delete-one).
    /// 中文: 重复例外：这些原始开始时间对应的实例被从系列中排除（删除单个实例）。
    #[serde(default)]
    pub exdates: Vec<DateTime<Utc>>,
    /// For an override instance: the original occurrence `DTSTART` it replaces.
    /// 中文: 覆盖实例：它替换的原始发生时刻。
    #[serde(default)]
    pub recurrence_id: Option<DateTime<Utc>>,
    /// For an override instance: UID of the master series it belongs to.
    /// 中文: 覆盖实例所属主系列(master series)的 UID。
    #[serde(default)]
    pub parent_uid: Option<String>,

    /// When the todo was started with `calman start <id>`; `stop` turns it
    /// into an event spanning [started_at, now]. Persisted in ICS as the
    /// private `X-CALMAN-STARTED` property (no VTODO standard exists).
    /// 中文: 用 `calman start <id>` 开始计时的时间；`stop` 会把它变成一个
    /// 从 `started_at` 到现在的“事件”。ICS 里存为私有属性 `X-CALMAN-STARTED`。
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,

    // Timestamps. 中文: 时间戳 — created_at 创建时刻；updated_at 最后修改时刻。
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Task {
    /// Source component kind: explicit VEVENT flag (todo defaults false).
    /// Legacy records without the field fall back to `dtstart`-inference.
    /// 中文: 判断是否为事件：显式 `event` 标志优先；旧数据没有该标志时，
    /// 用“有 `dtstart` 且没有 `due`”来推断。
    pub fn is_event(&self) -> bool {
        self.event || (self.dtstart.is_some() && self.due.is_none())
    }

    /// The task is a recurring series master (has `rrule`, is not an override
    /// or a virtual occurrence).
    /// 中文: 是否为重复系列的主任务：有 `rrule`，且不是覆盖实例或虚拟发生实例。
    pub fn is_parent(&self) -> bool {
        self.rrule.is_some()
            && self.recurrence_id.is_none()
            && self.parent_uid.is_none()
    }

    /// Create a brand-new task with generated UID and timestamps.
    /// 中文: 创建全新任务：自动生成 UUID 作为 uid，并把创建/更新时间设为当前 UTC 时间。
    /// Rust 概念：`impl Into<String>` 表示任何能转换成 `String` 的类型都能当 summary 传入。
    pub fn new(source: &str, summary: impl Into<String>) -> Self {
        let now = Utc::now();
        Task {
            uid: uuid::Uuid::new_v4().to_string(),
            source: source.to_string(),
            summary: summary.into(),
            description: None,
            status: TaskStatus::Pending,
            priority: None,
            tags: Vec::new(),
            due: None,
            percent_complete: None,
            completed_at: None,
            dtstart: None,
            dtend: None,
            rrule: None,
            location: None,
            event: false,
            allday: false,
            alarm_before: None,
            related_to: None,
            wait: None,
            exdates: Vec::new(),
            recurrence_id: None,
            parent_uid: None,
            started_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    //! 单元测试：验证状态判断、事件识别、重复模板识别和默认值。
    use super::*;

    #[test]
    fn parent_detection_excludes_virtual_occurrences() {
        let mut master = Task::new("work", "weekly");
        master.rrule = Some("FREQ=WEEKLY".into());
        assert!(master.is_parent());
        // A stored override (RECURRENCE-ID) is not a parent...
        let mut ov = master.clone();
        ov.recurrence_id = Some(chrono::Utc::now());
        assert!(!ov.is_parent());
        // ...and neither is a virtual occurrence row (parent_uid marks it).
        let mut occ = master.clone();
        occ.parent_uid = Some(master.uid.clone());
        assert!(!occ.is_parent());
    }

    #[test]
    fn new_task_defaults() {
        let t = Task::new("work", "buy milk");
        assert_eq!(t.source, "work");
        assert_eq!(t.summary, "buy milk");
        assert_eq!(t.status, TaskStatus::Pending);
        assert!(!t.is_event());
        assert_eq!(t.uid.len(), 36); // uuid v4
    }

    #[test]
    fn event_detection() {
        let mut t = Task::new("work", "standup");
        assert!(!t.is_event());
        t.dtstart = Some(Utc::now());
        assert!(t.is_event());
    }

    #[test]
    fn status_helpers() {
        assert!(TaskStatus::Pending.is_active());
        assert!(!TaskStatus::Pending.is_done());
        assert!(TaskStatus::Completed.is_done());
        assert!(TaskStatus::Cancelled.is_done());
    }
}
