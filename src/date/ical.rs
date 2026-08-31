//! iCalendar-style compact date parsing (feature `date-ical`).
//!
//! Per DESIGN.md §6.1 and Taskwarrior named dates, plus a `T`-marked compact
//! form (RFC 5545 `YYYYMMDDTHHMMSS` style), where `T` separates date and time:
//! `20260828T090000` is a full date-time; `20260828` (8 digits) is an all-day
//! `YYYYMMDD`; `<8` digits (`0823`, `25`) are treated as the trailing digits of
//! `YYYYMMDD` with the prefix filled from today (`0823`→2026-08-23, `25`→2026-08-25);
//! `0828T0900`/`25T` use the digits before `T` as trailing date digits and the
//! part after `T` as `HHMMSS`; `T0900`/`T09` use today's date plus `HHMMSS`
//! (zero-padded, `T` alone → today 00:00). Also accepted: `YYYY-MM-DD`,
//! `YYYY-MM-DD HH:MM`, `HH:MM` (today), `now`, named day boundaries, and
//! relative `+3d`/`-2w`/`+1h`. All-local input is resolved to UTC via
//! `local_to_utc` (DST-safe, CalDAV-safe).

use anyhow::{Result, bail};
use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, Months, NaiveDate, NaiveDateTime, NaiveTime, Utc,
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
        DateValue::Date(d) => Ok(local_midnight(d)),
        DateValue::Time(dt) => Ok(dt),
    }
}

/// Parse a date expression, distinguishing date-only from date-time forms.
pub fn parse_date_value(input: &str) -> Result<DateValue> {
    let s = input.trim().to_lowercase();
    let now = Local::now();

    // Named dates that are pure day boundaries → date-only.
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

    // Relative offsets: +3d, -2w, +1m, +1y, +2h, -1s (against now)
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

    // T-based / compact forms.
    if let Some(idx) = s.find('t') {
        let before = &s[..idx];
        let after = &s[idx + 1..];
        let date = resolve_compact_date(before, now)?;
        let time = resolve_compact_time(after)?;
        return Ok(DateValue::Time(local_to_utc(date.and_time(time))));
    }

    // Digits only, no T.
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

    // "MM-DD" → current year, e.g. `08-26`, `9-30`.
    if let Some((m, d)) = s.split_once('-')
        && let (Ok(m), Ok(d)) = (m.parse::<u32>(), d.parse::<u32>())
        && let Some(dt) = NaiveDate::from_ymd_opt(now.date_naive().year(), m, d)
    {
        return Ok(DateValue::Date(dt));
    }

    // "HH:MM" → today at that time
    if let Ok(t) = NaiveTime::parse_from_str(&s, "%H:%M") {
        let ndt = now.date_naive().and_time(t);
        return Ok(DateValue::Time(local_to_utc(ndt)));
    }

    bail!("could not parse date `{input}` (try 20260812, 0826, T0900, today, eow, +3d)")
}

/// Resolve a compact date string to a `NaiveDate`.
/// - empty → today
/// - >=8 digits → first 8 as `YYYYMMDD`
/// - <8 digits → trailing digits of today's `YYYYMMDD` (prefix from today)
fn resolve_compact_date(digits: &str, now: DateTime<Local>) -> Result<NaiveDate> {
    if digits.is_empty() {
        return Ok(now.date_naive());
    }
    if digits.len() >= 8 {
        return Ok(NaiveDate::parse_from_str(&digits[..8], "%Y%m%d")?);
    }
    let today = now.format("%Y%m%d").to_string();
    let n = digits.len();
    let prefix = &today[..8 - n];
    let full = format!("{prefix}{digits}");
    Ok(NaiveDate::parse_from_str(&full, "%Y%m%d")?)
}

/// Resolve a compact time string to a `NaiveTime`.
/// Digits are `HH[MM[SS]]` (left-aligned, zero-padded): `09`→09:00:00,
/// `0930`→09:30:00, `090000`→full, empty→00:00:00.
fn resolve_compact_time(digits: &str) -> Result<NaiveTime> {
    if digits.is_empty() {
        return Ok(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    }
    let hh = digits.get(..2).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
    let mm = digits.get(2..4).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
    let ss = digits.get(4..6).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
    NaiveTime::from_hms_opt(hh, mm, ss).ok_or_else(|| anyhow::anyhow!("bad time `{digits}`"))
}

/// Parse a human duration: `45min`, `1h`, `1h30m`, `2d`, `90`, `1w`.
/// Bare numbers mean minutes.
pub fn parse_duration(input: &str) -> Result<Duration> {
    let s = input.trim().to_lowercase();
    if s.is_empty() {
        bail!("empty duration");
    }
    if let Some(d) = parse_iso_duration(&s) {
        return Ok(d);
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
    } else if let Some(last) = parts.last_mut() {
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

/// Parse an ISO 8601 duration (`PT15M`, `PT1H30M`, `P7D`, `P2W`, `PT45S`).
/// Supported units: D/W (date part), H/M/S (time part). `M` is minutes.
fn parse_iso_duration(s: &str) -> Option<Duration> {
    let body = s.strip_prefix('p')?;
    let mut total = Duration::zero();
    let mut num = String::new();
    let mut any = false;
    for ch in body.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
        } else if ch == 't' {
            num.clear();
        } else {
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

/// Named dates. Week starts Monday (calman default); `eoww` uses `day_end`.
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

    let ((sh, sm, ss), (eh, em, es)) = super::day_bounds();
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

/// Monday of the week containing `d`.
fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(i64::from(d.weekday().num_days_from_monday()))
}

/// Convert a naive local `NaiveDateTime` to UTC, resolving DST ambiguity/gaps.
pub fn local_to_utc(ndt: NaiveDateTime) -> DateTime<Utc> {
    match ndt.and_local_timezone(Local) {
        LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
        LocalResult::None => ndt.and_utc(),
    }
}

/// Local midnight of `d` as UTC (used for all-day storage).
pub fn local_midnight(d: NaiveDate) -> DateTime<Utc> {
    local_to_utc(d.and_hms_opt(0, 0, 0).unwrap())
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
