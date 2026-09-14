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
use std::cmp::Ordering;

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
    // The rrule crate needs a DATETIME `UNTIL` to match a DATETIME `DTSTART`;
    // calman's own `until:`/iOS write a DATE-only `UNTIL` (e.g. 20260930),
    // which would fail to parse — normalise it to the UTC instant of that
    // local day's end (inclusive semantics) first.
    let rrule_str = crate::recurrence::rrule_with_until_datetime(rrule_str);
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

    let mut occs: Vec<_> = result
        .dates
        .into_iter()
        .map(|dt| DateTime::from_naive_utc_and_offset(dt.naive_utc(), Utc))
        .collect();

    // Order occurrences so the NEXT upcoming one is index 1, not the first in
    // the expansion window (which starts a day in the past). Future instances
    // ascend first, then past instances descend — so `1` is the next
    // occurrence and `take(N)` in `list` shows upcoming items, while past
    // ones (yesterday and older) stay addressable with larger numbers.
    let now = Utc::now();
    occs.sort_by(|a, b| {
        let a_future = *a >= now;
        let b_future = *b >= now;
        match (a_future, b_future) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (true, true) => a.cmp(b),
            (false, false) => b.cmp(a),
        }
    });

    occs
        .into_iter()
        .enumerate()
        .map(|(i, occurrence_start)| Occurrence {
            master: task.clone(),
            occurrence_start,
            index: i + 1,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Task;
    use chrono::{TimeZone, Utc};

    /// 纯日期 UNTIL（calman `until:`/iOS 写法）必须能正常展开：
    /// 每个 occurrence 的本地日都落在 [起始日, UNTIL 日] 之内，且相邻相隔一天。
    #[test]
    fn date_only_until_expands_inclusive() {
        let mut m = Task::new("work", "u");
        m.due = Some(Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0).unwrap());
        m.rrule = Some("FREQ=DAILY;UNTIL=20260930".into());
        m.status = crate::model::TaskStatus::Recurring;

        let occs = expand_task(
            &m,
            Utc.with_ymd_and_hms(2026, 9, 10, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 10, 2, 0, 0, 0).unwrap(),
        );
        assert!(!occs.is_empty(), "date-only UNTIL must expand");
        let start_local = m.due.unwrap().with_timezone(&chrono::Local).date_naive();
        for w in occs.windows(2) {
            let a = w[0].occurrence_start.with_timezone(&chrono::Local).date_naive();
            let b = w[1].occurrence_start.with_timezone(&chrono::Local).date_naive();
            assert!(a >= start_local, "before start: {a}");
            assert!(b <= chrono::NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(), "after until: {b}");
            assert_eq!(b - a, chrono::Duration::days(1));
        }
    }

    /// COUNT 序列不受影响。
    #[test]
    fn count_series_still_expands() {
        let mut m = Task::new("work", "c");
        m.due = Some(Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0).unwrap());
        m.rrule = Some("FREQ=DAILY;COUNT=3".into());
        m.status = crate::model::TaskStatus::Recurring;
        let occs = expand_task(
            &m,
            Utc.with_ymd_and_hms(2026, 9, 10, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 10, 2, 0, 0, 0).unwrap(),
        );
        assert_eq!(occs.len(), 3);
    }
}
