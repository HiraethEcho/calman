//! Date / time parsing module.
//!
//! `date-ical` (always on) provides the iCalendar-compact parser in `ical`.
//! `date-natural` (optional) adds natural-language parsing via `interim` and
//! NL-recurrence parsing via `text2rrule`, exposed through `natural`.

pub use ical::{DateValue, local_midnight, parse_datetime, parse_duration, resolve_end};

#[cfg(feature = "date-ical")]
mod ical;

#[cfg(feature = "date-natural")]
mod natural;
#[cfg(feature = "date-natural")]
pub use natural::{natural_to_rrule, parse_natural_date};

use anyhow::Result;
use std::sync::OnceLock;

/// Configured `eoww` end time (`HH:MM`), default 17:00.
static WORKWEEK_END: OnceLock<(u32, u32)> = OnceLock::new();

/// Set the working-week end time from config (`[date] workweek_end`, `"17:00"`).
/// Accepts `HH:MM`; falls back to 17:00 on parse failure.
pub fn set_workweek_end(hhmm: &str) {
    let t = hhmm.trim();
    let v = t
        .split_once(':')
        .and_then(|(h, m)| Some((h.trim().parse().ok()?, m.trim().parse().ok()?)))
        .or_else(|| {
            if t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()) {
                Some((t[..2].parse().ok()?, t[2..].parse().ok()?))
            } else {
                None
            }
        })
        .filter(|(h, m)| *h <= 23 && *m <= 59)
        .unwrap_or((17, 0));
    let _ = WORKWEEK_END.set(v);
}

/// Current working-week end time.
pub(crate) fn workweek_end() -> (u32, u32) {
    *WORKWEEK_END.get().unwrap_or(&(17, 0))
}

/// Parse a date expression, distinguishing date-only from date-time.
///
/// Tries the iCal-compact parser first; if that fails and `date-natural` is
/// enabled, falls back to the natural-language parser.
pub fn parse_date_value(input: &str) -> Result<DateValue> {
    match ical::parse_date_value(input) {
        Ok(v) => Ok(v),
        Err(_) => {
            #[cfg(feature = "date-natural")]
            {
                parse_natural_date(input)
            }
            #[cfg(not(feature = "date-natural"))]
            {
                anyhow::bail!(
                    "could not parse date `{input}` (natural-language parsing is disabled; enable feature `date-natural`)"
                )
            }
        }
    }
}
