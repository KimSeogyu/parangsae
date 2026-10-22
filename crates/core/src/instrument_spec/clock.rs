//! Validity instants and retrieval dates.
//!
//! `UtcDateTime` is the domain clock ([`UtcMicros`]). `UtcDate` has no
//! time-of-day, so a retrieval date cannot be used as a validity bound.
//! This module does not depend on the `time` crate: `parangsae-core` has no
//! crates.io dependencies.

use crate::time::UtcMicros;
use std::fmt;

/// UTC instant. Validity bounds use this, never [`UtcDate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcDateTime(UtcMicros);

impl UtcDateTime {
    pub const fn from_micros(ts: UtcMicros) -> Self {
        Self(ts)
    }

    pub const fn micros(self) -> UtcMicros {
        self.0
    }

    pub fn checked_add_micros(self, delta: i64) -> Option<Self> {
        self.0.checked_add_micros(delta).map(Self)
    }

    /// Calendar fields are interpreted as UTC. Years outside 1970..=2100,
    /// and impossible civil times, are `None`. There are no leap seconds.
    pub fn from_ymd_hms(
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
    ) -> Option<Self> {
        if !(1970..=2100).contains(&year) || hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        let year_i = i64::from(year);
        let month_i = i64::from(month);
        let day_i = i64::from(day);
        if month == 0 || day == 0 || day > days_in_month(year, month) {
            return None;
        }
        let days = days_from_civil(year_i, month_i, day_i);
        let secs = days.checked_mul(86_400)?;
        let secs = secs.checked_add(i64::from(hour).checked_mul(3_600)?)?;
        let secs = secs.checked_add(i64::from(minute).checked_mul(60)?)?;
        let secs = secs.checked_add(i64::from(second))?;
        let micros = secs.checked_mul(1_000_000)?;
        Some(Self(UtcMicros::from_micros(micros)))
    }

    pub fn from_epoch_millis(millis: i64) -> Option<Self> {
        if millis < 0 {
            return None;
        }
        let micros = millis.checked_mul(1_000)?;
        Some(Self(UtcMicros::from_micros(micros)))
    }
}

impl fmt::Display for UtcDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// UTC calendar day. Used for `source_date` and `retrieved` only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcDate {
    year: i16,
    month: u8,
    day: u8,
}

impl UtcDate {
    pub fn from_ymd(year: i32, month: u8, day: u8) -> Option<Self> {
        let year = i16::try_from(year).ok()?;
        if month == 0 || day == 0 || day > days_in_month(i32::from(year), month) {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// A day the table already checked. `from_ymd` is the validating constructor.
    pub const fn known(year: i16, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    pub const fn year(self) -> i16 {
        self.year
    }

    pub const fn month(self) -> u8 {
        self.month
    }

    pub const fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for UtcDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

const fn is_leap(year: i32) -> bool {
    let div4 = year % 4 == 0;
    let div100 = year % 100 == 0;
    let div400 = year % 400 == 0;
    div4 && (!div100 || div400)
}

const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Howard Hinnant's civil-from-days inverse, days since 1970-01-01.
/// Callers bound the year to 1970..=2100 before calling.
fn days_from_civil(mut y: i64, m: i64, d: i64) -> i64 {
    if m <= 2 {
        y -= 1;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_and_listing_match_known_instants() -> Result<(), String> {
        let epoch = UtcDateTime::from_ymd_hms(1970, 1, 1, 0, 0, 0).ok_or("epoch")?;
        if epoch.micros().micros() != 0 {
            return Err(format!("epoch {}", epoch.micros()));
        }
        let listing = UtcDateTime::from_ymd_hms(2019, 9, 25, 8, 0, 0).ok_or("listing")?;
        if listing.micros().micros() != 1_569_398_400_000_000 {
            return Err(format!("listing {}", listing.micros()));
        }
        let server = UtcDateTime::from_epoch_millis(1_609_789_862_039).ok_or("server")?;
        if server.micros().micros() != 1_609_789_862_039_000 {
            return Err(format!("server {}", server.micros()));
        }
        Ok(())
    }

    #[test]
    fn impossible_days_are_rejected() -> Result<(), String> {
        if UtcDateTime::from_ymd_hms(2021, 2, 29, 0, 0, 0).is_some() {
            return Err("2021-02-29 was accepted".to_string());
        }
        if UtcDate::from_ymd(2019, 9, 31).is_some() {
            return Err("2019-09-31 was accepted".to_string());
        }
        if UtcDateTime::from_ymd_hms(1969, 12, 31, 0, 0, 0).is_some() {
            return Err("1969-12-31 was accepted".to_string());
        }
        Ok(())
    }
}
