//! # 命令行参数解析（Taskwarrior/dstask 风格）
//!
//! 中文说明：本模块逐个扫描命令行 token，解析为 [`ParsedArgs`]。
//! [`ParsedArgs`] 含命令枚举 [`Command`] 与各字段：来源、ID、文本、优先级、日期、标签、过滤 token 等。
//! 识别顺序：先命令词 → 再 ID → 再属性前缀（`due:`/`priority:` 等）→ 其余归入 `filter_tokens`/文本。
//!
//! English: free-form argument parsing in the Taskwarrior/dstask style.
//! Examples:
//! - `calman`                         → list (default)
//! - `calman add buy milk priority:H +home due:eod`
//! - `calman 1 modify new content pri:L -home due:20260824`
//! - `calman +OVERDUE list`
//! - `calman count status:pending`

use crate::date::{DateValue, local_midnight, parse_date_value};
use crate::model::{TaskStatus, priority_from_str};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};

/// 命令枚举：`None` 表示未给命令，默认走 `list` 报表。
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
    Info,
    Start,
    Stop,
    #[cfg(feature = "tui")]
    Tui,
    Help,
}

/// 一条 Taskwarrior 风格的 `rc.report.<name>.<key>=<value>` 覆盖项（来自命令行）。
/// A Taskwarrior-style `rc.report.<name>.<key>=<value>` override from argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RcReport {
    pub name: String,
    pub key: String,
    pub value: String,
}

/// 完整解析后的命令行结果：所有字段汇总到这里，供后续子命令使用。
/// Fully parsed command line.
#[derive(Debug, Clone, Default)]
pub struct ParsedArgs {
    pub cmd: Option<Command>,
    /// 当 `cmd == List` 时，记录是 `ls`/`list`/`next` 中的哪一个报表名。
    /// Listing subcommand name (`ls`/`list`/`next`) when `cmd == List`.
    pub report_name: Option<String>,
    pub sources: Vec<String>,
    pub ids: Vec<String>,
    pub text: String,
    pub priority: Option<u8>,
    pub due: Option<DateTime<Utc>>,
    /// `due:` 若给的是纯日期（无时间），则视为全天事件（all-day）。
    /// `due:` was given as a date-only value (all-day semantics).
    pub due_allday: bool,
    pub status: Option<TaskStatus>,
    pub tags: Vec<String>,
    pub anti_tags: Vec<String>,
    pub rel: Option<String>,
    /// 提醒表达式 `wait:<date>`/`wait:-1d`/`wait:PT12H`，在 `add`/`modify` 中
    /// 解析为相对任务日期的偏移量。
    /// Wait expression (`wait:<date>` / `wait:-1d` / `wait:PT12H`), resolved
    /// to an offset against the task's date in `add`/`modify`.
    pub wait: Option<String>,
    /// 定位某次重复发生：`on:<date>` 保存该次发生的原始 DTSTART。
    /// Occurrence addressing: original DTSTART of the target occurrence (`on:<date>`).
    pub occ_date: Option<DateTime<Utc>>,
    /// 收集到的 `rc.report.<name>.<key>=<value>` token（如 columns/labels）。
    /// `rc.report.<name>.<key>=<value>` tokens (e.g. columns/labels).
    pub rc_reports: Vec<RcReport>,
    /// 原始过滤 token（命令之后、非 rc、非 ID），供 `list`/`count` 过滤使用。
    /// Raw filter tokens (post-command, non-rc, non-id) for list/count.
    pub filter_tokens: Vec<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub location: Option<String>,
    pub repeat: Option<String>,
    pub description: Option<String>,
    /// `for:1h` / `for:45min` — 事件时长，`to:` 的替代写法。
    /// `for:1h` / `for:45min` — event length, alternative to `to:`.
    pub span: Option<String>,
    /// `allday` 或 `+allday` 标记：表示全天事件。
    /// `allday` or `+allday` flag.
    pub allday: bool,
    /// `all-future` 关键字：把针对某次发生的修改/删除应用到整个剩余序列（拆分/截断），
    /// 无需交互确认。
    /// `all-future` bare keyword: apply an occurrence modify/delete to the
    /// whole remaining series (split/truncate) without the interactive prompt.
    pub apply_all_future: bool,
    /// `alert:15min` — 开始/到期前的 VALARM 提前量。
    /// `alert:15min` — VALARM lead time before start/due.
    pub alert: Option<String>,
}

/// 自由文本属性的捕获状态：属性值可能出现在后续多个 token 上
/// （例如 `desc: "some more information"` 被拆成两个 token）。
/// Free-text attribute whose value may arrive on following tokens
/// (e.g. `desc: "some more information"`).
#[derive(Clone, Copy)]
enum Capture {
    Description,
    Location,
}

/// 若 `s` 以 `prefix`（忽略大小写）开头，则返回去掉前缀后的剩余部分；否则 `None`。
/// 中文：用 ASCII 大小写无关匹配判断前缀，避免依赖 locale，保证跨平台一致。
/// True if `t` begins a recognised attribute token — used to end capture mode.
/// Case-insensitive prefix strip (ASCII-safe for a locale-independent match).
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let n = prefix.len();
    (s.len() >= n && s.get(..n).is_some_and(|p| p.eq_ignore_ascii_case(prefix)))
        .then(|| &s[n..])
}

/// 判断一个 token 是否以已知属性前缀开头（用于结束 capture 模式）。
/// 中文：这里用 `to_ascii_lowercase()` 把 token 转小写再统一前缀判断，
/// 比逐字比较更简洁；也把 `+tag`/`-tag` 识别为属性类 token。
/// English: true if `t` begins a recognised attribute token.
fn is_attr_token(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    l.starts_with("rc.")
        || l.starts_with("source:")
        || l.starts_with("src:")
        || l.starts_with("due:")
        || l.starts_with("from:")
        || l.starts_with("to:")
        || l.starts_with("repeat:")
        || l.starts_with("recur:")
        || l.starts_with("for:")
        || l.starts_with("alert:")
        || l.starts_with("desc:")
        || l.starts_with("location:")
        || l.starts_with("rel:")
        || l.starts_with("wait:")
        || l.starts_with("count:")
        || l.starts_with("until:")
        || l == "all-future"
        || l.starts_with("status:")
        || l.starts_with("priority:")
        || l.starts_with("pri:")
        || (t.starts_with('+') && t.len() > 1)
        || (t.starts_with('-') && t.len() > 1)
}

/// 解析原始 argv（binary/全局参数之后）为结构化 `ParsedArgs`。
/// Parse raw argv (after the binary/global flags) into structured args.
pub fn parse(args: &[String]) -> Result<ParsedArgs> {
    let mut q = ParsedArgs::default();
    // ids_exhausted：一旦开始处理非 ID token，之后不再把数字当作 ID（Taskwarrior 行为）。
    let mut ids_exhausted = false;
    // literal：遇到 `--` 之后，所有 token 都当作纯文本，不再解析属性。
    let mut literal = false; // after `--`, everything is text
    // capture：正在收集 `desc:`/`location:` 等自由文本属性的值。
    let mut capture: Option<Capture> = None; // free-text attr value collection

    // 逐 token 扫描输入。for 循环对 `args`（&[String]）迭代，`tok` 是 &String。
    for tok in args {
        if literal {
            push_text(&mut q, tok);
            continue;
        }
        let lower = tok.to_ascii_lowercase();

        // capture 模式：`desc:`/`location:` 的值可能分布在后续 token 上，
        // 不断收集单词，直到遇到下一个属性 token 或 `--` 才停止。
        // 这里 match `target`（Option<Capture> 的可变借用）来选字段槽位。
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

        // 命令词只在 cmd 尚未确定时识别 → 允许 `+OVERDUE list` 过滤词在前。
        // `if let` + 守卫（`let` chains）：当且仅当尚无命令时才尝试匹配命令词。
        if q.cmd.is_none()
            && let Some((cmd, report)) = command_word(tok)
        {
            q.cmd = Some(cmd);
            q.report_name = report;
            continue;
        }

        // 全局 TW rc 开关（如 `rc.verbose=header`）接受但忽略。
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

        if !ids_exhausted {
            // ID 可以是纯数字（`5`），也可以是“发生 ID”（`5.2`，主任务.第几次）。
            // split_once('.') 拆成两段，两段都能 parse 成 usize 才算发生 ID。
            let is_occurrence_id = tok
                .split_once('.')
                .map(|(a, b)| a.parse::<usize>().is_ok() && b.parse::<usize>().is_ok())
                .unwrap_or(false);
            if tok.parse::<usize>().is_ok() || is_occurrence_id {
                q.ids.push(tok.clone());
                continue;
            }
        }

        // 先记录为过滤 token，再尝试解析成属性 → 同一 token 既可用于 `list`/`count`
        // 过滤，也可在 `add`/`modify` 中当作字段（date token 与 filter token 消歧）。
        q.filter_tokens.push(tok.clone());

        // 下面是一串 `else if` 属性前缀匹配（attribute scanning）。
        // 每个分支都 `strip_prefix` 出剩余值并填入 `q` 的对应字段。
        if let Some(rest) = lower
            .strip_prefix("priority:")
            .or_else(|| lower.strip_prefix("pri:"))
        {
            q.priority = Some(parse_priority(rest)?);
        } else if let Some(len) = strip_prefix_ci(&lower, "source:").map(|_| 7).or_else(|| strip_prefix_ci(&lower, "src:").map(|_| 4)) {
            q.sources
                .extend(tok[len..].split(',').map(|s| s.trim().to_string()));
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
        } else if let Some(rest) = lower.strip_prefix("on:") {
            let dv = parse_date_value(rest)?;
            q.occ_date = Some(match dv {
                DateValue::Date(d) => local_midnight(d),
                DateValue::Time(dt) => dt,
            });
        } else if let Some(rest) = lower.strip_prefix("rel:") {
            q.rel = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("wait:") {
            q.wait = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("from:") {
            q.from = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("to:") {
            q.to = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("location:") {
            if rest.is_empty() {
                q.location = Some(String::new());
                capture = Some(Capture::Location);
            } else {
                q.location = Some(rest.to_string());
            }
        } else if let Some(rest) = lower.strip_prefix("count:") {
            // `recur:daily count:5` — series length lands on the recur rule.
            match &mut q.repeat {
                Some(r) => {
                    r.push_str(" count:");
                    r.push_str(rest);
                }
                None => bail!("`count:` requires `recur:`/`repeat:`"),
            }
        } else if let Some(rest) = lower.strip_prefix("until:") {
            match &mut q.repeat {
                Some(r) => {
                    r.push_str(" until:");
                    r.push_str(rest);
                }
                None => bail!("`until:` requires `recur:`/`repeat:`"),
            }
        } else if let Some(rest) = lower.strip_prefix("repeat:")
            .or_else(|| lower.strip_prefix("recur:"))
        {
            q.repeat = Some(rest.to_string());
        } else if let Some(rest) = lower.strip_prefix("for:") {
            q.span = Some(rest.to_string());
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
        } else if lower == "all-future" {
            q.apply_all_future = true;
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

        // 处理完第一个非命令、非 ID token 后，ID 收集阶段结束。
        ids_exhausted = true;
    }

    Ok(q)
}

/// 虚拟/过滤 token：`add`/`modify` 不能把这些当作字面标签，但它们仍会进 `filter_tokens`
/// 供 `list`/`count` 使用。
/// Filter/virtual tokens that `add`/`modify` must not treat as literal tags.
/// They are still recorded in `filter_tokens` for `list`/`count`.
fn is_filter_only_plus(name: &str) -> bool {
    is_virtual_tag(&name.to_ascii_lowercase())
}

/// 虚拟标签表：`+PENDING`、`+overdue` 等由过滤引擎识别的“伪标签”。
/// 中文：`matches!` 宏用一张字面量列表做匹配，比一连串 `==` 更紧凑。
/// English: virtual tag vocabulary used by the filter engine.
fn is_virtual_tag(l: &str) -> bool {
    matches!(
        l,
        "overdue"
            | "pending"
            | "completed"
            | "done"
            | "cancelled"
            | "canceled"
            | "in-progress"
            | "inprogress"
            | "in-process"
            | "inprocess"
            | "started"
            | "tagged"
            | "untagged"
            | "due"
            | "todo"
            | "event"
            | "parent"
            | "recurring"
            | "waiting"
    )
}

/// `-xxx` 过滤 token 中不应当作“反标签”的部分（虚拟标签、`-status:`/`-due:` 等）。
/// English: filter-only tokens for the minus (`-`) prefix that must not become anti-tags.
fn is_filter_only_minus(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    is_virtual_tag(&l)
        || strip_prefix_ci(&l, "status:").is_some()
        || strip_prefix_ci(&l, "source:").is_some()
        || strip_prefix_ci(&l, "src:").is_some()
        || strip_prefix_ci(&l, "type:").is_some()
        || strip_prefix_ci(&l, "priority:").is_some()
        || strip_prefix_ci(&l, "pri:").is_some()
        || strip_prefix_ci(&l, "date").is_some()
        || strip_prefix_ci(&l, "from").is_some()
        || strip_prefix_ci(&l, "due").is_some()
}

/// 解析 `rc.report.<name>.<key>=<value>` 为 [`RcReport`]；非该前缀返回 `Ok(None)`。
/// 中文：先定位 `=` 拆出 name_key 与 value，再对 name_key 用 `.` 拆 name/key。
/// 返回 `Result`：格式不对时 `bail!` 报错。
/// English: parse a report rc override token.
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

/// 把命令词（`add`/`list`/`done`…）映射到 [`Command`]；非命令词返回 `None`。
/// 中文：用 `match` 对转小写的词分派，并返回可选的报表名。
/// English: recognise a leading command word into a `Command`.
fn command_word(tok: &str) -> Option<(Command, Option<String>)> {
    let (cmd, report) = match tok.to_ascii_lowercase().as_str() {
        "add" => (Command::Add, None),
        "list" => (Command::List, Some("list".to_string())),
        "ls" => (Command::List, Some("ls".to_string())),
        "next" => (Command::List, Some("next".to_string())),
        "done" | "complete" => (Command::Done, None),
        "start" => (Command::Start, None),
        "stop" => (Command::Stop, None),
        "delete" | "rm" => (Command::Delete, None),
        "modify" | "mod" => (Command::Modify, None),
        "info" => (Command::Info, None),
        "count" => (Command::Count, None),
        "sync" => (Command::Sync, None),
        #[cfg(feature = "tui")]
        "tui" => (Command::Tui, None),
        "help" | "filters" => (Command::Help, None),
        _ => return None,
    };
    Some((cmd, report))
}

/// 解析优先级字符串（H/M/L 或 0-9）为 `u8`；非法值返回错误。
/// English: parse a priority value into a number.
fn parse_priority(v: &str) -> Result<u8> {
    priority_from_str(v).ok_or_else(|| anyhow::anyhow!("bad priority `{v}` (use H, M, L or 0-9)"))
}

/// 解析状态字符串为 [`TaskStatus`]；未知状态报错。
/// 中文：`match` 把多个同义词（如 `in-progress`/`started`）映射到同一枚举变体。
/// English: parse a status string into a `TaskStatus`.
fn parse_status(v: &str) -> Result<TaskStatus> {
    Ok(match v.to_ascii_lowercase().as_str() {
        "pending" => TaskStatus::Pending,
        "in-progress" | "inprogress" | "in-process" | "inprocess" | "started" => {
            TaskStatus::InProgress
        }
        "completed" | "done" => TaskStatus::Completed,
        "cancelled" | "canceled" => TaskStatus::Cancelled,
        "recurring" => TaskStatus::Recurring,
        _ => bail!("unknown status `{v}`"),
    })
}

/// 把 token 追加到任务文本；首个词作为开头，后续词以空格连接。
/// English: append a token to the free-text task description.
fn push_text(q: &mut ParsedArgs, tok: &str) {
    if q.text.is_empty() {
        q.text = tok.to_string();
    } else {
        q.text.push(' ');
        q.text.push_str(tok);
    }
}

#[cfg(test)]
// 测试模块：仅验证解析逻辑，解释从略（按规范保持原样）。
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
        assert_eq!(q.priority, Some(1));
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
            "from:0826T0900",
            "for:45min",
            "alert:15min",
            "+team",
        ]);
        assert_eq!(q.span.as_deref(), Some("45min"));
        assert_eq!(q.alert.as_deref(), Some("15min"));
        let q2 = p(&["add", "Conf", "from:20260826", "allday"]);
        assert!(q2.allday);
    }

    #[test]
    fn for_duration() {
        let q = p(&["add", "x", "from:25T0930", "for:1h"]);
        assert_eq!(q.span.as_deref(), Some("1h"));
    }

    #[test]
    fn old_event_syntax_is_plain_text() {
        // Renamed tokens are no longer special: they flow into task text.
        let q = p(&["add", "x", "start:20260901"]);
        assert_eq!(q.text, "x start:20260901");
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
        assert_eq!(q.priority, Some(9));
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
        let q = p(&["add", "x", "+overdue", "+home", "-due"]);
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
    fn info_command_both_orders() {
        let q = p(&["info", "3"]);
        assert_eq!(q.cmd, Some(Command::Info));
        assert_eq!(q.ids, vec!["3"]);
        let q2 = p(&["3", "info"]);
        assert_eq!(q2.cmd, Some(Command::Info));
        assert_eq!(q2.ids, vec!["3"]);
    }

    #[test]
    fn start_stop_commands() {
        let q = p(&["start", "1", "3"]);
        assert_eq!(q.cmd, Some(Command::Start));
        assert_eq!(q.ids, vec!["1", "3"]);
        let q2 = p(&["stop", "1"]);
        assert_eq!(q2.cmd, Some(Command::Stop));
        assert_eq!(q2.ids, vec!["1"]);
    }

    #[test]
    fn all_future_keyword() {
        let q = p(&["modify", "5.2", "all-future", "due:tomorrow"]);
        assert!(q.apply_all_future);
        assert_eq!(q.ids, vec!["5.2"]);
        assert_eq!(q.text, "");
    }

    #[test]
    fn recur_is_alias_for_repeat() {
        let q = p(&["add", "x", "recur:daily"]);
        assert_eq!(q.repeat.as_deref(), Some("daily"));
        let q2 = p(&["add", "x", "repeat:weekly"]);
        assert_eq!(q2.repeat.as_deref(), Some("weekly"));
    }

    #[test]
    fn count_and_until_attach_to_recur() {
        let q = p(&["add", "x", "recur:daily", "count:5"]);
        assert_eq!(q.repeat.as_deref(), Some("daily count:5"));
        let q2 = p(&["add", "x", "recur:daily", "until:eom"]);
        assert_eq!(q2.repeat.as_deref(), Some("daily until:eom"));
        assert!(parse(&["add", "x", "count:5"].map(String::from)).is_err());
    }
}

/// 把 `wait:` 表达式解析为相对 `anchor`（todo 用 due，event 用 dtstart）的秒数偏移。
///
/// 中文：日期形式（`2026-09-01`/`0826`/`T0900`…）→ `target - anchor`
/// （纯日期 = 本地零点，即该日 00:00 起可见）。
/// 时长形式（`-1d`/`PT12H`/`2w`/`1h30m`）→ 有符号秒数（负 = 日期之前，用于重复任务的逐次 wait）。
///
/// - Date forms (`2026-09-01`, `0826`, `T0900`, …) → `target - anchor`
///   (date-only = local midnight, i.e. visible from that day 00:00).
/// - Duration forms (`-1d`, `PT12H`, `2w`, `1h30m`) → signed seconds
///   (negative = before the date, for recurring per-occurrence waits).
pub fn resolve_wait(expr: &str, anchor: DateTime<Utc>) -> Result<i64> {
    let s = expr.trim();
    if s.is_empty() {
        bail!("empty wait");
    }
    // 先判断是否像“时长”：带正负号（`-1d`）、ISO（`PT12H`）、或以字母结尾（`2w`）。
    // 日期（`2026-09-01`/`0826`/`T0900`/`17`/`tomorrow`）则落到日期解析。
    let dur_first = s.starts_with(['-', '+'])
        || s.starts_with('P')
        || s.chars().last().is_some_and(|c| c.is_ascii_alphabetic());
    if dur_first {
        let (sign, body) = match s.strip_prefix(['-', '+']) {
            Some(b) => (if s.starts_with('-') { -1 } else { 1 }, b),
            None => (1, s),
        };
        if let Ok(d) = crate::date::parse_duration(body) {
            return Ok(d.num_seconds() * sign);
        }
    }
    let dv = parse_date_value(s)?;
    let target = match dv {
        DateValue::Date(d) => local_midnight(d),
        DateValue::Time(dt) => dt,
    };
    Ok(target.signed_duration_since(anchor).num_seconds())
}

#[cfg(test)]
// 测试模块：验证 `wait:` 各日期/时长形式，解释从略（保持原样）。
mod wait_tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn wait_date_forms() {
        let anchor = Utc.with_ymd_and_hms(2026, 8, 28, 12, 0, 0).unwrap();
        // date-only → local midnight of that day minus anchor
        let off = resolve_wait("2026-08-29", anchor).unwrap();
        assert!(off > 0);
        assert!(off < 86_400); // 20:00 local → next midnight
    }

    #[test]
    fn wait_duration_forms() {
        let anchor = Utc::now();
        assert_eq!(resolve_wait("-1d", anchor).unwrap(), -86_400);
        assert_eq!(resolve_wait("PT12H", anchor).unwrap(), 43_200);
        assert_eq!(resolve_wait("2w", anchor).unwrap(), 1_209_600);
    }

    #[test]
    fn wait_empty_rejected() {
        let anchor = Utc::now();
        assert!(resolve_wait("", anchor).is_err());
    }
}
