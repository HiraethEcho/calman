//! Date / time parsing module.
//!
//! `date-ical` (always on) provides the iCalendar-compact parser in `ical`.
//! `date-natural` (optional) adds natural-language parsing via `interim` and
//! NL-recurrence parsing via `text2rrule`, exposed through `natural`.
//! 中文: 日期/时间解析模块。`ical` 子模块解析 iCalendar 紧凑格式（始终启用）；
//! `natural` 子模块解析自然语言（可选特性 `date-natural`）。
//! 对外主要提供：`parse_date_value`（解析日期表达式）、`set_day_bounds`
//! （设置一天的起止）、`parse_datetime` / `parse_duration` 等。
//! Rust 概念：`OnceLock` 是只初始化一次的全局容器，用来保存配置。

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
/// 中文: 一天中的时刻，用元组 `(时, 分, 秒)` 表示，例如 `(8, 30, 0)`。
type Hms = (u32, u32, u32);
/// Day start/end configured from `[date]`.
/// 中文: 一天的起止时刻，例如 `((8,0,0), (18,0,0))`。
type DayBounds = (Hms, Hms);
/// Configured day start/end (`HH:MM:SS`), defaults 00:00:00 / 23:59:59.
/// 中文: 全局保存一天的起止配置。`OnceLock` 只能被设置一次，之后只读。
static DAY_BOUNDS: OnceLock<DayBounds> = OnceLock::new();

/// Set day start/end from config (`[date] day_start` / `day_end`, `"HH:MM:SS"`);
/// falls back to 00:00:00 / 23:59:59 on parse failure.
/// 中文: 从配置读取一天的起止时间。`unwrap_or` 表示解析失败时用默认值，
/// 这样配置写错也不会让程序崩溃。
pub fn set_day_bounds(start: &str, end: &str) {
    let s = parse_hms(start).unwrap_or((0, 0, 0));
    let e = parse_hms(end).unwrap_or((23, 59, 59));
    // OnceLock 只能写入一次；重复设置会被忽略。
    let _ = DAY_BOUNDS.set((s, e));
}

/// Parse `"HH:MM[:SS]"` into `(hour, minute, second)`; `None` on bad input.
/// 中文: 把 `"HH:MM[:SS]"` 字符串解析成 `(时, 分, 秒)`。解析失败返回 `None`。
fn parse_hms(s: &str) -> Option<(u32, u32, u32)> {
    // `collect::<Option<_>>()`：任一数字段解析失败，整个 collect 就变成 None。
    let parts: Vec<u32> = s.trim().split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let (h, m, sec) = match parts.as_slice() {
        // 只有两位就当作分、秒缺省为 0；三位则全部使用。
        [h, m] => (*h, *m, 0),
        [h, m, sec] => (*h, *m, *sec),
        _ => return None,
    };
    // 校验数值范围合法后，用 `then_some` 把条件变成 Option。
    (h <= 23 && m <= 59 && sec <= 59).then_some((h, m, sec))
}

/// Current day start/end times.
/// 中文: 读取当前配置的一天起止；还没设置过就用默认值（00:00:00 – 23:59:59）。
pub(crate) fn day_bounds() -> DayBounds {
    *DAY_BOUNDS.get().unwrap_or(&((0, 0, 0), (23, 59, 59)))
}

/// Parse a date expression, distinguishing date-only from date-time.
///
/// Tries the iCal-compact parser first; if that fails and `date-natural` is
/// enabled, falls back to the natural-language parser.
/// 中文: 解析日期表达式，并区分“仅日期”和“日期+时间”。
/// 先尝试 iCal 紧凑格式，失败时若启用了 `date-natural` 特性，就改用自然语言解析。
/// Rust 概念：`Result<T, E>` 表示“成功值 T 或错误 E”；`bail!` 直接返回错误。
pub fn parse_date_value(input: &str) -> Result<DateValue> {
    match ical::parse_date_value(input) {
        Ok(v) => Ok(v),
        Err(_) => {
            // #[cfg] 是编译期开关：只有启用对应 feature 时，这段代码才会被编译。
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
