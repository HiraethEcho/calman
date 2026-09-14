//! End-to-end CLI tests: filter grammar, rc report overrides, default reports.
#![cfg(feature = "storage-jsonl")]

use std::process::Command;
use tempfile::tempdir;

fn calman(home: &std::path::Path, args: &[&str]) -> (String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_calman"))
        .args(args)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        out.status.success(),
    )
}

#[test]
fn default_report_hides_past_events_and_rc_override_works() {
    let dir = tempdir().unwrap();
    let home = dir.path();

    assert!(calman(home, &["add", "todo task", "due:tomorrow"]).1);
    assert!(calman(home, &["add", "future event", "from:tomorrow"]).1);
    assert!(calman(home, &["add", "past event", "from:20260101"]).1);

    let (out, ok) = calman(home, &["next"]);
    assert!(ok);
    assert!(out.contains("todo task"));
    assert!(!out.contains("past event"));

    let (out, ok) = calman(
        home,
        &[
            "rc.report.next.columns=id,date,summary",
            "rc.report.next.labels=ID,DATE,TASK",
            "next",
        ],
    );
    assert!(ok);
    assert!(out.contains("TASK"));
    assert!(out.contains("todo task"));
}

#[test]
fn count_source_exclusion() {
    let dir = tempdir().unwrap();
    let home = dir.path();
    assert!(calman(home, &["add", "x", "due:tomorrow"]).1);

    let (out, ok) = calman(home, &["count", "-source:work"]);
    assert!(ok);
    assert_eq!(out.trim(), "0");
}

#[test]
fn add_to_ics_dir_collection_resolves_composite_source() {
    let dir = tempdir().unwrap();
    let home = dir.path();
    let cfg_dir = home.join(".config").join("calman");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    let ics_root = dir.path().join("ics");
    std::fs::create_dir_all(ics_root.join("sorge")).unwrap();
    std::fs::write(ics_root.join("sorge").join(".keep.ics"), "").unwrap();
    std::fs::write(
        cfg_dir.join("config.toml"),
        format!(
            "[defaults]\nwrite_source = \"remote/sorge\"\n[[source]]\nname = \"remote\"\ntype = \"ics-dir\"\nlocation = \"{}\"\n",
            ics_root.display()
        ),
    )
    .unwrap();

    let (_, ok) = calman(
        home,
        &["add", "ev", "due:tomorrow", "pri:H", "source:remote/sorge"],
    );
    assert!(
        ok,
        "add to ics-dir collection should resolve composite source"
    );

    let (out, ok) = calman(home, &["list", "source:remote/sorge"]);
    assert!(ok);
    assert!(out.contains("ev"));

    // default write source is the same collection, no source arg needed
    let (_, ok) = calman(home, &["add", "ev2", "due:tomorrow"]);
    assert!(ok);
    let (out, _) = calman(home, &["list", "source:remote/sorge"]);
    assert!(out.contains("ev2"));
}

#[test]
fn date_only_due_overdue_respects_config() {
    let dir = tempdir().unwrap();
    let home = dir.path();
    let cfg_dir = home.join(".config").join("calman");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    let store = dir.path().join("work");
    let cfg = format!(
        "[defaults]\nwrite_source = \"work\"\n[[source]]\nname = \"work\"\ntype = \"jsonl\"\nlocation = \"{}\"\n",
        store.display()
    );

    // Date-only due is NOT overdue on its own day (fixed behavior).
    std::fs::write(cfg_dir.join("config.toml"), &cfg).unwrap();
    assert!(calman(home, &["add", "d", "due:today"]).1);
    let (out, _) = calman(home, &["+OVERDUE", "count"]);
    assert_eq!(out.trim(), "0");

    // A past date-only due is overdue.
    assert!(calman(home, &["add", "e", "due:-1d"]).1);
    let (out, _) = calman(home, &["+OVERDUE", "count"]);
    assert_eq!(out.trim(), "1");
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            for ch in it.by_ref() {
                if ch == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn modify_date_only_start_becomes_allday() {
    let dir = tempdir().unwrap();
    let home = dir.path();
    // Default config writes to the `work` jsonl source.
    assert!(calman(home, &["add", "evt", "from:T0900", "for:1h"]).1);
    let (out, _) = calman(home, &["list"]);
    let id = strip_ansi(&out)
        .lines()
        .find(|l| l.contains("evt"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    assert!(calman(home, &["modify", &id, "from:20260828"]).1);
    let jsonl = std::fs::read_to_string(home.join(".local/share/calman/work/tasks.jsonl")).unwrap();
    let line = jsonl.lines().find(|l| l.contains("\"summary\":\"evt\"")).unwrap();
    assert!(line.contains("\"allday\":true"), "expected all-day: {line}");
    assert!(line.contains("\"dtend\":null"), "expected dtend cleared: {line}");
}

#[cfg(feature = "recur-expand")]
#[test]
fn done_occurrence_writes_ios_style_completed_copy_and_rolls_master() {
    let dir = tempdir().unwrap();
    let home = dir.path();

    // Timed recurring todo anchored at +2d (FREQ=DAILY).
    assert!(calman(home, &["add", "week", "recur:daily", "due:+2d"]).1);
    let (out, ok) = calman(home, &["list"]);
    assert!(ok, "list failed: {out}");
    let id = strip_ansi(&out)
        .lines()
        .find(|l| l.contains("week"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    assert!(calman(home, &["done", &id]).1, "done {id} failed");

    let jsonl = std::fs::read_to_string(home.join(".local/share/calman/work/tasks.jsonl")).unwrap();
    let rows: Vec<serde_json::Value> = jsonl
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

    // Master: keeps RRULE, its anchor (due) advanced by exactly one day.
    let master = rows
        .iter()
        .find(|r| r["rrule"].as_str().is_some())
        .expect("master row");
    // Completed copy: standalone, iOS-style.
    let copy = rows
        .iter()
        .find(|r| r["status"] == "completed" && r["rrule"].is_null())
        .expect("completed copy");
    assert!(copy["recurrence_id"].is_null(), "no RECURRENCE-ID");
    assert!(copy["parent_uid"].is_null(), "standalone, no parent link");
    assert_eq!(copy["percent_complete"], 100);
    assert!(copy["dtstart"].is_string(), "DTSTART written on the copy");

    let copy_due = chrono::DateTime::parse_from_rfc3339(copy["due"].as_str().unwrap()).unwrap();
    let master_due =
        chrono::DateTime::parse_from_rfc3339(master["due"].as_str().unwrap()).unwrap();
    assert_eq!(master_due - copy_due, chrono::Duration::days(1));
}

