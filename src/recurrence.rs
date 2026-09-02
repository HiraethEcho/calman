//! # 重复规则解析 → 标准 RFC 5545 `RRULE`
//!
//! 中文说明：本模块把用户的重复表达式（`recur:`/`repeat:`）归一化为标准 `RRULE` 字符串，
//! 例如 `daily` → `FREQ=DAILY`。输出不含 `RRULE:` 前缀，由 `storage/ics.rs` 添加。
//!
//! 两种输入风格：
//! - 原始透传：以 `FREQ=`（或 `RRULE:`）开头的字符串原样转大写输出（兼容 iOS/Outlook）。
//! - 友好表达式：`daily|weekly|monthly|yearly`、`every N d|w|m|y`、
//!   `mon..sun`/`weekend`/`weekday`、`for N times`、`for N weeks`、
//!   `count:N`、`until:<date>`/`until:eoy`/`until:eom`。
//!
//! English: recurrence rule parsing → standard RFC 5545 `RRULE` value.
//! - Raw passthrough: any string starting with `FREQ=` (or `RRULE:`) is
//!   normalized to upper-case and emitted verbatim (iOS/Outlook compatible).
//! - Friendly expression: `daily|weekly|monthly|yearly`, `every N d|w|m|y`,
//!   `mon..sun` / `weekend` / `weekday`, `for N times`, `for N weeks`,
//!   `count:N`, `until:<date>` / `until:eoy` / `until:eom`.
//!
//! Output has no `RRULE:` prefix; `storage/ics.rs` adds it.

#[cfg(feature = "date-natural")]
use crate::date::{DateValue, parse_date_value};
use anyhow::{Result, bail};
#[cfg(feature = "date-natural")]
use chrono::{Datelike, Local, NaiveDate};

/// 把星期 token 映射为 `BYDAY` 代码（MO/TU/WE/TH/FR/SA/SU）。
/// 中文：先 `trim_end_matches('.')` 允许 `mon.` 这种带点缩写；
/// 再用 `starts_with` 匹配前几个字符，返回 `Option<&'static str>`（静态字符串）。
/// English: map a weekday token to its `BYDAY` code.
#[cfg(feature = "date-natural")]
fn weekday_code(t: &str) -> Option<&'static str> {
    let t = t.trim_end_matches('.');
    if t.starts_with("mon") {
        Some("MO")
    } else if t.starts_with("tue") {
        Some("TU")
    } else if t.starts_with("wed") {
        Some("WE")
    } else if t.starts_with("thu") {
        Some("TH")
    } else if t.starts_with("fri") {
        Some("FR")
    } else if t.starts_with("sat") {
        Some("SA")
    } else if t.starts_with("sun") {
        Some("SU")
    } else {
        None
    }
}

/// 把单位 token（`d`/`day`/`days`/`w`/`week`…）映射为 `FREQ` 值。
/// 中文：按首字母区分 DAILY/WEEKLY/MONTHLY/YEARLY，其余返回 `None`。
/// English: map a unit token to a `FREQ` value.
#[cfg(feature = "date-natural")]
fn freq_from_unit(s: &str) -> Option<&'static str> {
    let s = s.trim_end_matches('.');
    if s.starts_with('d') {
        Some("DAILY")
    } else if s.starts_with('w') {
        Some("WEEKLY")
    } else if s.starts_with('m') {
        Some("MONTHLY")
    } else if s.starts_with('y') {
        Some("YEARLY")
    } else {
        None
    }
}

/// 把 `7d`/`30days` 拆成 `(数量, 频率)`。
/// 中文：用 `char_indices()` 找到第一个非数字字符的下标，再 `split_at` 拆成数字与后缀。
/// 迭代器 `.find()` 返回满足条件的第一个位置，`.map()` 取出下标。
/// English: split `7d` / `30days` into `(count, freq)`.
#[cfg(feature = "date-natural")]
fn number_unit(t: &str) -> Option<(u32, &'static str)> {
    // 第一个非 ASCII 数字字符的位置 = 数字与后缀的分界点；
    // char_indices 产生 (byte_index, char) 对，按字节下标拆分安全。
    let split = t
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)?;
    if split == 0 {
        return None;
    }
    let (digits, suffix) = t.split_at(split);
    let n: u32 = digits.parse().ok()?;
    let freq = freq_from_unit(suffix)?;
    Some((n, freq))
}

/// 生成“今年最后一天”（`until:eoy` 的目标）为 `YYYYMMDD` 字符串。
/// 中文：`Local::now().year()` 取当前年，`from_ymd_opt` 构造 12-31（校验版，返回 Option）。
/// English: return end-of-year date as `YYYYMMDD`.
#[cfg(feature = "date-natural")]
fn eoy() -> String {
    let y = Local::now().year();
    NaiveDate::from_ymd_opt(y, 12, 31)
        .unwrap()
        .format("%Y%m%d")
        .to_string()
}

/// 生成“本月最后一天”（`until:eom` 的目标）为 `YYYYMMDD` 字符串。
/// 中文：取下月 1 日再 `pred_opt()`（前一天），12 月特判回 12-31。
/// English: return end-of-month date as `YYYYMMDD`.
#[cfg(feature = "date-natural")]
fn eom() -> String {
    let now = Local::now().date_naive();
    let (y, m) = (now.year(), now.month());
    let last = if m == 12 {
        NaiveDate::from_ymd_opt(y, 12, 31).unwrap()
    } else {
        NaiveDate::from_ymd_opt(y, m + 1, 1)
            .unwrap()
            .pred_opt()
            .unwrap()
    };
    last.format("%Y%m%d").to_string()
}

/// 解析 `until:` 的值（`eoy`/`eom` 或具体日期）为 `YYYYMMDD`。
/// 中文：先匹配两个关键字，否则交给 `parse_date_value`；
/// 时间值会转到本地时区再取日期（`with_timezone(&Local).date_naive()`）。
/// English: parse an `until:` value into a `YYYYMMDD` date string.
#[cfg(feature = "date-natural")]
fn parse_until(v: &str) -> Result<String> {
    match v {
        "eoy" => return Ok(eoy()),
        "eom" => return Ok(eom()),
        _ => {}
    }
    let dv = parse_date_value(v)?;
    let date = match dv {
        DateValue::Date(d) => d,
        DateValue::Time(dt) => dt.with_timezone(&Local).date_naive(),
    };
    Ok(date.format("%Y%m%d").to_string())
}

/// 把用户重复表达式归一化为标准 `RRULE` 值。
///
/// 中文：优先处理三种“原始/透传”形式——`FREQ=` 透传、`RRULE:` 去前缀、ISO 周期（`P7D`）。
/// 开启 `date-natural` 时，先尝试自然语言解析，失败再退回内置友好解析器；
/// 未开启时只接受 `FREQ=`/`RRULE:`。
///
/// - Raw `FREQ=` / `RRULE:` passthrough (always available).
/// - With `date-natural`: also accept natural language via [`text2rrule`],
///   falling back to the built-in friendly parser for forms it doesn't cover
///   (e.g. `until:eoy`, `for N times`).
/// - Without `date-natural`: only raw `FREQ=`/`RRULE:` are accepted.
pub fn normalize_recurrence(input: &str) -> Result<String> {
    let s = input.trim();
    if s.is_empty() {
        bail!("empty recurrence");
    }
    let up = s.to_uppercase();
    if up.starts_with("FREQ=") {
        return Ok(up);
    }
    if let Some(v) = up.strip_prefix("RRULE:") {
        return Ok(v.to_string());
    }
    // ISO 周期（`P7D`/`P2W`/`P1M`/`P1Y`）转 RRULE：不依赖 date-natural，始终可用。
    if let Some(r) = iso_period_to_rrule(&up) {
        return Ok(r);
    }
    // 条件编译：`#[cfg(feature = "date-natural")]` 决定启用自然语言还是报错。
    #[cfg(feature = "date-natural")]
    {
        if let Ok(r) = crate::date::natural_to_rrule(s) {
            return Ok(r);
        }
        normalize_friendly(s)
    }
    #[cfg(not(feature = "date-natural"))]
    {
        bail!(
            "only RFC 5545 RRULE accepted in this build (enable `date-natural` for \
             natural-language recurrence): `{input}`"
        )
    }
}

/// 把 ISO 8601 周期（`P7D`/`P2W`/`P1M`/`P1Y`）映射为 `RRULE`。
/// 中文：去掉 `P` 后，最后一位是单位，前面是数字；`n==1` 时省略 `INTERVAL`。
/// 始终可用（不依赖 `date-natural`）。
/// Map an ISO 8601 period (`P7D`, `P2W`, `P1M`, `P1Y`) to an `RRULE`.
/// Always available (no `date-natural` needed).
fn iso_period_to_rrule(up: &str) -> Option<String> {
    let body = up.strip_prefix('P')?;
    if body.len() < 2 {
        return None;
    }
    // split_at 末尾一个字符：`P7D` → 数字 `7` + 单位 `D`。
    let (num, unit) = body.split_at(body.len() - 1);
    let n: u32 = num.parse().ok()?;
    if n == 0 {
        return None;
    }
    let freq = match unit {
        "D" => "DAILY",
        "W" => "WEEKLY",
        "M" => "MONTHLY",
        "Y" => "YEARLY",
        _ => return None,
    };
    if n == 1 {
        Some(format!("FREQ={freq}"))
    } else {
        Some(format!("FREQ={freq};INTERVAL={n}"))
    }
}

/// 把 `RRULE` 渲染回 ISO 8601 周期（`P7D`/`P2W`/`P1M`/`P1Y`）。
/// 中文：没有 `FREQ` 的非周期规则直接返回原始文本（fallback）。
/// 这里多次用 `.split(';').find_map(...)`：按分号拆段，寻找并提取指定键，
/// 是“在集合里找某项”的惯用迭代器写法。
/// Render an `RRULE` as an ISO 8601 period (`P7D`, `P2W`, `P1M`, `P1Y`).
/// Non-periodic rules (no `FREQ`) fall back to the raw RRULE text.
pub fn rrule_period(rrule: &str) -> String {
    let up = rrule.to_uppercase();
    // 找 `FREQ=` 段 → 决定单位字母；找不到时 `find_map` 返回 None。
    let freq = up.split(';').find_map(|p| p.strip_prefix("FREQ="));
    // `INTERVAL=` 段解析为 u32；缺省时下面用 unwrap_or(1)，即“每 1 个周期”。
    let interval = up
        .split(';')
        .find_map(|p| p.strip_prefix("INTERVAL="))
        .and_then(|v| v.parse::<u32>().ok());
    let unit = match freq {
        Some("DAILY") => "D",
        Some("WEEKLY") => "W",
        Some("MONTHLY") => "M",
        Some("YEARLY") => "Y",
        _ => return rrule.to_string(),
    };
    format!("P{}{}", interval.unwrap_or(1), unit)
}

/// 内置友好重复解析器（`date-natural` 下的兜底实现）。
/// 中文：这是一个小型“分词 + 逐个 token 匹配”的解析器，
/// 先用 `split` + 迭代器把输入切成 token（去空白/逗号，丢弃 `and`/`every` 虚词），
/// 再循环按关键字填 `freq`/`interval`/`byday`/`count`/`until`，最后拼成 RRULE。
/// Built-in friendly recurrence parser (fallback used under `date-natural`).
#[cfg(feature = "date-natural")]
fn normalize_friendly(input: &str) -> Result<String> {
    let s = input.trim();
    if s.is_empty() {
        bail!("empty recurrence");
    }
    // 分词：空白或逗号分隔；`map` 去首尾空白，`filter` 丢弃空串与连接词。
    // 收集进 `Vec<String>` 后可以按下标访问 i/i+1，便于处理 `for 5 times`。
    let toks: Vec<String> = s
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty() && t != "and" && t != "every")
        .collect();

    // 解析状态：频率、间隔、星期几集合、次数上限、截止日期。
    // Option<T> 初始为 None，遇到对应 token 时才填入（“有没有”由类型本身表达）。
    let mut freq: Option<&str> = None;
    let mut interval: Option<u32> = None;
    let mut byday: Vec<&str> = Vec::new();
    let mut count: Option<u32> = None;
    let mut until: Option<String> = None;

    // 主扫描循环：`i` 是当前 token 下标，某些分支会 `i += 1`/`+= 2` 跳过已消费的 token。
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        // 按 token 内容分发。`weekend`/`weekday` 同时填 BYDAY 并把默认频率设为 WEEKLY。
        match t.as_str() {
            "daily" | "day" | "days" => freq = Some("DAILY"),
            "weekly" | "week" | "weeks" => freq = Some("WEEKLY"),
            "monthly" | "month" | "months" => freq = Some("MONTHLY"),
            "yearly" | "year" | "years" | "annual" => freq = Some("YEARLY"),
            "weekend" => {
                byday.extend_from_slice(&["SA", "SU"]);
                freq = freq.or(Some("WEEKLY"));
            }
            "weekday" => {
                byday.extend_from_slice(&["MO", "TU", "WE", "TH", "FR"]);
                freq = freq.or(Some("WEEKLY"));
            }
            "for" => {
                // `for N times`：解析后两个 token（数量 + 单位），成功则 `i += 2` 跳过。
                // let-chains：`if let (Some(next), Some(unit)) = ... && let Ok(n) = ...` 多条件合一。
                if let (Some(next), Some(unit)) = (toks.get(i + 1), toks.get(i + 2))
                    && let Ok(n) = next.parse::<u32>()
                {
                    match unit.as_str() {
                        "times" | "time" => count = Some(n),
                        // `for 7 weeks`：每周按 BYDAY 个数发生 → 总次数 = 周数 × 每周发生次数。
                        "weeks" | "week" => {
                            let mult = if byday.is_empty() {
                                1
                            } else {
                                byday.len() as u32
                            };
                            count = Some(n * mult);
                        }
                        _ => count = Some(n),
                    }
                    i += 2;
                }
            }
            "times" | "time" => {}
            "count" => {
                if let Some(next) = toks.get(i + 1) {
                    let v = next.strip_prefix(':').unwrap_or(next);
                    if let Ok(n) = v.parse::<u32>() {
                        count = Some(n);
                    }
                }
            }
            "until" => {
                if let Some(next) = toks.get(i + 1) {
                    let v = next.strip_prefix(':').unwrap_or(next);
                    until = Some(parse_until(v)?);
                    i += 1;
                }
            }
            "eoy" => until = Some(eoy()),
            "eom" => until = Some(eom()),
            _ => {
                // 默认分支处理 `until:`/`count:` 前缀、星期 token、`7d` 数字单位、纯数字、`x5` 次数。
                if let Some(v) = t.strip_prefix("until:") {
                    until = Some(parse_until(v)?);
                } else if let Some(v) = t.strip_prefix("count:") {
                    if let Ok(n) = v.parse::<u32>() {
                        count = Some(n);
                    }
                } else if let Some(code) = weekday_code(t) {
                    byday.push(code);
                    freq = freq.or(Some("WEEKLY"));
                } else if let Some((n, f)) = number_unit(t) {
                    freq = freq.or(Some(f));
                    interval = Some(n);
                } else if let Ok(n) = t.parse::<u32>() {
                    if let Some(unit) = toks.get(i + 1)
                        && let Some(f) = freq_from_unit(unit)
                    {
                        freq = freq.or(Some(f));
                        interval = Some(n);
                        i += 1;
                    }
                } else if let Some(stripped) = t.strip_prefix('x')
                    && let Ok(n) = stripped.parse::<u32>()
                {
                    count = Some(n);
                }
            }
        }
        i += 1;
    }

    // 频率是必需的；都没有就报错（错误信息面向用户，包含原始输入）。
    let freq = freq.ok_or_else(|| {
        anyhow::anyhow!("recurrence needs a frequency (daily/weekly/...): `{input}`")
    })?;

    // 开始拼装 RRULE：频率必填，`INTERVAL` 只有 >1 才写（1 是默认值，省略更干净）。
    let mut parts = vec![format!("FREQ={freq}")];
    if let Some(n) = interval
        && n > 1
    {
        parts.push(format!("INTERVAL={n}"));
    }
    if !byday.is_empty() {
        // 用 HashSet 去重（如 `tuesday and tuesday`），再按原顺序收集唯一值。
        // `seen.insert(c)` 返回 bool：true 表示首次出现，才 push 进 uniq。
        let mut seen = std::collections::HashSet::new();
        let mut uniq = Vec::new();
        for c in byday {
            if seen.insert(c) {
                uniq.push(c);
            }
        }
        parts.push(format!("BYDAY={}", uniq.join(",")));
    }
    // COUNT 与 UNTIL 互斥场合由调用方取舍；这里按出现情况分别追加。
    if let Some(n) = count {
        parts.push(format!("COUNT={n}"));
    }
    if let Some(u) = until {
        parts.push(format!("UNTIL={u}"));
    }
    // 用 `;` 连接各个部件：`FREQ=DAILY;INTERVAL=2;BYDAY=MO,FR`。
    Ok(parts.join(";"))
}

#[cfg(all(test, feature = "date-natural"))]
// 测试模块：验证自然语言重复解析，解释从略（保持原样）。
mod tests {
    use super::*;

    fn r(s: &str) -> String {
        normalize_recurrence(s).unwrap()
    }

    #[test]
    fn raw_passthrough() {
        assert_eq!(r("FREQ=DAILY;UNTIL=20260925"), "FREQ=DAILY;UNTIL=20260925");
        assert_eq!(r("rrule:FREQ=WEEKLY"), "FREQ=WEEKLY");
    }

    #[test]
    fn basic_freq() {
        assert_eq!(r("daily"), "FREQ=DAILY");
        assert_eq!(r("weekly"), "FREQ=WEEKLY");
        assert_eq!(r("monthly"), "FREQ=MONTHLY");
        assert_eq!(r("yearly"), "FREQ=YEARLY");
    }

    #[test]
    fn interval() {
        assert_eq!(r("every 7d"), "FREQ=DAILY;INTERVAL=7");
        assert_eq!(r("7d"), "FREQ=DAILY;INTERVAL=7");
        assert_eq!(r("every 2 weeks"), "FREQ=WEEKLY;INTERVAL=2");
        assert_eq!(r("30d"), "FREQ=DAILY;INTERVAL=30");
    }

    #[test]
    fn byday() {
        assert_eq!(r("every tuesday and friday"), "FREQ=WEEKLY;BYDAY=TU,FR");
        assert_eq!(r("every weekend"), "FREQ=WEEKLY;BYDAY=SA,SU");
    }

    #[test]
    fn count_and_until() {
        assert_eq!(
            r("every friday for 5 times"),
            "FREQ=WEEKLY;BYDAY=FR;COUNT=5"
        );
        assert_eq!(
            r("every tuesday and friday for 7 weeks"),
            "FREQ=WEEKLY;BYDAY=TU,FR;COUNT=14"
        );
        assert_eq!(r("daily until:20260925"), "FREQ=DAILY;UNTIL=20260925");
        assert_eq!(
            r("daily until:eoy").len(),
            "FREQ=DAILY;UNTIL=20261231".len()
        );
    }
}

#[cfg(test)]
// 测试模块：验证 ISO 周期映射，解释从略（保持原样）。
mod iso_tests {
    use super::*;

    #[test]
    fn iso_period_to_rrule_mapping() {
        assert_eq!(iso_period_to_rrule("P7D"), Some("FREQ=DAILY;INTERVAL=7".into()));
        assert_eq!(iso_period_to_rrule("P2W"), Some("FREQ=WEEKLY;INTERVAL=2".into()));
        assert_eq!(iso_period_to_rrule("P1M"), Some("FREQ=MONTHLY".into()));
        assert_eq!(iso_period_to_rrule("P1Y"), Some("FREQ=YEARLY".into()));
        assert_eq!(iso_period_to_rrule("P0D"), None);
        assert_eq!(iso_period_to_rrule("PT15M"), None);
    }

    #[test]
    fn normalize_recurrence_accepts_iso_period_without_natural() {
        assert_eq!(
            normalize_recurrence("P7D").unwrap(),
            "FREQ=DAILY;INTERVAL=7"
        );
        assert_eq!(normalize_recurrence("p2w").unwrap(), "FREQ=WEEKLY;INTERVAL=2");
    }

    #[test]
    fn rrule_period_rendering() {
        assert_eq!(rrule_period("FREQ=DAILY;INTERVAL=7"), "P7D");
        assert_eq!(rrule_period("FREQ=WEEKLY;BYDAY=TU,FR"), "P1W");
        assert_eq!(rrule_period("FREQ=MONTHLY;INTERVAL=3"), "P3M");
        assert_eq!(rrule_period("FREQ=YEARLY"), "P1Y");
        assert_eq!(rrule_period("garbage"), "garbage");
    }
}
