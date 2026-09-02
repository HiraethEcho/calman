//! End-to-end CLI tests: filter grammar, rc report overrides, default reports.
//! 集成测试 (integration tests)：把 cargo 构建出的 calman 二进制当成外部进程运行，
//! 在临时目录 (temp dir) 里端到端验证真实 CLI 行为。
//! tests/ 目录下的每个 .rs 文件都会编译成独立的测试 crate (test crate)。
#![cfg(feature = "storage-jsonl")]

use std::process::Command;
use tempfile::tempdir;

/// 运行 calman 二进制（端到端），返回 (stdout 文本, 是否成功)。
/// `env!("CARGO_BIN_EXE_calman")` 是 cargo 编译期注入的变量，
/// 指向当前 crate 构建出的 calman 可执行文件路径。
/// 测试把 HOME 指向临时目录，calman 的配置/数据都会写进临时目录，
/// 避免污染开发机的真实用户目录；同时移除 XDG_CONFIG_HOME 防止读到真实配置。
fn calman(home: &std::path::Path, args: &[&str]) -> (String, bool) {
    // `Command` 用于启动外部进程 (spawn an external process)。
    let out = Command::new(env!("CARGO_BIN_EXE_calman"))
        .args(args)
        // 覆盖 HOME 环境变量 → calman 使用临时配置/数据目录 (temp dir isolation)。
        .env("HOME", home)
        // 移除该变量 → 不会读到开发机上的真实配置 (isolate from real config)。
        .env_remove("XDG_CONFIG_HOME")
        // output() 同步等待进程结束并捕获输出；unwrap() 失败时 panic，测试直接失败。
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
    // 手工写入 config.toml：把 write_source 指向 ics-dir 集合 (composite source)。
    let cfg_dir = home.join(".config").join("calman");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    let ics_root = dir.path().join("ics");
    std::fs::create_dir_all(ics_root.join("sorge")).unwrap();
    // 空目录里放占位文件 .keep.ics，让 ics-dir 能识别这个集合 (placeholder marker)。
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

    // 关键行为：仅含日期的 due 在当天不算逾期 (fixed behavior)。
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
    // ANSI 转义序列 (escape codes) 用来给终端文本上色；断言前先剥掉，只比较纯文本。
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
    // 默认配置会把任务写到 `work` 这个 jsonl source（默认 config 行为）。
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

