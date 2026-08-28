//! Free-form argument parsing in the Taskwarrior/dstask style.
//!
//! Examples:
//! - `calman`                         → list (default)
//! - `calman add buy milk priority:H +home due:eod`
//! - `calman 1 modify new content pri:L -home due:20260824`
//! - `calman +OVERDUE list`
//! - `calman count status:pending`

use crate::date_parser::{DateValue, local_midnight, parse_date_value};
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
    Help,
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
    /// `due:` was given as a date-only value (all-day semantics).
    pub due_allday: bool,
    pub status: Option<TaskStatus>,
    pub tags: Vec<String>,
    pub anti_tags: Vec<String>,
    pub rel: Option<String>,
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

/// Free-text attribute whose value may arrive on following tokens
/// (e.g. `desc: "some more information"`).
#[derive(Clone, Copy)]
enum Capture {
    Description,
    Location,
}

/// True if `t` begins a recognised attribute token — used to end capture mode.
fn is_attr_token(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    l.starts_with("rc.")
        || l.starts_with("source:")
        || l.starts_with("due:")
        || l.starts_with("start:")
        || l.starts_with("end:")
        || l.starts_with("repeat:")
        || l.starts_with("recur:")
        || l.starts_with("duration:")
        || l.starts_with("dur:")
        || l.starts_with("alert:")
        || l.starts_with("desc:")
        || l.starts_with("location:")
        || l.starts_with("rel:")
        || l.starts_with("status:")
        || l.starts_with("priority:")
        || l.starts_with("pri:")
        || (t.starts_with('+') && t.len() > 1)
        || (t.starts_with('-') && t.len() > 1)
}

/// Parse raw argv (after the binary/global flags) into structured args.
pub fn parse(args: &[String]) -> Result<ParsedArgs> {
    let mut q = ParsedArgs::default();
    let mut ids_exhausted = false;
    let mut literal = false; // after `--`, everything is text
    let mut capture: Option<Capture> = None; // free-text attr value collection

    for tok in args {
        if literal {
            push_text(&mut q, tok);
            continue;
        }
        let lower = tok.to_ascii_lowercase();

        // Capture mode: a free-text attribute (`desc:`, `location:`) whose
        // value follows the colon on later tokens (e.g. `desc: "some info"`).
        // Collect words until the next recognised attribute token or `--`.
        if let Some(target) = &mut capture {
            let stop = tok == "--" || is_attr_token(&lower);
            if stop {
                capture = None;
            } else {
                let slot = match target {
                    Capture::Description => &mut q.description,
                    Capture::Location => &mut q.location,
                };
                match slot {
                    Some(s) => {
                        if s.is_empty() {
                            *s = tok.to_string();
                        } else {
                            s.push(' ');
                            s.push_str(tok);
                        }
                    }
                    None => *slot = Some(tok.to_string()),
                }
                continue;
            }
        }

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
        } else if let Some(rest) = lower.strip_prefix("due:") {
            let dv = parse_date_value(rest)?;
            let dtv = match dv {
                DateValue::Date(d) => {
                    q.due_allday = true;
                    local_midnight(d)
                }
                DateValue::Time(dt) => dt,
            };
            q.due = Some(dtv);
        } else if let Some(rest) = lower.strip_prefix("status:") {
            // `active` is a filter-only status; `status:<x>` also feeds modify.
            if rest != "active" {
                q.status = Some(parse_status(rest)?);
            }
        } else if let Some(rest) = lower.strip_prefix("rel:") {
            q.rel = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("start:") {
            q.start = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("end:") {
            q.end = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("location:") {
            if rest.is_empty() {
                q.location = Some(String::new());
                capture = Some(Capture::Location);
            } else {
                q.location = Some(rest.to_string());
            }
        } else if let Some(rest) = lower
            .strip_prefix("repeat:")
            .or_else(|| lower.strip_prefix("recur:"))
        {
            q.repeat = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("duration:") {
            q.duration = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("dur:") {
            q.duration = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("alert:") {
            q.alert = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("desc:") {
            if rest.is_empty() {
                q.description = Some(String::new());
                capture = Some(Capture::Description);
            } else {
                q.description = Some(rest.to_string());
            }
        } else if lower == "allday" || lower == "+allday" {
            q.allday = true;
        } else if tok.starts_with('+') && tok.len() > 1 {
            let name = &tok[1..];
            if name.eq_ignore_ascii_case("allday") {
                q.allday = true;
            } else if !is_filter_only_plus(name) {
                q.tags.push(name.to_string());
            }
        } else if tok.starts_with('-') && tok.len() > 1 {
            if !is_filter_only_minus(&lower[1..]) {
                q.anti_tags.push(tok[1..].to_string());
            }
        } else {
            push_text(&mut q, tok);
        }

        ids_exhausted = true;
    }

    Ok(q)
}

/// Filter/virtual tokens that `add`/`modify` must not treat as literal tags.
/// They are still recorded in `filter_tokens` for `list`/`count`.
fn is_filter_only_plus(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    matches!(
        l.as_str(),
        "overdue"
            | "pending"
            | "completed"
            | "done"
            | "active"
            | "cancelled"
            | "canceled"
            | "in-progress"
            | "inprogress"
            | "in-process"
            | "inprocess"
            | "started"
            | "tagged"
            | "untagged"
            | "scheduled"
            | "todo"
            | "event"
    )
}

fn is_filter_only_minus(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    matches!(
        l.as_str(),
        "overdue"
            | "pending"
            | "completed"
            | "done"
            | "active"
            | "cancelled"
            | "canceled"
            | "in-progress"
            | "inprogress"
            | "in-process"
            | "inprocess"
            | "started"
            | "tagged"
            | "untagged"
            | "scheduled"
            | "todo"
            | "event"
    ) || l.starts_with("status:")
        || l.starts_with("source:")
        || l.starts_with("type:")
        || l.starts_with("priority:")
        || l.starts_with("pri:")
        || l.starts_with("due")
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
        "help" | "filters" => (Command::Help, None),
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
        let q = p(&["+PENDING"]);
        assert_eq!(q.cmd, None);
        assert_eq!(q.filter_tokens, vec!["+PENDING"]);
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
        assert_eq!(q.filter_tokens, vec!["+OVERDUE"]);
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
    }

    #[test]
    fn global_rc_tokens_ignored() {
        let q = p(&["rc.verbose=header", "next", "rc.report.next.columns=id"]);
        assert_eq!(q.cmd, Some(Command::List));
        assert_eq!(q.filter_tokens, Vec::<String>::new());
        assert_eq!(q.rc_reports.len(), 1);
    }

    #[test]
    fn add_does_not_treat_virtual_tags_as_literal_tags() {
        let q = p(&["add", "x", "+overdue", "+home", "-scheduled"]);
        assert_eq!(q.tags, vec!["home"]);
        assert!(q.anti_tags.is_empty());
    }

    #[test]
    fn help_command() {
        let q = p(&["help"]);
        assert_eq!(q.cmd, Some(Command::Help));
        let q2 = p(&["filters"]);
        assert_eq!(q2.cmd, Some(Command::Help));
    }

    #[test]
    fn recur_is_alias_for_repeat() {
        let q = p(&["add", "x", "recur:daily"]);
        assert_eq!(q.repeat.as_deref(), Some("daily"));
        let q2 = p(&["add", "x", "repeat:weekly"]);
        assert_eq!(q2.repeat.as_deref(), Some("weekly"));
    }
}
