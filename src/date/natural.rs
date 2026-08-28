//! Natural-language date / recurrence parsing (feature `date-natural`).
//!
//! - `parse_natural_date` uses [`interim`] (maintained `chrono-english` fork)
//!   for free-form English like `next tuesday`, `in 3 days`, `tomorrow 8pm`.
//! - `natural_to_rrule` uses [`text2rrule`] to turn `every tuesday` into an
//!   RFC 5545 `RRULE` value (`FREQ=WEEKLY;BYDAY=TU`).

use crate::date::DateValue;
use anyhow::{Result, bail};
use chrono::{Local, Utc};

/// Parse a natural-language date expression into a `DateValue::Time`.
pub fn parse_natural_date(input: &str) -> Result<DateValue> {
    match interim::parse_date_string(input, Local::now(), interim::Dialect::Uk) {
        Ok(dt) => Ok(DateValue::Time(dt.with_timezone(&Utc))),
        Err(e) => bail!("natural date parse failed for `{input}`: {e}"),
    }
}

/// Parse a natural-language recurrence description into an RFC 5545 RRULE value.
pub fn natural_to_rrule(input: &str) -> Result<String> {
    text2rrule::text2rrule(input)
        .map_err(|e| anyhow::anyhow!("recurrence parse failed for `{input}`: {e}"))
}
