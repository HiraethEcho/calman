//! Natural-language date parsing.
//!
//! Per DESIGN.md §6.1 and Taskwarrior named dates:
//! keywords, period boundaries (`sod`/`eod`/`sow`/`eow`/...), weekdays,
//! ISO/compact dates, and relative offsets. Dates assume system local time,
//! converted to UTC for storage (CalDAV-compatible concrete timestamps).

use anyhow::{Result, bail};
use chrono::{
    DateTime, Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc,
};

/// A parsed date: date-only (all-day candidate) or a concrete date-time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateValue {
    Date(NaiveDate),
    Time(DateTime<Utc>),
}

/// Parse a user date/time expression into a UTC timestamp.
/// Date-only forms land at local midnight (todo `due` semantics).
pub fn parse_datetime(input: &str) -> Result<DateTime<Utc>> {
    match parse_date_value(input)? {
        DateValue::Date(d) => Ok(local_to_utc(d.and_hms_opt(0, 0, 0).unwrap())),
        DateValue::Time(dt) => Ok(dt),
    }
}

/// Parse a date expression, distinguishing date-only from date-time forms.
///
/// Forms: `20260812` (all-day), `20260826-0900`, `0826` (this year),
/// `17` (this month), `-0900` (today), `YYYY-MM-DD [HH:MM]`, `HH:MM`,
/// named dates, relative offsets.
pub fn parse_date_value(input: &str) -> Result<DateValue> {
    let s = input.trim().to_lowercase();
    let now = Local::now();

    // Named dates that are pure day boundaries → date-only.
    const DATE_ONLY_NAMED: &[&str] = &[
        "today",
        "sod",
        "tomorrow",
        "sond",
        "yesterday",
        "sow",
        "soww",
        "som",
        "soq",
        "soy",
        "sonw",
        "sonww",
        "sonm",
        "sony",
        "sopw",
        "sopww",
        "sopm",
        "sopy",
    ];
    if DATE_ONLY_NAMED.contains(&s.as_str()) {
        let dt = named_date(&s).expect("named date");
        return Ok(DateValue::Date(dt.with_timezone(&Local).date_naive()));
    }
    if let Some(dt) = named_date(&s) {
        return Ok(DateValue::Time(dt));
    }

    // `-HHMM` → today at that time
    if let Some(t) = s.strip_prefix('-')
        && t.len() == 4
        && t.chars().all(|c| c.is_ascii_digit())
    {
        let d = now.date_naive();
        let ndt = d
            .and_hms_opt(t[..2].parse()?, t[2..].parse()?, 0)
            .ok_or_else(|| anyhow::anyhow!("bad time `{input}`"))?;
        return Ok(DateValue::Time(local_to_utc(ndt)));
    }

    // Relative offsets: +3d, -2w, +1m, +1y, +2h (against now)
    if let Some(rest) = s.strip_prefix(['+', '-']) {
        let unit = rest.chars().last().unwrap_or('d');
        if !unit.is_ascii_digit()
            && let Ok(n) = rest[..rest.len() - 1].parse::<i64>()
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

    // "YYYYMMDD-HHMM"
    if let Some((d, t)) = s.split_once('-')
        && d.len() == 8
        && t.len() == 4
        && d.chars().all(|c| c.is_ascii_digit())
        && t.chars().all(|c| c.is_ascii_digit())
    {
        let date = NaiveDate::parse_from_str(d, "%Y%m%d")?;
        let ndt = date
            .and_hms_opt(t[..2].parse()?, t[2..].parse()?, 0)
            .ok_or_else(|| anyhow::anyhow!("bad time `{input}`"))?;
        return Ok(DateValue::Time(local_to_utc(ndt)));
    }

    // "MMDD-HHMM" → this year (e.g. 0826-0930)
    if let Some((d, t)) = s.split_once('-')
        && d.len() == 4
        && t.len() == 4
        && d.chars().all(|c| c.is_ascii_digit())
        && t.chars().all(|c| c.is_ascii_digit())
    {
        let m: u32 = d[..2].parse()?;
        let day: u32 = d[2..].parse()?;
        if let Some(date) = NaiveDate::from_ymd_opt(now.year(), m, day) {
            let ndt = date
                .and_hms_opt(t[..2].parse()?, t[2..].parse()?, 0)
                .ok_or_else(|| anyhow::anyhow!("bad time `{input}`"))?;
            return Ok(DateValue::Time(local_to_utc(ndt)));
        }
    }

    // "DD-HHMM" → this year, this month (e.g. 25-0930)
    if let Some((d, t)) = s.split_once('-')
        && d.len() <= 2
        && t.len() == 4
        && d.chars().all(|c| c.is_ascii_digit())
        && t.chars().all(|c| c.is_ascii_digit())
    {
        let day: u32 = d.parse()?;
        if let Some(date) = NaiveDate::from_ymd_opt(now.year(), now.month(), day) {
            let ndt = date
                .and_hms_opt(t[..2].parse()?, t[2..].parse()?, 0)
                .ok_or_else(|| anyhow::anyhow!("bad time `{input}`"))?;
            return Ok(DateValue::Time(local_to_utc(ndt)));
        }
    }

    // "YYYY-MM-DD"
    if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
        return Ok(DateValue::Date(d));
    }

    // "YYYYMMDD"
    if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y%m%d") {
        return Ok(DateValue::Date(d));
    }

    let is_digits = |x: &str| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit());

    // "MMDD" → this year
    if s.len() == 4 && is_digits(&s) {
        let m: u32 = s[..2].parse()?;
        let d: u32 = s[2..].parse()?;
        if let Some(date) = NaiveDate::from_ymd_opt(now.year(), m, d) {
            return Ok(DateValue::Date(date));
        }
    }

    // "DD" → this year, this month
    if s.len() <= 2 && is_digits(&s) {
        let d: u32 = s.parse()?;
        if let Some(date) = NaiveDate::from_ymd_opt(now.year(), now.month(), d) {
            return Ok(DateValue::Date(date));
        }
    }

    // "HH:MM" → today at that time
    if let Ok(t) = NaiveTime::parse_from_str(&s, "%H:%M") {
        let ndt = now.date_naive().and_time(t);
        return Ok(DateValue::Time(local_to_utc(ndt)));
    }

    bail!("could not parse date `{input}` (try 20260812, 0826, -0900, today, eow, +3d)")
}

/// Parse a human duration: `45min`, `1h`, `1h30m`, `2d`, `90`, `1w`.
/// Bare numbers mean minutes.
pub fn parse_duration(input: &str) -> Result<Duration> {
    let s = input.trim().to_lowercase();
    if s.is_empty() {
        bail!("empty duration");
    }
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
    } else if !parts.is_empty() {
        // trailing unit after a completed part
        if let Some(last) = parts.last_mut() {
            last.1 = unit;
        }
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

/// Named dates. Week starts Monday (calman default); `eoww` uses 17:00.
fn named_date(s: &str) -> Option<DateTime<Utc>> {
    let today = Local::now().date_naive();
    let week_start = monday_of(today);
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

    let (first, _last, tm) = match s {
        "now" => return Some(Utc::now()),
        "today" | "sod" => (today, today, (0, 0, 0)),
        "tomorrow" | "sond" => (
            today + Duration::days(1),
            today + Duration::days(1),
            (0, 0, 0),
        ),
        "yesterday" => (
            today - Duration::days(1),
            today - Duration::days(1),
            (0, 0, 0),
        ),
        "eod" => (today, today, (23, 59, 59)),
        "eond" => (
            today + Duration::days(1),
            today + Duration::days(1),
            (23, 59, 59),
        ),
        "sow" | "soww" => (week_start, week_start, (0, 0, 0)),
        "eow" => (
            week_start + Duration::days(6),
            week_start + Duration::days(6),
            (23, 59, 59),
        ),
        "eoww" => (
            week_start + Duration::days(4),
            week_start + Duration::days(4),
            (17, 0, 0),
        ),
        "sonw" | "sonww" => (
            week_start + Duration::days(7),
            week_start + Duration::days(7),
            (0, 0, 0),
        ),
        "eonw" | "eonww" => (
            week_start + Duration::days(13),
            week_start + Duration::days(13),
            (23, 59, 59),
        ),
        "sopw" | "sopww" => (
            week_start - Duration::days(7),
            week_start - Duration::days(7),
            (0, 0, 0),
        ),
        "eopw" | "eopww" => (
            week_start - Duration::days(1),
            week_start - Duration::days(1),
            (23, 59, 59),
        ),
        "som" => {
            let (f, _) = month(today);
            (f, f, (0, 0, 0))
        }
        "eom" => {
            let (_, l) = month(today);
            (l, l, (23, 59, 59))
        }
        "sonm" => {
            let (f, _) = month(today.checked_add_months(Months::new(1))?);
            (f, f, (0, 0, 0))
        }
        "eonm" => {
            let (_, l) = month(today.checked_add_months(Months::new(1))?);
            (l, l, (23, 59, 59))
        }
        "sopm" => {
            let (f, _) = month(today.checked_sub_months(Months::new(1))?);
            (f, f, (0, 0, 0))
        }
        "eopm" => {
            let (_, l) = month(today.checked_sub_months(Months::new(1))?);
            (l, l, (23, 59, 59))
        }
        "soq" => {
            let (f, _) = quarter(today);
            (f, f, (0, 0, 0))
        }
        "eoq" => {
            let (_, l) = quarter(today);
            (l, l, (23, 59, 59))
        }
        "soy" => {
            let (f, _) = year(today);
            (f, f, (0, 0, 0))
        }
        "eoy" => {
            let (_, l) = year(today);
            (l, l, (23, 59, 59))
        }
        "sony" => {
            let (f, _) = year(today.checked_add_months(Months::new(12))?);
            (f, f, (0, 0, 0))
        }
        "eony" => {
            let (_, l) = year(today.checked_add_months(Months::new(12))?);
            (l, l, (23, 59, 59))
        }
        "sopy" => {
            let (f, _) = year(today.checked_sub_months(Months::new(12))?);
            (f, f, (0, 0, 0))
        }
        "eopy" => {
            let (_, l) = year(today.checked_sub_months(Months::new(12))?);
            (l, l, (23, 59, 59))
        }
        _ => return None,
    };
    Some(local_to_utc(first.and_time(
        chrono::NaiveTime::from_hms_opt(tm.0, tm.1, tm.2).unwrap(),
    )))
}

/// Monday of the week containing `d`.
fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(i64::from(d.weekday().num_days_from_monday()))
}

/// Convert a naive local `NaiveDateTime` to UTC, resolving DST ambiguity/gaps.
fn local_to_utc(ndt: NaiveDateTime) -> DateTime<Utc> {
    Local
        .from_local_datetime(&ndt)
        .single()
        .unwrap_or_else(|| Local.from_utc_datetime(&ndt))
        .with_timezone(&Utc)
}

/// Local midnight of `d` as UTC (used for all-day storage).
pub fn local_midnight(d: NaiveDate) -> DateTime<Utc> {
    local_to_utc(d.and_hms_opt(0, 0, 0).unwrap())
}

/// `d` at `HH:MM` local time, as UTC (used for `modify start:<date>` defaults).
pub fn time_on_date(d: NaiveDate, hhmm: &str) -> Result<DateTime<Utc>> {
    let t = NaiveTime::parse_from_str(hhmm.trim(), "%H:%M")
        .map_err(|_| anyhow::anyhow!("bad default time `{hhmm}` (expected HH:MM)"))?;
    Ok(local_to_utc(d.and_time(t)))
}

/// Compute `dtend` from user input. All-day `end` is inclusive: stored DTEND = day after.
pub fn resolve_end(start: DateTime<Utc>, allday: bool, end: DateValue) -> Result<DateTime<Utc>> {
    let start_local = start.with_timezone(&Local);
    if allday {
        let d = match end {
            DateValue::Date(d) => d,
            DateValue::Time(dt) => dt.with_timezone(&Local).date_naive(),
        };
        if d <= start_local.date_naive() {
            anyhow::bail!("all-day end must be after start");
        }
        Ok(local_midnight(d + Duration::days(1)))
    } else {
        let e = match end {
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
    fn compact_date_parses() {
        let d = parse_datetime("20260824").unwrap();
        assert_eq!(
            d.with_timezone(&Local).date_naive(),
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap()
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
        assert_eq!(eoww.with_timezone(&Local).hour(), 17);
        assert_eq!(
            eoww.with_timezone(&Local).weekday().num_days_from_monday(),
            4
        ); // Friday

        let eom = parse_datetime("eom").unwrap();
        let ld = eom.with_timezone(&Local).date_naive();
        assert_eq!(ld.day(), last_day(ld.year(), ld.month()));

        let et = parse_datetime("eond").unwrap().with_timezone(&Local);
        let tm = parse_datetime("tomorrow").unwrap().with_timezone(&Local);
        assert_eq!(et.date_naive(), tm.date_naive());
        assert_eq!(et.hour(), 23);
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
    fn date_only_and_compact_forms() {
        let now = Local::now();
        // MMDD → this year
        let d = parse_date_value("0826").unwrap();
        assert_eq!(
            d,
            DateValue::Date(NaiveDate::from_ymd_opt(now.year(), 8, 26).unwrap())
        );
        // DD → this month
        let d = parse_date_value("17").unwrap();
        assert_eq!(
            d,
            DateValue::Date(NaiveDate::from_ymd_opt(now.year(), now.month(), 17).unwrap())
        );
        // -0900 → today 09:00
        let d = parse_date_value("-0900").unwrap();
        let t = match d {
            DateValue::Time(dt) => dt.with_timezone(&Local),
            _ => panic!(),
        };
        assert_eq!(t.date_naive(), now.date_naive());
        assert_eq!(t.hour(), 9);
        // YYYYMMDD-HHMM
        let d = parse_date_value("20260826-0900").unwrap();
        assert!(matches!(d, DateValue::Time(_)));
        // DD-HHMM → this month day 25 at 09:30
        let d = parse_date_value("25-0930").unwrap();
        match d {
            DateValue::Time(dt) => {
                let t = dt.with_timezone(&Local);
                assert_eq!(
                    t.date_naive(),
                    NaiveDate::from_ymd_opt(now.year(), now.month(), 25).unwrap()
                );
                assert_eq!(t.hour(), 9);
                assert_eq!(t.minute(), 30);
            }
            _ => panic!("expected time"),
        }
        // MMDD-HHMM → this year
        let d = parse_date_value("0826-0930").unwrap();
        assert!(matches!(d, DateValue::Time(_)));
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
    fn rejects_garbage() {
        assert!(parse_datetime("not-a-date").is_err());
    }
}
