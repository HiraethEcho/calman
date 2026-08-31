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

/// Clock time as `(hour, minute, second)`.
type Hms = (u32, u32, u32);
/// Day start/end configured from `[date]`.
type DayBounds = (Hms, Hms);
/// Configured day start/end (`HH:MM:SS`), defaults 00:00:00 / 23:59:59.
static DAY_BOUNDS: OnceLock<DayBounds> = OnceLock::new();

/// Set day start/end from config (`[date] day_start` / `day_end`, / `day_end`,
/// `"HH:MM:SS"`); falls back to 00:00:00 / 23:59:59 on parse failure.
pub fn set_day_bounds(start: &str, end: &str) {
    let s = parse_hms(start).unwrap_or((0, 0, 0));
    let e = parse_hms(end).unwrap_or((23, 59, 59));
    let _ = DAY_BOUNDS.set((s, e));
}

fn parse_hms(s: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<u32> = s.trim().split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let (h, m, sec) = match parts.as_slice() {
        [h, m] => (*h, *m, 0),
        [h, m, sec] => (*h, *m, *sec),
        _ => return None,
    };
    (h <= 23 && m <= 59 && sec <= 59).then_some((h, m, sec))
}

/// Current day start/end times.
pub(crate) fn day_bounds() -> DayBounds {
    *DAY_BOUNDS.get().unwrap_or(&((0, 0, 0), (23, 59, 59)))
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
