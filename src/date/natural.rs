//! Natural-language date / recurrence parsing (feature `date-natural`).
//!
//! - `parse_natural_date` uses [`interim`] (maintained `chrono-english` fork)
//!   for free-form English like `next tuesday`, `in 3 days`, `tomorrow 8pm`.
//! - `natural_to_rrule` uses [`text2rrule`] to turn `every tuesday` into an
//!   RFC 5545 `RRULE` value (`FREQ=WEEKLY;BYDAY=TU`).
//! 中文: 自然语言日期/重复解析模块（可选特性 `date-natural`）。
//! `parse_natural_date` 把 `next tuesday` 这类英文短语解析成具体时间；
//! `natural_to_rrule` 把 `every tuesday` 转成 RFC 5545 的 RRULE 字符串。
//! 注意：解析时以本地时区 `Local` 为参考，结果统一转成 UTC 存储。

use crate::date::DateValue;
use anyhow::{Result, bail};
use chrono::{Local, Utc};

/// Parse a natural-language date expression into a `DateValue::Time`.
/// 中文: 把自然语言日期（如 `next tuesday`）解析成 `DateValue::Time`。
/// 使用 `interim` 库，并以当前本地时间作为“今天”的参考点。
/// 解析成功后把本地时间转成 UTC，保证存储时区统一。
pub fn parse_natural_date(input: &str) -> Result<DateValue> {
    // `Uk` 表示英式习惯（日/月顺序）；`Local::now()` 提供相对时间的基准。
    match interim::parse_date_string(input, Local::now(), interim::Dialect::Uk) {
        Ok(dt) => Ok(DateValue::Time(dt.with_timezone(&Utc))),
        Err(e) => bail!("natural date parse failed for `{input}`: {e}"),
    }
}

/// Parse a natural-language recurrence description into an RFC 5545 RRULE value.
/// 中文: 把自然语言重复描述（如 `every tuesday`）转成 RFC 5545 RRULE 字符串。
/// `text2rrule` 返回 `Result`，`map_err` 把它的错误类型包装成统一的 `anyhow::Error`。
pub fn natural_to_rrule(input: &str) -> Result<String> {
    text2rrule::text2rrule(input)
        .map_err(|e| anyhow::anyhow!("recurrence parse failed for `{input}`: {e}"))
}
