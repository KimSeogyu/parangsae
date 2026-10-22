//! UTC civil dates. The collector partitions by these days. No timezone database.

use parangsae_core::UtcMicros;

const MICROS_PER_DAY: i64 = 86_400_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CivilDay {
    year: i32,
    month: u8,
    day: u8,
}

impl CivilDay {
    pub const fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        if month < 1 || month > 12 || day < 1 {
            return None;
        }
        if day > days_in_month(year, month) {
            return None;
        }
        Some(Self { year, month, day })
    }

    pub const fn year(self) -> i32 {
        self.year
    }

    pub const fn month(self) -> u8 {
        self.month
    }

    pub const fn day(self) -> u8 {
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

    pub fn epoch_days(self) -> i64 {
        days_from_civil(
            i64::from(self.year),
            i64::from(self.month),
            i64::from(self.day),
        )
    }

    pub fn start_us(self) -> UtcMicros {
        UtcMicros::from_micros(self.epoch_days().saturating_mul(MICROS_PER_DAY))
    }

    pub fn end_us(self) -> UtcMicros {
        self.next().start_us()
    }

    /// First spot-kline archive day whose timestamps are microseconds.
    pub const SPOT_MICROS_FROM: Self = Self {
        year: 2025,
        month: 1,
        day: 1,
    };
}

impl std::fmt::Display for CivilDay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

pub const fn days_in_month(year: i32, month: u8) -> u8 {
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

const fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

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

pub fn day_of(ts: UtcMicros) -> Result<CivilDay, super::Error> {
    if ts.micros() < 0 {
        return Err(super::Error::check(
            "date",
            "timestamp is before the unix epoch",
        ));
    }
    let days = ts.micros() / MICROS_PER_DAY;
    civil_from_days(days).ok_or_else(|| super::Error::check("date", "day is out of range"))
}

fn civil_from_days(mut z: i64) -> Option<CivilDay> {
    z += 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = u64::try_from(z - era * 146097).ok()?;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = i64::try_from(yoe).ok()? + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);
    let year = i32::try_from(year).ok()?;
    let month = u8::try_from(m).ok()?;
    let day = u8::try_from(d).ok()?;
    CivilDay::new(year, month, day)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayRange {
    pub from: CivilDay,
    pub to: CivilDay,
}

impl DayRange {
    pub fn new(from: CivilDay, to: CivilDay) -> Result<Self, super::Error> {
        if from > to {
            return Err(super::Error::Usage(
                "date range is reversed; --from must be on or before --to".into(),
            ));
        }
        Ok(Self { from, to })
    }

    pub fn days(self) -> Vec<CivilDay> {
        let mut out = Vec::new();
        let mut cursor = self.from;
        loop {
            out.push(cursor);
            if cursor == self.to {
                break;
            }
            cursor = cursor.next();
        }
        out
    }

    pub fn contains(self, day: CivilDay) -> bool {
        self.from <= day && day <= self.to
    }

    pub fn start_us(self) -> UtcMicros {
        self.from.start_us()
    }

    pub fn end_us(self) -> UtcMicros {
        self.to.end_us()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct YearMonth {
    year: i32,
    month: u8,
}

impl YearMonth {
    pub fn new(year: i32, month: u8) -> Option<Self> {
        if (1..=12).contains(&month) {
            Some(Self { year, month })
        } else {
            None
        }
    }

    pub const fn year(self) -> i32 {
        self.year
    }

    pub const fn month(self) -> u8 {
        self.month
    }

    /// Day 1 of a month that `new` already restricted to 1..=12.
    pub fn first(self) -> CivilDay {
        CivilDay {
            year: self.year,
            month: self.month,
            day: 1,
        }
    }

    /// Last civil day. `days_in_month` is 28..=31 for every month `new` accepts.
    pub fn last(self) -> CivilDay {
        CivilDay {
            year: self.year,
            month: self.month,
            day: days_in_month(self.year, self.month),
        }
    }

    pub fn overlaps(self, range: DayRange) -> bool {
        self.first() <= range.to && self.last() >= range.from
    }
}

pub fn parse_day(text: &str) -> Result<CivilDay, super::Error> {
    let mut parts = text.split('-');
    let year = parts
        .next()
        .ok_or_else(|| super::Error::Usage(format!("bad date {text}")))?;
    let month = parts
        .next()
        .ok_or_else(|| super::Error::Usage(format!("bad date {text}")))?;
    let day = parts
        .next()
        .ok_or_else(|| super::Error::Usage(format!("bad date {text}")))?;
    if parts.next().is_some() || year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return Err(super::Error::Usage(format!("bad date {text}")));
    }
    let year: i32 = year
        .parse()
        .map_err(|_| super::Error::Usage(format!("bad date {text}")))?;
    let month: u8 = month
        .parse()
        .map_err(|_| super::Error::Usage(format!("bad date {text}")))?;
    let day: u8 = day
        .parse()
        .map_err(|_| super::Error::Usage(format!("bad date {text}")))?;
    CivilDay::new(year, month, day).ok_or_else(|| super::Error::Usage(format!("bad date {text}")))
}

pub fn parse_utc_datetime(text: &str) -> Result<UtcMicros, super::Error> {
    let (date, time) = text.split_once(' ').ok_or_else(|| {
        super::Error::check(
            "datetime",
            format!("expected YYYY-MM-DD HH:MM:SS, got {text}"),
        )
    })?;
    if time.len() != 8 || date.len() != 10 {
        return Err(super::Error::check(
            "datetime",
            format!("expected YYYY-MM-DD HH:MM:SS, got {text}"),
        ));
    }
    let day = parse_day(date)?;
    let hour: i64 = parse_hms(&time[0..2], 23, text)?;
    let minute: i64 = parse_hms(&time[3..5], 59, text)?;
    let second: i64 = parse_hms(&time[6..8], 59, text)?;
    if time.as_bytes().get(2) != Some(&b':') || time.as_bytes().get(5) != Some(&b':') {
        return Err(super::Error::check("datetime", format!("bad time {text}")));
    }
    let micros = hour * 3_600_000_000 + minute * 60_000_000 + second * 1_000_000;
    day.start_us()
        .checked_add_micros(micros)
        .ok_or_else(|| super::Error::check("datetime", "timestamp overflow"))
}

fn parse_hms(text: &str, max: i64, full: &str) -> Result<i64, super::Error> {
    if text.len() != 2 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(super::Error::check("datetime", format!("bad time {full}")));
    }
    let value: i64 = text
        .parse()
        .map_err(|_| super::Error::check("datetime", format!("bad time {full}")))?;
    if value > max {
        return Err(super::Error::check("datetime", format!("bad time {full}")));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u8, d: u8) -> Result<CivilDay, String> {
        CivilDay::new(y, m, d).ok_or_else(|| "fixture date".to_string())
    }

    #[test]
    fn known_epoch_days() -> Result<(), String> {
        assert_eq!(day(1970, 1, 1)?.epoch_days(), 0);
        assert_eq!(day(1970, 1, 1)?.start_us().micros(), 0);
        assert_eq!(day(2020, 1, 1)?.start_us().micros(), 1_577_836_800_000_000);
        assert_eq!(day(2025, 1, 1)?.start_us().micros(), 1_735_689_600_000_000);
        assert_eq!(day(2026, 9, 1)?.start_us().micros(), 1_788_220_800_000_000);
        assert_eq!(
            day(2026, 10, 20)?.start_us().micros(),
            1_792_454_400_000_000
        );
        Ok(())
    }

    #[test]
    fn round_trip_known_days() -> Result<(), String> {
        for (y, m, d) in [
            (1970, 1, 1),
            (2020, 2, 29),
            (2021, 2, 28),
            (2024, 2, 29),
            (2025, 1, 1),
            (2026, 9, 30),
            (2099, 12, 31),
        ] {
            let civil = day(y, m, d)?;
            assert_eq!(
                day_of(civil.start_us()).map_err(|err| err.to_string())?,
                civil
            );
        }
        Ok(())
    }

    #[test]
    fn february_29_on_a_common_year_is_rejected() {
        assert!(CivilDay::new(2026, 2, 29).is_none());
        assert!(CivilDay::new(2024, 2, 29).is_some());
    }

    #[test]
    fn datetime_is_utc_and_rejects_a_bad_clock() -> Result<(), String> {
        let ts = parse_utc_datetime("2026-09-01 00:00:00").map_err(|err| err.to_string())?;
        assert_eq!(ts.micros(), day(2026, 9, 1)?.start_us().micros());
        assert!(parse_utc_datetime("2026-09-01 24:00:00").is_err());
        assert!(parse_utc_datetime("2026-09-01T00:00:00Z").is_err());
        Ok(())
    }
}
