//! ICS 存储后端: 每个任务保存为一个 `<UID>.ics` 文件, 使用 VTODO/VEVENT 组件。
//! (ICS storage backend: one `.ics` file per task, using VTODO/VEVENT components.)
//!
//! ICS 是 RFC 5545 定义的 iCalendar 纯文本格式。顶层是
//! `BEGIN:VCALENDAR` / `END:VCALENDAR`, 中间是组件 (component):
//! `VEVENT`(事件) 或 `VTODO`(待办)。
//! 每个属性 (property) 是一行 `NAME;PARAM=VALUE:内容`,
//! 例如 `DUE;TZID=Asia/Shanghai:20260825T170000`。
//! 长行需要按 75 字节折叠 (line folding), 续行以空格开头;
//! 文本里的 `\` `;` `,` 和换行需要转义 (escaping)。
//! 时间有两种形态: `VALUE=DATE` 表示全天日期 (all-day, 无时分秒),
//! 否则是带时区的 `DATE-TIME`; 重复规则用 `RRULE` 表达,
//! 时区定义可能内嵌在 `VTIMEZONE` 块中。
//! 本模块实现核心 `Task` 字段与标准 iCalendar 属性之间的映射 (field mapping)。
//!
//! 结构: `IcsStorage` 负责读写文件; `render_ics` 把 `Task` 序列化为文本;
//! `parse_ics` 把文本解析回 `Task`; 其余辅助函数处理折叠、转义、日期格式。

use super::{Storage, atomic_write};
use crate::date::local_midnight;
use crate::model::{Task, TaskStatus};
use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;
use std::fs;
use std::path::{Path, PathBuf};

/// 以目录为根的文件存储: 每个任务一个 `.ics` 文件。
/// (Storage rooted at a source location, one `.ics` file per task.)
///
/// Rust 的 struct 把相关字段打包成一个类型; 这里把 `tasks` 缓存在内存,
/// 读写时才碰磁盘 —— 实现简单、速度快, 但多进程同时写需要外部锁。
pub struct IcsStorage {
    dir: PathBuf,
    tz: Tz,
    tasks: Vec<Task>,
}

/// `IcsStorage` 的方法实现块 (impl block): 定义该类型的关联行为。
impl IcsStorage {
    /// 打开(必要时创建)存储目录, 扫描其中所有 `.ics` 文件并解析为 `Task`。
    /// (Open/create the storage dir, read every `*.ics` file, parse each into a `Task`.)
    ///
    /// - `location`: 目录路径; `tz`: 默认 IANA 时区, 用于解析无 TZID 的时间。
    /// - `&Path` 是借用 (borrow): 只读路径, 不取得所有权 (ownership)。
    /// - `Result<Self>` 表示操作可能失败; `?` 运算符遇错立即向上返回 (error propagation)。
    pub fn open(location: &Path, tz: Tz) -> Result<Self> {
        fs::create_dir_all(location)
            .with_context(|| format!("create storage dir {}", location.display()))?;
        let mut tasks = Vec::new();
        // 第一遍 (first pass): 读取并解析每个文件, 把存储键从文件内 UID 改成文件名,
        // 同时记录 `文件内 UID -> 文件名` 的映射表。
        // 为什么需要映射: 外部 CalDAV 文件可能文件名 ≠ UID;
        // 而 override(单次改期)文件会沿用主事件的 UID,
        // 必须靠这张表把 `parent_uid` 解析到真正的主任务文件。
        let mut uid_to_file: std::collections::HashMap<String, String> = Default::default();
        let entries =
            fs::read_dir(location).with_context(|| format!("read dir {}", location.display()))?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            // 只处理 `.ics` 文件。`extension()` 返回 `Option`,
            // `and_then` 链式取出字符串后与 `Some("ics")` 比较;
            // 不是该扩展名就 `continue` 跳到下一个目录项。
            if path.extension().and_then(|e| e.to_str()) != Some("ics") {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path) {
                match parse_ics(&content) {
                    Ok(mut task) => {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            // 主文件(没有 RECURRENCE-ID)优先占位 UID→文件 映射:
                            // override 复用主事件的 ICS UID, 不能让 override 反客为主,
                            // 否则后续把 parent_uid 解析成 override 自己的文件。
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
        // 第二遍 (second pass): 把每个任务的 `parent_uid` 从文件内 UID
        // 换算成存储文件名, 使 override 能指向真正的主任务文件。
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

    /// 计算某个 uid 对应的文件路径: `<目录>/<uid>.ics`。
    /// (Return the file path for a uid.)
    /// `&self` 表示只借用 self(不改状态); `format!` 是字符串格式化宏。
    fn file_for(&self, uid: &str) -> PathBuf {
        self.dir.join(format!("{uid}.ics"))
    }
}

impl Storage for IcsStorage {
    /// 返回当前内存中全部任务的切片 (slice)。
    /// (Borrowed view of all tasks.) 返回 `&[Task]` 是借用而非拷贝。
    fn list(&self) -> &[Task] {
        &self.tasks
    }

    /// 新增任务: 先原子写入 ICS 文件, 成功后再加入内存列表。
    /// (Add: atomically write the file first, then push to memory.)
    /// 原子写入 (atomic_write) 防止写一半留下损坏文件。
    fn add(&mut self, task: Task) -> Result<()> {
        atomic_write(&self.file_for(&task.uid), render_ics(&task, self.tz)?.as_bytes())?;
        self.tasks.push(task);
        Ok(())
    }

    /// 按 uid 查找任务, 用闭包 `f` 修改它, 再写回文件并刷新 `updated_at`。
    /// (Update: find by uid, apply closure `f`, persist, bump `updated_at`.)
    /// `FnOnce` 闭包只能调用一次; `Option<Task>` 返回 `None` 表示 uid 不存在。
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

    /// 删除任务: 先从内存移除, 再删除对应的 `.ics` 文件(若存在)。
    /// (Remove from memory, then delete the file if present.)
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

/// 生成日期属性行: 全天用 `;VALUE=DATE` + `YYYYMMDD`, 带时间用 `;TZID=<时区>` + 本地时间。
/// (Render a date property: all-day `;VALUE=DATE` or timed `;TZID=` form.)
/// DUE/DTSTART/DTEND/EXDATE/RECURRENCE-ID 共用此格式化逻辑。
fn ics_date_prop(name: &str, d: DateTime<Utc>, allday: bool, tz: Tz) -> String {
    if allday {
        // 全天日期不带时区: 取本地时区的日期部分, 只保留 YYYYMMDD。
        format!("{name};VALUE=DATE:{}", d.with_timezone(&Local).format("%Y%m%d"))
    } else {
        format!("{name};TZID={}:{}", tz.name(), dt_local(d, tz))
    }
}

/// 按 RFC 5545 规则把长属性行折叠 (fold): 超过 75 字节时断开, 续行以空格开头。
/// (Fold lines at 75 octets; continuation lines start with a space.)
/// 注意: 这里按字符数近似字节数 (approximated via chars), 对纯 ASCII 属性足够。
fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut remaining = line;
    // 75 字节中折行空格占 1 字节, 所以每段正文最多 73 字符。
    // `char_indices` 给出每个 UTF-8 字符的字节偏移, 避免从字符中间切开。
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

/// 把 `Task` 序列化成完整的 VCALENDAR 文本(含折行)。
/// (Render a task as a full VCALENDAR document.)
///
/// 要点:
/// - 顶层固定写 `BEGIN:VCALENDAR` / `VERSION:2.0` / `CALSCALE:GREGORIAN` / `PRODID`。
/// - 组件由 `task.event` 决定: 事件 → `VEVENT`, 待办 → `VTODO`。
/// - 必填属性: `UID`、`SUMMARY`、`DTSTAMP`(RFC 5545 要求)、`STATUS`。
/// - 所有文本值经 `escape_text` 转义, 所有属性行经 `fold` 折叠。
pub fn render_ics(task: &Task, tz: Tz) -> Result<String> {
    let mut lines = String::new();
    lines.push_str("BEGIN:VCALENDAR\r\n");
    lines.push_str("VERSION:2.0\r\n");
    lines.push_str("CALSCALE:GREGORIAN\r\n");
    lines.push_str("PRODID:-//calman//calman//EN\r\n");

    // 事件用 VEVENT, 待办用 VTODO; 两种组件允许的属性集合不同。
    let component = if task.event { "VEVENT" } else { "VTODO" };
    lines.push_str(&format!("BEGIN:{component}\r\n"));

    let mut props = Vec::new();
    // override 组件沿用主事件的 UID(CalDAV 约定); 存储键保存在 parent_uid。
    // `as_deref()` 把 `Option<String>` 变成 `Option<&str>`, 再 `unwrap_or` 回退到自己的 uid。
    let uid = task.parent_uid.as_deref().unwrap_or(&task.uid);
    props.push(format!("UID:{}", escape_text(uid)));
    props.push(format!("SUMMARY:{}", escape_text(&task.summary)));
    // DTSTAMP 是 RFC 5545 要求的属性; SEQUENCE 帮助 CalDAV 同步判断顺序。
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
    // VEVENT 只允许 TENTATIVE/CONFIRMED/CANCELLED 三种状态,
    // calman 的 Completed/InProgress 若直接写 STATUS 会变成 Pending 回来;
    // 因此用私有属性 X-CALMAN-STATUS 保存真实状态(其他客户端会忽略它)。
    if component == "VEVENT" && matches!(task.status, TaskStatus::Completed | TaskStatus::InProgress) {
        props.push(format!("X-CALMAN-STATUS:{}", status_to_ics(task.status)));
    }
    // iPhone 等标准客户端要求事件明确声明透明度 (transparency)。
    if component == "VEVENT" {
        props.push("TRANSP:OPAQUE".to_string());
    }
    if let Some(p) = task.priority {
        props.push(format!("PRIORITY:{p}"));
    }
    if !task.tags.is_empty() {
        // CATEGORIES 用逗号分隔多个标签; 每个标签先转义再 join。
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
    // RFC 5545 规定 VTODO 没有 DTSTART/DTEND —— 只有 VEVENT 才写它们。
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
    // 每个被排除的重复发生项 (excluded occurrence) 输出一条 EXDATE。
    for ex in &task.exdates {
        props.push(ics_date_prop("EXDATE", *ex, task.allday, tz));
    }
    // override 实例输出 RECURRENCE-ID, 标明它修改的是哪一次发生。
    if let Some(rid) = task.recurrence_id {
        props.push(ics_date_prop("RECURRENCE-ID", rid, task.allday, tz));
    }
    if let Some(l) = &task.location {
        props.push(format!("LOCATION:{}", escape_text(l)));
    }
    if let Some(w) = task.wait {
        props.push(format!("X-CALMAN-WAIT-OFFSET:{w}"));
    }
    if let Some(s) = task.started_at {
        props.push(ics_date_prop("X-CALMAN-STARTED", s, false, tz));
    }
    if let Some(d) = &task.description {
        props.push(format!("DESCRIPTION:{}", escape_text(d)));
    }
    if let Some(r) = &task.related_to {
        props.push(format!("RELATED-TO:{r}"));
    }

    // 最后把所有属性行折叠后写入, 再追加闹钟子组件 (VALARM)。
    for p in props {
        lines.push_str(&fold(&p));
    }
    if let Some(secs) = task.alarm_before {
        // VALARM 是嵌套在组件内的子块; TRIGGER 用负的 ISO 8601 时长表示“提前多少秒”。
        lines.push_str(&format!("BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT{secs}S\r\nDESCRIPTION:{}\r\nEND:VALARM\r\n", escape_text(&task.summary)));
    }
    lines.push_str(&format!("END:{component}\r\n"));
    lines.push_str("END:VCALENDAR\r\n");
    Ok(lines)
}

/// 把 VCALENDAR 文本解析成一个 `Task`。
/// (Parse a VCALENDAR document into a `Task`.)
///
/// 采用迭代式逐行解析 (iterative line loop): 先拆行, 再按属性名分发处理。
/// 局部变量用 `let mut` 在循环中累积状态; `Option` 字段以 `None` 表示“属性未出现”。
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
    let mut started_at = None;

    // 这些 `let mut` 变量逐行累积属性值; 循环结束后组装成 `Task`。
    // 例如 `due: Option<DateTime<Utc>>` 在遇到 DUE 属性前一直是 None。
    for raw in unfold(content) {
        // 属性行格式: `名字;参数=值:内容`。
        // 先按第一个冒号切出 `name`(含参数)和 `value`(内容);
        // `name.split(';').next()` 再取冒号前第一段作为属性名(如 DUE),
        // 参数(如 TZID=Asia/Shanghai)留在 name 中供 `param_tzid` 读取。
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
        // VTIMEZONE 块内部有自己的 DTSTART/RRULE 定义,
        // 必须跳过, 不能覆盖组件(VEVENT/VTODO)的真实属性 ——
        // CalDAV 服务器常用 VTIMEZONE 内嵌时区定义。
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
            // 先整体反转义, 再按逗号切分: 这样转义过的 `\,` 才不会误切成两个标签。
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
            "X-CALMAN-STARTED" => {
                let tz = param_tzid(name);
                started_at = match &tz {
                    Some(tz) => parse_tz(value, tz),
                    None => parse_dt(value),
                };
            }
            "EXDATE" => {
                // RFC 5545 允许 EXDATE 用逗号分隔多个被排除的时间。
                // 每个值再按“全天 / 带 TZID / 普通 UTC”三种情况分别解析。
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
                // override 组件的 UID 仍是主事件的 UID(CalDAV 约定);
                // 把当前 UID 记到 parent_uid, 存储时再用文件名作为自己的键。
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

    // 带 RRULE 的组件就是重复主任务 (recurring master)。
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
        started_at,
    })
}

/// 把 UTC 时间格式化成 ICS 的 UTC 时间戳 (basic format, 末尾带 Z)。
/// (Format a UTC instant as an ICS UTC timestamp ending in `Z`.)
/// 例如 `2026-08-25T02:00:00Z` → `20260825T020000Z`。
fn dt(d: DateTime<Utc>) -> String {
    d.to_rfc3339_opts(SecondsFormat::Secs, true)
        .replace(['-', ':', '+'], "")
        .trim_end_matches('Z')
        .to_string()
        + "Z"
}

/// 把 UTC 时刻换算成 `tz` 时区的本地挂钟时间, 不带 Z。
/// (Format a UTC instant as the local wall-clock time in `tz` (no `Z`).)
/// 这样的时间必须配合属性上的 `TZID=` 参数一起使用才有完整含义。
fn dt_local(d: DateTime<Utc>, tz: Tz) -> String {
    d.with_timezone(&tz).format("%Y%m%dT%H%M%S").to_string()
}

/// 解析末尾带 Z 的 UTC 时间戳, 形如 `20260825T020000Z`。
/// (Parse a UTC timestamp ending in `Z`.) 失败时返回 `None` 而不是报错。
fn parse_dt(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%SZ") {
        return Some(DateTime::from_naive_utc_and_offset(naive, chrono::Utc));
    }
    None
}

/// 解析 `VALUE=DATE` 的纯日期值: 当作本地时区的当天零点 (midnight) 存入 UTC。
/// (Parse `VALUE=DATE` date-only values as local midnight UTC.)
/// ICS 全天事件不含时区, calman 统一用本地午夜作为内部表示。
fn parse_all_day(s: &str) -> Option<DateTime<Utc>> {
    let d = chrono::NaiveDate::parse_from_str(s.trim(), "%Y%m%d").ok()?;
    Some(local_midnight(d))
}

/// 从属性名里提取 `TZID=` 参数, 例如 `DTSTART;TZID=Asia/Shanghai` → `Asia/Shanghai`。
/// (Extract `TZID=` parameter from a property name.)
/// 用 `split(';')` 把参数逐段切开, `find_map` 只返回第一个匹配的 TZID。
fn param_tzid(name: &str) -> Option<String> {
    name.split(';')
        .find_map(|p| p.strip_prefix("TZID="))
        .map(|s| s.to_string())
}

/// 把带 IANA `TZID` 的本地时间解析成 UTC。
/// (Parse a local wall time with an IANA `TZID` into UTC.)
/// 未知/自定义 TZID 回退到系统本地时区 (no data loss on unusual files)。
fn parse_tz(value: &str, tzid: &str) -> Option<DateTime<Utc>> {
    let s = value.trim();
    let naive = NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M"))
        .ok()?;
    if let Ok(tz) = tzid.parse::<Tz>() {
        // DST 切换可能导致一个本地时间出现两次 (ambiguous) 或不存在;
        // `earliest()` 取第一次出现, 实在没有就按 UTC 解释, 避免解析失败。
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

/// 解析 VALARM 的触发时长, 如 `-PT15M`、`-PT900S`、`-P1D`, 统一换算成秒。
/// (Parse a VALARM trigger like `-PT15M`, `-PT900S`, `-P1D` → seconds.)
/// 负号表示“提前”, 返回负数供调用方使用。
fn parse_trigger(s: &str) -> Option<i64> {
    let s = s.trim().trim_start_matches('-');
    let s = s.strip_prefix('P')?;
    let (date_part, time_part) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    // ISO 8601 时长: `P` 后是日期部分(D=天/W=周), `T` 后是时间部分(H/M/S)。
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

/// 把 calman 状态映射成 ICS 的 VTODO STATUS 值。
/// (Map internal `TaskStatus` to the ICS VTODO `STATUS` value.)
fn status_to_ics(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Pending => "NEEDS-ACTION",
        TaskStatus::InProgress => "IN-PROCESS",
        TaskStatus::Recurring => "NEEDS-ACTION",
        TaskStatus::Completed => "COMPLETED",
        TaskStatus::Cancelled => "CANCELLED",
    }
}

/// VEVENT 只允许 TENTATIVE/CONFIRMED/CANCELLED(RFC 5545 限制)。
/// (VEVENT permits only TENTATIVE/CONFIRMED/CANCELLED.)
/// 因此非取消状态统一写成 CONFIRMED, 真实状态由 X-CALMAN-STATUS 保存。
fn event_status_to_ics(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Cancelled => "CANCELLED",
        TaskStatus::Recurring => "CONFIRMED",
        _ => "CONFIRMED",
    }
}

/// 解析 ICS STATUS 字符串回 calman 状态; 也兼容拼写 CANCELED。
/// (Parse an ICS `STATUS` value back to `TaskStatus`.)
fn status_from_ics(s: &str) -> TaskStatus {
    match s.to_uppercase().as_str() {
        "COMPLETED" => TaskStatus::Completed,
        "CANCELLED" | "CANCELED" => TaskStatus::Cancelled,
        "IN-PROCESS" => TaskStatus::InProgress,
        _ => TaskStatus::Pending,
    }
}

/// 按 RFC 5545 转义文本值: `\` → `\\`, `;` → `\;`, `,` → `\,`, 换行 → `\n`。
/// (Escape text per RFC 5545 so reserved characters survive round-trip.)
fn escape_text(s: &str) -> String {
    // 顺序重要: 必须先转义反斜杠, 否则后续替换产生的 `\` 会被误判。
    s.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

/// 反转义 ICS 文本: 把 `\n` `\,` `\;` `\\` 还原成真实字符。
/// (Unescape RFC 5545 text values.)
/// 用迭代器逐个字符扫描, 遇到反斜杠时看下一个字符决定还原成什么。
fn unescape_text(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // 反斜杠后跟 n/逗号/分号/反斜杠 → 还原; 未知序列保留原样(宽容解析)。
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

/// 把 RFC 5545 折叠行展开 (unfold): 去掉续行开头的空格, 拼回完整逻辑行。
/// (Unfold folded lines, dropping continuation spaces.)
/// 这是读取 ICS 的第一步: 先合并续行, 才能按冒号正确切分属性。
fn unfold(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    // 以空格开头的行是上一行的续行 (continuation line), 去掉空格后拼接。
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

// 测试模块: `#[cfg(test)]` 只在 `cargo test` 时编译, 正常构建会忽略。
// (Test module: compiled only under `cargo test`; ordinary builds skip it.)
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

    #[test]
    fn started_at_roundtrips_via_xcalman_started() {
        let mut t = make_task();
        let now = chrono::DateTime::from_timestamp(chrono::Utc::now().timestamp(), 0).unwrap(); // ICS stores seconds
        t.started_at = Some(now);
        let out = render_ics(&t, Tz::UTC).unwrap();
        assert!(out.contains("X-CALMAN-STARTED"));
        let back = parse_ics(&out).unwrap();
        assert_eq!(back.started_at, t.started_at);
        // Unstarted tasks carry no property.
        let plain = parse_ics(&render_ics(&make_task(), Tz::UTC).unwrap()).unwrap();
        assert!(plain.started_at.is_none());
    }
}

