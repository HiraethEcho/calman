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
