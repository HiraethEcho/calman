//! Core data model: `Task` struct and `TaskStatus` enum.
//!
//! Layout follows DESIGN.md §2.1. A task carrying a `dtstart` is treated as an
//! *event*; otherwise it is a *todo*.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Lifecycle state of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Recurring,
    Completed,
    Cancelled,
}

impl TaskStatus {
    pub fn is_done(&self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }

    /// `Pending` and `InProgress` are active (recurring is its own state).
    pub fn is_active(&self) -> bool {
        !self.is_done() && !self.is_recurring()
    }

    /// A recurring template is its own state.
    pub fn is_recurring(&self) -> bool {
        matches!(self, Self::Recurring)
    }
}

/// Map a priority token (`high`/`medium`/`low` or 0-9) to a level.
pub fn priority_from_str(s: &str) -> Option<u8> {
    match s.to_ascii_lowercase().as_str() {
        "high" | "h" => Some(9),
        "medium" | "m" => Some(5),
        "low" | "l" => Some(1),
        _ => s.parse().ok(),
    }
}

/// A unified task/event record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    // Identity and ownership.
    pub uid: String,
    pub source: String,

    // Core fields.
    pub summary: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub priority: Option<u8>,
    pub tags: Vec<String>,

    // VTODO (tasks).
    pub due: Option<DateTime<Utc>>,
    pub percent_complete: Option<u8>,
    pub completed_at: Option<DateTime<Utc>>,

    // VEVENT (events).
    pub dtstart: Option<DateTime<Utc>>,
    pub dtend: Option<DateTime<Utc>>,
    pub rrule: Option<String>,
    pub location: Option<String>,
    /// All-day event: `.ics` renders `VALUE=DATE` (date-only, no timezone).
    #[serde(default)]
    pub allday: bool,
    /// Seconds before start for a VALARM reminder (`alert:15min` → 900).
    #[serde(default)]
    pub alarm_before: Option<i64>,

    // Relations.
    pub related_to: Option<String>,
    /// Recurrence exceptions (RFC 5545).
    /// `exdates`: occurrence original `DTSTART`s excluded from the series (delete-one).
    #[serde(default)]
    pub exdates: Vec<DateTime<Utc>>,
    /// For an override instance: the original occurrence `DTSTART` it replaces.
    #[serde(default)]
    pub recurrence_id: Option<DateTime<Utc>>,
    /// For an override instance: UID of the master series it belongs to.
    #[serde(default)]
    pub parent_uid: Option<String>,

    // Timestamps.
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Task {
    /// A task with a `dtstart` is an event; otherwise a todo.
    pub fn is_event(&self) -> bool {
        self.dtstart.is_some()
    }

    /// The task is a recurring series master (has `rrule`, is not an override).
    pub fn is_parent(&self) -> bool {
        self.rrule.is_some() && self.recurrence_id.is_none()
    }

    /// Create a brand-new task with generated UID and timestamps.
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
            allday: false,
            alarm_before: None,
            related_to: None,
            exdates: Vec::new(),
            recurrence_id: None,
            parent_uid: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
