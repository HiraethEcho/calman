//! 子命令处理器与共享 CLI 辅助函数。
//! Subcommand handlers and shared CLI helpers.
//!
//! 数据流（Data flow）：解析目标 ID（parse target IDs）→ 打开存储（open storage）→ 应用变更（apply change）→ 保存（save）→ 打印结果（print）。
//! 各子命令共用本模块的“合并视图 + 短 ID”机制，共享逻辑集中在这里。

pub mod add;
pub mod count;
pub mod delete;
pub mod done;
pub mod info;
pub mod list;
pub mod modify;
pub mod series;
pub mod start;
pub mod stop;
pub mod sync;
#[cfg(feature = "tui")]
pub mod tui;

use crate::config::{Config, ContextKind, Source, SourceType};
use crate::model::{Task, TaskStatus};
use crate::source;
#[cfg(feature = "storage-ics")]
use crate::storage::ics::IcsStorage;
#[cfg(feature = "storage-jsonl")]
use crate::storage::jsonl::JsonlStorage;
use crate::storage::{Storage, Store};
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
#[cfg(feature = "recur-expand")]
use chrono::{Duration, Local};

/// 跨选定数据源合并后的一行，带动态短 ID。
/// One merged row across selected sources, with a dynamic short ID.
///
/// Rust 概念：`pub` 表示字段对外可见；`Option<usize>` 表示“可能有值，也可能没有”。
pub struct Row {
    pub id: usize,
    pub source: String,
    pub task: Task,
    /// 虚拟展开的重复事件序号（recur-expand）：显示为 `id.occ`。
    /// Virtual expanded occurrence index (recur-expand): renders as `id.occ`.
    pub occ: Option<usize>,
}

/// 打印给用户的过滤器/命令速查表（cheat-sheet）。
/// Filter cheat-sheet printed by `calman help` / `calman filters`.
pub fn print_filter_help() {
    print!(
        r#"calman — task & event manager (CLI)

COMMANDS
  calman add <text> [opts]        add todo (due:) or event (from:)
  calman list|ls|next [filter]   run a report (bare `calman` → next)
  calman done <id>               mark completed
  calman delete <id>             hard delete
  calman modify <id> [opts]      change fields
  calman info <id> | <id> info   show full details
  calman start <id>              record start time (→ in-progress)
  calman stop <id>               turn started todo into an event (todo done)
  calman count [filter]          print number of matches
  calman sync [source]           run external sync command
  calman help | filters          show this cheat-sheet

START / STOP (todo → event)
  calman start <id>        set started_at + status in-progress
  calman stop <id>         copy todo to an event [started_at, now], todo done
                           new event links back via rel:<todo-uid>

COMMON OPTIONS (add / modify)
  due:<date>        todo deadline (date-only → all-day todo)
  from:<date>       event start (date-only → all-day event)
  to:<date> for:<dur>  event end / length
  pri:H|M|L         priority (9/5/1)
  +tag -tag         tags
  source:<name>     write/list source (ics-dir: `name/collection`)
  rel:<id>          parent relation (RELATED-TO)
  recur:<rule>      recurrence (alias `repeat:`)
  location:<text> alert:<lead> desc:<text>
  wait:<date|dur> hide until <expr> relative to due/start (e.g. wait:+1d,
                    wait:sopd; hidden while `date + wait > now`, +WAITING shows)
  on:<date>         target one occurrence of a recurring series (needs `recur-expand`)
  <id>.<n>          nth upcoming occurrence (e.g. `done 5.2`, `modify 5.1 summary:x`)
                    expanded occurrences also get plain sequential IDs (`done 5` works)

RECURRENCE (recur: / repeat:)  → standard RFC 5545 RRULE
  raw passthrough : recur:FREQ=WEEKLY;BYDAY=TU,FR;UNTIL=20260925
  frequency        : daily weekly monthly yearly
  interval         : every 7d | 7d | every 2 weeks   (→ INTERVAL)
  weekdays         : every tuesday and friday | every weekend (→ BYDAY)
  end              : for 5 times | for 7 weeks (weeks×weekday→COUNT)
                     | count:5 | until:20260925 | until:eoy | until:eom
                     (`count:`/`until:` may be separate tokens: recur:daily count:5)
  e.g. every tuesday and friday for 7 weeks
       → FREQ=WEEKLY;BYDAY=TU,FR;COUNT=14
  series model     : master = status:recurring, virtual tag +PARENT
                     hidden from ls/list/next by default (show: `+PARENT`)
  occurrences      : done <id> on:<date> → Completed override record
                     delete <id> on:<date> → EXDATE (skip one)
                     modify <id>.<n> … → RECURRENCE-ID override (same UID)
                     expanded rows carry plain IDs; `done 5` targets one occurrence
                     `all-future` keyword on modify/delete an occurrence:
                       modify → split series (old keeps past, new edited series starts here)
                       delete → truncate series (this occurrence and all later removed)
                     interactive prompt (TTY) asks first; non-TTY defaults to single

DATE-ONLY DUE (fixed overdue policy)
  a date-only `due` is owed only AFTER its day passes (today's due is not overdue)
  stored as DUE;VALUE=DATE in ICS (iOS Reminders compatible)

FILTER GRAMMAR (shared by CLI args and report `filter`)
  type:todo | type:event | type:all        (+TODO / +EVENT aliases)
  source:work  -source:work                include / exclude a source
  due:<day> exact | due.before:<   strict < | due.by:<   <= | due.after:>=
  date:<day> (unified: todo→due, event→dtstart) + date.before:/date.by:/date.after:
  from:<day> exact | from.before:/from.by:/from.after:  (events' dtstart only)
  status:pending|in-progress|completed|cancelled|recurring|active
  +OVERDUE +PENDING +DUE +COMPLETED +CANCELLED +IN-PROCESS +STARTED +TAGGED +UNTAGGED +PARENT  +tag / -tag
  Composition: adjacent atoms = and; `and` binds tighter than `or`:
    A B or C D     = (A and B) or (C and D)
    (A or B) C     = (A or B) and C

SEPARATE EVENT / TODO
  calman type:event             future events only (report default still applies)
  calman type:todo +PENDING     active todos
  calman rc.report.next.filter='type:event' next   all events incl. past

REPORTS & OVERRIDES (Taskwarrior rc style)
  builtin: ls / list / next (bare `calman` → next)
  rc.report.<name>.columns=id,date,summary   custom columns (script-friendly)
  rc.report.<name>.labels=ID,DATE,TASK
  rc.report.<name>.filter=...  rc.report.<name>.sort=...

ICONS (nerdfont) — 3-level fallback: column `icons` > [icons.todo]/[icons.event] > builtin
  [icons.todo]   pending=○ in-progress=● completed=✓ cancelled=✕
  [icons.event]  pending=󰃭 (calendar; cancelled=✕)   event non-cancelled → calendar

SOURCES
  jsonl / ics / ics-dir (Radicale/vdirsync). ics-dir collections:
    source:remote           → expands all collections
    source:remote/sorge      → one collection (composite reference)

CONFIG (two tiers)
  config.default.toml  complete default reference — self-contained; lists every default option (what calman uses with no config file)
  config.example.toml  annotated custom sample (copy & edit)
  include = ["config.default.toml", "colorscheme.example.toml"]
"#
    );
}

/// 打开某个 source 对应的存储后端。
/// Open the storage backend for a source.
///
/// Rust 概念：`&Config` 是只读借用（borrow），函数只“借用”配置而不拿走所有权；
/// `Result<Store>` 表示可能成功（Ok）也可能失败（Err），调用方用 `?` 处理错误。
pub fn open_storage(conf: &Config, src: &Source) -> Result<Store> {
    let loc = src.abs_location();
    let tz = conf.date.tz();
    // match 是 Rust 的模式匹配（pattern matching）；这里按存储类型选择实现。
    // `#[cfg(feature = "...")]` 是编译期开关：未启用该 feature 时，这段代码不参与编译。
    Ok(match src.source_type {
        #[cfg(feature = "storage-jsonl")]
        SourceType::Jsonl => Store::Jsonl(JsonlStorage::open(&loc)?),
        #[cfg(not(feature = "storage-jsonl"))]
        SourceType::Jsonl => {
            bail!("this build was compiled without the `storage-jsonl` feature")
        }
        #[cfg(feature = "storage-ics")]
        SourceType::Ics => Store::Ics(IcsStorage::open(&loc, tz)?),
        #[cfg(not(feature = "storage-ics"))]
        SourceType::Ics => {
            bail!("this build was compiled without the `storage-ics` feature")
        }
        SourceType::IcsDir => {
            bail!(
                "IcsDir source `{}` must be resolved to a collection before opening storage",
                src.name
            )
        }
    })
}

/// 解析实际使用的数据源列表：命令行 `--source` 覆盖 > 上下文默认源。
/// Resolve the effective source list: `--source` override > context defaults.
///
/// 处理 `IcsDir` 展开（Handles `IcsDir` expansion）：
/// - `source:name/collection` → 单个虚拟 `Ics` source（只指一个集合）
/// - `source:name`（IcsDir 目录）→ 展开目录下所有集合
/// - `source:name`（普通 source）→ 直接使用
pub fn resolve_sources(
    conf: &Config,
    override_: Option<&[String]>,
    ctx: ContextKind,
) -> Result<Vec<Source>> {
    // 优先级：显式 `--source` > 当前 context 的默认源。
    // `Option<&[String]>`：有值（Some）或没有值（None）。
    let names: Vec<String> = match override_ {
        Some(ns) => ns.to_vec(),
        None => conf.context_sources(ctx),
    };
    // 逐个名字解析；IcsDir 可能展开出多个集合，所以用 extend 追加。
    // `?` 遇到 Err 会提前返回错误，调用方拿到提示。
    let mut out = Vec::new();
    for name in names {
        let resolved = source::resolve_source_name(&conf.sources, &name)?;
        out.extend(resolved);
    }
    if out.is_empty() {
        bail!("no sources selected");
    }
    Ok(out)
}

/// 把 `sources` 里的所有任务合并成 `Row`，并分配顺序短 ID。
/// Merge all tasks from `sources` into `Row`s with sequential short IDs.
///
/// Taskwarrior 风格编号：**最早创建**的任务是 ID 1。
/// 先按 `created_at` 排序（相同时用 UID 决胜），再分配 ID，
/// 所以 ID 不依赖存储的遍历顺序，跨会话保持稳定。
pub fn load_merged(conf: &Config, sources: &[Source]) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    // 遍历每个 source，打开存储，把任务全部收进内存列表。
    for src in sources {
        let st = open_storage(conf, src)?;
        for t in st.list() {
            let mut t = t.clone(); // clone：复制一份，避免动到存储里的原对象
            t.source = src.name.clone();
            rows.push(Row {
                id: 0, // 占位，排序后再填
                source: src.name.clone(),
                task: t,
                occ: None,
            });
        }
    }
    // 闭包（closure）`|a, b| { ... }` 是“一段可传的函数”；这里定义比较规则。
    // 先比创建时间，再比 UID —— 保证排序结果唯一、稳定。
    rows.sort_by(|a, b| {
        a.task
            .created_at
            .cmp(&b.task.created_at)
            .then_with(|| a.task.uid.cmp(&b.task.uid))
    });
    // enumerate() 同时给出下标 i 和元素；iter_mut() 允许修改每个元素。
    for (i, r) in rows.iter_mut().enumerate() {
        r.id = i + 1; // ID 从 1 开始
    }
    Ok(rows)
}

/// 同 [`load_merged`]，但把重复系列（recurring series）展开成虚拟 occurrence 行。
/// Like [`load_merged`], but with recurring series expanded into virtual
/// occurrence rows (Taskwarrior-style).
///
/// 真实行和虚拟行都分配连续整数 ID；occurrence 行额外带 `occ`，
/// 供 `on:<date>` / `id.n` 定位。未启用 `recur-expand` feature 时等价于 `load_merged`。
pub fn load_merged_expanded(conf: &Config, sources: &[Source]) -> Result<Vec<Row>> {
    #[cfg(feature = "recur-expand")]
    {
        let mut rows = load_merged(conf, sources)?;
        expand_occurrences(&mut rows, conf);
        Ok(rows)
    }
    #[cfg(not(feature = "recur-expand"))]
    {
        load_merged(conf, sources)
    }
}

/// 把重复父任务展开成虚拟 occurrence 行，并重新顺序编号（recur-expand）。
/// Expand recurring parents into virtual occurrence rows and renumber all
/// rows sequentially (recur-expand).
///
/// `&mut Vec<Row>` 是可修改借用：函数可以直接改调用方的 rows。
#[cfg(feature = "recur-expand")]
fn expand_occurrences(rows: &mut Vec<Row>, conf: &Config) {
    use crate::model::{Task, TaskStatus};
    use crate::recur_expand::expand_task;
    use chrono::{DateTime, Duration, Utc};
    use std::collections::HashMap;

    // 先收集“覆盖记录”（override）：parent_uid -> (recurrence_id -> task)。
    // 覆盖记录代表某一次 occurrence 曾被单独完成或修改。
    let mut overrides: HashMap<String, HashMap<DateTime<Utc>, Task>> = HashMap::new();
    for r in rows.iter() {
        if let (Some(pid), Some(rid)) = (&r.task.parent_uid, r.task.recurrence_id) {
            overrides
                .entry(pid.clone())
                .or_default()
                .insert(rid, r.task.clone());
        }
    }
    // 覆盖记录不单独显示，而是挂在父系列对应 occurrence 上，所以先过滤掉。
    // retain 保留满足闭包条件的元素，其余删除。
    rows.retain(|r| r.task.recurrence_id.is_none());

    let now = Utc::now();
    let after = now - Duration::days(1);
    let before = now + Duration::days(366);
    let mut extra: Vec<Row> = Vec::new();

    for r in rows.iter() {
        // 只展开“活跃”系列：已完成/已取消的父任务不再生成新 occurrence。
        // Expand active series only (done/cancelled masters stop expanding).
        if !r.task.is_parent() || r.task.status.is_done() {
            continue;
        }
        // Past occurrences are treated like ordinary items: expand the whole
        // series from its start (not a 1-day window) so overdue instances
        // show up. Only FUTURE occurrences are capped by
        // `[defaults] recur_expand_count`.
        let series_start = r.task.dtstart.or(r.task.due);
        let occs = expand_task(
            &r.task,
            series_start.unwrap_or(after) - Duration::seconds(1),
            before,
        );
        let ovs = overrides.get(&r.task.uid);
        // 跳过已有完成/取消覆盖的 occurrence，只显示仍然可操作的实例。
        // Skip occurrences that already have a completed/cancelled override
        // so only actionable instances are shown (a finished instance whose
        // slot is still ahead no longer occupies the `recur_expand_count` budget).
        let occs: Vec<_> = occs
            .into_iter()
            .filter(|oc| match ovs.and_then(|m| m.get(&oc.occurrence_start)) {
                Some(ov) => !ov.status.is_done(),
                None => true,
            })
            .collect();
        // 把 occurrence 分成「过去的」与「未来的」：过去的一律展示（逾期实例
        // 也是普通条目）；未来的才受 `recur_expand_count` 限制。
        // Split occurrences into past and future: ALL past instances are shown
        // (overdue occurrences are ordinary items); only FUTURE ones are capped
        // by `[defaults] recur_expand_count`.
        let (past, future): (Vec<_>, Vec<_>) = occs
            .into_iter()
            .partition(|oc| oc.occurrence_start < now);
        let limit = conf.defaults.recur_expand_count;
        // `limit == 0` 表示不限制（usize::MAX ≈ 全部）。
        // `limit == 0` means unlimited (usize::MAX ≈ everything).
        let occs: Vec<_> = past
            .into_iter()
            .chain(future.into_iter().take(if limit == 0 {
                usize::MAX
            } else {
                limit
            }))
            .collect();
        for occ in occs {
            let override_task = ovs.and_then(|m| m.get(&occ.occurrence_start)).cloned();
            let mut t = override_task.clone().unwrap_or_else(|| occ.master.clone());
            if override_task.is_none() {
                // 虚拟 occurrence：由模板生成，日期就是它自己的时间段。
                // 用 parent_uid 标记“这是父系列的一次实例”，避免被当成父任务。
                // Virtual occurrence from the template: single active instance
                // dated at its own slot.
                t.parent_uid = Some(r.task.uid.clone());
                t.status = TaskStatus::Pending;
                if t.is_event() {
                    let delta =
                        occ.occurrence_start - t.dtstart.unwrap_or(occ.occurrence_start);
                    t.dtstart = Some(occ.occurrence_start);
                    t.dtend = t.dtend.map(|e| e + delta);
                } else {
                    t.due = Some(occ.occurrence_start);
                }
                // 生效创建时间 = 它自己的日期（影响 ID 排序）。
                // Effective creation time = its own date (ID ordering).
                t.created_at = occ.occurrence_start;
            }
            // 已存储的覆盖记录保留自己的日期/状态（改期或完成）。
            // Stored overrides keep their own date/status (reschedule/completed).
            extra.push(Row {
                id: r.id,
                source: r.source.clone(),
                task: t,
                occ: Some(occ.index),
            });
        }
    }
    rows.extend(extra);

    // 两段式 ID 排序（基于日历日期，与具体时刻无关）：
    // Two-segment ID ordering (calendar-date based, time of day irrelevant):
    //  1) 未完成 todo（含 occurrence）+ 今天或以后开始的 event —— created ASC
    //  2) 已完成 todo + 今天之前开始的 event —— created DESC
    // Occurrence 行的 created_at 已设为自己的日期（见上），
    // 所以逾期 occurrence 也按自己的日期落进第一段。
    // (Occurrence rows carry their own date as created_at, so overdue
    // occurrences sort into segment 1 by occurrence date.)
    let today = Local::now().date_naive();
    let first = |r: &Row| -> bool {
        if r.task.is_event() {
            // event 按开始日期分：不早于今天 → 第一段。
            // Events are split by their start date (>= today ⇒ segment 1).
            r.task
                .dtstart
                .is_some_and(|d| d.with_timezone(&Local).date_naive() >= today)
        } else {
            // todo（含虚拟 occurrence）只按是否完成分；
            // 逾期 occurrence 仍留在第一段，到期日不影响分段。
            // Todos, virtual occurrences included, are split by completion
            // only (overdue occurrences stay in segment 1); due date irrelevant.
            !r.task.status.is_done()
        }
    };
    let cmp_asc = |a: &Row, b: &Row| {
        a.task
            .created_at
            .cmp(&b.task.created_at)
            .then_with(|| a.task.uid.cmp(&b.task.uid))
    };
    rows.sort_by(|a, b| match (first(a), first(b)) {
        (true, true) => cmp_asc(a, b),
        (false, false) => cmp_asc(b, a),
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
    });

    // 最后给整张表重新分配连续的短 ID（Taskwarrior 风格）。
    // Taskwarrior-style plain sequential IDs over the whole list.
    for (i, r) in rows.iter_mut().enumerate() {
        r.id = i + 1;
    }
}

/// 为某一次 occurrence 构造“覆盖兄弟记录”（override sibling）。
/// Build a per-occurrence override sibling: same UID family, `recurrence_id`
/// = the occurrence's original DTSTART, acting as a completed or edited
/// replacement instance. Used by `done` (Completed) and `modify` (Pending).
///
/// 关键点：新记录有新的 UID，但 `parent_uid` 指向主任务，`recurrence_id` 指向
/// 被替换的那一次实例 —— 这样存储层能认出它属于哪一次。
pub fn override_for_occurrence(
    master: &Task,
    occ: DateTime<Utc>,
    status: TaskStatus,
) -> Task {
    let mut ov = master.clone();
    ov.uid = uuid::Uuid::new_v4().to_string();
    ov.parent_uid = Some(master.uid.clone());
    ov.recurrence_id = Some(occ);
    ov.rrule = None;
    ov.exdates = Vec::new();
    ov.status = status;
    if ov.is_event() {
        let delta = occ - ov.dtstart.unwrap_or(occ);
        ov.dtstart = Some(occ);
        ov.dtend = ov.dtend.map(|e| e + delta);
    } else {
        ov.due = Some(occ);
    }
    let now = chrono::Utc::now();
    ov.created_at = now;
    ov.updated_at = now;
    ov
}

/// 一个已解析的目标，可能指向重复系列中的某一次 occurrence。
/// A resolved target that may address a single occurrence of a recurring series.
pub struct OccurrenceTarget {
    pub uid: String,
    pub source: String,
    /// 目标 occurrence 的原始 DTSTART（None = 整个任务/系列）。
    /// Original DTSTART of the target occurrence (None = whole task/series).
    pub occ_date: Option<DateTime<Utc>>,
}

/// 解析 ID 参数，支持单次 occurrence 形式 `id.n` 和 `on:<date>`。
/// Resolve ID arguments, including per-occurrence forms `id.n` and `on:<date>`.
///
/// `on:<date>` 作用于解析出的每个 ID；`id.n` 指向父系列第 n 个即将到来的
/// occurrence（从 1 开始）。occurrence 解析需要 `recur-expand` feature。
pub fn resolve_targets_occ(
    conf: &Config,
    override_: Option<&[String]>,
    ids: &[String],
    occ_date: Option<DateTime<Utc>>,
) -> Result<Vec<OccurrenceTarget>> {
    let sources = resolve_sources(conf, override_, ContextKind::Cli)?;
    let rows = load_merged_expanded(conf, &sources)?;
    let mut out = Vec::new();
    for id in ids {
        // `id.n` 形式：rsplit_once('.') 从右边切一次，得到父 ID 和 occurrence 序号。
        // `if let Some(...)` 表示“如果能拆出点号就进这个分支”。
        if let Some((pid, pn)) = id.rsplit_once('.') {
            let parent_id: usize = pid.parse().map_err(|_| {
                anyhow::anyhow!("bad occurrence ID `{id}` (expected `<id>.<n>`)")
            })?;
            let n: usize = pn
                .parse()
                .map_err(|_| anyhow::anyhow!("bad occurrence number `{id}` (expected `<id>.<n>`)"))?;
            if n == 0 {
                bail!("occurrence numbers are 1-based: `{id}`");
            }
            // checked_sub(1)：把 1-based ID 转成 0-based 下标，并防止 0-1 下溢。
            // `.get(...)` 返回 Option；`.ok_or_else(|| ...)?` 把 None 转成 Err 并提前返回。
            let row = rows
                .get(parent_id.checked_sub(1).unwrap_or(usize::MAX))
                .ok_or_else(|| anyhow::anyhow!("no task with ID `{parent_id}`"))?;
            if !row.task.is_parent() {
                bail!("task `{parent_id}` is not a recurring parent");
            }
            let occ = resolve_nth_occurrence(&row.task, n)?;
            out.push(OccurrenceTarget {
                uid: row.task.uid.clone(),
                source: row.source.clone(),
                occ_date: Some(occ),
            });
        } else if let Ok(n) = id.parse::<usize>() {
            // 纯数字：当作合并列表里的短 ID，1-based 转下标。
            let row = rows
                .get(n.checked_sub(1).unwrap_or(usize::MAX))
                .ok_or_else(|| anyhow::anyhow!("no task with ID `{id}`"))?;
            out.push(target_from_row(row, id, occ_date)?);
        } else {
            // 不是数字：当作 UID 精确匹配。
            // find 返回第一个满足闭包条件的元素（Option）。
            let found = rows
                .iter()
                .find(|r| r.task.uid == *id)
                .ok_or_else(|| anyhow::anyhow!("no task with UID `{id}`"))?;
            out.push(target_from_row(found, id, occ_date)?);
        }
    }
    Ok(out)
}

/// 为真实行或虚拟行构造 `OccurrenceTarget`；当行是父任务时，`on:<date>`
/// 按本地日期解析对应 occurrence。
/// Build an `OccurrenceTarget` for a real or virtual row; `on:<date>`
/// resolves a series occurrence by local day when the row is a master.
fn target_from_row(
    row: &Row,
    id: &str,
    occ_date: Option<DateTime<Utc>>,
) -> Result<OccurrenceTarget> {
    if row.occ.is_some() {
        // 虚拟 occurrence 行：目标就是那一次实例。
        // 存储里的覆盖记录有自己的 UID，所以这里统一解析回父任务 UID。
        // A virtual occurrence row: target that single occurrence, addressed
        // through its parent series.
        let occ = row
            .task
            .dtstart
            .or(row.task.due)
            .ok_or_else(|| anyhow::anyhow!("occurrence `{id}` has no date"))?;
        let uid = row
            .task
            .parent_uid
            .clone()
            .unwrap_or_else(|| row.task.uid.clone());
        return Ok(OccurrenceTarget {
            uid,
            source: row.source.clone(),
            occ_date: Some(occ),
        });
    }
    let occ = match occ_date {
        Some(d) => Some(resolve_occurrence_date(&row.task, d)?),
        None => None,
    };
    Ok(OccurrenceTarget {
        uid: row.task.uid.clone(),
        source: row.source.clone(),
        occ_date: occ,
    })
}

/// 解析重复父任务第 n 个（1-based）即将到来的 occurrence。
/// Resolve the nth upcoming occurrence (1-based) of a recurring parent.
///
/// `&Task` 是只读借用：这里只需要读任务，不需要改它。
pub fn resolve_nth_occurrence(t: &Task, n: usize) -> Result<DateTime<Utc>> {
    #[cfg(feature = "recur-expand")]
    {
        let now = Utc::now();
        let occs = crate::recur_expand::expand_task(
            t,
            now - Duration::days(1),
            now + Duration::days(366),
        );
        occs.into_iter()
            .find(|o| o.index == n)
            .map(|o| o.occurrence_start)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no occurrence `{n}` in the next year for recurring task `{}`",
                    t.uid
                )
            })
    }
    #[cfg(not(feature = "recur-expand"))]
    {
        let _ = (t, n);
        bail!("occurrence addressing requires the `recur-expand` feature")
    }
}

/// 找到本地日期与 `date` 匹配的那次 occurrence（`on:<date>` 形式）。
/// Find the occurrence whose local day matches `date` (the `on:<date>` form).
pub fn resolve_occurrence_date(t: &Task, date: DateTime<Utc>) -> Result<DateTime<Utc>> {
    #[cfg(feature = "recur-expand")]
    {
        let now = Utc::now();
        let day = date.with_timezone(&Local).date_naive();
        let occs = crate::recur_expand::expand_task(
            t,
            now - Duration::days(1),
            now + Duration::days(366),
        );
        occs.into_iter()
            .find(|o| o.occurrence_start.with_timezone(&Local).date_naive() == day)
            .map(|o| o.occurrence_start)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no occurrence on `{day}` for recurring task `{}`",
                    t.uid
                )
            })
    }
    #[cfg(not(feature = "recur-expand"))]
    {
        let _ = (t, date);
        bail!("occurrence addressing requires the `recur-expand` feature")
    }
}

/// 把 source 名字解析成具体的 `Source`，包括 `ics-dir` 组合引用
/// （`remote/sorge` → 虚拟 `ics` source）。
/// Resolve a source name to its concrete `Source`, including composite
/// `ics-dir` references (`remote/sorge` → virtual `ics` source).
pub fn resolve_source(conf: &Config, name: &str) -> Result<Source> {
    let mut resolved = source::resolve_source_name(&conf.sources, name)?;
    resolved
        .pop()
        .ok_or_else(|| anyhow::anyhow!("unknown source `{name}`"))
}

/// 把 ID 参数解析成 `(uid, source_name)` 列表。
/// Resolve ID arguments to `(uid, source_name)` pairs.
///
/// 数字是合并列表里的短 ID；非数字按 UID 直接匹配。允许混合使用。
pub fn resolve_targets(
    conf: &Config,
    override_: Option<&[String]>,
    ids: &[String],
) -> Result<Vec<(String, String)>> {
    Ok(resolve_targets_occ(conf, override_, ids, None)?
        .into_iter()
        .map(|t| (t.uid, t.source))
        .collect())
}

/// 仅在 TTY（交互终端）下向用户询问 yes/no；非交互输入默认返回 `false`
/// （即只处理单次 occurrence）。脚本可用 `all-future` 关键字强制 `true`。
/// Prompt the user (TTY only) for a yes/no decision; non-interactive input
/// falls back to `false` (single occurrence).
pub fn confirm(prompt: &str) -> Result<bool> {
    use std::io::{IsTerminal, Write};
    // is_terminal() 判断 stdin 是不是终端：管道/脚本输入不是，就不该阻塞等待回答。
    if !std::io::stdin().is_terminal() {
        return Ok(false);
    }
    eprint!("{prompt} [y/N] ");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let t = line.trim().to_ascii_lowercase();
    Ok(t == "y" || t == "yes")
}

// 测试模块：仅 `cargo test` 时编译，不参与正式构建。
// Test module: compiled only when running tests.
#[cfg(all(test, feature = "storage-jsonl"))]
mod tests {
    use super::*;
    use crate::config::{Config, SourceType};
    use tempfile::tempdir;

    fn source(dir: &std::path::Path, name: &str) -> Source {
        Source {
            name: name.into(),
            source_type: SourceType::Jsonl,
            location: dir.display().to_string(),
            sync: None,
        }
    }

    #[test]
    fn ids_assigned_oldest_first() {
        let dir = tempdir().unwrap();
        let mut st = crate::storage::jsonl::JsonlStorage::open(dir.path()).unwrap();
        let first = Task::new("work", "newer");
        let mut second = Task::new("work", "older");
        // older created earlier: rewind its timestamp manually
        second.created_at = first.created_at - chrono::Duration::days(1);
        st.add(second).unwrap();
        st.add(first).unwrap();
        drop(st);

        let rows = load_merged(&Config::default(), &[source(dir.path(), "work")]).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, 1);
        assert_eq!(rows[0].task.summary, "older");
        assert_eq!(rows[1].id, 2);
        assert_eq!(rows[1].task.summary, "newer");
    }

    #[cfg(feature = "recur-expand")]
    #[test]
    fn expansion_skips_done_occurrences() {
        use crate::model::TaskStatus;
        use chrono::{Duration, Utc};
        let dir = tempdir().unwrap();
        let mut st = crate::storage::jsonl::JsonlStorage::open(dir.path()).unwrap();
        let now = Utc::now();
        // Recurring daily todo: first occurrence is one hour ahead.
        let mut master = Task::new("work", "daily");
        master.due = Some(now + Duration::hours(1));
        master.rrule = Some("FREQ=DAILY".into());
        master.status = TaskStatus::Recurring;
        st.add(master).unwrap();
        // Complete the first occurrence exactly like `done <id>` does.
        let master = st.list()[0].clone();
        // Expand first to get the exact (second-truncated) occurrence stamp.
        let occ = crate::recur_expand::expand_task(
            &master,
            now - Duration::days(1),
            now + Duration::days(366),
        )
        .first()
        .unwrap()
        .occurrence_start;
        let ov = super::override_for_occurrence(&master, occ, TaskStatus::Completed);
        st.add(ov).unwrap();
        drop(st);

        let rows =
            load_merged_expanded(&Config::default(), &[source(dir.path(), "work")]).unwrap();
        // The next occurrence (tomorrow) is the one shown, not the completed one.
        let occ_rows: Vec<_> = rows.iter().filter(|r| r.occ.is_some()).collect();
        assert_eq!(occ_rows.len(), 1);
        assert_eq!(occ_rows[0].task.due, Some(occ + Duration::days(1)));
        assert_eq!(occ_rows[0].task.status, TaskStatus::Pending);
    }

    #[cfg(feature = "recur-expand")]
    #[test]
    fn expansion_keeps_all_past_occurrences_and_limits_future() {
        use crate::model::TaskStatus;
        use chrono::{Duration, Utc};
        let dir = tempdir().unwrap();
        let mut st = crate::storage::jsonl::JsonlStorage::open(dir.path()).unwrap();
        let now = Utc::now();
        // Daily todo: two occurrences already past (30h and 6h ago), next two ahead.
        let mut master = Task::new("work", "daily");
        master.due = Some(now - Duration::hours(30));
        master.rrule = Some("FREQ=DAILY".into());
        master.status = TaskStatus::Recurring;
        st.add(master).unwrap();
        drop(st);

        let rows =
            load_merged_expanded(&Config::default(), &[source(dir.path(), "work")]).unwrap();
        let occ: Vec<_> = rows.iter().filter(|r| r.occ.is_some()).collect();
        // Past occurrences are all shown (2), future capped at 1 (@ limit=1).
        assert_eq!(occ.len(), 3);
        let dates: Vec<_> = occ
            .iter()
            .map(|r| r.task.due.unwrap().with_timezone(&Local).date_naive())
            .collect();
        // Oldest first: -30h, -6h, then the next future day.
        assert!(dates[1] >= dates[0]);
        assert!(dates[2] > dates[1]);
    }

    #[cfg(feature = "recur-expand")]
    #[test]
    fn two_segment_id_order() {
        use crate::model::TaskStatus;
        use chrono::{Duration, Utc};
        let dir = tempdir().unwrap();
        let mut st = crate::storage::jsonl::JsonlStorage::open(dir.path()).unwrap();
        let now = Utc::now();
        // created order: old-event(-12d) < done(-10d) < past(-9d) < future(-8d) < active(-1d)
        let mut old_ev = Task::new("work", "old-event");
        old_ev.dtstart = Some(now - Duration::days(400)); // last year
        old_ev.created_at = now - Duration::days(12);
        let mut done = Task::new("work", "done");
        done.status = TaskStatus::Completed;
        done.created_at = now - Duration::days(10);
        let mut past = Task::new("work", "overdue");
        past.due = Some(now - Duration::days(3));
        past.created_at = now - Duration::days(9);
        let mut future = Task::new("work", "future");
        future.dtstart = Some(now + Duration::days(2));
        future.created_at = now - Duration::days(8);
        let active = Task::new("work", "active");
        for t in [old_ev, done, past, future, active] {
            st.add(t).unwrap();
        }
        drop(st);

        let rows = load_merged_expanded(&Config::default(), &[source(dir.path(), "work")]).unwrap();
        let ids: Vec<(usize, &str)> = rows
            .iter()
            .map(|r| (r.id, r.task.summary.as_str()))
            .collect();
        // segment 1: not-done todos + today-or-future events, created ASC
        // (overdue has a past due but is NOT done → stays in segment 1)
        // segment 2: done todos + past events, created DESC
        // (old-event from last year is the earliest-created → ends the index)
        assert_eq!(
            ids,
            vec![
                (1, "overdue"),
                (2, "future"),
                (3, "active"),
                (4, "done"),
                (5, "old-event"),
            ]
        );
    }

    #[test]
    fn ids_not_shifted_by_filter() {
        let dir = tempdir().unwrap();
        let mut st = crate::storage::jsonl::JsonlStorage::open(dir.path()).unwrap();
        for (i, sum) in ["a", "b", "c"].iter().enumerate() {
            let mut t = Task::new("work", *sum);
            t.created_at += chrono::Duration::hours(i as i64);
            st.add(t).unwrap();
        }
        // complete the middle task
        let rows_all = load_merged(&Config::default(), &[source(dir.path(), "work")]).unwrap();
        let uid_b = rows_all[1].task.uid.clone();
        let mut st = crate::storage::jsonl::JsonlStorage::open(dir.path()).unwrap();
        st.update(&uid_b, |t| {
            t.status = crate::model::TaskStatus::Completed;
            Ok(())
        })
        .unwrap();

        let rows = load_merged(&Config::default(), &[source(dir.path(), "work")]).unwrap();
        // actives = a (1), c (3); completed b keeps id 2 in the full index
        let f = crate::filter::parse_expr(&["status:active".to_string()]).unwrap();
        let shown: Vec<usize> = rows
            .iter()
            .filter(|r| f.matches(&r.task))
            .map(|r| r.id)
            .collect();
        assert_eq!(shown, vec![1, 3]);
    }
}

