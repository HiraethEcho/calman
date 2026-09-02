//! 过滤器表达式引擎：供 `list`、`count` 与报表 `filter` 字符串使用。
//! Filter expression engine for `list`, `count` and report `filter` strings.
//!
//! 语法（CLI 过滤器 token 与 `[report.<name>].filter` 共用）：
//! Grammar (shared by CLI filter tokens and `[report.<name>].filter`):
//!   expr   := or
//!   or     := and ('or' and)*
//!   and    := unary (('and')? unary)*
//!   unary  := '(' or ')' | atom
//!   atom   := 'status:' | 'type:' | 'source:' | 'priority:' | 'due:'
//!           | 'due.before:' | 'due.by:' | 'due.after:' | '+tag' | '+VIRTUAL'
//!           | '-tag' | '-VIRTUAL' | '-status:' | '-source:' | ...
//!
//! 这条文法的核心是优先级：`or` 最低，`and` 其次，`unary`（括号/单个原子）最高。
//! 所以 `A B or C D` 会被解析成 `(A B) or (C D)`，而不是 `A (B or C) D`。
//! Precedence: `or` is lowest, `and` next, `unary` highest — so `A B or C D`
//! parses as `(A B) or (C D)`.
//!
//! `-<atom>` 会把原子包进 `Not`（取反）。`+PENDING` 匹配任何未完成任务
//! （事件也算 pending）。`due*` 是统一日期：todo 用 `due`，事件用 `dtstart`。
//! `-<atom>` wraps the atom in `Not`. `+PENDING` matches any active task. The
//! `due*` key is unified: todos use `due`, events use `dtstart`.
//!
//! 实现概览（面向初学者）：先用 `tokenize` 把字符串切成 token，再用
//! `parse_or`/`parse_and`/`parse_unary` 一组互相递归调用的函数（递归下降解析器）
//! 建成一棵 `Expr` 表达式树，最后 `matches_with` 递归地求值这棵树。
//! Overview: `tokenize` → recursive-descent parser builds an `Expr` tree →
//! `matches_with` evaluates it recursively.

use crate::date::parse_datetime;
use crate::model::{Task, TaskStatus, priority_from_str};
use anyhow::{Result, bail};
use chrono::{DateTime, Local, Utc};

/// `due*` 子句如何与任务日期比较。
/// How a `due*` clause compares against the task date.
///
/// Rust 概念：这是一个枚举（enum），枚举就是“一组可能的值”的类型；
/// 后面的 `match` 会按枚举成员分别处理。
/// Rust concept: an enum is a type with a fixed set of possible values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DueOp {
    On,
    Before,
    By,
    After,
}

/// `type:` 的任务类型选择器。
/// Task type selector for `type:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeSel {
    Todo,
    Event,
    All,
}

/// 内置虚拟标签，可用作 `+NAME` / `-NAME`。
/// Builtin virtual tags that are usable as `+NAME` / `-NAME`.
///
/// 虚拟标签不是任务里真实存储的标签，而是根据任务状态/日期“现算”出来的布尔值，
/// 例如“已过期”“等待中”“有父任务”。
/// Virtual tags are computed on the fly from task state/dates, not stored on tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flag {
    Overdue,
    Pending,
    Completed,
    Cancelled,
    InProgress,
    Started,
    Tagged,
    Untagged,
    Due,
    Parent,
    Waiting,
}

/// 单个“正向”原子（不含取反）。取反放在 `Expr::Not` 里。
/// A single positive atom. Negations live in `Expr::Not`.
///
/// Rust 概念：`Option<T>` 表示“可能有值，也可能没有”；字段大多是可选的，
/// 例如 `status: Option<TaskStatus>`——只有用户写了 `status:...` 才有值。
/// 若没有值，该项就不参与过滤（跳过该条件）。
/// Rust concept: `Option<T>` means "maybe a value, maybe none"; a `None` field
/// simply skips that condition.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub status: Option<TaskStatus>,
    pub active: bool,
    pub type_sel: Option<TypeSel>,
    pub source: Option<String>,
    pub priority: Option<u8>,
    pub tags: Vec<String>,
    pub due: Option<(DueOp, DateTime<Utc>)>,
    /// `date:` 过滤器——统一日期：todo→due，event→dtstart。
    /// `date:` filter — unified date: todo→due, event→dtstart.
    pub date: Option<(DueOp, DateTime<Utc>)>,
    /// `from:` 过滤器——只针对 VEVENT 的 `dtstart`。
    /// `from:` filter — VEVENT `dtstart` only.
    pub from: Option<(DueOp, DateTime<Utc>)>,
    pub flags: Vec<Flag>,
}

/// 过滤器表达式树。
/// A filter expression tree.
///
/// 递归数据结构：一个表达式要么是单个原子，要么是取反/与/或三种组合之一。
/// `Box<Expr>` 把子表达式放到堆上，让“树套树”的无限递归结构可以表示。
/// A recursive data structure; `Box<Expr>` lets an expression hold sub-expressions.
/// Rust 概念：enum 的成员可以携带数据，这就是“带数据的枚举”。
/// Rust concept: enum variants can carry data (tagged union).
#[derive(Debug, Clone)]
pub enum Expr {
    Atom(Filter),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

impl Expr {
    /// 测试用的便捷入口；生产代码使用 `matches_with`。
    /// Test convenience; production uses `matches_with`.
    #[cfg(test)]
    pub fn matches(&self, t: &Task) -> bool {
        self.matches_with(t)
    }

    /// 递归求值表达式树：一个原子是否符合该任务。
    /// Recursively evaluate the tree: does this task satisfy the expression?
    ///
    /// `match self` 按枚举成员分支：`Not` 取反、`And` 两边都成立、`Or` 任一边成立。
    /// 递归体现在子表达式（`a.matches_with(t)`）再次调用本函数。
    /// `match` branches on the enum variant; recursion happens on sub-expressions.
    pub fn matches_with(&self, t: &Task) -> bool {
        match self {
            Expr::Atom(f) => f.matches_with(t),
            Expr::Not(e) => !e.matches_with(t),
            Expr::And(a, b) => {
                a.matches_with(t) && b.matches_with(t)
            }
            Expr::Or(a, b) => {
                a.matches_with(t) || b.matches_with(t)
            }
        }
    }
}

/// 任务的“统一日期”：todo → `due`，事件 → `dtstart`。
/// The unified "date" of a task: todos → `due`, events → `dtstart`.
pub fn task_date(t: &Task) -> Option<DateTime<Utc>> {
    if t.is_event() { t.dtstart } else { t.due }
}

impl Filter {
    /// 判断一个“原子过滤器”是否命中任务：逐项检查每个已设置的字段。
    /// Does a single atom filter match the task? Each set field is checked in turn.
    ///
    /// 逻辑是“与”：只要有一项不满足就 `return false`；全部通过才返回 `true`。
    /// Logic is AND: any failing field returns `false` immediately.
    pub fn matches_with(&self, t: &Task) -> bool {
        // 若指定了状态且不相等 → 不匹配。
        // If a status is required and differs → no match.
        if let Some(s) = self.status
            && t.status != s
        {
            return false;
        }
        if self.active && !t.status.is_active() {
            return false;
        }
        if let Some(ty) = self.type_sel {
            let is_event = t.is_event();
            let ok = match ty {
                TypeSel::Todo => !is_event,
                TypeSel::Event => is_event,
                TypeSel::All => true,
            };
            if !ok {
                return false;
            }
        }
        if let Some(src) = &self.source
            && !t.source.eq_ignore_ascii_case(src)
        {
            return false;
        }
        if let Some(p) = self.priority
            && t.priority != Some(p)
        {
            return false;
        }
        // 所有要求的标签都必须存在（`.any` 判断任务是否含该标签）。
        // Every required tag must be present (`.any` checks containment).
        for tag in &self.tags {
            if !t.tags.iter().any(|x| x.eq_ignore_ascii_case(tag)) {
                return false;
            }
        }
        // 每个要求的虚拟标签都必须为真。
        // Every required virtual flag must evaluate to true.
        for f in &self.flags {
            if !flag_matches(*f, t) {
                return false;
            }
        }
        if let Some((op, dt)) = self.due
            && !due_matches(op, dt, task_date(t))
        {
            return false;
        }
        // `date:` 匹配统一日期（todo 用 due，事件用 dtstart）。
        // `date:` matches the unified date (due for todos, dtstart for events).
        if let Some((op, dt)) = self.date
            && !due_matches(op, dt, task_date(t))
        {
            return false;
        }
        // `from:` 只匹配 VEVENT 的 `dtstart`；todo 没有 dtstart，永远不匹配。
        // 这保证了取反时（如 `-from.after:now`）todo 不会“漏网”反被命中。
        // `from:` matches `dtstart` only; a todo (no dtstart) never matches, so
        // it must also not slip through the negation.
        if let Some((op, dt)) = self.from {
            let ok = match t.dtstart {
                Some(start) => due_matches(op, dt, Some(start)),
                None => false,
            };
            if !ok {
                return false;
            }
        }
        true
    }
}

/// 判断一个 VTODO 是否已过期：`due` 已过、且未完成/未取消。
/// 事件（event）永远不匹配；循环模板（recurring master）不是真实实例，也不匹配。
/// True when a VTODO's due has passed and it is not done (recurring
/// templates excluded — they are not real instances). Events never match.
pub fn is_overdue(t: &Task) -> bool {
    if t.is_event() {
        return false; // 事件用 dtstart，没有 due 语义，永不判逾期。 Events never match.
    }
    let Some(d) = t.due else {
        return false; // 没有 due 就谈不上过期。 No due → never overdue.
    };
    if !t.status.is_active() {
        return false; // 已完成/已取消不再算过期。Finished/cancelled are not overdue.
    }
    if t.allday {
        // 全天（仅日期）任务按本地日历日比较：过期从截止日的“下一天”开始。
        // Date-only (all-day) task: compare local calendar days.
        // Overdue starts the day AFTER the due day.
        let due_day = d.with_timezone(&Local).date_naive();
        let today = Local::now().date_naive();
        today > due_day
    } else {
        // 带具体时间的任务：直接与当前时刻比较。
        // Timed task: plain instant comparison against now.
        d < Utc::now()
    }
}

/// 计算单个虚拟标签是否为真。
/// Evaluate whether one virtual flag is true for this task.
///
/// Rust 概念：`match f` 把 `Flag` 枚举逐个成员分支处理；每个分支返回一个 `bool`。
/// Rust concept: `match` handles each enum variant and returns a `bool` per arm.
fn flag_matches(f: Flag, t: &Task) -> bool {
    match f {
        Flag::Pending => t.status.is_active(),
        Flag::Completed => t.status == TaskStatus::Completed,
        Flag::Cancelled => t.status == TaskStatus::Cancelled,
        Flag::InProgress => t.status == TaskStatus::InProgress,
        Flag::Started => t.started_at.is_some(),
        Flag::Tagged => !t.tags.is_empty(),
        Flag::Untagged => t.tags.is_empty(),
        // +DUE：todo 且带 `due` 字段（事件用 `dtstart`，不属于 due）。
        // +DUE: a todo with a due date (events use dtstart, not due).
        Flag::Due => !t.is_event() && t.due.is_some(),
        // +PARENT：只有带子任务的“父任务”才命中。
        // +PARENT: only tasks that have children (master/parent tasks) match.
        Flag::Parent => t.is_parent(),
        // +WAITING：任务有 `wait`（等待秒数）且在等待期内（到期日 + wait 还没到 now）。
        // `.zip()` 把两个 Option 合并成一个；`.is_some_and` 在两者都有值时执行闭包判断。
        // +WAITING: task has a wait offset and the waiting window (due + wait)
        // has not passed. `.zip()` pairs two Options; `.is_some_and` checks both.
        Flag::Waiting => t
            .wait
            .zip(task_date(t))
            .is_some_and(|(w, d)| d + chrono::Duration::seconds(w) > Utc::now()),
        // +OVERDUE：已过期且仍处于 active —— 具体规则见 `is_overdue`。
        // +OVERDUE: past due AND still active — see `is_overdue`.
        Flag::Overdue => is_overdue(t),
    }
}

/// 用 `DueOp` 把过滤日期和目标（maybe 有值）做比较。
/// Compare the filter date against the task's (optional) date using `DueOp`.
///
/// Rust 概念：`let Some(have) = have else` 是“若没有值就提前返回 false”的简洁写法；
/// 这也是 `Option` 的常见解构方式。
/// Rust concept: `let Some(...) = ... else` unwraps an Option, returning early on `None`.
fn due_matches(op: DueOp, want: DateTime<Utc>, have: Option<DateTime<Utc>>) -> bool {
    let Some(have) = have else {
        return false;
    };
    match op {
        // “恰好这一天” = 本地日历日相同（全天语义），匹配 `due:today`/`due:eow` 的直觉。
        // Exact = same local calendar day (date-only semantics), matching
        // `due:today`/`due:eow` expectations.
        DueOp::On => {
            let w = want.with_timezone(&Local).date_naive();
            have.with_timezone(&Local).date_naive() == w
        }
        // 时间点比较：早于 / 不晚于 / 不早于。
        // Instant comparisons: strictly before / at or before / at or after.
        DueOp::Before => have < want,
        DueOp::By => have <= want,
        DueOp::After => have >= want,
    }
}

/// 把过滤器字符串切成 token：按空白切分，并把左右括号单独切成一个 token。
/// Tokenize a filter string (splits on whitespace and parentheses).
///
/// `std::mem::take` 会取走 `cur` 的当前内容并留一个空字符串——相当于
/// “把积累的字符倒进 token 列表再清空”。
/// `std::mem::take` moves the accumulated string out and resets it to empty.
pub fn tokenize(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    // 逐字符扫描：空白结束当前 token；括号本身也是边界，且自身独立成 token。
    // Scan char by char: whitespace ends a token; parentheses are delimiters and
    // become standalone tokens (so the parser can see grouping).
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else if ch == '(' || ch == ')' {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(ch.to_string());
        } else {
            cur.push(ch);
        }
    }
    // 收尾：最后一个 token 可能没有后续空白。
    // Flush the final token (no trailing whitespace needed).
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 解析过滤器字符串（相邻原子之间隐含 `and`）。
/// Parse a filter string (implicit `and` between adjacent atoms).
pub fn parse_expr_str(s: &str) -> Result<Expr> {
    let toks = tokenize(s);
    parse_expr(&toks)
}

/// 把 token 列表（CLI argv 每个参数也可再切分）解析成表达式。
/// Parse a token list (CLI argv tokens) into an expression.
///
/// 空列表表示“匹配一切”。
/// An empty list matches everything.
///
/// Rust 概念：`args: &[String]` 是字符串的“切片”——借用一段连续数据，不拥有它；
/// 解析器用下标 `pos: &mut usize` 在切片上移动，而不是用迭代器。若改用迭代器写法，
/// 常见做法是 `.peekable()`（可“偷看”下一个 token）。
/// Rust concept: `&[String]` is a borrowed slice; this parser walks it with an
/// index. An iterator-based design would use `.peekable()` to look ahead.
pub fn parse_expr(args: &[String]) -> Result<Expr> {
    if args.is_empty() {
        return Ok(Expr::Atom(Filter::default()));
    }
    let mut toks: Vec<String> = Vec::new();
    // CLI 的一个参数里也可能含空格/括号（例如 `"due:today +x"`），所以逐个再 tokenize。
    // Each CLI arg may itself contain spaces or parens, so tokenize each one.
    for a in args {
        toks.extend(tokenize(a));
    }
    let mut pos = 0usize;
    let e = parse_or(&toks, &mut pos)?;
    // 解析完还有剩余 token → 语法错误。
    // Anything left after parsing is a syntax error.
    if pos != toks.len() {
        bail!("unexpected filter token `{}`", toks[pos]);
    }
    Ok(e)
}

/// 解析 `or` 层：先解析若干 `and` 项，遇到 `or` 就组合成 Or 节点。
/// Parse the `or` layer: combine `and` operands around `or` keywords.
///
/// 优先级关键点：`or` 是最低优先级，所以先调用 `parse_and` 把更紧的操作组合掉。
/// 于是 `A B or C D` = `(A B) or (C D)`。
/// Precedence hinge: `or` is lowest, so `parse_and` runs first —
/// `A B or C D` becomes `(A B) or (C D)`.
/// Rust 概念：`&mut usize` 是“可变引用”计数器；解析函数之间通过它共享当前位置。
/// Rust concept: `&mut usize` shares a mutable position between parser functions.
fn parse_or(toks: &[String], pos: &mut usize) -> Result<Expr> {
    let mut left = parse_and(toks, pos)?;
    // 循环消费所有 `or`，每次把右边再解析成一个 and 表达式。
    // Consume every `or`, parsing the right side as an and-expression each time.
    while *pos < toks.len() && toks[*pos].eq_ignore_ascii_case("or") {
        *pos += 1;
        let right = parse_and(toks, pos)?;
        left = Expr::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

/// 解析 `and` 层：相邻原子默认就是 `and`，显式写 `and` 也可以。
/// Parse the `and` layer: adjacent atoms imply `and`; explicit `and` also works.
///
/// 遇到 `or` 或 `)` 就停手，把控制权交回上层（`parse_or` 或括号处理）。
/// Stops at `or`/`)`, handing control back up the precedence chain.
fn parse_and(toks: &[String], pos: &mut usize) -> Result<Expr> {
    let mut left = parse_unary(toks, pos)?;
    loop {
        if *pos >= toks.len() {
            return Ok(left);
        }
        let t = toks[*pos].to_ascii_lowercase();
        if t == "or" || t == ")" {
            // `or` 是新 or 表达式的开头；`)` 是括号组的结尾——都不属于 and 层。
            // `or` starts a new or-expression; `)` closes a paren group.
            return Ok(left);
        }
        if t == "and" {
            // 显式 `and` 只是跳过关键字，继续读取下一个操作数。
            // Explicit `and` is a no-op keyword; just skip it.
            *pos += 1;
            continue;
        }
        // 没有关键字 → 隐式 and：把下一个 unary 并入左侧。
        // No keyword → implicit and: fold the next unary into the left side.
        let right = parse_unary(toks, pos)?;
        left = Expr::And(Box::new(left), Box::new(right));
    }
}

/// 解析最内层：括号组或单个原子（分词符）。
/// Parse the innermost layer: a parenthesized group or a single atom token.
///
/// 这是递归下降解析器的“递归点”：看到 `(` 就递归调用 `parse_or` 解析括号内部，
/// 因此任意深度的嵌套括号都能处理。
/// Recursion point: `(` triggers a recursive `parse_or` call, so nesting works.
fn parse_unary(toks: &[String], pos: &mut usize) -> Result<Expr> {
    if *pos >= toks.len() {
        bail!("unexpected end of filter");
    }
    let tok = toks[*pos].clone();
    if tok == "(" {
        *pos += 1;
        let e = parse_or(toks, pos)?;
        // 括号必须闭合；否则报“缺少 )”。
        // A paren group must close; otherwise report missing `)`.
        if *pos >= toks.len() || toks[*pos] != ")" {
            bail!("missing `)` in filter");
        }
        *pos += 1;
        return Ok(e);
    }
    if tok == ")" {
        bail!("unexpected `)` in filter");
    }
    *pos += 1;
    parse_atom(&tok)
}

/// 解析单个原子 token（允许 `-<atom>` / `+<atom>`；`-` 表示取反）。
/// Parse one atom token (which may be `-<atom>` / `+<atom>` for negation).
///
/// 取反策略：对带前缀的字段分支，直接把该字段包进 `Expr::Not`；
/// 对其它以 `-` 开头的 token，把剩余部分当正原子解析后整体取反。
/// Negation strategy: known `-field:` prefixes build a negated field atom;
/// any other `-` token parses the rest as positive then wraps it in `Not`.
fn parse_atom(tok: &str) -> Result<Expr> {
    let lower = tok.to_ascii_lowercase();
    // `.strip_prefix` 尝试去掉前缀；`.or_else` 在第一个闭包没匹配时尝试第二个别名。
    // `.strip_prefix` tries a prefix; `.or_else` falls back to the alias closure.
    if let Some(inner) = lower.strip_prefix("-source:").or_else(|| lower.strip_prefix("-src:")) {
        let f = Filter {
            source: Some(inner.to_string()),
            ..Filter::default()
        };
        // 结构体更新语法 `..Filter::default()`：其余字段用默认值（即不参与过滤）。
        // Struct update syntax: remaining fields use their defaults (no filtering).
        return Ok(Expr::Not(Box::new(Expr::Atom(f))));
    }
    if let Some(inner) = lower.strip_prefix("-status:") {
        return Ok(Expr::Not(Box::new(Expr::Atom(parse_status_atom(
            &format!("status:{inner}"),
        )?))));
    }
    if let Some(inner) = lower.strip_prefix("-type:") {
        let sel = match inner {
            "todo" => TypeSel::Todo,
            "event" => TypeSel::Event,
            "all" => TypeSel::All,
            _ => bail!("unknown type `{inner}` (use todo, event, all)"),
        };
        return Ok(Expr::Not(Box::new(Expr::Atom(Filter {
            type_sel: Some(sel),
            ..Filter::default()
        }))));
    }
    if let Some(rest) = lower.strip_prefix("-due.before:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Due, DueOp::Before, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-due.by:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Due, DueOp::By, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-due.after:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Due, DueOp::After, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-due:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Due, DueOp::On, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-date.before:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Date, DueOp::Before, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-date.by:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Date, DueOp::By, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-date.after:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Date, DueOp::After, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-date:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::Date, DueOp::On, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-from.before:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::From, DueOp::Before, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-from.by:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::From, DueOp::By, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-from.after:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::From, DueOp::After, rest)?)));
    }
    if let Some(rest) = lower.strip_prefix("-from:") {
        return Ok(Expr::Not(Box::new(field_atom(DateField::From, DueOp::On, rest)?)));
    }
    if let Some(rest) = lower
        .strip_prefix("-priority:")
        .or_else(|| lower.strip_prefix("-pri:"))
    {
        let p = priority_from_str(rest)
            .ok_or_else(|| anyhow::anyhow!("bad priority `{rest}` (use H, M, L or 0-9)"))?;
        return Ok(Expr::Not(Box::new(Expr::Atom(Filter {
            priority: Some(p),
            ..Filter::default()
        }))));
    }
    if let Some(rest) = lower.strip_prefix("source:").or_else(|| lower.strip_prefix("src:")) {
        return Ok(Expr::Atom(Filter {
            source: Some(rest.to_string()),
            ..Filter::default()
        }));
    }
    // 兜底取反：`-tag`、`-VIRTUAL` 等——把去掉 `-` 的部分当正原子解析再取反。
    // Generic negation fallback for `-tag`/`-VIRTUAL`: parse the rest as positive,
    // then wrap the result in `Not`.
    if lower.starts_with('-') && lower.len() > 1 {
        let inner = &tok[1..];
        let e = parse_positive(&format!("+{inner}"))?;
        return Ok(Expr::Not(Box::new(e)));
    }
    parse_positive(tok)
}

/// 解析“正原子”token（不带前导 `-`）。
/// Parse a positive atom token (no leading `-`).
///
/// 是一长串 `if let Some(rest) = lower.strip_prefix(...)` 的“前缀分派”：
/// Rust 概念：类似于 switch 的链，逐个尝试匹配已知前缀，命中就返回对应过滤器。
/// A long prefix-dispatch chain: each `strip_prefix` tries one known keyword.
fn parse_positive(tok: &str) -> Result<Expr> {
    let lower = tok.to_ascii_lowercase();

    if let Some(rest) = lower
        .strip_prefix("priority:")
        .or_else(|| lower.strip_prefix("pri:"))
    {
        let p = priority_from_str(rest)
            .ok_or_else(|| anyhow::anyhow!("bad priority `{rest}` (use H, M, L or 0-9)"))?;
        return Ok(Expr::Atom(Filter {
            priority: Some(p),
            ..Filter::default()
        }));
    }

    if let Some(rest) = lower.strip_prefix("due.before:") {
        return field_atom(DateField::Due, DueOp::Before, rest);
    }
    if let Some(rest) = lower.strip_prefix("due.by:") {
        return field_atom(DateField::Due, DueOp::By, rest);
    }
    if let Some(rest) = lower.strip_prefix("due.after:") {
        return field_atom(DateField::Due, DueOp::After, rest);
    }
    if let Some(rest) = lower.strip_prefix("due:") {
        return field_atom(DateField::Due, DueOp::On, rest);
    }

    if let Some(rest) = lower.strip_prefix("date.before:") {
        return field_atom(DateField::Date, DueOp::Before, rest);
    }
    if let Some(rest) = lower.strip_prefix("date.by:") {
        return field_atom(DateField::Date, DueOp::By, rest);
    }
    if let Some(rest) = lower.strip_prefix("date.after:") {
        return field_atom(DateField::Date, DueOp::After, rest);
    }
    if let Some(rest) = lower.strip_prefix("date:") {
        return field_atom(DateField::Date, DueOp::On, rest);
    }

    if let Some(rest) = lower.strip_prefix("from.before:") {
        return field_atom(DateField::From, DueOp::Before, rest);
    }
    if let Some(rest) = lower.strip_prefix("from.by:") {
        return field_atom(DateField::From, DueOp::By, rest);
    }
    if let Some(rest) = lower.strip_prefix("from.after:") {
        return field_atom(DateField::From, DueOp::After, rest);
    }
    if let Some(rest) = lower.strip_prefix("from:") {
        return field_atom(DateField::From, DueOp::On, rest);
    }

    if let Some(_rest) = lower.strip_prefix("status:") {
        return parse_status_atom(&lower).map(Expr::Atom);
    }

    if let Some(rest) = lower.strip_prefix("type:") {
        let sel = match rest {
            "todo" => TypeSel::Todo,
            "event" => TypeSel::Event,
            "all" => TypeSel::All,
            _ => bail!("unknown type `{rest}` (use todo, event, all)"),
        };
        return Ok(Expr::Atom(Filter {
            type_sel: Some(sel),
            ..Filter::default()
        }));
    }

    // `+NAME` 分支：先看是不是 `+todo`/`+event`（类型别名）、虚拟标签、
    // 最后才当作普通标签。判断顺序很重要。
    // `+NAME`: try type aliases, then virtual flags, then ordinary tags. Order matters.
    if tok.starts_with('+') && tok.len() > 1 {
        let name = &tok[1..];
        let lname = name.to_ascii_lowercase();
        let type_sel = match lname.as_str() {
            "todo" => Some(TypeSel::Todo),
            "event" => Some(TypeSel::Event),
            _ => None,
        };
        if let Some(sel) = type_sel {
            return Ok(Expr::Atom(Filter {
                type_sel: Some(sel),
                ..Filter::default()
            }));
        }
        if let Some(f) = virtual_flag(&lname) {
            return Ok(Expr::Atom(Filter {
                flags: vec![f],
                ..Filter::default()
            }));
        }
        // 不是内置名 → 当作真实标签过滤（保留原始大小写）。
        // Not a builtin name → treat as a real task tag (keep original casing).
        return Ok(Expr::Atom(Filter {
            tags: vec![name.to_string()],
            ..Filter::default()
        }));
    }

    bail!("unknown filter token `{tok}`")
}

/// 把虚拟标签的名字（小写）映射到 `Flag` 枚举。
/// Map a lowercased virtual-tag name to its `Flag` enum value.
fn virtual_flag(lname: &str) -> Option<Flag> {
    match lname {
        "overdue" => Some(Flag::Overdue),
        // `active` 别名已移除：`+ACTIVE` 现在按字面标签（literal tag）处理。
        // The `active` alias was removed; `+ACTIVE` is now a literal tag.
        "pending" => Some(Flag::Pending),
        "completed" | "done" => Some(Flag::Completed),
        "cancelled" | "canceled" => Some(Flag::Cancelled),
        "in-progress" | "inprogress" | "in-process" | "inprocess" => {
            Some(Flag::InProgress)
        }
        "started" => Some(Flag::Started),
        "tagged" => Some(Flag::Tagged),
        "untagged" => Some(Flag::Untagged),
        "due" => Some(Flag::Due),
        "parent" => Some(Flag::Parent),
        "waiting" => Some(Flag::Waiting),
        // `=> None` 表示“不是虚拟标签”。
        // `None` means the name is not a virtual tag.
        _ => None,
    }
}

/// 原子过滤的目标日期字段。
/// Which date field an atom targets.
#[derive(Clone, Copy)]
enum DateField {
    /// `due:` — 统一日期（todo→due，event→dtstart）。
    /// `due:` — unified date (todo→due, event→dtstart).
    Due,
    /// `date:` — 统一日期（与 `due` 相同，显式别名）。
    /// `date:` — unified date (same as `due`, explicit alias).
    Date,
    /// `from:` — 仅 VEVENT `dtstart`。
    /// `from:` — VEVENT `dtstart` only.
    From,
}

/// 把日期字段、比较运算符和解析出的时间组装成一个原子过滤器。
/// Build one atom filter from a date field, operator, and parsed datetime.
fn field_atom(field: DateField, op: DueOp, rest: &str) -> Result<Expr> {
    // `parse_datetime` 把自然语言日期（如 `today`）解析成具体时间，失败时 `?` 返回错误。
    // `parse_datetime` turns natural-language dates into instants; `?` propagates errors.
    let dt = parse_datetime(rest)?;
    // 根据字段类型，把 `(op, dt)` 放进对应的 Option 字段，其余字段保持默认。
    // `matches!` 是“是不是这个枚举成员”的简写；`.then_some` 在 true 时给出 Some。
    // Put the pair into the matching field; `matches!` + `.then_some` produce Some
    // only for the selected field.
    let f = Filter {
        due: (matches!(field, DateField::Due)).then_some((op, dt)),
        date: (matches!(field, DateField::Date)).then_some((op, dt)),
        from: (matches!(field, DateField::From)).then_some((op, dt)),
        ..Filter::default()
    };
    Ok(Expr::Atom(f))
}

/// 解析 `status:` 后面的值（字符串已小写）。
/// Parse the value after `status:` (input already lowercased).
fn parse_status_atom(lower: &str) -> Result<Filter> {
    let rest = lower.strip_prefix("status:").unwrap_or(lower);
    Ok(Filter {
        status: match rest {
            "active" => None,
            "pending" => Some(TaskStatus::Pending),
            "in-progress" | "inprogress" | "in-process" | "inprocess" | "started" => {
                Some(TaskStatus::InProgress)
            }
            "completed" | "done" => Some(TaskStatus::Completed),
            "cancelled" | "canceled" => Some(TaskStatus::Cancelled),
            "recurring" => Some(TaskStatus::Recurring),
            _ => bail!("unknown status `{rest}`"),
        },
        // 特殊：`status:active` 不是某个原始状态，而是单独一个布尔子句。
        // Special case: `status:active` is its own boolean clause, not a raw TaskStatus.
        active: rest == "active",
        ..Filter::default()
    })
}

#[cfg(test)]
// 测试模块：验证解析与匹配逻辑，学习时可先跳过。
// Tests only; safe to skip while learning.
mod tests {
    use super::*;
    use crate::model::Task;
    use chrono::Duration;

    fn todo(overdue_days: i64) -> Task {
        let mut t = Task::new("work", "x");
        t.due = Some(Utc::now() - Duration::days(overdue_days));
        t
    }

    fn future_event() -> Task {
        let mut t = Task::new("work", "meet");
        t.dtstart = Some(Utc::now() + Duration::days(1));
        t
    }

    #[test]
    fn empty_matches_all() {
        let e = parse_expr(&[]).unwrap();
        assert!(e.matches(&Task::new("work", "any")));
    }

    #[test]
    fn started_flag_matches_started_at() {
        use chrono::Utc;
        let mut t = todo(0);
        let e = parse_expr_str("+STARTED").unwrap();
        assert!(!e.matches(&t));
        t.started_at = Some(Utc::now());
        assert!(e.matches(&t));

        // `status:started` stays an in-progress alias (status-based).
        let s = parse_expr_str("status:started").unwrap();
        let mut u = todo(0);
        u.status = TaskStatus::InProgress;
        assert!(s.matches(&u));
        assert!(!s.matches(&todo(0)));
    }

    #[test]
    fn overdue_wait_stays_waiting() {
        use chrono::{Duration, Utc};
        let w = parse_expr_str("+WAITING").unwrap();
        let o = parse_expr_str("+OVERDUE").unwrap();
        let mut t = todo(0);
        // Overdue timed todo with a +1d wait window: overdue AND waiting.
        t.due = Some(Utc::now() - Duration::hours(2));
        t.wait = Some(86_400);
        assert!(o.matches(&t));
        assert!(w.matches(&t));
        // A future instance with the same wait is also waiting, not overdue.
        let mut f = todo(0);
        f.due = Some(Utc::now() + Duration::hours(2));
        f.wait = Some(86_400);
        assert!(!o.matches(&f));
        assert!(w.matches(&f));
    }

    #[test]
    fn status_and_tags() {
        let mut t = Task::new("work", "x");
        t.status = TaskStatus::InProgress;
        t.tags = vec!["urgent".into()];
        let e = parse_expr(&["status:in-progress".into(), "+urgent".into()]).unwrap();
        assert!(e.matches(&t));
    }

    #[test]
    fn and_or_bools() {
        let mut t = Task::new("work", "x");
        t.tags = vec!["a".into()];
        let e = parse_expr_str("+a or +b").unwrap();
        assert!(e.matches(&t));
        let e2 = parse_expr_str("+a and +b").unwrap();
        assert!(!e2.matches(&t));
    }

    #[test]
    fn parens_grouping() {
        let mut t = Task::new("work", "x");
        t.due = Some(Utc::now() + Duration::days(1));
        let e = parse_expr_str("+z or (+x due.after:today)").unwrap();
        assert!(!e.matches(&t));
    }

    #[test]
    fn source_filter() {
        let t = Task::new("work", "x");
        let e = parse_expr_str("source:work").unwrap();
        assert!(e.matches(&t));
        let e2 = parse_expr_str("-source:work").unwrap();
        assert!(!e2.matches(&t));
    }

    #[test]
    fn due_exact_only() {
        // due:today = same calendar day
        let e = parse_expr_str("due:today").unwrap();
        assert!(e.matches(&todo(0)));
        assert!(!e.matches(&todo(1)));
    }

    #[test]
    fn due_before_by_after() {
        let t = todo(10); // due in past
        assert!(parse_expr_str("due.before:now").unwrap().matches(&t));
        assert!(parse_expr_str("due.by:now").unwrap().matches(&t));
        assert!(!parse_expr_str("due.after:now").unwrap().matches(&t));
    }

    #[test]
    fn date_filter_uses_due_for_todo_dtstart_for_event() {
        // todo: date = due (past) → after:now false, before:now true
        let t = todo(2);
        assert!(!parse_expr_str("date.after:now").unwrap().matches(&t));
        assert!(parse_expr_str("date.before:now").unwrap().matches(&t));
        assert!(parse_expr_str("date.by:now").unwrap().matches(&t));
        // event: date = dtstart (future) → after:now true
        let ev = future_event();
        assert!(parse_expr_str("date.after:now").unwrap().matches(&ev));
        assert!(!parse_expr_str("date.before:now").unwrap().matches(&ev));
        // todo without any date never matches date filters
        let bare = Task::new("work", "bare");
        assert!(!parse_expr_str("date:today").unwrap().matches(&bare));
        // negation
        assert!(parse_expr_str("-date.after:now").unwrap().matches(&t));
    }

    #[test]
    fn from_before_by_after() {
        let ev = future_event(); // dtstart in future (relative to `now`)
        assert!(!parse_expr_str("from.before:now").unwrap().matches(&ev));
        assert!(!parse_expr_str("from.by:now").unwrap().matches(&ev));
        assert!(parse_expr_str("from.after:now").unwrap().matches(&ev));
        // todos have no dtstart → never match from filters
        let t = todo(10);
        assert!(!parse_expr_str("from.after:now").unwrap().matches(&t));
        // negation
        assert!(parse_expr_str("-from.after:now").unwrap().matches(&t));
    }

    #[test]
    fn type_all() {
        assert!(
            !parse_expr_str("type:todo")
                .unwrap()
                .matches(&future_event())
        );
        assert!(
            parse_expr_str("type:event")
                .unwrap()
                .matches(&future_event())
        );
        assert!(parse_expr_str("type:all").unwrap().matches(&future_event()));
        assert!(
            !parse_expr_str("-type:event")
                .unwrap()
                .matches(&future_event())
        );
    }

    #[test]
    fn event_date_is_dtstart() {
        assert!(
            parse_expr_str("due.after:now")
                .unwrap()
                .matches(&future_event())
        );
    }

    #[test]
    fn builtin_list_filter_negates_status() {
        let mut done = Task::new("work", "x");
        done.status = TaskStatus::Completed;
        let e = parse_expr_str("-status:completed -status:cancelled").unwrap();
        assert!(!e.matches(&done));
        assert!(e.matches(&Task::new("work", "active")));
    }

    #[test]
    fn cli_tokens_parse_same_grammar() {
        let e = parse_expr(&["type:event".into(), "due.after:sod".into()]).unwrap();
        assert!(e.matches(&future_event()));
    }

    #[test]
    fn negated_pending_excludes_events() {
        let e = parse_expr_str("-PENDING").unwrap();
        assert!(!e.matches(&future_event()));
    }

    #[test]
    fn negated_due() {
        // Not(due.after:now): a past-due todo satisfies it.
        let e3 = parse_expr_str("-due.after:now").unwrap();
        assert!(e3.matches(&todo(10)));
        // Not(priority:H)
        let e4 = parse_expr_str("-priority:H").unwrap();
        assert!(e4.matches(&Task::new("work", "x")));
    }

    #[test]
    fn active_alias_removed_is_a_literal_tag() {
        // `+ACTIVE` is no longer a virtual alias of `+PENDING`; it is a tag.
        let e = parse_expr_str("+active").unwrap();
        assert!(!e.matches(&Task::new("work", "x")));
        let mut tagged = Task::new("work", "x");
        tagged.tags = vec!["active".into()];
        assert!(e.matches(&tagged));
    }

    #[test]
    fn due_flag_matches_todos_with_due_only() {
        let e = parse_expr_str("+DUE").unwrap();
        assert!(e.matches(&todo(10)));
        assert!(!e.matches(&Task::new("work", "bare"))); // no due
        assert!(!e.matches(&future_event())); // events have dtstart, not due
        // negation
        let ne = parse_expr_str("-DUE").unwrap();
        assert!(!ne.matches(&todo(10)));
        assert!(ne.matches(&Task::new("work", "bare")));
    }

    #[test]
    fn overdue_only_matches_todos() {
        let o = parse_expr_str("+OVERDUE").unwrap();
        // A VTODO with a past due matches.
        assert!(o.matches(&todo(10)));
        // A todo with a future due does not.
        let mut future = todo(0);
        future.due = Some(Utc::now() + Duration::days(1));
        assert!(!o.matches(&future));
        // Events never match, even with a past dtstart.
        let mut past_event = future_event();
        past_event.dtstart = Some(Utc::now() - Duration::days(1));
        assert!(!o.matches(&past_event));
    }

    #[test]
    fn scheduled_alias_removed_is_a_literal_tag() {
        // `+SCHEDULED` is no longer a virtual tag; it is a literal tag.
        let e = parse_expr_str("+scheduled").unwrap();
        assert!(!e.matches(&future_event()));
        let mut tagged = future_event();
        tagged.tags = vec!["scheduled".into()];
        assert!(e.matches(&tagged));
    }

    #[test]
    fn waiting_flag_future_hidden_now_visible() {
        use chrono::{Duration, Utc};
        let mut t = Task::new("work", "deferred");
        t.due = Some(Utc::now() + Duration::days(7));
        // wait = 1 day before due → still waiting now.
        t.wait = Some(-86_400);
        let e = parse_expr_str("+WAITING").unwrap();
        assert!(e.matches(&t));
        // wait already passed (e.g. due passed long ago) → not waiting.
        let mut t2 = t.clone();
        t2.wait = Some(-86_400 * 30);
        assert!(!e.matches(&t2));

        // Default report exclusion: `-WAITING` must not match waiting tasks.
        let hide = parse_expr_str("-WAITING").unwrap();
        assert!(!hide.matches(&t));
    }
}
