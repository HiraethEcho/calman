//! Optional recurrence expansion via the `rrule` crate.
//!
//! When the `recur-expand` feature is enabled, this module provides
//! in-memory occurrence generation for recurring master tasks, so
//! `list`/`next` can emit virtual occurrence rows.
//!
//! **Important**: expansion is read-only — it never writes back to storage.
//! Per-occurrence mutations (`done <id> on:<date>`, `modify <id>.n`) happen via
//! `Task.exdates` / `Task.recurrence_id` fields; those are serialised normally
//! by the existing JSONL/ICS back-ends.

use chrono::{DateTime, TimeZone, Utc};
use rrule::{RRuleSet, Tz};

use crate::model::Task;

/// An expanded occurrence derived from a master recurring task.
#[derive(Debug, Clone)]
pub struct Occurrence {
    /// The parent/master task this occurrence belongs to.
    pub master: Task,
    /// The original DTSTART of this specific occurrence.
    pub occurrence_start: DateTime<Utc>,
    /// 1-based index of this occurrence among all generated occurrences.
    pub index: usize,
}

/// Expand a master recurring task into ordered occurrences within the given
/// `[after, before)` window, excluding dates in `task.exdates`.
///
/// Returns occurrences sorted by `occurrence_start`.
pub fn expand_task(task: &Task, after: DateTime<Utc>, before: DateTime<Utc>) -> Vec<Occurrence> {
    let Some(rrule_str) = &task.rrule else {
        return Vec::new();
    };
    let Some(dtstart) = task.dtstart.or(task.due) else {
        return Vec::new();
    };

    // Build a full RRuleSet string: DTSTART + RRULE + EXDATEs.
    let mut s = format!(
        "DTSTART:{}\nRRULE:{}",
        dtstart.format("%Y%m%dT%H%M%SZ"),
        rrule_str
    );
    if !task.exdates.is_empty() {
        let ex: Vec<String> = task
            .exdates
            .iter()
            .map(|d| d.format("%Y%m%dT%H%M%SZ").to_string())
            .collect();
        s.push_str("\nEXDATE:");
        s.push_str(&ex.join(","));
    }

    let rset: RRuleSet = match s.parse() {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let after_tz: DateTime<Tz> = Tz::UTC.from_utc_datetime(&after.naive_utc());
    let before_tz: DateTime<Tz> = Tz::UTC.from_utc_datetime(&before.naive_utc());
    let result = rset.after(after_tz).before(before_tz).all(10_000);

    result
        .dates
        .into_iter()
        .map(|dt| DateTime::from_naive_utc_and_offset(dt.naive_utc(), Utc))
        .enumerate()
        .map(|(i, occurrence_start)| Occurrence {
            master: task.clone(),
            occurrence_start,
            index: i + 1,
        })
        .collect()
}
