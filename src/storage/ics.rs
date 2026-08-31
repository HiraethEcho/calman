//! ICS storage backend (one `<UID>.ics` per task; VTODO/VEVENT).
//!
//! Layout per DESIGN.md §2.2.B. Field mapping is a minimal but standard
//! iCalendar serialization covering the core `Task` fields.

use super::{Storage, atomic_write};
use crate::date::local_midnight;
use crate::model::{Task, TaskStatus};
use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;
use std::fs;
use std::path::{Path, PathBuf};

/// Storage rooted at a source location holding one `.ics` file per task.
pub struct IcsStorage {
    dir: PathBuf,
    tz: Tz,
    tasks: Vec<Task>,
}

impl IcsStorage {
    pub fn open(location: &Path, tz: Tz) -> Result<Self> {
        fs::create_dir_all(location)
            .with_context(|| format!("create storage dir {}", location.display()))?;
        let mut tasks = Vec::new();
        // First pass: read + parse, remap storage uid to the filename, and
        // record in-file ICS UID -> filename so override `parent_uid`s (which
        // carry the master's ICS UID) can be resolved to the master's storage
        // key below (external CalDAV files may use a filename ≠ UID).
        let mut uid_to_file: std::collections::HashMap<String, String> = Default::default();
        let entries =
            fs::read_dir(location).with_context(|| format!("read dir {}", location.display()))?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("ics") {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path) {
                match parse_ics(&content) {
                    Ok(mut task) => {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            // Master files (no RECURRENCE-ID) win the UID→file
                            // map: overrides reuse the master's ICS UID and must
                            // not shadow the master's storage key.
                            if task.recurrence_id.is_none()
                                || !uid_to_file.contains_key(&task.uid)
                            {
                                uid_to_file.insert(task.uid.clone(), stem.to_string());
                            }
                            task.uid = stem.to_string();
                        }
                        tasks.push(task);
                    }
                    Err(e) => eprintln!(
                        "warning: skipping unparseable {}: {e}",
                        path.display()
                    ),
                }
            }
        }
        for t in &mut tasks {
            if let Some(pid) = &t.parent_uid
                && let Some(key) = uid_to_file.get(pid)
            {
                t.parent_uid = Some(key.clone());
            }
        }
        Ok(IcsStorage {
            dir: location.to_path_buf(),
            tz,
            tasks,
        })
    }

    fn file_for(&self, uid: &str) -> PathBuf {
        self.dir.join(format!("{uid}.ics"))
    }
}

impl Storage for IcsStorage {
    fn list(&self) -> &[Task] {
        &self.tasks
    }

    fn add(&mut self, task: Task) -> Result<()> {
        atomic_write(&self.file_for(&task.uid), render_ics(&task, self.tz)?.as_bytes())?;
        self.tasks.push(task);
        Ok(())
    }

    fn update<F>(&mut self, uid: &str, f: F) -> Result<Option<Task>>
    where
        F: FnOnce(&mut Task) -> Result<()>,
    {
        let Some(pos) = self.tasks.iter().position(|t| t.uid == uid) else {
            return Ok(None);
        };
        let t = &mut self.tasks[pos];
        f(t)?;
        t.updated_at = Utc::now();
        let t = &self.tasks[pos];
        atomic_write(&self.file_for(uid), render_ics(t, self.tz)?.as_bytes())?;
        Ok(Some(t.clone()))
    }

    fn remove(&mut self, uid: &str) -> Result<Option<Task>> {
        let Some(pos) = self.tasks.iter().position(|t| t.uid == uid) else {
            return Ok(None);
        };
        let removed = self.tasks.remove(pos);
        let path = self.file_for(uid);
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(Some(removed))
    }
}

/// Render a date property (DUE/DTSTART/DTEND/EXDATE/RECURRENCE-ID) in its
/// all-day (`;VALUE=DATE`) or timed (`;TZID=`) form.
fn ics_date_prop(name: &str, d: DateTime<Utc>, allday: bool, tz: Tz) -> String {
    if allday {
        format!("{name};VALUE=DATE:{}", d.with_timezone(&Local).format("%Y%m%d"))
    } else {
        format!("{name};TZID={}:{}", tz.name(), dt_local(d, tz))
    }
}

fn fold(line: &str) -> String {
    // RFC 5545 line folding at 75 octets (approximated via chars).
    let mut out = String::new();
    let mut remaining = line;
    while remaining.chars().count() > 73 {
        let cut = remaining
            .char_indices()
            .nth(73)
            .map(|(i, _)| i)
            .unwrap_or(remaining.len());
        out.push_str(&remaining[..cut]);
        out.push_str("\r\n ");
        remaining = &remaining[cut..];
    }
    out.push_str(remaining);
    out.push_str("\r\n");
    out
}

/// Render a task as a full VCALENDAR document.
pub fn render_ics(task: &Task, tz: Tz) -> Result<String> {
    let mut lines = String::new();
    lines.push_str("BEGIN:VCALENDAR\r\n");
    lines.push_str("VERSION:2.0\r\n");
    lines.push_str("CALSCALE:GREGORIAN\r\n");
    lines.push_str("PRODID:-//calman//calman//EN\r\n");

    let component = if task.event { "VEVENT" } else { "VTODO" };
    lines.push_str(&format!("BEGIN:{component}\r\n"));

    let mut props = Vec::new();
    // Override components use the master's UID; parent_uid holds the stored uid.
    let uid = task.parent_uid.as_deref().unwrap_or(&task.uid);
    props.push(format!("UID:{}", escape_text(uid)));
    props.push(format!("SUMMARY:{}", escape_text(&task.summary)));
    // DTSTAMP is REQUIRED by RFC 5545; SEQUENCE aids CalDAV sync ordering.
    props.push(format!("DTSTAMP:{}", dt(Utc::now())));
    props.push("SEQUENCE:0".to_string());
    props.push(format!(
        "STATUS:{}",
        if component == "VEVENT" {
            event_status_to_ics(task.status)
        } else {
            status_to_ics(task.status)
        }
    ));
    // VEVENT only permits TENTATIVE/CONFIRMED/CANCELLED, so calman's own
    // Completed/InProgress would round-trip as Pending; persist them in a
    // private property (ignored by other clients).
    if component == "VEVENT" && matches!(task.status, TaskStatus::Completed | TaskStatus::InProgress) {
        props.push(format!("X-CALMAN-STATUS:{}", status_to_ics(task.status)));
    }
    // iPhone/standard clients expect explicit transparency on events.
    if component == "VEVENT" {
        props.push("TRANSP:OPAQUE".to_string());
    }
    if let Some(p) = task.priority {
        props.push(format!("PRIORITY:{p}"));
    }
    if !task.tags.is_empty() {
        props.push(format!(
            "CATEGORIES:{}",
            task.tags
                .iter()
                .map(|t| escape_text(t))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    props.push(format!("CREATED:{}", dt(task.created_at)));
    props.push(format!("LAST-MODIFIED:{}", dt(task.updated_at)));
    if let Some(d) = task.due {
        props.push(ics_date_prop("DUE", d, task.allday, tz));
    }
    if let Some(p) = task.percent_complete {
        props.push(format!("PERCENT-COMPLETE:{p}"));
    }
    if let Some(c) = task.completed_at {
        props.push(format!("COMPLETED:{}", dt(c)));
    }
    // VTODO has no DTSTART/DTEND per RFC 5545 — only VEVENT writes them.
    if task.event
        && let Some(d) = task.dtstart
    {
        props.push(ics_date_prop("DTSTART", d, task.allday, tz));
    }
    if task.event
        && let Some(d) = task.dtend
    {
        props.push(ics_date_prop("DTEND", d, task.allday, tz));
    }
    if let Some(r) = &task.rrule {
        props.push(format!("RRULE:{r}"));
    }
    // Emit EXDATE for each excluded occurrence.
    for ex in &task.exdates {
        props.push(ics_date_prop("EXDATE", *ex, task.allday, tz));
    }
    // Emit RECURRENCE-ID for override instances.
    if let Some(rid) = task.recurrence_id {
        props.push(ics_date_prop("RECURRENCE-ID", rid, task.allday, tz));
    }
    if let Some(l) = &task.location {
        props.push(format!("LOCATION:{}", escape_text(l)));
    }
    if let Some(w) = task.wait {
        props.push(format!("X-CALMAN-WAIT-OFFSET:{w}"));
    }
    if let Some(d) = &task.description {
        props.push(format!("DESCRIPTION:{}", escape_text(d)));
    }
    if let Some(r) = &task.related_to {
        props.push(format!("RELATED-TO:{r}"));
    }

    for p in props {
        lines.push_str(&fold(&p));
    }
    if let Some(secs) = task.alarm_before {
        lines.push_str(&format!("BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT{secs}S\r\nDESCRIPTION:{}\r\nEND:VALARM\r\n", escape_text(&task.summary)));
    }
    lines.push_str(&format!("END:{component}\r\n"));
    lines.push_str("END:VCALENDAR\r\n");
    Ok(lines)
}

/// Parse a VCALENDAR document into a `Task`.
pub fn parse_ics(content: &str) -> Result<Task> {
    let mut event_kind = false;
    let mut uid = String::new();
    let mut summary = String::new();
    let mut status = TaskStatus::Pending;
    let mut priority = None;
    let mut tags = Vec::new();
    let mut due = None;
    let mut percent = None;
    let mut completed_at = None;
    let mut dtstart = None;
    let mut dtend = None;
    let mut rrule = None;
    let mut location = None;
    let mut description = None;
    let mut related_to = None;
    let mut created_at = None;
    let mut updated_at = None;
    let mut allday = false;
    let mut alarm_before = None;
    let mut in_alarm = false;
    let mut in_vtimezone = false;
    let mut exdates = Vec::new();
    let mut recurrence_id = None;
    let mut parent_uid = None;
    let mut wait = None;

    for raw in unfold(content) {
        let (name, value) = raw
            .split_once(':')
            .map(|(n, v)| (n.trim(), v.trim()))
            .unwrap_or(("", ""));
        let key = name.split(';').next().unwrap_or("").to_uppercase();
        let tzid = param_tzid(name);
        if key == "BEGIN" && value == "VEVENT" {
            event_kind = true;
        }
        if key == "BEGIN" && value == "VTODO" {
            event_kind = false;
        }
        if key == "BEGIN" && value == "VALARM" {
            in_alarm = true;
            continue;
        }
        if key == "END" && value == "VALARM" {
            in_alarm = false;
            continue;
        }
        // VTIMEZONE blocks carry their own DTSTART/RRULE definitions that
        // must NOT overwrite the component's real properties (this is how
        // CalDAV servers ship recurring events).
        if key == "BEGIN" && value == "VTIMEZONE" {
            in_vtimezone = true;
            continue;
        }
        if key == "END" && value == "VTIMEZONE" {
            in_vtimezone = false;
            continue;
        }
        if in_vtimezone {
            continue;
        }
        match key.as_str() {
            "UID" => uid = unescape_text(value),
            "SUMMARY" => summary = unescape_text(value),
            "DESCRIPTION" => description = Some(unescape_text(value)),
            "STATUS" => status = status_from_ics(value),
            "X-CALMAN-STATUS" => status = status_from_ics(value),
            "PRIORITY" => priority = value.parse().ok(),
            // Unescape first so escaped commas (`\,`) in a tag survive splitting.
            "CATEGORIES" => tags = unescape_text(value).split(',').map(unescape_text).collect(),
            "DUE" if name.contains("VALUE=DATE") => {
                allday = true;
                due = parse_all_day(value);
            }
            "DUE" => {
                due = match &tzid {
                    Some(tz) => parse_tz(value, tz),
                    None => parse_dt(value),
                }
            }
            "PERCENT-COMPLETE" => percent = value.parse().ok(),
            "COMPLETED" => completed_at = parse_dt(value),
            "DTSTART" if name.contains("VALUE=DATE") => {
                allday = true;
                dtstart = parse_all_day(value);
            }
            "DTSTART" => {
                dtstart = match &tzid {
                    Some(tz) => parse_tz(value, tz),
                    None => parse_dt(value),
                }
            }
            "DTEND" if name.contains("VALUE=DATE") => {
                allday = true;
                dtend = parse_all_day(value);
            }
            "DTEND" => {
                dtend = match &tzid {
                    Some(tz) => parse_tz(value, tz),
                    None => parse_dt(value),
                }
            }
            "RRULE" => rrule = Some(value.to_string()),
            "X-CALMAN-WAIT-OFFSET" => wait = value.trim().parse::<i64>().ok(),
            "EXDATE" => {
                // RFC 5545 allows a comma-separated list of excluded datetimes.
                let tz = param_tzid(name);
                for v in value.split(',') {
                    let d = match (&tz, name.contains("VALUE=DATE")) {
                        (_, true) => parse_all_day(v),
                        (Some(tz), false) => parse_tz(v, tz),
                        (None, _) => parse_dt(v),
                    };
                    if let Some(dt) = d {
                        exdates.push(dt);
                    }
                }
            }
            "RECURRENCE-ID" => {
                let tz = param_tzid(name);
                let d = match (&tz, name.contains("VALUE=DATE")) {
                    (_, true) => parse_all_day(value),
                    (Some(tz), false) => parse_tz(value, tz),
                    (None, _) => parse_dt(value),
                };
                recurrence_id = d;
                // For an override component, the UID stays the master's uid;
                // we record the current uid as parent_uid and use the
                // current (per-component) uid for storage.
                if !uid.is_empty() {
                    parent_uid = Some(uid.clone());
                }
            }
            "LOCATION" => location = Some(unescape_text(value)),
            "RELATED-TO" => related_to = Some(value.to_string()),
            "CREATED" => created_at = parse_dt(value),
            "LAST-MODIFIED" => updated_at = parse_dt(value),
            "TRIGGER" if in_alarm => alarm_before = parse_trigger(value),
            _ => {}
        }
    }

    // A component carrying RRULE is the recurring master.
    if rrule.is_some() {
        status = TaskStatus::Recurring;
    }

    if uid.is_empty() {
        anyhow::bail!("ICS missing UID");
    }

    Ok(Task {
        uid,
        source: String::new(), // filled by caller
        summary,
        description,
        status,
        priority,
        tags,
        due,
        percent_complete: percent,
        completed_at,
        dtstart,
        dtend,
        rrule,
        location,
        related_to,
        event: event_kind,
        allday,
        alarm_before,
        created_at: created_at.unwrap_or_else(Utc::now),
        updated_at: updated_at.unwrap_or_else(Utc::now),
        exdates,
        recurrence_id,
        parent_uid,
        wait,
    })
}

fn dt(d: DateTime<Utc>) -> String {
    d.to_rfc3339_opts(SecondsFormat::Secs, true)
        .replace(['-', ':', '+'], "")
        .trim_end_matches('Z')
        .to_string()
        + "Z"
}

/// Format a UTC instant as the local wall-clock time in `tz` (no `Z`).
fn dt_local(d: DateTime<Utc>, tz: Tz) -> String {
    d.with_timezone(&tz).format("%Y%m%dT%H%M%S").to_string()
}

fn parse_dt(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%SZ") {
        return Some(DateTime::from_naive_utc_and_offset(naive, chrono::Utc));
    }
    None
}

/// Parse `VALUE=DATE` date-only values as local midnight UTC.
fn parse_all_day(s: &str) -> Option<DateTime<Utc>> {
    let d = chrono::NaiveDate::parse_from_str(s.trim(), "%Y%m%d").ok()?;
    Some(local_midnight(d))
}

/// Extract `TZID=` parameter from a property name like `DTSTART;TZID=Asia/Shanghai`.
fn param_tzid(name: &str) -> Option<String> {
    name.split(';')
        .find_map(|p| p.strip_prefix("TZID="))
        .map(|s| s.to_string())
}

/// Parse a local wall time with an IANA `TZID` into UTC.
/// Unknown/custom TZID falls back to the system local zone (no data loss).
fn parse_tz(value: &str, tzid: &str) -> Option<DateTime<Utc>> {
    let s = value.trim();
    let naive = NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M"))
        .ok()?;
    if let Ok(tz) = tzid.parse::<Tz>() {
        Some(
            tz.from_local_datetime(&naive)
                .earliest()
                .unwrap_or_else(|| tz.from_utc_datetime(&naive))
                .with_timezone(&Utc),
        )
    } else {
        Some(
            Local
                .from_local_datetime(&naive)
                .single()
                .unwrap_or_else(|| Local.from_utc_datetime(&naive))
                .with_timezone(&Utc),
        )
    }
}

/// Parse a VALARM trigger like `-PT15M`, `-PT900S`, `-P1D` → seconds.
fn parse_trigger(s: &str) -> Option<i64> {
    let s = s.trim().trim_start_matches('-');
    let s = s.strip_prefix('P')?;
    let (date_part, time_part) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let mut secs: i64 = 0;
    if let Some(d) = date_part.strip_suffix('D') {
        secs += d.parse::<i64>().ok()? * 86_400;
    } else if let Some(w) = date_part.strip_suffix('W') {
        secs += w.parse::<i64>().ok()? * 604_800;
    } else if !date_part.is_empty() {
        return None;
    }
    if let Some(t) = time_part {
        let (n, mult) = t
            .strip_suffix('S')
            .map(|n| (n, 1))
            .or_else(|| t.strip_suffix('M').map(|n| (n, 60)))
            .or_else(|| t.strip_suffix('H').map(|n| (n, 3600)))
            .ok_or(())
            .ok()?;
        secs += n.parse::<i64>().ok()? * mult;
    }
    Some(secs)
}

fn status_to_ics(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Pending => "NEEDS-ACTION",
        TaskStatus::InProgress => "IN-PROCESS",
        TaskStatus::Recurring => "NEEDS-ACTION",
        TaskStatus::Completed => "COMPLETED",
        TaskStatus::Cancelled => "CANCELLED",
    }
}

/// VEVENT permits only TENTATIVE/CONFIRMED/CANCELLED (RFC 5545).
fn event_status_to_ics(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Cancelled => "CANCELLED",
        TaskStatus::Recurring => "CONFIRMED",
        _ => "CONFIRMED",
    }
}

fn status_from_ics(s: &str) -> TaskStatus {
    match s.to_uppercase().as_str() {
        "COMPLETED" => TaskStatus::Completed,
        "CANCELLED" | "CANCELED" => TaskStatus::Cancelled,
        "IN-PROCESS" => TaskStatus::InProgress,
        _ => TaskStatus::Pending,
    }
}

fn escape_text(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

fn unescape_text(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some(',') => out.push(','),
                Some(';') => out.push(';'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Unfold RFC 5545 folded lines, dropping continuation spaces.
fn unfold(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(' ') {
            current.push_str(rest);
        } else {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            current = line.trim_end_matches('\r').to_string();
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn make_task() -> Task {
        let mut t = Task::new("work", "ship it");
        t.due = parse_dt("20260101T170000Z");
        t.tags = vec!["dev".into(), "urgent".into()];
        t
    }

    #[test]
    fn ics_roundtrip() {
        let t = make_task();
        let rendered = render_ics(&t, Tz::UTC).unwrap();
        let mut parsed = parse_ics(&rendered).unwrap();
        parsed.source = "work".into();
        assert_eq!(parsed.uid, t.uid);
        assert_eq!(parsed.summary, t.summary);
        assert_eq!(parsed.tags, t.tags);
        assert_eq!(parsed.due, t.due);
    }

    #[test]
    fn wait_offset_roundtrip_via_xprop() {
        let mut t = make_task();
        t.wait = Some(-86_400);
        let rendered = render_ics(&t, Tz::UTC).unwrap();
        assert!(rendered.contains("X-CALMAN-WAIT-OFFSET:-86400"));
        let parsed = parse_ics(&rendered).unwrap();
        assert_eq!(parsed.wait, Some(-86_400));
    }

    #[test]
    fn read_dir_picks_up_ics() {
        let dir = tempdir().unwrap();
        let t = make_task();
        std::fs::write(
            dir.path().join(format!("{}.ics", t.uid)),
            render_ics(&t, Tz::UTC).unwrap(),
        )
        .unwrap();
        let _ = std::io::stdout().flush();
        let s = IcsStorage::open(dir.path(), Tz::UTC).unwrap();
        assert_eq!(s.list().len(), 1);
        assert_eq!(s.list()[0].summary, "ship it");
    }

    #[test]
    fn allday_and_alarm_roundtrip() {
        let mut t = make_task();
        t.event = true;
        t.allday = true;
        t.dtstart = Some(crate::date::local_midnight(
            chrono::NaiveDate::from_ymd_opt(2026, 8, 12).unwrap(),
        ));
        t.dtend = Some(crate::date::local_midnight(
            chrono::NaiveDate::from_ymd_opt(2026, 8, 14).unwrap(),
        ));
        t.alarm_before = Some(900);
        let rendered = render_ics(&t, Tz::UTC).unwrap();
        assert!(rendered.contains("DTSTART;VALUE=DATE:20260812"));
        assert!(rendered.contains("DTEND;VALUE=DATE:20260814"));
        assert!(rendered.contains("TRIGGER:-PT900S"));

        let parsed = parse_ics(&rendered).unwrap();
        assert!(parsed.allday);
        assert_eq!(parsed.alarm_before, Some(900));
        assert_eq!(
            parsed.dtstart.unwrap().with_timezone(&Local).date_naive(),
            chrono::NaiveDate::from_ymd_opt(2026, 8, 12).unwrap()
        );
    }

    #[test]
    fn parses_iphone_tzid_event() {
        let content = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nCALSCALE:GREGORIAN\r\nPRODID:-//Apple Inc.//iPhone//EN\r\nBEGIN:VTIMEZONE\r\nTZID:Asia/Shanghai\r\nBEGIN:STANDARD\r\nDTSTART:19890917T020000\r\nTZNAME:GMT+8\r\nTZOFFSETFROM:+0900\r\nTZOFFSETTO:+0800\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:0914B2E8-7236-4A9B-AF40-C97BB83F3A50\r\nDTSTART;TZID=Asia/Shanghai:20260825T100000\r\nDTEND;TZID=Asia/Shanghai:20260825T104500\r\nCREATED:20260824T134604Z\r\nDESCRIPTION:备注\r\nDTSTAMP:20260824T134610Z\r\nLAST-MODIFIED:20260824T134604Z\r\nSEQUENCE:0\r\nSUMMARY:example\r\nTRANSP:OPAQUE\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let t = parse_ics(content).unwrap();
        assert_eq!(t.summary, "example");
        assert_eq!(t.description.as_deref(), Some("备注"));
        // 10:00 Asia/Shanghai = 02:00 UTC (UTC+8)
        assert_eq!(
            t.dtstart.unwrap().format("%Y%m%dT%H%M%SZ").to_string(),
            "20260825T020000Z"
        );
        assert_eq!(
            t.dtend.unwrap().format("%Y%m%dT%H%M%SZ").to_string(),
            "20260825T024500Z"
        );
    }

    #[test]
    fn parses_iphone_vtodo_tzid_due() {
        let content = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:abc-123\r\nSUMMARY:task\r\nDUE;TZID=Asia/Shanghai:20260825T170000\r\nDTSTAMP:20260824T134610Z\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let t = parse_ics(content).unwrap();
        assert_eq!(t.status, TaskStatus::Pending);
        assert_eq!(
            t.due.unwrap().format("%Y%m%dT%H%M%SZ").to_string(),
            "20260825T090000Z"
        );
    }

    #[test]
    fn rrule_parses_as_recurring() {
        let content = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:series-1\r\nSUMMARY:standup\r\nDTSTART;TZID=Asia/Shanghai:20260901T090000\r\nRRULE:FREQ=WEEKLY;BYDAY=TU\r\nDTSTAMP:20260824T134610Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let t = parse_ics(content).unwrap();
        assert_eq!(t.status, TaskStatus::Recurring);
        assert_eq!(t.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=TU"));
    }

    #[test]
    fn vtimezone_props_do_not_pollute_component() {
        // VTIMEZONE carries its own DTSTART/RRULE definitions; they must not
        // overwrite the VEVENT's real dtstart nor mark it recurring.
        let content = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:Asia/Shanghai\r\nBEGIN:STANDARD\r\nDTSTART:19890917T020000\r\nRRULE:FREQ=YEARLY;UNTIL=20490917T020000Z;BYMONTH=9\r\nTZOFFSETFROM:+0900\r\nTZOFFSETTO:+0800\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:plain-1\r\nSUMMARY:one-off\r\nDTSTART;TZID=Asia/Shanghai:20260901T100000\r\nDTSTAMP:20260824T134610Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let t = parse_ics(content).unwrap();
        // Not recurring: the RRULE above came from the timezone definition.
        assert_eq!(t.status, TaskStatus::Pending);
        assert!(t.rrule.is_none());
        // dtstart is the VEVENT's, not the TZ block's.
        assert_eq!(
            t.dtstart.unwrap().format("%Y%m%dT%H%M%SZ").to_string(),
            "20260901T020000Z"
        );
    }

    #[test]
    fn parse_override_sets_recurrence_id_and_parent_uid() {
        let content = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:series-1\r\nSUMMARY:rescheduled\r\nDTSTART;TZID=Asia/Shanghai:20260908T100000\r\nRECURRENCE-ID;TZID=Asia/Shanghai:20260901T090000\r\nDTSTAMP:20260824T134610Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let t = parse_ics(content).unwrap();
        assert!(t.recurrence_id.is_some());
        assert_eq!(t.parent_uid.as_deref(), Some("series-1"));
        assert_eq!(
            t.recurrence_id.unwrap().format("%Y%m%dT%H%M%SZ").to_string(),
            "20260901T010000Z"
        );
    }

    #[test]
    fn vtodo_with_dtstart_stays_todo() {
        // Non-standard VTODO carrying DTSTART must remain a todo (explicit
        // component flag wins over dtstart-inference).
        let content = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:weird-1\r\nSUMMARY:task-with-start\r\nDTSTART;TZID=Asia/Shanghai:20260901T090000\r\nDUE;TZID=Asia/Shanghai:20260902T090000\r\nDTSTAMP:20260824T134610Z\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let t = parse_ics(content).unwrap();
        assert!(!t.event);
        assert!(!t.is_event());
        // ...and renders back as VTODO without DTSTART.
        let out = render_ics(&t, chrono_tz::Tz::Asia__Shanghai).unwrap();
        assert!(out.contains("BEGIN:VTODO"));
        assert!(!out.contains("DTSTART"));
        assert!(out.contains("DUE;TZID=Asia/Shanghai"));
    }

    #[test]
    fn legacy_jsonl_without_event_flag_infers() {
        // Old JSONL records have no `event` field: deserialise with default
        // false, then infer event from dtstart-without-due.
        let json = r#"{"uid":"u1","source":"work","summary":"old event","description":null,"status":"pending","priority":null,"tags":[],"due":null,"percent_complete":null,"completed_at":null,"dtstart":"2026-09-01T02:00:00Z","dtend":null,"rrule":null,"location":null,"allday":false,"alarm_before":null,"related_to":null,"wait":null,"exdates":[],"recurrence_id":null,"parent_uid":null,"created_at":"2026-08-01T00:00:00Z","updated_at":"2026-08-01T00:00:00Z"}"#;
        let t: Task = serde_json::from_str(json).unwrap();
        assert!(!t.event);          // legacy: field absent → false
        assert!(t.is_event());      // but dtstart-without-due infers event
        // Old VTODO with due stays todo even with dtstart (due present).
        let json2 = json.replace("\"due\":null", "\"due\":\"2026-09-02T02:00:00Z\"");
        let t2: Task = serde_json::from_str(&json2).unwrap();
        assert!(!t2.is_event());
    }

    #[test]
    fn render_parent_emits_rrule_and_exdate() {
        let mut t = Task::new("work", "standup");
        t.event = true;
        t.status = TaskStatus::Recurring;
        t.dtstart = Some(chrono::Utc::now());
        t.rrule = Some("FREQ=WEEKLY;BYDAY=TU".to_string());
        t.exdates.push(chrono::Utc::now() + chrono::Duration::days(7));
        let out = render_ics(&t, chrono_tz::Tz::Asia__Shanghai).unwrap();
        assert!(out.contains("RRULE:FREQ=WEEKLY;BYDAY=TU"));
        assert!(out.contains("EXDATE;TZID=Asia/Shanghai:"));
        assert!(out.contains("STATUS:CONFIRMED"));
    }

    #[test]
    fn render_override_uses_parent_uid_and_recurrence_id() {
        let mut t = Task::new("work", "rescheduled");
        t.status = TaskStatus::Pending;
        t.dtstart = Some(chrono::Utc::now());
        t.parent_uid = Some("series-1".to_string());
        t.recurrence_id = Some(chrono::Utc::now() - chrono::Duration::days(7));
        let out = render_ics(&t, chrono_tz::Tz::Asia__Shanghai).unwrap();
        assert!(out.contains("UID:series-1"));
        assert!(out.contains("RECURRENCE-ID;TZID=Asia/Shanghai:"));
        assert!(!out.contains("RRULE:"));
    }

    #[test]
    fn vevent_completed_roundtrips_via_xcalman_status() {
        let mut t = Task::new("work", "done event");
        t.event = true;
        t.status = TaskStatus::Completed;
        t.dtstart = Some(chrono::Utc::now());
        let out = render_ics(&t, chrono_tz::Tz::UTC).unwrap();
        // VEVENT STATUS stays CONFIRMED (RFC legal), private prop keeps state.
        assert!(out.contains("STATUS:CONFIRMED"));
        assert!(out.contains("X-CALMAN-STATUS:COMPLETED"));
        let rt = parse_ics(&out).unwrap();
        assert_eq!(rt.status, TaskStatus::Completed);
        assert!(rt.event);
    }

    #[test]
    fn master_and_override_coexist_independently_updatable() {
        // Regression: override files carry the master's ICS UID (RECURRENCE-ID
        // semantics); the storage key must be the filename, otherwise
        // update/remove hit the wrong task.
        let dir = tempdir().unwrap();
        let mut master = Task::new("work", "series");
        master.event = true;
        master.status = TaskStatus::Recurring;
        master.dtstart = Some(chrono::Utc::now());
        master.rrule = Some("FREQ=WEEKLY".to_string());
        master.summary = "master-summary".to_string();
        let master_uid = master.uid.clone();

        let mut st = IcsStorage::open(dir.path(), Tz::UTC).unwrap();
        st.add(master).unwrap();
        let occ = chrono::Utc::now() + chrono::Duration::days(7);
        let mut ov = crate::cli::override_for_occurrence(
            st.list().iter().find(|t| t.uid == master_uid).unwrap(),
            occ,
            crate::model::TaskStatus::Completed,
        );
        ov.summary = "override-summary".to_string();
        st.add(ov).unwrap();
        drop(st);

        // Reload: both tasks present, distinct storage keys, master intact.
        let st = IcsStorage::open(dir.path(), Tz::UTC).unwrap();
        assert_eq!(st.list().len(), 2);
        let ov_in = st
            .list()
            .iter()
            .find(|t| t.summary == "override-summary")
            .unwrap();
        assert_ne!(ov_in.uid, master_uid);
        assert_eq!(ov_in.parent_uid.as_deref(), Some(master_uid.as_str()));

        // Updating the master must not touch the override.
        let mut st = IcsStorage::open(dir.path(), Tz::UTC).unwrap();
        st.update(&master_uid, |t| {
            t.summary = "master-v2".to_string();
            Ok(())
        })
        .unwrap();
        assert!(st.list().iter().any(|t| t.summary == "master-v2"));
        assert!(st.list().iter().any(|t| t.summary == "override-summary"));
    }

    #[test]
    fn escape_unescape_roundtrip_preserves_backslashes_and_newlines() {
        let s = "a\\b\nline2;comma,ok";
        let esc = escape_text(s);
        assert_eq!(unescape_text(&esc), s);
    }

    #[test]
    fn override_does_not_shadow_master_in_uid_map() {
        // Override files reuse the master's ICS UID. `open` must resolve the
        // override's parent_uid to the master's storage key (filename), not to
        // the override's own file — otherwise per-occurrence skip/filtering
        // breaks (completed occurrence keeps showing as pending).
        let dir = tempdir().unwrap();
        let master = concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\n",
            "UID:same-uid\r\nSUMMARY:series\r\n",
            "DUE;TZID=Asia/Shanghai:20260831T220000\r\n",
            "RRULE:FREQ=DAILY;COUNT=5\r\n",
            "DTSTAMP:20260831T120000Z\r\nEND:VTODO\r\nEND:VCALENDAR\r\n",
        );
        let override_ = concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\n",
            "UID:same-uid\r\nSUMMARY:series\r\nSTATUS:COMPLETED\r\n",
            "DUE;TZID=Asia/Shanghai:20260831T220000\r\n",
            "RECURRENCE-ID;TZID=Asia/Shanghai:20260831T220000\r\n",
            "DTSTAMP:20260831T120100Z\r\nEND:VTODO\r\nEND:VCALENDAR\r\n",
        );
        std::fs::write(dir.path().join("a-master.ics"), master).unwrap();
        std::fs::write(dir.path().join("z-override.ics"), override_).unwrap();

        let st = IcsStorage::open(dir.path(), Tz::UTC).unwrap();
        let mv = st
            .list()
            .iter()
            .find(|t| t.recurrence_id.is_none())
            .unwrap();
        let ov = st
            .list()
            .iter()
            .find(|t| t.recurrence_id.is_some())
            .unwrap();
        assert_eq!(mv.uid, "a-master");
        assert_eq!(ov.parent_uid.as_deref(), Some("a-master"));
    }
}
