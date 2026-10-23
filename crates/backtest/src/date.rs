//! UTC civil dates. Partition paths and daily equity use these days.
//! The conversion is Howard Hinnant's public-domain civil-from-days algorithm.

use std::fmt;

use parangsae_core::UtcMicros;

const MICROS_PER_DAY: i64 = 86_400_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcDate {
    year: i32,
    month: u8,
    day: u8,
}

impl UtcDate {
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('-');
        let year = parts.next()?;
        let month = parts.next()?;
        let day = parts.next()?;
        if parts.next().is_some() || year.len() != 4 || month.len() != 2 || day.len() != 2 {
            return None;
        }
        let year = year.parse::<i32>().ok()?;
        let month = month.parse::<u8>().ok()?;
        let day = day.parse::<u8>().ok()?;
        if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
            return None;
        }
        Some(Self { year, month, day })
    }

    pub fn year(self) -> i32 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }

    pub fn next(self) -> Self {
        if self.day < days_in_month(self.year, self.month) {
            Self {
                year: self.year,
                month: self.month,
                day: self.day + 1,
            }
        } else if self.month < 12 {
            Self {
                year: self.year,
                month: self.month + 1,
                day: 1,
            }
        } else {
            Self {
                year: self.year + 1,
                month: 1,
                day: 1,
            }
        }
    }

    pub fn start_us(self) -> UtcMicros {
        let days = days_from_civil(
            i64::from(self.year),
            i64::from(self.month),
            i64::from(self.day),
        );
        let micros = days
            .checked_mul(MICROS_PER_DAY)
            .expect("UTC date fits in microseconds");
        UtcMicros::from_micros(micros)
    }

    /// The UTC date containing `ts`. A timestamp exactly at midnight belongs to the new day.
    pub fn from_micros(ts: UtcMicros) -> Self {
        let days = ts.micros().div_euclid(MICROS_PER_DAY);
        civil_from_days(days)
    }
}

impl fmt::Display for UtcDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_from_civil(mut year: i64, month: i64, day: i64) -> i64 {
    if month <= 2 {
        year -= 1;
    }
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> UtcDate {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = u64::try_from(z - era * 146_097).expect("day of era fits");
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = i64::try_from(yoe).expect("year of era fits") + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    UtcDate {
        year: i32::try_from(year).expect("year fits"),
        month: u8::try_from(month).expect("month fits"),
        day: u8::try_from(day).expect("day fits"),
    }
}

#[cfg(test)]
mod tests {
    use super::{UtcDate, MICROS_PER_DAY};
    use parangsae_core::UtcMicros;

    #[test]
    fn unix_epoch_and_leap_day_round_trip() {
        assert_eq!(UtcDate::parse("1970-01-01").unwrap().start_us().micros(), 0);
        assert_eq!(
            UtcDate::from_micros(UtcMicros::from_micros(0)).to_string(),
            "1970-01-01"
        );
        assert_eq!(
            UtcDate::from_micros(UtcMicros::from_micros(MICROS_PER_DAY - 1)).to_string(),
            "1970-01-01"
        );
        assert_eq!(
            UtcDate::from_micros(UtcMicros::from_micros(MICROS_PER_DAY)).to_string(),
            "1970-01-02"
        );
        let start = UtcDate::parse("2024-01-01").unwrap();
        assert_eq!(start.start_us().micros(), 1_704_067_200_000_000);
        assert_eq!(UtcDate::from_micros(start.start_us()), start);
        let leap = UtcDate::parse("2024-02-29").unwrap();
        assert_eq!(leap.next().to_string(), "2024-03-01");
        assert!(UtcDate::parse("2024-02-30").is_none());
        assert!(UtcDate::parse("2023-02-29").is_none());
    }
}
