//! Load-time row checks. Each rejecting check is its own [`LoadError`].
//!
//! `available_ts >= event_ts` compares the stored clocks. It does not rewrite
//! either value.

use parangsae_core::{source_tick_us, validate_close_time, Interval, KlineSpan, Receipt, TimeUnit};

use crate::civil::CivilDay;
use crate::version::load::{LoadError, PartitionStats};
use crate::vision::{zero_volume_flat, KlineFact, TickDomain};

const ONE_MINUTE_US: i64 = 60_000_000;

pub struct KlineCheck<'a> {
    pub day: CivilDay,
    pub symbol: &'a str,
    pub stats: &'a PartitionStats,
    pub unit: TimeUnit,
    pub domain: TickDomain,
    pub require_archive: bool,
}

pub fn validate_klines(rows: &[KlineFact], check: &KlineCheck<'_>) -> Result<(), LoadError> {
    let day = check.day;
    for pair in rows.windows(2) {
        if pair[1].open_time <= pair[0].open_time {
            return Err(LoadError::PkOrder { date: day });
        }
    }
    let start = day.start_us();
    let end = day.end_us();
    for row in rows {
        if row.symbol.as_str() != check.symbol {
            return Err(LoadError::RowInvariant {
                date: day,
                check: "symbol",
            });
        }
        if row.open_time < start || row.open_time >= end {
            return Err(LoadError::RowInvariant {
                date: day,
                check: "pk_day",
            });
        }
    }
    let count = u64::try_from(rows.len()).map_err(|_| LoadError::Io("row count".into()))?;
    let (pk_min, pk_max, available_min, available_max) = bounds(rows);
    if count != check.stats.row_count
        || pk_min != check.stats.pk_min_us
        || pk_max != check.stats.pk_max_us
        || available_min != check.stats.available_min_us
        || available_max != check.stats.available_max_us
    {
        return Err(LoadError::PartitionStatsMismatch { date: day });
    }
    for row in rows {
        if !receipt_ok(row.receipt, check.require_archive) {
            return Err(LoadError::RowInvariant {
                date: day,
                check: "receipt",
            });
        }
    }
    for row in rows {
        if row.interval_us != ONE_MINUTE_US || row.open_time.micros() % ONE_MINUTE_US != 0 {
            return Err(LoadError::RowInvariant {
                date: day,
                check: "grid",
            });
        }
        let span = KlineSpan {
            open_time: row.open_time,
            close_time: row.close_time,
            interval: Interval::ONE_MINUTE,
            unit: check.unit,
        };
        if validate_close_time(span).is_err() {
            return Err(LoadError::RowInvariant {
                date: day,
                check: "close_time",
            });
        }
        if !ordered(
            row.low_ticks,
            row.open_ticks,
            row.close_ticks,
            row.high_ticks,
        ) {
            return Err(LoadError::RowInvariant {
                date: day,
                check: "ohlc",
            });
        }
        if row.taker_buy_volume.ticks() > row.volume.ticks()
            || row.taker_buy_quote_volume.ticks() > row.quote_volume.ticks()
        {
            return Err(LoadError::RowInvariant {
                date: day,
                check: "taker",
            });
        }
        if row.tick_domain != check.domain {
            return Err(LoadError::RowInvariant {
                date: day,
                check: "domain",
            });
        }
        let flat = zero_volume_flat(
            row.volume.ticks(),
            row.count,
            row.open_ticks,
            row.high_ticks,
            row.low_ticks,
            row.close_ticks,
        );
        if row.zero_volume_flat != flat {
            return Err(LoadError::RowInvariant {
                date: day,
                check: "zero_volume_flat",
            });
        }
    }
    let tick = source_tick_us(check.unit);
    for row in rows {
        let Some(expected_available) = row.close_time.checked_add_micros(tick) else {
            return Err(LoadError::AvailableTsRule { date: day });
        };
        if row.event_ts.micros() != row.open_time || row.available_ts.micros() != expected_available
        {
            return Err(LoadError::AvailableTsRule { date: day });
        }
    }
    // Stored clocks only. A row that is early stays early; nothing substitutes max(available, event).
    for row in rows {
        available_before_event(
            day,
            row.event_ts.micros().micros(),
            row.available_ts.micros().micros(),
        )?;
    }
    Ok(())
}

pub(crate) fn available_before_event(
    day: CivilDay,
    event_us: i64,
    available_us: i64,
) -> Result<(), LoadError> {
    if available_us < event_us {
        Err(LoadError::AvailableTsRule { date: day })
    } else {
        Ok(())
    }
}

fn bounds(rows: &[KlineFact]) -> (i64, i64, i64, i64) {
    let pk_min = rows
        .iter()
        .map(|row| row.open_time.micros())
        .min()
        .unwrap_or(0);
    let pk_max = rows
        .iter()
        .map(|row| row.open_time.micros())
        .max()
        .unwrap_or(0);
    let available_min = rows
        .iter()
        .map(|row| row.available_ts.micros().micros())
        .min()
        .unwrap_or(0);
    let available_max = rows
        .iter()
        .map(|row| row.available_ts.micros().micros())
        .max()
        .unwrap_or(0);
    (pk_min, pk_max, available_min, available_max)
}

fn receipt_ok(receipt: Receipt, require_archive: bool) -> bool {
    match receipt {
        Receipt::Archive { .. } => true,
        Receipt::Live { .. } => !require_archive,
    }
}

fn ordered(low: i64, open: i64, close: i64, high: i64) -> bool {
    low <= open && low <= close && open <= high && close <= high
}
