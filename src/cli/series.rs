//! Series-splitting helpers for per-occurrence `modify`/`delete`.
//!
//! Occurrence addressing (`id.n`) numbers instances relative to *now* (`1` =
//! next upcoming), so truncating a series needs the absolute 1-based index of
//! the occurrence within its own recurrence. That index is computed from the
//! expanded survivors before the occurrence + the EXDATE'd slots before it
//! (EXDATEs shift the COUNT sequence, so they must be added back).

use crate::model::Task;
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};

/// 1-based absolute index of `occ` in `master`'s series (counting excluded
/// slots), or `None` when the task is not recurring / `occ` predates DTSTART.
pub fn absolute_index(master: &Task, occ: DateTime<Utc>) -> Option<usize> {
    #[cfg(feature = "recur-expand")]
    {
        let dtstart = master.dtstart.or(master.due)?;
        if occ < dtstart || master.rrule.is_none() {
            return None;
        }
        let survived_before = crate::recur_expand::expand_task(
            master,
            dtstart - chrono::Duration::seconds(1),
            occ - chrono::Duration::seconds(1),
        )
        .len();
        let ex_before = master
            .exdates
            .iter()
            .filter(|e| **e < occ)
            .count();
        Some(survived_before + ex_before + 1)
    }
    #[cfg(not(feature = "recur-expand"))]
    {
        let _ = (master, occ);
        None
    }
}

/// Replace an RRULE's `COUNT` (dropping a conflicting `UNTIL`), so the series
/// keeps exactly `count` occurrences from its DTSTART.
pub fn set_count(rrule: &str, count: usize) -> String {
    let mut parts: Vec<String> = rrule
        .split(';')
        .filter(|p| {
            !p.is_empty() && !p.starts_with("COUNT=") && !p.starts_with("UNTIL=")
        })
        .map(str::to_string)
        .collect();
    parts.push(format!("COUNT={count}"));
    parts.join(";")
}

/// Remaining number of occurrences `count` in an RRULE (original `COUNT=…`),
/// accounting for the `abs_idx`-th occurrence becoming the new series start.
pub fn remaining_count(rrule: &str, abs_idx: usize) -> Option<usize> {
    let total = rrule
        .split(';')
        .find_map(|p| p.strip_prefix("COUNT="))
        .and_then(|v| v.parse::<usize>().ok())?;
    Some(total.saturating_sub(abs_idx - 1))
}

/// Truncate `master` so its series ends *before* `occ` (the occurrence and
/// everything after it is removed). Returns `true` when the series has no
/// remaining occurrences and the master should be deleted outright.
pub fn truncate_before(master: &mut Task, occ: DateTime<Utc>) -> Result<bool> {
    let Some(idx) = absolute_index(master, occ) else {
        bail!("`{}` is not a recurring occurrence of this task", occ);
    };
    if idx == 1 {
        return Ok(true);
    }
    let rrule = master.rrule.as_deref().ok_or_else(|| {
        anyhow::anyhow!("task `{}` has no recurrence rule", master.uid)
    })?;
    master.rrule = Some(set_count(rrule, idx - 1));
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Task;
    use chrono::TimeZone;

    fn dt(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    #[test]
    fn set_count_replaces_and_drops_until() {
        assert_eq!(
            set_count("FREQ=WEEKLY;BYDAY=MO;COUNT=5", 3),
            "FREQ=WEEKLY;BYDAY=MO;COUNT=3"
        );
        assert_eq!(
            set_count("FREQ=WEEKLY;UNTIL=20261231T000000Z", 4),
            "FREQ=WEEKLY;COUNT=4"
        );
    }

    #[test]
    fn remaining_count_subtracts_prefix() {
        assert_eq!(remaining_count("FREQ=WEEKLY;COUNT=10", 4), Some(7));
        assert_eq!(remaining_count("FREQ=WEEKLY", 4), None);
    }

    #[cfg(feature = "recur-expand")]
    #[test]
    fn absolute_index_counts_exdate_slots() {
        let mut t = Task::new("work", "weekly");
        t.dtstart = Some(dt(2026, 9, 1, 9)); // Tuesday
        t.rrule = Some("FREQ=WEEKLY".into());
        t.exdates = vec![dt(2026, 9, 1, 9)];
        // 09-08 is the 2nd algorithm occurrence (09-01 excluded by EXDATE).
        assert_eq!(absolute_index(&t, dt(2026, 9, 8, 9)), Some(2));
        assert_eq!(absolute_index(&t, dt(2026, 9, 22, 9)), Some(4));
    }

    #[cfg(feature = "recur-expand")]
    #[test]
    fn truncate_before_keeps_prefix_and_detects_first() {
        let mut t = Task::new("work", "series");
        t.dtstart = Some(dt(2026, 9, 1, 9));
        t.rrule = Some("FREQ=WEEKLY;COUNT=5".into());
        let mut t2 = t.clone();
        assert!(!truncate_before(&mut t2, dt(2026, 9, 15, 9)).unwrap()); // 3rd occ
        assert_eq!(t2.rrule.as_deref(), Some("FREQ=WEEKLY;COUNT=2"));
        assert!(truncate_before(&mut t, dt(2026, 9, 1, 9)).unwrap()); // 1st occ
    }
}
