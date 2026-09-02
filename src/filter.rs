//! Filter expression engine for `list`, `count` and report `filter` strings.
//!
//! Grammar (shared by CLI filter tokens and `[report.<name>].filter`):
//!   expr   := or
//!   or     := and ('or' and)*
//!   and    := unary (('and')? unary)*
//!   unary  := '(' or ')' | atom
//!   atom   := 'status:' | 'type:' | 'source:' | 'priority:' | 'due:'
//!           | 'due.before:' | 'due.by:' | 'due.after:' | '+tag' | '+VIRTUAL'
//!           | '-tag' | '-VIRTUAL' | '-status:' | '-source:' | ...
//!
//! `-<atom>` wraps the atom in `Not`. `+PENDING` matches any active task
//! (events count as pending). The `due*` key is unified: todos use `due`,
//! events use `dtstart`.

use crate::date::parse_datetime;
use crate::model::{Task, TaskStatus, priority_from_str};
use anyhow::{Result, bail};
use chrono::{DateTime, Local, Utc};

/// How a `due*` clause compares against the task date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DueOp {
    On,
    Before,
    By,
    After,
}

/// Task type selector for `type:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeSel {
    Todo,
    Event,
    All,
}

/// Builtin virtual tags that are usable as `+NAME` / `-NAME`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flag {
    Overdue,
    Pending,
    Completed,
    Cancelled,
    InProgress,
    Started,
    Tagged,
    Untagged,
    Scheduled,
    Parent,
    Waiting,
}

/// A single positive atom. Negations live in `Expr::Not`.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub status: Option<TaskStatus>,
    pub active: bool,
    pub type_sel: Option<TypeSel>,
    pub source: Option<String>,
    pub priority: Option<u8>,
    pub tags: Vec<String>,
    pub due: Option<(DueOp, DateTime<Utc>)>,
    /// `date:` filter — unified date: todo→due, event→dtstart.
    pub date: Option<(DueOp, DateTime<Utc>)>,
    /// `from:` filter — VEVENT `dtstart` only.
    pub from: Option<(DueOp, DateTime<Utc>)>,
    pub flags: Vec<Flag>,
}

/// A filter expression tree.
#[derive(Debug, Clone)]
pub enum Expr {
    Atom(Filter),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

impl Expr {
    /// Test convenience; production uses `matches_with`.
    #[cfg(test)]
    pub fn matches(&self, t: &Task) -> bool {
        self.matches_with(t)
    }

    pub fn matches_with(&self, t: &Task) -> bool {
        match self {
            Expr::Atom(f) => f.matches_with(t),
            Expr::Not(e) => !e.matches_with(t),
            Expr::And(a, b) => {
                a.matches_with(t) && b.matches_with(t)
            }
            Expr::Or(a, b) => {
                a.matches_with(t) || b.matches_with(t)
            }
        }
    }
}

/// The unified "date" of a task: todos → `due`, events → `dtstart`.
pub fn task_date(t: &Task) -> Option<DateTime<Utc>> {
    if t.is_event() { t.dtstart } else { t.due }
}

impl Filter {
    pub fn matches_with(&self, t: &Task) -> bool {
        if let Some(s) = self.status
            && t.status != s
        {
            return false;
        }
        if self.active && !t.status.is_active() {
            return false;
        }
        if let Some(ty) = self.type_sel {
            let is_event = t.is_event();
            let ok = match ty {
                TypeSel::Todo => !is_event,
                TypeSel::Event => is_event,
                TypeSel::All => true,
            };
            if !ok {
                return false;
            }
        }
        if let Some(src) = &self.source
            && !t.source.eq_ignore_ascii_case(src)
        {
            return false;
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
        for f in &self.flags {
            if !flag_matches(*f, t) {
                return false;
            }
        }
        if let Some((op, dt)) = self.due
            && !due_matches(op, dt, task_date(t))
        {
            return false;
        }
        // `date:` matches the unified date (due for todos, dtstart for events).
        if let Some((op, dt)) = self.date
            && !due_matches(op, dt, task_date(t))
        {
            return false;
        }
        // `from:` filters match VEVENT `dtstart` only; a todo (no dtstart)
        // never matches, so it must also not slip through the negation.
        if let Some((op, dt)) = self.from {
            let ok = match t.dtstart {
                Some(start) => due_matches(op, dt, Some(start)),
                None => false,
            };
            if !ok {
                return false;
            }
        }
        true
    }
}

/// True when the task's date has passed and it is not done (recurring
/// templates excluded — they are not real instances).
pub fn is_overdue(t: &Task) -> bool {
    let Some(d) = task_date(t) else {
        return false;
    };
    if !t.status.is_active() {
        return false;
    }
    if t.allday {
        // Date-only (all-day) task: compare local calendar days.
        // Overdue starts the day AFTER the due day.
        let due_day = d.with_timezone(&Local).date_naive();
        let today = Local::now().date_naive();
        today > due_day
    } else {
        d < Utc::now()
    }
}

fn flag_matches(f: Flag, t: &Task) -> bool {
    match f {
        Flag::Pending => t.status.is_active(),
        Flag::Completed => t.status == TaskStatus::Completed,
        Flag::Cancelled => t.status == TaskStatus::Cancelled,
        Flag::InProgress => t.status == TaskStatus::InProgress,
        Flag::Started => t.started_at.is_some(),
        Flag::Tagged => !t.tags.is_empty(),
        Flag::Untagged => t.tags.is_empty(),
        Flag::Scheduled => t.is_event(),
        Flag::Parent => t.is_parent(),
        Flag::Waiting => t
            .wait
            .zip(task_date(t))
            .is_some_and(|(w, d)| d + chrono::Duration::seconds(w) > Utc::now()),
        Flag::Overdue => is_overdue(t),
    }
}

fn due_matches(op: DueOp, want: DateTime<Utc>, have: Option<DateTime<Utc>>) -> bool {
    let Some(have) = have else {
        return false;
    };
    match op {
        // Exact = same local calendar day (date-only semantics), matching
        // `due:today`/`due:eow` expectations.
        DueOp::On => {
            let w = want.with_timezone(&Local).date_naive();
            have.with_timezone(&Local).date_naive() == w
        }
        DueOp::Before => have < want,
        DueOp::By => have <= want,
        DueOp::After => have >= want,
    }
}

/// Tokenize a filter string (splits on whitespace and parentheses).
pub fn tokenize(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else if ch == '(' || ch == ')' {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(ch.to_string());
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Parse a filter string (implicit `and` between adjacent atoms).
pub fn parse_expr_str(s: &str) -> Result<Expr> {
    let toks = tokenize(s);
    parse_expr(&toks)
}

/// Parse a token list (CLI argv tokens) into an expression.
///
/// An empty list matches everything.
pub fn parse_expr(args: &[String]) -> Result<Expr> {
    if args.is_empty() {
        return Ok(Expr::Atom(Filter::default()));
    }
    let mut toks: Vec<String> = Vec::new();
    for a in args {
        toks.extend(tokenize(a));
    }
    let mut pos = 0usize;
    let e = parse_or(&toks, &mut pos)?;
    if pos != toks.len() {
        bail!("unexpected filter token `{}`", toks[pos]);
    }
    Ok(e)
}

fn parse_or(toks: &[String], pos: &mut usize) -> Result<Expr> {
    let mut left = parse_and(toks, pos)?;
    while *pos < toks.len() && toks[*pos].eq_ignore_ascii_case("or") {
        *pos += 1;
        let right = parse_and(toks, pos)?;
        left = Expr::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_and(toks: &[String], pos: &mut usize) -> Result<Expr> {
    let mut left = parse_unary(toks, pos)?;
    loop {
        if *pos >= toks.len() {
            return Ok(left);
        }
        let t = toks[*pos].to_ascii_lowercase();
        if t == "or" || t == ")" {
            return Ok(left);
        }
        if t == "and" {
            *pos += 1;
            continue;
        }
        let right = parse_unary(toks, pos)?;
        left = Expr::And(Box::new(left), Box::new(right));
    }
}

fn parse_unary(toks: &[String], pos: &mut usize) -> Result<Expr> {
    if *pos >= toks.len() {
        bail!("unexpected end of filter");
    }
    let tok = toks[*pos].clone();
    if tok == "(" {
        *pos += 1;
        let e = parse_or(toks, pos)?;
        if *pos >= toks.len() || toks[*pos] != ")" {
            bail!("missing `)` in filter");
        }
        *pos += 1;
        return Ok(e);
    }
    if tok == ")" {
        bail!("unexpected `)` in filter");
    }
    *pos += 1;
    parse_atom(&tok)
}

/// Parse one atom token (which may be `-<atom>` / `+<atom>` for negation).
fn parse_atom(tok: &str) -> Result<Expr> {
    let lower = tok.to_ascii_lowercase();
    if let Some(inner) = lower.strip_prefix("-source:").or_else(|| lower.strip_prefix("-src:")) {
        let f = Filter {
            source: Some(inner.to_string()),
            ..Filter::default()
        };
        return Ok(Expr::Not(Box::new(Expr::Atom(f))));
    }
    if let Some(inner) = lower.strip_prefix("-status:") {
        return Ok(Expr::Not(Box::new(Expr::Atom(parse_status_atom(
            &format!("status:{inner}"),
        )?))));
    }
    if let Some(inner) = lower.strip_prefix("-type:") {
        let sel = match inner {
            "todo" => TypeSel::Todo,
            "event" => TypeSel::Event,
            "all" => TypeSel::All,
            _ => bail!("unknown type `{inner}` (use todo, event, all)"),
        };
        return Ok(Expr::Not(Box::new(Expr::Atom(Filter {
            type_sel: Some(sel),
            ..Filter::default()
        }))));
    }
    if let Some(rest) = lower.strip_prefix("-due.before:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Due, DueOp::Before, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-due.by:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Due, DueOp::By, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-due.after:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Due, DueOp::After, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-due:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Due, DueOp::On, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-date.before:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Date, DueOp::Before, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-date.by:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Date, DueOp::By, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-date.after:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Date, DueOp::After, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-date:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Date, DueOp::On, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-from.before:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::From, DueOp::Before, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-from.by:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::From, DueOp::By, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-from.after:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::From, DueOp::After, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-from:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::From, DueOp::On, rest)?)));
    }
    if let Some(rest) = lower
        .strip_prefix("-priority:")
        .or_else(|| lower.strip_prefix("-pri:"))
    {
        let p = priority_from_str(rest)
            .ok_or_else(|| anyhow::anyhow!("bad priority `{rest}` (use H, M, L or 0-9)"))?;
        return Ok(Expr::Not(Box::new(Expr::Atom(Filter {
            priority: Some(p),
            ..Filter::default()
        }))));
    }
    if let Some(rest) = lower.strip_prefix("source:").or_else(|| lower.strip_prefix("src:")) {
        return Ok(Expr::Atom(Filter {
            source: Some(rest.to_string()),
            ..Filter::default()
        }));
    }
    if lower.starts_with('-') && lower.len() > 1 {
        let inner = &tok[1..];
        let e = parse_positive(&format!("+{inner}"))?;
        return Ok(Expr::Not(Box::new(e)));
    }
    parse_positive(tok)
}

/// Parse a positive atom token (no leading `-`).
fn parse_positive(tok: &str) -> Result<Expr> {
    let lower = tok.to_ascii_lowercase();

    if let Some(rest) = lower
        .strip_prefix("priority:")
        .or_else(|| lower.strip_prefix("pri:"))
    {
        let p = priority_from_str(rest)
            .ok_or_else(|| anyhow::anyhow!("bad priority `{rest}` (use H, M, L or 0-9)"))?;
        return Ok(Expr::Atom(Filter {
            priority: Some(p),
            ..Filter::default()
        }));
    }

    if let Some(rest) = lower.strip_prefix("due.before:") {
        return field_atom(DateField::Due, DueOp::Before, rest);
    }
    if let Some(rest) = lower.strip_prefix("due.by:") {
        return field_atom(DateField::Due, DueOp::By, rest);
    }
    if let Some(rest) = lower.strip_prefix("due.after:") {
        return field_atom(DateField::Due, DueOp::After, rest);
    }
    if let Some(rest) = lower.strip_prefix("due:") {
        return field_atom(DateField::Due, DueOp::On, rest);
    }

    if let Some(rest) = lower.strip_prefix("date.before:") {
        return field_atom(DateField::Date, DueOp::Before, rest);
    }
    if let Some(rest) = lower.strip_prefix("date.by:") {
        return field_atom(DateField::Date, DueOp::By, rest);
    }
    if let Some(rest) = lower.strip_prefix("date.after:") {
        return field_atom(DateField::Date, DueOp::After, rest);
    }
    if let Some(rest) = lower.strip_prefix("date:") {
        return field_atom(DateField::Date, DueOp::On, rest);
    }

    if let Some(rest) = lower.strip_prefix("from.before:") {
        return field_atom(DateField::From, DueOp::Before, rest);
    }
    if let Some(rest) = lower.strip_prefix("from.by:") {
        return field_atom(DateField::From, DueOp::By, rest);
    }
    if let Some(rest) = lower.strip_prefix("from.after:") {
        return field_atom(DateField::From, DueOp::After, rest);
    }
    if let Some(rest) = lower.strip_prefix("from:") {
        return field_atom(DateField::From, DueOp::On, rest);
    }

    if let Some(_rest) = lower.strip_prefix("status:") {
        return parse_status_atom(&lower).map(Expr::Atom);
    }

    if let Some(rest) = lower.strip_prefix("type:") {
        let sel = match rest {
            "todo" => TypeSel::Todo,
            "event" => TypeSel::Event,
            "all" => TypeSel::All,
            _ => bail!("unknown type `{rest}` (use todo, event, all)"),
        };
        return Ok(Expr::Atom(Filter {
            type_sel: Some(sel),
            ..Filter::default()
        }));
    }

    if tok.starts_with('+') && tok.len() > 1 {
        let name = &tok[1..];
        let lname = name.to_ascii_lowercase();
        let type_sel = match lname.as_str() {
            "todo" => Some(TypeSel::Todo),
            "event" => Some(TypeSel::Event),
            _ => None,
        };
        if let Some(sel) = type_sel {
            return Ok(Expr::Atom(Filter {
                type_sel: Some(sel),
                ..Filter::default()
            }));
        }
        if let Some(f) = virtual_flag(&lname) {
            return Ok(Expr::Atom(Filter {
                flags: vec![f],
                ..Filter::default()
            }));
        }
        return Ok(Expr::Atom(Filter {
            tags: vec![name.to_string()],
            ..Filter::default()
        }));
    }

    bail!("unknown filter token `{tok}`")
}

fn virtual_flag(lname: &str) -> Option<Flag> {
    match lname {
        "overdue" => Some(Flag::Overdue),
        // `active` is an alias of `pending` (both = not done).
        "active" | "pending" => Some(Flag::Pending),
        "completed" | "done" => Some(Flag::Completed),
        "cancelled" | "canceled" => Some(Flag::Cancelled),
        "in-progress" | "inprogress" | "in-process" | "inprocess" => {
            Some(Flag::InProgress)
        }
        "started" => Some(Flag::Started),
        "tagged" => Some(Flag::Tagged),
        "untagged" => Some(Flag::Untagged),
        "scheduled" => Some(Flag::Scheduled),
        "parent" => Some(Flag::Parent),
        "waiting" => Some(Flag::Waiting),
        _ => None,
    }
}

/// Which date field an atom targets.
#[derive(Clone, Copy)]
enum DateField {
    /// `due:` — unified date (todo→due, event→dtstart).
    Due,
    /// `date:` — unified date (same as `due`, explicit alias).
    Date,
    /// `from:` — VEVENT `dtstart` only.
    From,
}

fn field_atom(field: DateField, op: DueOp, rest: &str) -> Result<Expr> {
    let dt = parse_datetime(rest)?;
    let f = Filter {
        due: (matches!(field, DateField::Due)).then_some((op, dt)),
        date: (matches!(field, DateField::Date)).then_some((op, dt)),
        from: (matches!(field, DateField::From)).then_some((op, dt)),
        ..Filter::default()
    };
    Ok(Expr::Atom(f))
}

fn parse_status_atom(lower: &str) -> Result<Filter> {
    let rest = lower.strip_prefix("status:").unwrap_or(lower);
    Ok(Filter {
        status: match rest {
            "active" => None,
            "pending" => Some(TaskStatus::Pending),
            "in-progress" | "inprogress" | "in-process" | "inprocess" | "started" => {
                Some(TaskStatus::InProgress)
            }
            "completed" | "done" => Some(TaskStatus::Completed),
            "cancelled" | "canceled" => Some(TaskStatus::Cancelled),
            "recurring" => Some(TaskStatus::Recurring),
            _ => bail!("unknown status `{rest}`"),
        },
        // `status:active` is its own boolean clause, not a raw TaskStatus.
        active: rest == "active",
        ..Filter::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Task;
    use chrono::Duration;

    fn todo(overdue_days: i64) -> Task {
        let mut t = Task::new("work", "x");
        t.due = Some(Utc::now() - Duration::days(overdue_days));
        t
    }

    fn future_event() -> Task {
        let mut t = Task::new("work", "meet");
        t.dtstart = Some(Utc::now() + Duration::days(1));
        t
    }

    #[test]
    fn empty_matches_all() {
        let e = parse_expr(&[]).unwrap();
        assert!(e.matches(&Task::new("work", "any")));
    }

    #[test]
    fn started_flag_matches_started_at() {
        use chrono::Utc;
        let mut t = todo(0);
        let e = parse_expr_str("+STARTED").unwrap();
        assert!(!e.matches(&t));
        t.started_at = Some(Utc::now());
        assert!(e.matches(&t));

        // `status:started` stays an in-progress alias (status-based).
        let s = parse_expr_str("status:started").unwrap();
        let mut u = todo(0);
        u.status = TaskStatus::InProgress;
        assert!(s.matches(&u));
        assert!(!s.matches(&todo(0)));
    }

    #[test]
    fn overdue_wait_stays_waiting() {
        use chrono::{Duration, Utc};
        let w = parse_expr_str("+WAITING").unwrap();
        let o = parse_expr_str("+OVERDUE").unwrap();
        let mut t = todo(0);
        // Overdue timed todo with a +1d wait window: overdue AND waiting.
        t.due = Some(Utc::now() - Duration::hours(2));
        t.wait = Some(86_400);
        assert!(o.matches(&t));
        assert!(w.matches(&t));
        // A future instance with the same wait is also waiting, not overdue.
        let mut f = todo(0);
        f.due = Some(Utc::now() + Duration::hours(2));
        f.wait = Some(86_400);
        assert!(!o.matches(&f));
        assert!(w.matches(&f));
    }

    #[test]
    fn status_and_tags() {
        let mut t = Task::new("work", "x");
        t.status = TaskStatus::InProgress;
        t.tags = vec!["urgent".into()];
        let e = parse_expr(&["status:in-progress".into(), "+urgent".into()]).unwrap();
        assert!(e.matches(&t));
    }

    #[test]
    fn and_or_bools() {
        let mut t = Task::new("work", "x");
        t.tags = vec!["a".into()];
        let e = parse_expr_str("+a or +b").unwrap();
        assert!(e.matches(&t));
        let e2 = parse_expr_str("+a and +b").unwrap();
        assert!(!e2.matches(&t));
    }

    #[test]
    fn parens_grouping() {
        let mut t = Task::new("work", "x");
        t.due = Some(Utc::now() + Duration::days(1));
        let e = parse_expr_str("+z or (+x due.after:today)").unwrap();
        assert!(!e.matches(&t));
    }

    #[test]
    fn source_filter() {
        let t = Task::new("work", "x");
        let e = parse_expr_str("source:work").unwrap();
        assert!(e.matches(&t));
        let e2 = parse_expr_str("-source:work").unwrap();
        assert!(!e2.matches(&t));
    }

    #[test]
    fn due_exact_only() {
        // due:today = same calendar day
        let e = parse_expr_str("due:today").unwrap();
        assert!(e.matches(&todo(0)));
        assert!(!e.matches(&todo(1)));
    }

    #[test]
    fn due_before_by_after() {
        let t = todo(10); // due in past
        assert!(parse_expr_str("due.before:now").unwrap().matches(&t));
        assert!(parse_expr_str("due.by:now").unwrap().matches(&t));
        assert!(!parse_expr_str("due.after:now").unwrap().matches(&t));
    }

    #[test]
    fn date_filter_uses_due_for_todo_dtstart_for_event() {
        // todo: date = due (past) → after:now false, before:now true
        let t = todo(2);
        assert!(!parse_expr_str("date.after:now").unwrap().matches(&t));
        assert!(parse_expr_str("date.before:now").unwrap().matches(&t));
        assert!(parse_expr_str("date.by:now").unwrap().matches(&t));
        // event: date = dtstart (future) → after:now true
        let ev = future_event();
        assert!(parse_expr_str("date.after:now").unwrap().matches(&ev));
        assert!(!parse_expr_str("date.before:now").unwrap().matches(&ev));
        // todo without any date never matches date filters
        let bare = Task::new("work", "bare");
        assert!(!parse_expr_str("date:today").unwrap().matches(&bare));
        // negation
        assert!(parse_expr_str("-date.after:now").unwrap().matches(&t));
    }

    #[test]
    fn from_before_by_after() {
        let ev = future_event(); // dtstart in future (relative to `now`)
        assert!(!parse_expr_str("from.before:now").unwrap().matches(&ev));
        assert!(!parse_expr_str("from.by:now").unwrap().matches(&ev));
        assert!(parse_expr_str("from.after:now").unwrap().matches(&ev));
        // todos have no dtstart → never match from filters
        let t = todo(10);
        assert!(!parse_expr_str("from.after:now").unwrap().matches(&t));
        // negation
        assert!(parse_expr_str("-from.after:now").unwrap().matches(&t));
    }

    #[test]
    fn type_all() {
        assert!(
            !parse_expr_str("type:todo")
                .unwrap()
                .matches(&future_event())
        );
        assert!(
            parse_expr_str("type:event")
                .unwrap()
                .matches(&future_event())
        );
        assert!(parse_expr_str("type:all").unwrap().matches(&future_event()));
        assert!(
            !parse_expr_str("-type:event")
                .unwrap()
                .matches(&future_event())
        );
    }

    #[test]
    fn event_date_is_dtstart() {
        assert!(
            parse_expr_str("due.after:now")
                .unwrap()
                .matches(&future_event())
        );
    }

    #[test]
    fn builtin_list_filter_negates_status() {
        let mut done = Task::new("work", "x");
        done.status = TaskStatus::Completed;
        let e = parse_expr_str("-status:completed -status:cancelled").unwrap();
        assert!(!e.matches(&done));
        assert!(e.matches(&Task::new("work", "active")));
    }

    #[test]
    fn cli_tokens_parse_same_grammar() {
        let e = parse_expr(&["type:event".into(), "due.after:sod".into()]).unwrap();
        assert!(e.matches(&future_event()));
    }

    #[test]
    fn negated_pending_excludes_events() {
        let e = parse_expr_str("-PENDING").unwrap();
        assert!(!e.matches(&future_event()));
    }

    #[test]
    fn active_alias_and_negated_due() {
        let e = parse_expr_str("+active").unwrap();
        assert!(e.matches(&Task::new("work", "x")));
        let e2 = parse_expr_str("-active").unwrap();
        assert!(!e2.matches(&Task::new("work", "x")));
        // Not(due.after:now): a past-due todo satisfies it.
        let e3 = parse_expr_str("-due.after:now").unwrap();
        assert!(e3.matches(&todo(10)));
        // Not(priority:H)
        let e4 = parse_expr_str("-priority:H").unwrap();
        assert!(e4.matches(&Task::new("work", "x")));
    }

    #[test]
    fn waiting_flag_future_hidden_now_visible() {
        use chrono::{Duration, Utc};
        let mut t = Task::new("work", "deferred");
        t.due = Some(Utc::now() + Duration::days(7));
        // wait = 1 day before due → still waiting now.
        t.wait = Some(-86_400);
        let e = parse_expr_str("+WAITING").unwrap();
        assert!(e.matches(&t));
        // wait already passed (e.g. due passed long ago) → not waiting.
        let mut t2 = t.clone();
        t2.wait = Some(-86_400 * 30);
        assert!(!e.matches(&t2));

        // Default report exclusion: `-WAITING` must not match waiting tasks.
        let hide = parse_expr_str("-WAITING").unwrap();
        assert!(!hide.matches(&t));
    }
}
