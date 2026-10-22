//! CSV and zip parsing. One pass, at the boundary, into domain rows.

use std::io::{Cursor, Read};

use parangsae_core::{
    Amount, AvailableTs, EventTs, Price, Qty, Ratio, Receipt, Symbol, UtcMicros, MICROS_PER_MILLIS,
};
use zip::ZipArchive;

use super::rows::{zero_volume_flat, BookFact, FundingFact, KlineFact, MetricsFact, TickDomain};
use super::{
    SCHEMA_BOOK, SCHEMA_FUNDING, SCHEMA_KLINE_HEADER, SCHEMA_KLINE_HEADERLESS, SCHEMA_METRICS,
};
use crate::civil::parse_utc_datetime;
use crate::normalize::{SourceClock, FUNDING_CLOCK};
use crate::scale::{parse_ticks, parse_ticks_exact, TICK_PLACES};
use crate::Error;

const KLINE_HEADER: &str =
    "open_time,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore";
const FUNDING_HEADER: &str = "calc_time,funding_interval_hours,last_funding_rate";
const METRICS_HEADER: &str = "create_time,symbol,sum_open_interest,sum_open_interest_value,count_toptrader_long_short_ratio,sum_toptrader_long_short_ratio,count_long_short_ratio,sum_taker_long_short_vol_ratio";
const BOOK_HEADER: &str = "timestamp,percentage,depth,notional";

const ONE_MINUTE_US: i64 = 60_000_000;
/// Placeholder publication lag (`available_ts_rules/v0.2`). Not a clamp.
pub const FUNDING_AVAILABLE_LAG_US: i64 = 60_000_000;
const FIVE_MINUTES_US: i64 = 300_000_000;
const EIGHT_HOURS_US: i64 = 8 * 3_600_000_000;
const SNAP_TOLERANCE_US: i64 = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed<T> {
    pub schema_version: u32,
    pub rows: Vec<T>,
}

pub fn unzip_csv(bytes: &[u8]) -> Result<String, Error> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|err| Error::Zip(err.to_string()))?;
    let mut csv_index = None;
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|err| Error::Zip(err.to_string()))?
            .name()
            .to_owned();
        if name.ends_with('/') {
            continue;
        }
        if csv_index.is_some() {
            return Err(Error::Zip("archive has more than one file".into()));
        }
        if !name.ends_with(".csv") {
            return Err(Error::Zip(format!("entry {name} is not a csv")));
        }
        csv_index = Some(index);
    }
    let index = csv_index.ok_or_else(|| Error::Zip("archive has no csv".into()))?;
    let mut file = archive
        .by_index(index)
        .map_err(|err| Error::Zip(err.to_string()))?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|err| Error::Zip(err.to_string()))?;
    if text.starts_with('\u{feff}') {
        text.remove(0);
    }
    Ok(text)
}

pub fn parse_klines(
    csv: &str,
    symbol: &Symbol,
    clock: SourceClock,
    domain: TickDomain,
    receipt: Receipt,
    record_zero_volume_flat: bool,
) -> Result<Parsed<KlineFact>, Error> {
    let lines = nonempty_lines(csv);
    if lines.is_empty() {
        return Err(Error::Csv {
            line: 1,
            detail: "kline file is empty".into(),
        });
    }
    let (schema_version, data) = if lines[0] == KLINE_HEADER {
        (SCHEMA_KLINE_HEADER, &lines[1..])
    } else {
        (SCHEMA_KLINE_HEADERLESS, lines.as_slice())
    };
    let mut rows = Vec::with_capacity(data.len());
    for (offset, line) in data.iter().enumerate() {
        let line_no = if schema_version == SCHEMA_KLINE_HEADER {
            offset + 2
        } else {
            offset + 1
        };
        rows.push(parse_kline_line(
            line,
            line_no,
            &KlineParse {
                symbol,
                clock,
                domain,
                receipt,
                record_zero_volume_flat,
                schema_version,
            },
        )?);
    }
    Ok(Parsed {
        schema_version,
        rows,
    })
}

struct KlineParse<'a> {
    symbol: &'a Symbol,
    clock: SourceClock,
    domain: TickDomain,
    receipt: Receipt,
    record_zero_volume_flat: bool,
    schema_version: u32,
}

fn parse_kline_line(
    line: &str,
    line_no: usize,
    source: &KlineParse<'_>,
) -> Result<KlineFact, Error> {
    let fields = split_fields(line, line_no, 12)?;
    let open_raw = parse_int(fields[0], line_no, "open_time")?;
    let close_raw = parse_int(fields[6], line_no, "close_time")?;
    let open_time = source.clock.read(open_raw)?;
    let close_time = source.clock.read(close_raw)?;
    let open_ticks = ohlc_ticks(fields[1], line_no, source.domain)?;
    let high_ticks = ohlc_ticks(fields[2], line_no, source.domain)?;
    let low_ticks = ohlc_ticks(fields[3], line_no, source.domain)?;
    let close_ticks = ohlc_ticks(fields[4], line_no, source.domain)?;
    let volume = Qty::from_ticks(parse_ticks(fields[5])?)?;
    let quote_volume = Amount::from_ticks(parse_ticks(fields[7])?)?;
    let count = parse_int(fields[8], line_no, "count")?;
    if count < 0 {
        return Err(Error::check("negative", format!("line {line_no} count")));
    }
    let taker_buy_volume = Qty::from_ticks(parse_ticks(fields[9])?)?;
    let taker_buy_quote_volume = Amount::from_ticks(parse_ticks(fields[10])?)?;
    let available = close_time
        .checked_add_micros(MICROS_PER_MILLIS)
        .ok_or_else(|| {
            Error::check(
                "close_time",
                format!("line {line_no} available_ts overflow"),
            )
        })?;
    Ok(KlineFact {
        symbol: source.symbol.clone(),
        interval_us: ONE_MINUTE_US,
        open_time,
        close_time,
        open_ticks,
        high_ticks,
        low_ticks,
        close_ticks,
        volume,
        quote_volume,
        count,
        taker_buy_volume,
        taker_buy_quote_volume,
        event_ts: EventTs::from_micros(open_time),
        available_ts: AvailableTs::from_micros(available),
        receipt: source.receipt,
        schema_version: source.schema_version,
        tick_domain: source.domain,
        zero_volume_flat: if source.record_zero_volume_flat {
            zero_volume_flat(
                volume.ticks(),
                count,
                open_ticks,
                high_ticks,
                low_ticks,
                close_ticks,
            )
        } else {
            false
        },
    })
}

fn ohlc_ticks(text: &str, line_no: usize, domain: TickDomain) -> Result<i64, Error> {
    let ticks = parse_ticks(text)?;
    match domain {
        TickDomain::Price => {
            Price::from_ticks(ticks)
                .map_err(|err| Error::check("negative", format!("line {line_no} price: {err}")))?;
        }
        TickDomain::Ratio => {
            let _ = Ratio::from_ticks(ticks);
        }
    }
    Ok(ticks)
}

pub fn parse_funding(
    csv: &str,
    symbol: &Symbol,
    receipt: Receipt,
) -> Result<Parsed<FundingFact>, Error> {
    let data = headed_lines(csv, FUNDING_HEADER)?;
    let mut rows = Vec::with_capacity(data.len());
    for (offset, line) in data.iter().enumerate() {
        let line_no = offset + 2;
        let fields = split_fields(line, line_no, 3)?;
        let raw_calc = FUNDING_CLOCK.read(parse_int(fields[0], line_no, "calc_time")?)?;
        let interval_hours = parse_int(fields[1], line_no, "funding_interval_hours")?;
        if interval_hours != 8 {
            return Err(Error::check(
                "funding_interval",
                format!("line {line_no} funding_interval_hours is {interval_hours}, declared 8"),
            ));
        }
        let rate = Ratio::from_ticks(parse_ticks_exact(fields[2], TICK_PLACES)?);
        let funding_time = snap_funding_time(raw_calc).map_err(|err| {
            Error::check(
                "funding_snap",
                format!("line {line_no} calc_time {} {err}", raw_calc.micros()),
            )
        })?;
        let available = funding_time
            .checked_add_micros(FUNDING_AVAILABLE_LAG_US)
            .ok_or_else(|| {
                Error::check(
                    "available_ts",
                    format!("line {line_no} funding_time + 60s overflows"),
                )
            })?;
        rows.push(FundingFact {
            symbol: symbol.clone(),
            raw_calc_time: raw_calc,
            funding_time,
            interval_hours,
            rate,
            event_ts: EventTs::from_micros(raw_calc),
            available_ts: AvailableTs::from_micros(available),
            receipt,
            schema_version: SCHEMA_FUNDING,
        });
    }
    Ok(Parsed {
        schema_version: SCHEMA_FUNDING,
        rows,
    })
}

pub fn snap_funding_time(raw: UtcMicros) -> Result<UtcMicros, String> {
    if raw.micros() < 0 {
        return Err("is before the unix epoch".into());
    }
    let base = raw.micros() / EIGHT_HOURS_US * EIGHT_HOURS_US;
    let mut best: Option<(i64, i64)> = None;
    for slot in [base, base + EIGHT_HOURS_US] {
        let distance = (raw.micros() - slot).abs();
        let replace = match best {
            None => true,
            Some((best_distance, _)) => distance < best_distance,
        };
        if replace {
            best = Some((distance, slot));
        }
    }
    let (distance, slot) = best.ok_or_else(|| "has no settlement slot".to_owned())?;
    if distance > SNAP_TOLERANCE_US {
        return Err(format!(
            "is {distance}us from the nearest 8h slot {slot}, tolerance is {SNAP_TOLERANCE_US}us"
        ));
    }
    Ok(UtcMicros::from_micros(slot))
}

pub fn parse_metrics(
    csv: &str,
    symbol: &Symbol,
    receipt: Receipt,
) -> Result<Parsed<MetricsFact>, Error> {
    let data = headed_lines(csv, METRICS_HEADER)?;
    let mut rows = Vec::with_capacity(data.len());
    for (offset, line) in data.iter().enumerate() {
        let line_no = offset + 2;
        let fields = split_fields(line, line_no, 8)?;
        if fields[1] != symbol.as_str() {
            return Err(Error::check(
                "symbol",
                format!("line {line_no} symbol is {}, expected {symbol}", fields[1]),
            ));
        }
        let create_time = parse_utc_datetime(fields[0])?;
        let available = create_time
            .checked_add_micros(FIVE_MINUTES_US)
            .ok_or_else(|| {
                Error::check(
                    "metrics_grid",
                    format!("line {line_no} available_ts overflow"),
                )
            })?;
        rows.push(MetricsFact {
            symbol: symbol.clone(),
            create_time,
            sum_open_interest: Qty::from_ticks(parse_ticks_exact(fields[2], TICK_PLACES)?)?,
            sum_open_interest_value: Amount::from_ticks(parse_ticks_exact(
                fields[3],
                TICK_PLACES,
            )?)?,
            count_toptrader_long_short_ratio: Ratio::from_ticks(parse_ticks(fields[4])?),
            sum_toptrader_long_short_ratio: Ratio::from_ticks(parse_ticks(fields[5])?),
            count_long_short_ratio: Ratio::from_ticks(parse_ticks(fields[6])?),
            sum_taker_long_short_vol_ratio: Ratio::from_ticks(parse_ticks(fields[7])?),
            event_ts: EventTs::from_micros(create_time),
            available_ts: AvailableTs::from_micros(available),
            receipt,
            available_lag_verified: false,
            schema_version: SCHEMA_METRICS,
        });
    }
    Ok(Parsed {
        schema_version: SCHEMA_METRICS,
        rows,
    })
}

pub fn parse_book(csv: &str, symbol: &Symbol, receipt: Receipt) -> Result<Parsed<BookFact>, Error> {
    let data = headed_lines(csv, BOOK_HEADER)?;
    let mut rows = Vec::with_capacity(data.len());
    for (offset, line) in data.iter().enumerate() {
        let line_no = offset + 2;
        let fields = split_fields(line, line_no, 4)?;
        let timestamp = parse_utc_datetime(fields[0])?;
        rows.push(BookFact {
            symbol: symbol.clone(),
            timestamp,
            percentage: Ratio::from_ticks(parse_ticks(fields[1])?),
            depth: Qty::from_ticks(parse_ticks(fields[2])?)?,
            notional: Amount::from_ticks(parse_ticks(fields[3])?)?,
            level_schema: 0,
            event_ts: EventTs::from_micros(timestamp),
            available_ts: AvailableTs::from_micros(timestamp),
            receipt,
            schema_version: SCHEMA_BOOK,
        });
    }
    Ok(Parsed {
        schema_version: SCHEMA_BOOK,
        rows,
    })
}

fn headed_lines<'a>(csv: &'a str, header: &str) -> Result<Vec<&'a str>, Error> {
    let lines = nonempty_lines(csv);
    if lines.is_empty() {
        return Err(Error::Csv {
            line: 1,
            detail: "file is empty".into(),
        });
    }
    if lines[0] != header {
        return Err(Error::check(
            "schema",
            format!("header {:?} does not match the schema", lines[0]),
        ));
    }
    Ok(lines.into_iter().skip(1).collect())
}

fn nonempty_lines(csv: &str) -> Vec<&str> {
    csv.split('\n')
        .map(|line| line.trim_end_matches('\r').trim())
        .filter(|line| !line.is_empty())
        .collect()
}

fn split_fields(line: &str, line_no: usize, expected: usize) -> Result<Vec<&str>, Error> {
    let fields: Vec<&str> = line.split(',').collect();
    if fields.len() != expected {
        return Err(Error::Csv {
            line: line_no,
            detail: format!("expected {expected} columns, found {}", fields.len()),
        });
    }
    Ok(fields)
}

fn parse_int(text: &str, line_no: usize, field: &str) -> Result<i64, Error> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::Csv {
            line: line_no,
            detail: format!("{field} is not an integer"),
        });
    }
    text.parse::<i64>().map_err(|_| Error::Csv {
        line: line_no,
        detail: format!("{field} does not fit in i64"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::CivilDay;
    use crate::normalize::{spot_kline_clock, UM_KLINE_CLOCK};
    use crate::scale::format_ticks;
    use parangsae_core::RunId;
    use proptest::prelude::*;

    fn symbol() -> Symbol {
        Symbol::new("BTCUSDT").expect("symbol")
    }

    fn receipt() -> Receipt {
        Receipt::Archive {
            fetched_at_run: RunId::from_bytes([1; 16]),
        }
    }

    fn fixture(name: &str) -> String {
        let path = format!(
            "{}/../../fixtures/vision/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(path).expect("fixture")
    }

    #[test]
    fn headerless_um_and_headed_um_parse() {
        let headerless = parse_klines(
            &fixture("um_kline_2021-06-01_headerless.csv"),
            &symbol(),
            UM_KLINE_CLOCK,
            TickDomain::Price,
            receipt(),
            true,
        )
        .unwrap();
        assert_eq!(headerless.schema_version, SCHEMA_KLINE_HEADERLESS);
        assert_eq!(headerless.rows.len(), 3);
        assert_eq!(headerless.rows[0].open_ticks, 3_724_436_000_000);
        let headed = parse_klines(
            &fixture("um_kline_2026-10-20_header.csv"),
            &symbol(),
            UM_KLINE_CLOCK,
            TickDomain::Price,
            receipt(),
            true,
        )
        .unwrap();
        assert_eq!(headed.schema_version, SCHEMA_KLINE_HEADER);
        assert_eq!(headed.rows[0].open_ticks, 8_190_330_000_000);
    }

    #[test]
    fn spot_micros_parse_and_millis_declaration_fails() {
        let csv = fixture("spot_kline_2025-01-01_us.csv");
        let day = CivilDay::new(2025, 1, 1).unwrap();
        let parsed = parse_klines(
            &csv,
            &symbol(),
            spot_kline_clock(day),
            TickDomain::Price,
            receipt(),
            true,
        )
        .unwrap();
        assert_eq!(parsed.rows[0].open_time.micros(), 1_735_689_600_000_000);
        let err = parse_klines(
            &csv,
            &symbol(),
            UM_KLINE_CLOCK,
            TickDomain::Price,
            receipt(),
            true,
        )
        .unwrap_err();
        assert_eq!(err.check_name(), Some("timestamp_unit"));
    }

    #[test]
    fn funding_snap_keeps_the_raw_calc_time() {
        let parsed =
            parse_funding(&fixture("funding_2026-09-01.csv"), &symbol(), receipt()).unwrap();
        assert_eq!(parsed.rows[0].raw_calc_time.micros(), 1_788_220_800_005_000);
        assert_eq!(parsed.rows[0].funding_time.micros(), 1_788_220_800_000_000);
        assert_eq!(
            parsed.rows[0].available_ts.micros().micros(),
            1_788_220_860_000_000
        );
        assert_eq!(
            parsed.rows[0].event_ts.micros().micros(),
            1_788_220_800_005_000
        );
        let err =
            parse_funding(&fixture("funding_off_slot.csv"), &symbol(), receipt()).unwrap_err();
        assert_eq!(err.check_name(), Some("funding_snap"));
    }

    #[test]
    fn slice_zip_round_trips_through_unzip() {
        let bytes = include_bytes!("../../../../fixtures/vision/um_kline_slice.zip");
        let csv = unzip_csv(bytes).unwrap();
        let parsed = parse_klines(
            &csv,
            &symbol(),
            UM_KLINE_CLOCK,
            TickDomain::Price,
            receipt(),
            true,
        )
        .unwrap();
        assert_eq!(parsed.rows.len(), 3);
    }

    proptest! {
        #[test]
        fn kline_parser_round_trip(minute in 0i64..500, price in 1i64..50_000_000_000i64, volume in 0i64..1_000_000_000i64) {
            let open_ms = 1_622_505_600_000 + minute * 60_000;
            let close_ms = open_ms + 59_999;
            let line = format!(
                "{open_ms},{},{},{},{},{},{close_ms},{},1,{},{},0",
                format_ticks(price),
                format_ticks(price),
                format_ticks(price),
                format_ticks(price),
                format_ticks(volume),
                format_ticks(volume),
                format_ticks(volume),
                format_ticks(volume),
            );
            let parsed = parse_klines(
                &line,
                &symbol(),
                UM_KLINE_CLOCK,
                TickDomain::Price,
                receipt(),
                true,
            )
            .unwrap();
            prop_assert_eq!(parsed.rows.len(), 1);
            prop_assert_eq!(parsed.rows[0].open_ticks, price);
            prop_assert_eq!(parsed.rows[0].volume.ticks(), volume);
            prop_assert_eq!(parsed.rows[0].open_time.micros(), open_ms * 1_000);
            prop_assert_eq!(parsed.rows[0].close_time.micros(), close_ms * 1_000);
        }
    }

    #[test]
    fn kline_scientific_notation_stays_rejected() -> Result<(), Error> {
        let csv = "1577836800000,1e-3,1,1,1,0,1577836859999,0,0,0,0,0\n";
        let err = parse_klines(
            csv,
            &symbol(),
            UM_KLINE_CLOCK,
            TickDomain::Price,
            receipt(),
            true,
        )
        .err()
        .ok_or_else(|| Error::Usage("kline exponent was accepted".into()))?;
        match err {
            Error::Scale(crate::error::ScaleError::Charset) => Ok(()),
            other => Err(other),
        }
    }

    #[test]
    fn metrics_open_interest_exact_exponent_is_zero() -> Result<(), Error> {
        let parsed = parse_metrics(
            &fixture("metrics_open_interest_exponent.csv"),
            &symbol(),
            receipt(),
        )?;
        let row = parsed
            .rows
            .first()
            .ok_or_else(|| Error::Usage("metrics fixture has no row".into()))?;
        if row.sum_open_interest.ticks() != 0 || row.sum_open_interest_value.ticks() != 0 {
            return Err(Error::check(
                "scale",
                format!(
                    "open interest ticks {} value {}",
                    row.sum_open_interest.ticks(),
                    row.sum_open_interest_value.ticks()
                ),
            ));
        }
        Ok(())
    }

    #[test]
    fn metrics_ratio_exponent_stays_on_the_plain_parser() -> Result<(), Error> {
        let csv = "create_time,symbol,sum_open_interest,sum_open_interest_value,count_toptrader_long_short_ratio,sum_toptrader_long_short_ratio,count_long_short_ratio,sum_taker_long_short_vol_ratio\n2020-09-01 00:00:00,BTCUSDT,1,1,0E-8,1,1,1\n";
        let err = parse_metrics(csv, &symbol(), receipt())
            .err()
            .ok_or_else(|| Error::Usage("metrics ratio exponent was accepted".into()))?;
        match err {
            Error::Scale(crate::error::ScaleError::Charset) => Ok(()),
            other => Err(other),
        }
    }
}
