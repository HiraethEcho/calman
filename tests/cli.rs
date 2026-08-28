//! End-to-end CLI tests: filter grammar, rc report overrides, default reports.

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
    assert!(calman(home, &["add", "future event", "start:tomorrow"]).1);
    assert!(calman(home, &["add", "past event", "start:20260101"]).1);

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
