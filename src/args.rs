//! Free-form argument parsing in the Taskwarrior/dstask style.
//!
//! Examples:
//! - `calman`                         → list (default)
//! - `calman add buy milk priority:H +home due:eod`
//! - `calman 1 modify new content pri:L -home due:20260824`
//! - `calman +OVERDUE list`
//! - `calman count status:pending`

use crate::date_parser::parse_datetime;
use crate::model::{TaskStatus, priority_from_str};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};

/// Recognized commands. `None` means the default report (`list`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Add,
    List,
    Done,
    Delete,
    Modify,
    Count,
    Sync,
    Tui,
}

/// How a `due` attribute applies in filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DueMod {
    #[default]
    On,
    Before,
    By,
    After,
}

/// A Taskwarrior-style `rc.report.<name>.<key>=<value>` override from argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RcReport {
    pub name: String,
    pub key: String,
    pub value: String,
}

/// Fully parsed command line.
#[derive(Debug, Clone, Default)]
pub struct ParsedArgs {
    pub cmd: Option<Command>,
    /// Listing subcommand name (`ls`/`list`/`next`) when `cmd == List`.
    pub report_name: Option<String>,
    pub sources: Vec<String>,
    pub ids: Vec<String>,
    pub text: String,
    pub priority: Option<u8>,
    pub due: Option<DateTime<Utc>>,
    pub due_mod: DueMod,
    pub status: Option<TaskStatus>,
    pub tags: Vec<String>,
    pub anti_tags: Vec<String>,
    pub overdue: bool,
    pub pending: bool,
    pub completed: bool,
    pub active: bool,
    pub cancelled: bool,
    pub in_progress: bool,
    pub tagged: bool,
    pub untagged: bool,
    pub scheduled: bool,
    pub r#type: Option<String>,
    pub anti_source: Option<String>,
    pub rel: Option<String>,
    pub anti_pending: bool,
    pub anti_active: bool,
    pub anti_completed: bool,
    pub anti_cancelled: bool,
    pub anti_in_progress: bool,
    pub anti_overdue: bool,
    pub anti_tagged: bool,
    pub anti_untagged: bool,
    pub anti_scheduled: bool,
    pub anti_type: Option<String>,
    /// `rc.report.<name>.<key>=<value>` tokens (e.g. columns/labels).
    pub rc_reports: Vec<RcReport>,
    /// Raw filter tokens (post-command, non-rc, non-id) for list/count.
    pub filter_tokens: Vec<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub location: Option<String>,
    pub repeat: Option<String>,
    pub description: Option<String>,
    /// `duration:1h` / `duration:45min` — alternative to `end:`.
    pub duration: Option<String>,
    /// `allday` or `+allday` flag.
    pub allday: bool,
    /// `alert:15min` — VALARM lead time before start/due.
    pub alert: Option<String>,
}

impl ParsedArgs {}

/// Parse raw argv (after the binary/global flags) into structured args.
pub fn parse(args: &[String]) -> Result<ParsedArgs> {
    let mut q = ParsedArgs::default();
    let mut ids_exhausted = false;
    let mut literal = false; // after `--`, everything is text

    for tok in args {
        if literal {
            push_text(&mut q, tok);
            continue;
        }
        let lower = tok.to_ascii_lowercase();

        if tok == "--" {
            literal = true;
            ids_exhausted = true;
            continue;
        }

        if q.cmd.is_none()
            && let Some((cmd, report)) = command_word(tok)
        {
            q.cmd = Some(cmd);
            q.report_name = report;
            continue;
        }

        // Global TW rc knobs (e.g. rc.verbose=header) are accepted and ignored.
        if lower.starts_with("rc.") && !lower.starts_with("rc.report.") {
            ids_exhausted = true;
            continue;
        }

        if let Some(rc) = parse_rc(tok)? {
            q.rc_reports.push(rc);
            ids_exhausted = true;
            continue;
        }

        if !ids_exhausted && tok.parse::<usize>().is_ok() {
            q.ids.push(tok.clone());
            continue;
        }

        q.filter_tokens.push(tok.clone());

        if let Some(rest) = lower
            .strip_prefix("priority:")
            .or_else(|| lower.strip_prefix("pri:"))
        {
            q.priority = Some(parse_priority(rest)?);
        } else if tok.len() > 7 && tok[..7].eq_ignore_ascii_case("source:") {
            q.sources
                .extend(tok[7..].split(',').map(|s| s.trim().to_string()));
        } else if tok.len() > 8 && tok[..8].eq_ignore_ascii_case("-source:") {
            q.anti_source = Some(tok[8..].to_string());
        } else if let Some(rest) = lower.strip_prefix("due.before:") {
            q.due = Some(parse_datetime(rest)?);
            q.due_mod = DueMod::Before;
        } else if let Some(rest) = lower.strip_prefix("due.by:") {
            q.due = Some(parse_datetime(rest)?);
            q.due_mod = DueMod::By;
        } else if let Some(rest) = lower.strip_prefix("due.after:") {
            q.due = Some(parse_datetime(rest)?);
            q.due_mod = DueMod::After;
        } else if let Some(rest) = lower.strip_prefix("due:") {
            q.due = Some(parse_datetime(rest)?);
            q.due_mod = DueMod::On;
        } else if let Some(rest) = lower.strip_prefix("status:") {
            if rest == "active" {
                q.active = true;
            } else {
                q.status = Some(parse_status(rest)?);
            }
        } else if let Some(rest) = lower.strip_prefix("type:") {
            q.r#type = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("rel:") {
            q.rel = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("start:") {
            q.start = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("end:") {
            q.end = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("location:") {
            q.location = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("repeat:") {
            q.repeat = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("duration:") {
            q.duration = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("dur:") {
            q.duration = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("alert:") {
            q.alert = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("desc:") {
            q.description = Some(rest.to_string());
        } else if lower == "allday" || lower == "+allday" {
            q.allday = true;
        } else if tok.starts_with('+') && tok.len() > 1 {
            match lower.as_str() {
                "+overdue" => q.overdue = true,
                "+pending" => q.pending = true,
                "+completed" | "+done" => q.completed = true,
                "+active" => q.active = true,
                "+cancelled" | "+canceled" => q.cancelled = true,
                "+in-progress" | "+inprogress" | "+in-process" | "+inprocess" | "+started" => {
                    q.in_progress = true
                }
                "+tagged" => q.tagged = true,
                "+untagged" => q.untagged = true,
                "+scheduled" => q.scheduled = true,
                "+todo" => q.r#type = Some("todo".to_string()),
                "+event" => q.r#type = Some("event".to_string()),
                _ => q.tags.push(tok[1..].to_string()),
            }
        } else if tok.starts_with('-') && tok.len() > 1 {
            match &lower[1..] {
                "overdue" => q.anti_overdue = true,
                "completed" | "done" => q.anti_completed = true,
                "cancelled" | "canceled" => q.anti_cancelled = true,
                "active" => q.anti_active = true,
                "in-progress" | "inprogress" | "in-process" | "inprocess" | "started" => {
                    q.anti_in_progress = true
                }
                "pending" => q.anti_pending = true,
                "tagged" => q.anti_tagged = true,
                "untagged" => q.anti_untagged = true,
                "scheduled" => q.anti_scheduled = true,
                "todo" => q.anti_type = Some("todo".to_string()),
                "event" => q.anti_type = Some("event".to_string()),
                _ => q.anti_tags.push(tok[1..].to_string()),
            }
        } else {
            push_text(&mut q, tok);
        }

        ids_exhausted = true;
    }

    Ok(q)
}

fn parse_rc(tok: &str) -> Result<Option<RcReport>> {
    if !tok.to_ascii_lowercase().starts_with("rc.report.") {
        return Ok(None);
    }
    let rest = &tok["rc.report.".len()..];
    let (name_key, value) = rest.split_once('=').ok_or_else(|| {
        anyhow::anyhow!("bad rc override `{tok}` (expected rc.report.<name>.<key>=<value>)")
    })?;
    let (name, key) = name_key.split_once('.').ok_or_else(|| {
        anyhow::anyhow!("bad rc override `{tok}` (expected rc.report.<name>.<key>=<value>)")
    })?;
    if name.is_empty() || key.is_empty() {
        anyhow::bail!("bad rc override `{tok}`");
    }
    Ok(Some(RcReport {
        name: name.to_string(),
        key: key.to_ascii_lowercase(),
        value: value.to_string(),
    }))
}

fn command_word(tok: &str) -> Option<(Command, Option<String>)> {
    let (cmd, report) = match tok.to_ascii_lowercase().as_str() {
        "add" => (Command::Add, None),
        "list" => (Command::List, Some("list".to_string())),
        "ls" => (Command::List, Some("ls".to_string())),
        "next" => (Command::List, Some("next".to_string())),
        "done" | "complete" => (Command::Done, None),
        "delete" | "rm" => (Command::Delete, None),
        "modify" | "mod" => (Command::Modify, None),
        "count" => (Command::Count, None),
        "sync" => (Command::Sync, None),
        "tui" => (Command::Tui, None),
        _ => return None,
    };
    Some((cmd, report))
}

fn parse_priority(v: &str) -> Result<u8> {
    priority_from_str(v).ok_or_else(|| anyhow::anyhow!("bad priority `{v}` (use H, M, L or 0-9)"))
}

fn parse_status(v: &str) -> Result<TaskStatus> {
    Ok(match v.to_ascii_lowercase().as_str() {
        "pending" => TaskStatus::Pending,
        "in-progress" | "inprogress" | "in-process" | "inprocess" | "started" => {
            TaskStatus::InProgress
        }
        "completed" | "done" => TaskStatus::Completed,
        "cancelled" | "canceled" => TaskStatus::Cancelled,
        _ => bail!("unknown status `{v}`"),
    })
}

fn push_text(q: &mut ParsedArgs, tok: &str) {
    if q.text.is_empty() {
        q.text = tok.to_string();
    } else {
        q.text.push(' ');
        q.text.push_str(tok);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> ParsedArgs {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn bare_means_list() {
        assert_eq!(p(&[]).cmd, None);
        assert_eq!(p(&["+PENDING"]).cmd, None);
        assert!(p(&["+PENDING"]).pending);
    }

    #[test]
    fn add_parses_attributes() {
        let q = p(&[
            "add",
            "buy milk",
            "priority:H",
            "+home",
            "+urgent",
            "due:eod",
        ]);
        assert_eq!(q.cmd, Some(Command::Add));
        assert_eq!(q.text, "buy milk");
        assert_eq!(q.priority, Some(9));
        assert_eq!(q.tags, vec!["home", "urgent"]);
        assert!(q.due.is_some());
    }

    #[test]
    fn filter_before_command() {
        let q = p(&["+OVERDUE", "list"]);
        assert_eq!(q.cmd, Some(Command::List));
        assert!(q.overdue);
    }

    #[test]
    fn event_attributes() {
        let q = p(&[
            "add",
            "Meet",
            "start:0826-0900",
            "duration:45min",
            "alert:15min",
            "+team",
        ]);
        assert_eq!(q.duration.as_deref(), Some("45min"));
        assert_eq!(q.alert.as_deref(), Some("15min"));
        let q2 = p(&["add", "Conf", "start:20260826", "allday"]);
        assert!(q2.allday);
    }

    #[test]
    fn dur_alias() {
        let q = p(&["add", "x", "start:25-0930", "dur:1h"]);
        assert_eq!(q.duration.as_deref(), Some("1h"));
    }

    #[test]
    fn source_attribute() {
        let q = p(&["source:work,personal", "list"]);
        assert_eq!(q.sources, vec!["work", "personal"]);
        assert_eq!(q.cmd, Some(Command::List));

        let q2 = p(&["add", "x", "source:work"]);
        assert_eq!(q2.sources, vec!["work"]);
    }

    #[test]
    fn modify_id_first() {
        let q = p(&[
            "1",
            "modify",
            "new content",
            "pri:L",
            "-bar",
            "due:20260824",
        ]);
        assert_eq!(q.cmd, Some(Command::Modify));
        assert_eq!(q.ids, vec!["1"]);
        assert_eq!(q.text, "new content");
        assert_eq!(q.priority, Some(1));
        assert_eq!(q.anti_tags, vec!["bar"]);
    }

    #[test]
    fn ids_after_command() {
        let q = p(&["done", "1", "2"]);
        assert_eq!(q.cmd, Some(Command::Done));
        assert_eq!(q.ids, vec!["1", "2"]);
    }

    #[test]
    fn literal_double_dash() {
        let q = p(&["modify", "--", "priority:H literal"]);
        assert_eq!(q.text, "priority:H literal");
        assert_eq!(q.priority, None);
    }

    #[test]
    fn count_keeps_filters() {
        let q = p(&["count", "status:completed"]);
        assert_eq!(q.cmd, Some(Command::Count));
        assert_eq!(q.status, Some(TaskStatus::Completed));
    }

    #[test]
    fn rc_report_override_parsed() {
        let q = p(&[
            "rc.report.next.columns=id,summary",
            "rc.report.next.labels=ID,SUMMARY",
            "next",
        ]);
        assert_eq!(q.cmd, Some(Command::List));
        assert_eq!(q.report_name.as_deref(), Some("next"));
        assert_eq!(q.rc_reports.len(), 2);
        assert_eq!(q.rc_reports[0].name, "next");
        assert_eq!(q.rc_reports[0].key, "columns");
        assert_eq!(q.rc_reports[0].value, "id,summary");
    }

    #[test]
    fn filter_tokens_exclude_command_rc_ids() {
        let q = p(&[
            "rc.report.next.columns=id",
            "+PENDING",
            "source:work",
            "-source:personal",
            "list",
        ]);
        assert_eq!(
            q.filter_tokens,
            vec!["+PENDING", "source:work", "-source:personal"]
        );
        assert_eq!(q.anti_source.as_deref(), Some("personal"));
    }

    #[test]
    fn global_rc_tokens_ignored() {
        let q = p(&["rc.verbose=header", "next", "rc.report.next.columns=id"]);
        assert_eq!(q.cmd, Some(Command::List));
        assert_eq!(q.filter_tokens, Vec::<String>::new());
        assert_eq!(q.rc_reports.len(), 1);
    }
}
