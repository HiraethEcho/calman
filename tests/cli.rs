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

    // Default: date-only due is NOT overdue on its own day.
    std::fs::write(cfg_dir.join("config.toml"), &cfg).unwrap();
    assert!(calman(home, &["add", "d", "due:today"]).1);
    let (out, _) = calman(home, &["+OVERDUE", "count"]);
    assert_eq!(out.trim(), "0");

    // Toggle on: date-only due counts as overdue starting that day.
    let cfg_on = format!("{}\n[date]\ndue_date_overdue_today = true\n", cfg);
    std::fs::write(cfg_dir.join("config.toml"), cfg_on).unwrap();
    let (out, _) = calman(home, &["+OVERDUE", "count"]);
    assert_eq!(out.trim(), "1");
}
