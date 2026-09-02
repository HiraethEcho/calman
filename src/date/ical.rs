//! iCalendar 风格紧凑日期解析（feature `date-ical`）。
//! iCalendar-style compact date parsing (feature `date-ical`).
//!
//! 解析用户输入的日期/时间字符串，风格参照 Taskwarrior 命名日期，并支持
//! RFC 5545 的 `YYYYMMDDTHHMMSS` 紧凑写法（`T` 分隔日期与时间）：
//! `20260828T090000` 是完整日期时间；`20260828`（8 位）是全天日期
//! （all-day）；少于 8 位（`0823`、`25`）当作 `YYYYMMDD` 的末尾几位，
//! 前缀用今天的年月日补齐（`0823`→2026-08-23，`25`→2026-08-25）；
//! `0828T0900`/`25T` 用 `T` 前的数字当末尾日期、`T` 后的数字当 `HHMMSS`；
//! `T0900`/`T09` 用今天日期加 `HHMMSS`（自动补零，单独 `T` → 今天 00:00）。
//! 也支持：`YYYY-MM-DD`、`YYYY-MM-DD HH:MM`、`HH:MM`（今天）、`now`、
//! 命名日边界（`eod`/`sow`/`eom`…）和相对时间 `+3d`/`-2w`/`+1h`。
//! 所有本地时间输入通过 `local_to_utc` 转成 UTC（DST 安全、CalDAV 安全）。

use anyhow::{Result, bail};
use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, Months, NaiveDate, NaiveDateTime, NaiveTime, Utc,
};

/// 解析后的日期值：纯日期（可能是全天候选，all-day）或具体日期时间。
/// A parsed date: date-only (all-day candidate) or a concrete date-time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateValue {
    /// 只有日期，没有时刻（例如 `20260824`、`today`）。
    Date(NaiveDate),
    /// 精确到时刻的 UTC 时间（例如 `20260828T090000`、`now`）。
    Time(DateTime<Utc>),
}

/// 把用户日期/时间表达式解析成 UTC 时间戳。
/// Parse a user date/time expression into a UTC timestamp.
/// Date-only forms land at local midnight (todo `due` semantics).
/// 纯日期形式会落到本地午夜（待办 `due` 的语义）。
pub fn parse_datetime(input: &str) -> Result<DateTime<Utc>> {
    match parse_date_value(input)? {
        DateValue::Date(d) => Ok(local_midnight(d)),
        DateValue::Time(dt) => Ok(dt),
    }
}

/// 解析日期表达式，区分「只有日期」和「具体日期时间」两种形态。
/// Parse a date expression, distinguishing date-only from date-time forms.
pub fn parse_date_value(input: &str) -> Result<DateValue> {
    // 去掉首尾空格并转小写，让 `TODAY`/`T0900` 也能匹配。
    let s = input.trim().to_lowercase();
    let now = Local::now(); // 当前本地时间，作为「今天/补齐前缀」的基准。

    // Named dates that are pure day boundaries → date-only.
    // 纯日边界的命名词：只返回日期（Date），不包含时刻。
    const DATE_ONLY_NAMED: &[&str] = &[
        "today",
        "tomorrow",
        "sond",
        "yesterday",
    ];
    if DATE_ONLY_NAMED.contains(&s.as_str()) {
        let dt = named_date(&s).expect("named date");
        return Ok(DateValue::Date(dt.with_timezone(&Local).date_naive()));
    }
    if let Some(dt) = named_date(&s) {
        return Ok(DateValue::Time(dt));
    }

    // 相对偏移：`+3d`、`-2w`、`+1m`、`+1y`、`+2h`、`-1s`（相对当前时刻）。
    if let Some(rest) = s.strip_prefix(['+', '-']) {
        let unit = rest.chars().last().unwrap_or('d');
        let num = rest.strip_suffix(unit).unwrap_or(rest);
        if !unit.is_ascii_digit()
            && let Ok(n) = num.parse::<i64>()
        {
            let sign: i64 = if s.starts_with('+') { 1 } else { -1 };
            let now_utc = Utc::now();
            let delta = match unit {
                'd' => Duration::days(sign * n),
                'w' => Duration::weeks(sign * n),
                'h' => Duration::hours(sign * n),
                's' => Duration::seconds(sign * n),
                'm' => Duration::days(sign * n * 30),
                'y' => Duration::days(sign * n * 365),
                _ => bail!("unknown relative unit `{unit}` in `{input}`"),
            };
            return Ok(DateValue::Time(now_utc + delta));
        }
    }

    // "YYYY-MM-DD HH:MM"
    if let Ok(ndt) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M") {
        return Ok(DateValue::Time(local_to_utc(ndt)));
    }

    // 带 `T` 的紧凑形式：`T` 前是日期部分，`T` 后是时间部分。
    if let Some(idx) = s.find('t') {
        let before = &s[..idx];
        let after = &s[idx + 1..];
        let date = resolve_compact_date(before, now)?;
        let time = resolve_compact_time(after)?;
        return Ok(DateValue::Time(local_to_utc(date.and_time(time))));
    }

    // 纯数字且没有 `T`：8 位是 `YYYYMMDD`，更短则用今天补齐前缀。
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
        if s.len() == 8 {
            let d = NaiveDate::parse_from_str(&s, "%Y%m%d")?;
            return Ok(DateValue::Date(d));
        }
        // <8 digits → trailing digits of YYYYMMDD, prefix filled from today.
        let d = resolve_compact_date(&s, now)?;
        return Ok(DateValue::Date(d));
    }

    // "YYYY-MM-DD"
    if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
        return Ok(DateValue::Date(d));
    }

    // "MM-DD" → 当前年份，例如 `08-26`、`9-30`。
    if let Some((m, d)) = s.split_once('-')
        && let (Ok(m), Ok(d)) = (m.parse::<u32>(), d.parse::<u32>())
        && let Some(dt) = NaiveDate::from_ymd_opt(now.date_naive().year(), m, d)
    {
        return Ok(DateValue::Date(dt));
    }

    // "HH:MM" → 今天该时刻（例如 `18:30`）。
    if let Ok(t) = NaiveTime::parse_from_str(&s, "%H:%M") {
        let ndt = now.date_naive().and_time(t);
        return Ok(DateValue::Time(local_to_utc(ndt)));
    }

    bail!("could not parse date `{input}` (try 20260812, 0826, T0900, today, eow, +3d)")
}

/// 把紧凑日期字符串解析成 `NaiveDate`（无时区的日历日期）。
/// Resolve a compact date string to a `NaiveDate`.
/// - 空字符串 → 今天
/// - ≥8 位 → 取前 8 位当作 `YYYYMMDD`
/// - <8 位 → 当作今天 `YYYYMMDD` 的末尾数字（前缀取自今天）
fn resolve_compact_date(digits: &str, now: DateTime<Local>) -> Result<NaiveDate> {
    if digits.is_empty() {
        return Ok(now.date_naive());
    }
    if digits.len() >= 8 {
        return Ok(NaiveDate::parse_from_str(&digits[..8], "%Y%m%d")?);
    }
    // `25` → 今天的 `20260825` 的后 2 位 → 2026年08月25日。
    let today = now.format("%Y%m%d").to_string();
    let n = digits.len();
    let prefix = &today[..8 - n];
    let full = format!("{prefix}{digits}");
    Ok(NaiveDate::parse_from_str(&full, "%Y%m%d")?)
}

/// 把紧凑时间字符串解析成 `NaiveTime`（无时区的钟表时间）。
/// Resolve a compact time string to a `NaiveTime`.
/// 数字是 `HH[MM[SS]]`（左对齐、自动补零）：`09`→09:00:00，
/// `0930`→09:30:00，`090000`→完整，空→00:00:00。
fn resolve_compact_time(digits: &str) -> Result<NaiveTime> {
    // 也接受冒号形式（`09:30`、`9:30`）：去掉冒号后还是左对齐的
    // `HH[MM[SS]]`；3 位数字表示 `HMM`（`930` → 09:30），不是 93 小时。
    let cleaned: String = digits.chars().filter(|c| *c != ':').collect();
    let cleaned = if cleaned.len() == 3 {
        format!("0{cleaned}")
    } else {
        cleaned
    };
    if cleaned.is_empty() {
        return Ok(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    }
    let hh = cleaned.get(..2).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
    let mm = cleaned.get(2..4).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
    let ss = cleaned.get(4..6).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
    NaiveTime::from_hms_opt(hh, mm, ss).ok_or_else(|| anyhow::anyhow!("bad time `{digits}`"))
}

/// 解析人类可读时长：`45min`、`1h`、`1h30m`、`2d`、`90`、`1w`。
/// Parse a human duration: `45min`, `1h`, `1h30m`, `2d`, `90`, `1w`.
/// 裸数字表示分钟（bare numbers mean minutes）。
pub fn parse_duration(input: &str) -> Result<Duration> {
    let s = input.trim().to_lowercase();
    if s.is_empty() {
        bail!("empty duration");
    }
    if let Some(d) = parse_iso_duration(&s) {
        return Ok(d);
    }
    // 逐字符扫描：数字进 `num`，字母进 `unit`。遇到新数字时说明上一个
    // 「数字+单位」片段结束了，先推入 `parts`。
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut num = String::new();
    let mut unit = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            if !unit.is_empty() {
                parts.push((std::mem::take(&mut num), std::mem::take(&mut unit)));
            }
            num.push(ch);
        } else {
            unit.push(ch);
        }
    }
    if !num.is_empty() {
        parts.push((num, unit));
    } else if let Some(last) = parts.last_mut() {
        // 只有末尾单位没有数字的情况（不应该发生，防御性兜底）。
        last.1 = unit;
    }
    if parts.is_empty() {
        bail!("could not parse duration `{input}` (try 45min, 1h, 1h30m, 2d)");
    }
    let mut total = Duration::zero();
    for (n, u) in parts {
        let n: i64 = n
            .parse()
            .map_err(|_| anyhow::anyhow!("bad duration number `{n}`"))?;
        let d = match u.as_str() {
            "" | "m" | "min" | "mins" | "minute" | "minutes" => Duration::minutes(n),
            "h" | "hr" | "hrs" | "hour" | "hours" => Duration::hours(n),
            "d" | "day" | "days" => Duration::days(n),
            "w" | "week" | "weeks" => Duration::weeks(n),
            "s" | "sec" | "secs" | "second" | "seconds" => Duration::seconds(n),
            _ => bail!("unknown duration unit `{u}`"),
        };
        total += d;
    }
    Ok(total)
}

/// 解析 ISO 8601 时长（`PT15M`、`PT1H30M`、`P7D`、`P2W`、`PT45S`）。
/// Parse an ISO 8601 duration (`PT15M`, `PT1H30M`, `P7D`, `P2W`, `PT45S`).
/// 支持单位：日期部分 D/W，时间部分 H/M/S（`M` 指分钟）。
fn parse_iso_duration(s: &str) -> Option<Duration> {
    let body = s.strip_prefix('p')?; // 必须以 `P` 开头，否则直接返回 None。
    let mut total = Duration::zero();
    let mut num = String::new();
    let mut any = false;
    for ch in body.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
        } else if ch == 't' {
            // `T` 是「日期部分」与「时间部分」的分界符，数字重新开始累计。
            num.clear();
        } else {
            // 遇到单位字母：把之前累计的数字换算成对应时长并累加。
            let n: i64 = num.parse().ok()?;
            let d = match ch {
                'd' => Duration::days(n),
                'w' => Duration::weeks(n),
                'h' => Duration::hours(n),
                'm' => Duration::minutes(n),
                's' => Duration::seconds(n),
                _ => return None,
            };
            total += d;
            num.clear();
            any = true;
        }
    }
    any.then_some(total)
}

/// 命名日期。周一是一周的开始（calman 默认）；`eoww` 使用 `day_end` 作为时刻。
/// Named dates. Week starts Monday (calman default); `eoww` uses `day_end`.
fn named_date(s: &str) -> Option<DateTime<Utc>> {
    let today = Local::now().date_naive();
    let week_start = monday_of(today);
    // 三个闭包分别算出「月/季/年」的首日与末日，用于 `som`/`eom`/`soq`…
    let month = |d: NaiveDate| -> (NaiveDate, NaiveDate) {
        let first = NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap();
        let last = first.checked_add_months(Months::new(1)).unwrap() - Duration::days(1);
        (first, last)
    };
    let quarter = |d: NaiveDate| -> (NaiveDate, NaiveDate) {
        let q_first_month = ((d.month() - 1) / 3) * 3 + 1;
        let first = NaiveDate::from_ymd_opt(d.year(), q_first_month, 1).unwrap();
        let last = first.checked_add_months(Months::new(3)).unwrap() - Duration::days(1);
        (first, last)
    };
    let year = |d: NaiveDate| -> (NaiveDate, NaiveDate) {
        (
            NaiveDate::from_ymd_opt(d.year(), 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(d.year(), 12, 31).unwrap(),
        )
    };

    // 取出配置的当日开始/结束时刻：`sod`（start of day）用开始时刻，
    // `eod`（end of day）用结束时刻。
    let ((sh, sm, ss), (eh, em, es)) = super::day_bounds();
    // 每个命名词返回 (首日, 末日, 时刻)；这里只用 first + tm 构造日期时间。
    let (first, _last, tm) = match s {
        "now" => return Some(Utc::now()),
        "today" | "sod" => (today, today, (sh, sm, ss)),
        "tomorrow" | "sond" => (
            today + Duration::days(1),
            today + Duration::days(1),
            (sh, sm, ss),
        ),
        "yesterday" => (
            today - Duration::days(1),
            today - Duration::days(1),
            (sh, sm, ss),
        ),
        "eod" => (today, today, (eh, em, es)),
        "eond" => (
            today + Duration::days(1),
            today + Duration::days(1),
            (eh, em, es),
        ),
        "sopd" => (
            today - Duration::days(1),
            today - Duration::days(1),
            (sh, sm, ss),
        ),
        "eopd" => (
            today - Duration::days(1),
            today - Duration::days(1),
            (eh, em, es),
        ),
        "sow" | "soww" => (week_start, week_start, (sh, sm, ss)),
        "eow" => (
            week_start + Duration::days(6),
            week_start + Duration::days(6),
            (eh, em, es),
        ),
        "eoww" => (
            week_start + Duration::days(4),
            week_start + Duration::days(4),
            (eh, em, es),
        ),
        "sonw" | "sonww" => (
            week_start + Duration::days(7),
            week_start + Duration::days(7),
            (sh, sm, ss),
        ),
        "eonw" | "eonww" => (
            week_start + Duration::days(13),
            week_start + Duration::days(13),
            (eh, em, es),
        ),
        "sopw" | "sopww" => (
            week_start - Duration::days(7),
            week_start - Duration::days(7),
            (sh, sm, ss),
        ),
        "eopw" | "eopww" => (
            week_start - Duration::days(1),
            week_start - Duration::days(1),
            (eh, em, es),
        ),
        "som" => {
            let (f, _) = month(today);
            (f, f, (sh, sm, ss))
        }
        "eom" => {
            let (_, l) = month(today);
            (l, l, (eh, em, es))
        }
        "sonm" => {
            let (f, _) = month(today.checked_add_months(Months::new(1))?);
            (f, f, (sh, sm, ss))
        }
        "eonm" => {
            let (_, l) = month(today.checked_add_months(Months::new(1))?);
            (l, l, (eh, em, es))
        }
        "sopm" => {
            let (f, _) = month(today.checked_sub_months(Months::new(1))?);
            (f, f, (sh, sm, ss))
        }
        "eopm" => {
            let (_, l) = month(today.checked_sub_months(Months::new(1))?);
            (l, l, (eh, em, es))
        }
        "soq" => {
            let (f, _) = quarter(today);
            (f, f, (sh, sm, ss))
        }
        "eoq" => {
            let (_, l) = quarter(today);
            (l, l, (eh, em, es))
        }
        "soy" => {
            let (f, _) = year(today);
            (f, f, (sh, sm, ss))
        }
        "eoy" => {
            let (_, l) = year(today);
            (l, l, (eh, em, es))
        }
        "sony" => {
            let (f, _) = year(today.checked_add_months(Months::new(12))?);
            (f, f, (sh, sm, ss))
        }
        "eony" => {
            let (_, l) = year(today.checked_add_months(Months::new(12))?);
            (l, l, (eh, em, es))
        }
        "sopy" => {
            let (f, _) = year(today.checked_sub_months(Months::new(12))?);
            (f, f, (sh, sm, ss))
        }
        "eopy" => {
            let (_, l) = year(today.checked_sub_months(Months::new(12))?);
            (l, l, (eh, em, es))
        }
        _ => return None,
    };
    Some(local_to_utc(first.and_time(
        chrono::NaiveTime::from_hms_opt(tm.0, tm.1, tm.2).unwrap(),
    )))
}

/// 返回包含 `d` 的那个星期的周一。
/// Monday of the week containing `d`.
fn monday_of(d: NaiveDate) -> NaiveDate {
    // 周一为第 0 天，往前偏移即可回到本周一。
    d - Duration::days(i64::from(d.weekday().num_days_from_monday()))
}

/// 把「无时区的本地时间」转成 UTC；处理夏令时（DST）的歧义/空洞。
/// Convert a naive local `NaiveDateTime` to UTC, resolving DST ambiguity/gaps.
pub fn local_to_utc(ndt: NaiveDateTime) -> DateTime<Utc> {
    match ndt.and_local_timezone(Local) {
        // 唯一结果直接取；夏令时重叠（Ambiguous）取第一个（通常为较早时刻）。
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
        // 不存在的时间（DST 跳日）退化为按 UTC 原样理解。
        LocalResult::None => ndt.and_utc(),
    }
}

/// `d` 的本地午夜（转成 UTC），用于全天任务的存储。
/// Local midnight of `d` as UTC (used for all-day storage).
pub fn local_midnight(d: NaiveDate) -> DateTime<Utc> {
    local_to_utc(d.and_hms_opt(0, 0, 0).unwrap())
}

/// 从用户输入计算 `dtend`。全天（all-day）的 `end` 是「包含式」：
/// 存储的 DTEND = 结束日期的下一天（iCalendar 惯例）。
/// Compute `dtend` from user input. All-day `end` is inclusive: stored DTEND = day after.
pub fn resolve_end(start: DateTime<Utc>, allday: bool, end: DateValue) -> Result<DateTime<Utc>> {
    let start_local = start.with_timezone(&Local);
    if allday {
        let d = match end {
            DateValue::Date(d) => d,
            DateValue::Time(dt) => dt.with_timezone(&Local).date_naive(),
        };
        // 全天事件的结束日期必须严格晚于开始日期。
        if d <= start_local.date_naive() {
            anyhow::bail!("all-day end must be after start");
        }
        Ok(local_midnight(d + Duration::days(1)))
    } else {
        let e = match end {
            // 只有日期时，沿用开始时刻（例如 09:00 开始，end 2026-08-25 → 09:00）。
            DateValue::Date(d) => local_to_utc(d.and_time(start_local.time())),
            DateValue::Time(dt) => dt,
        };
        Ok(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn relative_offsets() {
        let now = Utc::now();
        assert!(parse_datetime("+2d").unwrap() > now);
        assert!(parse_datetime("-1w").unwrap() < now);
    }

    #[test]
    fn iso_date_parses() {
        let d = parse_datetime("2026-01-02").unwrap();
        assert_eq!(
            d.with_timezone(&Local).date_naive(),
            NaiveDate::from_ymd_opt(2026, 1, 2).unwrap()
        );
    }

    #[test]
    fn compact_t_date_parses() {
        let d = parse_date_value("20260828T090000").unwrap();
        let t = match d {
            DateValue::Time(dt) => dt.with_timezone(&Local),
            _ => panic!("expected time"),
        };
        assert_eq!(t.year(), 2026);
        assert_eq!(t.month(), 8);
        assert_eq!(t.day(), 28);
        assert_eq!(t.hour(), 9);
        assert_eq!(t.minute(), 0);
    }

    #[test]
    fn mm_dd_gets_current_year() {
        let now = Local::now();
        for s in ["08-26", "8-26", "9-30"] {
            let d = parse_date_value(s).unwrap();
            assert_eq!(
                d,
                DateValue::Date(NaiveDate::from_ymd_opt(now.year(), s.split_once('-').unwrap().0.trim_start_matches('0').parse().unwrap(), s.split_once('-').unwrap().1.parse().unwrap()).unwrap())
            );
        }
        // 02-29 on a non-leap year → falls through to error
        let leap_year = now.year();
        let valid = NaiveDate::from_ymd_opt(leap_year, 2, 29).is_some();
        assert_eq!(parse_date_value("02-29").is_err(), !valid);
    }

    #[test]
    fn trailing_digits_fill_from_today() {
        let now = Local::now();
        // 0823 → MMDD this year
        let d = resolve_compact_date("0823", Local::now()).unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(now.year(), 8, 23).unwrap());
        // 25 → DD this month
        let d = resolve_compact_date("25", Local::now()).unwrap();
        assert_eq!(
            d,
            NaiveDate::from_ymd_opt(now.year(), now.month(), 25).unwrap()
        );
        // 260823 → YYMMDD
        let d = resolve_compact_date("260823", Local::now()).unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 8, 23).unwrap());
    }

    #[test]
    fn t_colon_time_parses() {
        // `20260826T09:30` / `0826T09:30` / `T09:30` → 09:30
        for (s, wanted) in [("20260826T09:30", "093000"), ("0826T0930", "093000"), ("T9:30", "093000")] {
            let d = parse_date_value(s).unwrap();
            let t = match d {
                DateValue::Time(dt) => dt.with_timezone(&Local),
                _ => panic!("expected time: {s}"),
            };
            assert_eq!(t.format("%H%M%S").to_string(), wanted, "{s}");
        }
    }

    #[test]
    fn t_prefix_is_today() {
        let now = Local::now();
        let d = parse_date_value("T09").unwrap();
        let t = match d {
            DateValue::Time(dt) => dt.with_timezone(&Local),
            _ => panic!(),
        };
        assert_eq!(t.date_naive(), now.date_naive());
        assert_eq!(t.hour(), 9);
        assert_eq!(t.minute(), 0);
        assert_eq!(t.second(), 0);
        // T alone → midnight today
        let d = parse_date_value("T").unwrap();
        let t = match d {
            DateValue::Time(dt) => dt.with_timezone(&Local),
            _ => panic!(),
        };
        assert_eq!(t.date_naive(), now.date_naive());
        assert_eq!(t.hour(), 0);
    }

    #[test]
    fn t_after_trailing_date() {
        let now = Local::now();
        // 25T0930 → 25th of this month at 09:30
        let d = parse_date_value("25T0930").unwrap();
        let t = match d {
            DateValue::Time(dt) => dt.with_timezone(&Local),
            _ => panic!(),
        };
        assert_eq!(
            t.date_naive(),
            NaiveDate::from_ymd_opt(now.year(), now.month(), 25).unwrap()
        );
        assert_eq!(t.hour(), 9);
        assert_eq!(t.minute(), 30);
    }

    #[test]
    fn eight_digits_is_allday() {
        let d = parse_date_value("20260824").unwrap();
        assert_eq!(
            d,
            DateValue::Date(NaiveDate::from_ymd_opt(2026, 8, 24).unwrap())
        );
    }

    #[test]
    fn period_bounds() {
        let sow = parse_datetime("sow").unwrap();
        let eow = parse_datetime("eow").unwrap();
        assert!(eow > sow);
        assert_eq!(
            sow.with_timezone(&Local).weekday().num_days_from_monday(),
            0
        );
        let eoww = parse_datetime("eoww").unwrap();
        assert_eq!(eoww.with_timezone(&Local).hour(), eow.with_timezone(&Local).hour()); // day_end, no workweek_end
        assert_eq!(
            eoww.with_timezone(&Local).weekday().num_days_from_monday(),
            4
        );
        let eom = parse_datetime("eom").unwrap();
        let ld = eom.with_timezone(&Local).date_naive();
        assert_eq!(ld.day(), last_day(ld.year(), ld.month()));
        let et = parse_datetime("eond").unwrap().with_timezone(&Local);
        let tm = parse_datetime("tomorrow").unwrap().with_timezone(&Local);
        assert_eq!(et.date_naive(), tm.date_naive());
        assert_eq!(et.hour(), 23);

        // previous-day boundaries: sopd = yesterday day_start, eopd = yesterday day_end
        let sopd = parse_datetime("sopd").unwrap().with_timezone(&Local);
        let eopd = parse_datetime("eopd").unwrap().with_timezone(&Local);
        let yest = parse_datetime("yesterday").unwrap().with_timezone(&Local);
        assert_eq!(sopd.date_naive(), yest.date_naive());
        assert_eq!(eopd.date_naive(), yest.date_naive());
        assert_eq!(sopd.hour(), parse_datetime("sod").unwrap().with_timezone(&Local).hour());
        assert_eq!(eopd.hour(), parse_datetime("eod").unwrap().with_timezone(&Local).hour());
        assert!(eopd > sopd);
    }

    fn last_day(y: i32, m: u32) -> u32 {
        NaiveDate::from_ymd_opt(y, m, 1)
            .unwrap()
            .checked_add_months(Months::new(1))
            .unwrap()
            .pred_opt()
            .unwrap()
            .day()
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("45min").unwrap(), Duration::minutes(45));
        assert_eq!(parse_duration("1h").unwrap(), Duration::hours(1));
        assert_eq!(parse_duration("1h30m").unwrap(), Duration::minutes(90));
        assert_eq!(parse_duration("2d").unwrap(), Duration::days(2));
        assert_eq!(parse_duration("90").unwrap(), Duration::minutes(90));
        assert!(parse_duration("bogus").is_err());
    }

    #[test]
    fn iso_durations() {
        assert_eq!(parse_duration("PT15M").unwrap(), Duration::minutes(15));
        assert_eq!(parse_duration("PT1H30M").unwrap(), Duration::minutes(90));
        assert_eq!(parse_duration("P7D").unwrap(), Duration::days(7));
        assert_eq!(parse_duration("P2W").unwrap(), Duration::weeks(2));
        assert_eq!(parse_duration("pt45s").unwrap(), Duration::seconds(45));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_datetime("not-a-date").is_err());
    }
}
