//! Filter logic for `list` and `count`.
//!
//! Filters per DESIGN.md §4.2.B: `status:`, `priority:`, `tags:`, `due:`,
//! predefined `+OVERDUE`/`+PENDING`/`+COMPLETED`, plus Taskwarrior-style
//! `-tag` exclusion and `due.before:`/`due.after:`.

use crate::args::{DueMod, ParsedArgs};
use crate::model::{Task, TaskStatus};
use chrono::{DateTime, Local, NaiveDate, Utc};

/// A parsed, composable task predicate.
#[derive(Default)]
pub struct Filter {
    status: Option<TaskStatus>,
    pending: bool,
    completed: bool,
    overdue: bool,
    active: bool,
    done: bool,
    cancelled: bool,
    in_progress: bool,
    tagged: bool,
    untagged: bool,
    scheduled: bool,
    r#type: Option<String>,
    anti_pending: bool,
    anti_active: bool,
    anti_completed: bool,
    anti_cancelled: bool,
    anti_in_progress: bool,
    anti_overdue: bool,
    anti_tagged: bool,
    anti_untagged: bool,
    anti_scheduled: bool,
    anti_type: Option<String>,
    priority: Option<u8>,
    tags: Vec<String>,
    anti_tags: Vec<String>,
    due_on: Option<NaiveDate>,
    due_before: Option<DateTime<Utc>>,
    due_after: Option<DateTime<Utc>>,
}

impl Filter {
    /// Build from parsed command-line arguments.
    pub fn from_parsed(q: &ParsedArgs) -> Filter {
        let (due_on, due_before, due_after) = match (q.due, q.due_mod) {
            (Some(d), DueMod::On) => (Some(d.with_timezone(&Local).date_naive()), None, None),
            (Some(d), DueMod::Before) => (None, Some(d), None),
            (Some(d), DueMod::After) => (None, None, Some(d)),
            (None, _) => (None, None, None),
        };
        Filter {
            status: q.status,
            pending: q.pending,
            completed: q.completed,
            overdue: q.overdue,
            active: q.active,
            done: q.done,
            cancelled: q.cancelled,
            in_progress: q.in_progress,
            tagged: q.tagged,
            untagged: q.untagged,
            scheduled: q.scheduled,
            r#type: q.r#type.clone(),
            anti_pending: q.anti_pending,
            anti_active: q.anti_active,
            anti_completed: q.anti_completed,
            anti_cancelled: q.anti_cancelled,
            anti_in_progress: q.anti_in_progress,
            anti_overdue: q.anti_overdue,
            anti_tagged: q.anti_tagged,
            anti_untagged: q.anti_untagged,
            anti_scheduled: q.anti_scheduled,
            anti_type: q.anti_type.clone(),
            priority: q.priority,
            tags: q.tags.clone(),
            anti_tags: q.anti_tags.clone(),
            due_on,
            due_before,
            due_after,
        }
    }

    /// Does this task satisfy every clause?
    pub fn matches(&self, t: &Task) -> bool {
        if let Some(s) = self.status
            && t.status != s
        {
            return false;
        }
        if self.pending && !t.status.is_active() {
            return false;
        }
        if self.completed && !t.status.is_done() {
            return false;
        }
        if self.active && t.status.is_done() {
            return false;
        }
        if self.done && t.status != TaskStatus::Completed {
            return false;
        }
        if self.cancelled && t.status != TaskStatus::Cancelled {
            return false;
        }
        if self.in_progress && t.status != TaskStatus::InProgress {
            return false;
        }
        if self.tagged && t.tags.is_empty() {
            return false;
        }
        if self.untagged && !t.tags.is_empty() {
            return false;
        }
        if self.scheduled && !t.is_event() {
            return false;
        }
        if self.anti_pending && t.status.is_active() {
            return false;
        }
        if self.anti_active && !t.status.is_done() {
            return false;
        }
        if self.anti_completed && t.status == TaskStatus::Completed {
            return false;
        }
        if self.anti_cancelled && t.status == TaskStatus::Cancelled {
            return false;
        }
        if self.anti_in_progress && t.status == TaskStatus::InProgress {
            return false;
        }
        if self.anti_overdue {
            let past_due = t.due.is_some_and(|d| d < Utc::now());
            if past_due && !t.status.is_done() {
                return false;
            }
        }
        if self.anti_tagged && !t.tags.is_empty() {
            return false;
        }
        if self.anti_untagged && t.tags.is_empty() {
            return false;
        }
        if self.anti_scheduled && t.is_event() {
            return false;
        }
        if let Some(ty) = &self.anti_type {
            match ty.as_str() {
                "todo" => {
                    if !t.is_event() {
                        return false;
                    }
                }
                "event" => {
                    if t.is_event() {
                        return false;
                    }
                }
                _ => {}
            }
        }
        if let Some(ty) = &self.r#type {
            match ty.as_str() {
                "todo" => {
                    if t.is_event() {
                        return false;
                    }
                }
                "event" => {
                    if !t.is_event() {
                        return false;
                    }
                }
                _ => {}
            }
        }
        if self.overdue {
            let past_due = t.due.is_some_and(|d| d < Utc::now());
            if !past_due || t.status.is_done() {
                return false;
            }
        }
        if let Some(p) = self.priority
            && t.priority != Some(p)
        {
            return false;
        }
        for tag in &self.tags {
            if !t.tags.iter().any(|x| x.eq_ignore_ascii_case(tag)) {
                return false;
            }
        }
        for anti in &self.anti_tags {
            if t.tags.iter().any(|x| x.eq_ignore_ascii_case(anti)) {
                return false;
            }
        }
        if let Some(d) = self.due_on {
            match t.due {
                Some(due) if due.with_timezone(&Local).date_naive() == d => {}
                _ => return false,
            }
        }
        if let Some(d) = self.due_before
            && t.due.as_ref().is_some_and(|due| due >= &d)
        {
            return false;
        }
        if let Some(d) = self.due_after
            && t.due.as_ref().is_some_and(|due| due < &d)
        {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::parse;
    use crate::model::Task;
    use chrono::Duration;

    fn in_progress() -> Task {
        let mut t = Task::new("work", "x");
        t.status = TaskStatus::InProgress;
        t.priority = Some(9);
        t.tags = vec!["urgent".into()];
        t
    }

    fn q(args: &[&str]) -> ParsedArgs {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn empty_matches_all() {
        let f = Filter::from_parsed(&ParsedArgs::default());
        assert!(f.matches(&Task::new("work", "any")));
    }

    #[test]
    fn status_and_tags() {
        let f = Filter::from_parsed(&q(&["status:in-progress", "+urgent"]));
        assert!(f.matches(&in_progress()));
        let done = Filter::from_parsed(&q(&["status:completed"]));
        assert!(!done.matches(&in_progress()));
    }

    #[test]
    fn priority_filter() {
        let f = Filter::from_parsed(&q(&["priority:H"]));
        assert!(f.matches(&in_progress()));
        let lo = Filter::from_parsed(&q(&["priority:low"]));
        assert!(!lo.matches(&in_progress()));
    }

    #[test]
    fn predefined_flags() {
        let f = Filter::from_parsed(&q(&["+PENDING"]));
        assert!(f.matches(&in_progress()));
        let f2 = Filter::from_parsed(&q(&["+OVERDUE"]));
        assert!(!f2.matches(&in_progress())); // no due date
    }

    #[test]
    fn anti_tag_excludes() {
        let f = Filter::from_parsed(&q(&["-urgent"]));
        assert!(!f.matches(&in_progress()));
        let g = Filter::from_parsed(&q(&["-home"]));
        assert!(g.matches(&in_progress()));
    }

    #[test]
    fn virtual_tags() {
        let mut t = Task::new("work", "x");
        t.status = TaskStatus::InProgress;
        assert!(Filter::from_parsed(&q(&["+IN-PROCESS"])).matches(&t));
        assert!(Filter::from_parsed(&q(&["+ACTIVE"])).matches(&t));
        assert!(!Filter::from_parsed(&q(&["+DONE"])).matches(&t));
        t.status = TaskStatus::Completed;
        assert!(Filter::from_parsed(&q(&["+DONE"])).matches(&t));
        assert!(Filter::from_parsed(&q(&["status:active"])).matches(&Task::new("work", "y")));
    }

    #[test]
    fn type_filter() {
        let t = Task::new("work", "x");
        assert!(Filter::from_parsed(&q(&["type:todo"])).matches(&t));
        let mut e = Task::new("work", "y");
        e.dtstart = Some(Utc::now());
        assert!(Filter::from_parsed(&q(&["type:event"])).matches(&e));
        assert!(!Filter::from_parsed(&q(&["type:event"])).matches(&t));
    }

    #[test]
    fn anti_virtual_tag() {
        let mut t = Task::new("work", "x");
        t.due = Some(Utc::now() - Duration::hours(1)); // overdue
        assert!(!Filter::from_parsed(&q(&["-OVERDUE"])).matches(&t));
        assert!(Filter::from_parsed(&q(&["-DONE"])).matches(&t));
        t.status = TaskStatus::Completed;
        assert!(!Filter::from_parsed(&q(&["-DONE"])).matches(&t));
        let mut e = Task::new("work", "y");
        e.dtstart = Some(Utc::now());
        assert!(!Filter::from_parsed(&q(&["-EVENT"])).matches(&e));
        assert!(Filter::from_parsed(&q(&["-EVENT"])).matches(&t));
    }

    #[test]
    fn due_before_after() {
        let mut t = in_progress();
        let now = Utc::now();
        t.due = Some(now + Duration::days(3));

        let soon = Filter::from_parsed(&q(&[&format!(
            "due.before:{}",
            (now + Duration::days(1)).format("%Y-%m-%d")
        )]));
        assert!(!soon.matches(&t));

        let later = Filter::from_parsed(&q(&[&format!(
            "due.after:{}",
            (now + Duration::days(1)).format("%Y-%m-%d")
        )]));
        assert!(later.matches(&t));
    }
}
