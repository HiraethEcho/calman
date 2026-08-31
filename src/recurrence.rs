//! Recurrence rule parsing → standard RFC 5545 `RRULE` value.
//!
//! Two input styles:
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

/// Map a weekday token to its `BYDAY` code.
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

/// Map a unit token (`d`/`day`/`days`/`w`/`week`...) to a `FREQ` value.
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

/// Split `7d` / `30days` into `(count, freq)`.
#[cfg(feature = "date-natural")]
fn number_unit(t: &str) -> Option<(u32, &'static str)> {
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

#[cfg(feature = "date-natural")]
fn eoy() -> String {
    let y = Local::now().year();
    NaiveDate::from_ymd_opt(y, 12, 31)
        .unwrap()
        .format("%Y%m%d")
        .to_string()
}

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

/// Normalize a user recurrence expression into a standard `RRULE` value.
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
    if let Some(r) = iso_period_to_rrule(&up) {
        return Ok(r);
    }
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

/// Map an ISO 8601 period (`P7D`, `P2W`, `P1M`, `P1Y`) to an `RRULE`.
/// Always available (no `date-natural` needed).
fn iso_period_to_rrule(up: &str) -> Option<String> {
    let body = up.strip_prefix('P')?;
    if body.len() < 2 {
        return None;
    }
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

/// Render an `RRULE` as an ISO 8601 period (`P7D`, `P2W`, `P1M`, `P1Y`).
/// Non-periodic rules (no `FREQ`) fall back to the raw RRULE text.
pub fn rrule_period(rrule: &str) -> String {
    let up = rrule.to_uppercase();
    let freq = up.split(';').find_map(|p| p.strip_prefix("FREQ="));
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

/// Built-in friendly recurrence parser (fallback used under `date-natural`).
#[cfg(feature = "date-natural")]
fn normalize_friendly(input: &str) -> Result<String> {
    let s = input.trim();
    if s.is_empty() {
        bail!("empty recurrence");
    }
    let toks: Vec<String> = s
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty() && t != "and" && t != "every")
        .collect();

    let mut freq: Option<&str> = None;
    let mut interval: Option<u32> = None;
    let mut byday: Vec<&str> = Vec::new();
    let mut count: Option<u32> = None;
    let mut until: Option<String> = None;

    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
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
                if let (Some(next), Some(unit)) = (toks.get(i + 1), toks.get(i + 2))
                    && let Ok(n) = next.parse::<u32>()
                {
                    match unit.as_str() {
                        "times" | "time" => count = Some(n),
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

    let freq = freq.ok_or_else(|| {
        anyhow::anyhow!("recurrence needs a frequency (daily/weekly/...): `{input}`")
    })?;

    let mut parts = vec![format!("FREQ={freq}")];
    if let Some(n) = interval
        && n > 1
    {
        parts.push(format!("INTERVAL={n}"));
    }
    if !byday.is_empty() {
        let mut seen = std::collections::HashSet::new();
        let mut uniq = Vec::new();
        for c in byday {
            if seen.insert(c) {
                uniq.push(c);
            }
        }
        parts.push(format!("BYDAY={}", uniq.join(",")));
    }
    if let Some(n) = count {
        parts.push(format!("COUNT={n}"));
    }
    if let Some(u) = until {
        parts.push(format!("UNTIL={u}"));
    }
    Ok(parts.join(";"))
}

#[cfg(all(test, feature = "date-natural"))]
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
