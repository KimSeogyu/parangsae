//! Integrity checks from the data plan. A fault rejects the archive's rows.
//! A gap is recorded and still fails the command, except an allowlisted missing
//! bookDepth day.

use std::collections::{BTreeMap, BTreeSet};

use parangsae_core::{validate_close_time, Interval, KlineSpan, TimeUnit, UtcMicros};

use super::rows::{BookFact, FundingFact, KlineFact, MetricsFact};
use crate::civil::{CivilDay, YearMonth};

pub const BOOK_GAP_LIMIT_US: i64 = 90_000_000;
const ONE_MINUTE_US: i64 = 60_000_000;
const FIVE_MINUTES_US: i64 = 300_000_000;
const EIGHT_HOURS_US: i64 = 8 * 3_600_000_000;
const MINUTES_PER_DAY: i64 = 1_440;
const METRICS_PER_DAY: i64 = 288;
const PCT_SCALE: i64 = 100_000_000;
const PCT_FIFTH: i64 = 20_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaultReport {
    pub name: &'static str,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapKind {
    Grid,
    BookSpan,
    FundingSlot,
    MissingFile,
    AllowlistedMissing,
}

impl GapKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::BookSpan => "book_gap",
            Self::FundingSlot => "funding_slot",
            Self::MissingFile => "missing_file",
            Self::AllowlistedMissing => "allowlisted_missing",
        }
    }

    pub const fn fails_command(self) -> bool {
        match self {
            Self::AllowlistedMissing => false,
            Self::Grid | Self::BookSpan | Self::FundingSlot | Self::MissingFile => true,
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "grid" => Some(Self::Grid),
            "book_gap" => Some(Self::BookSpan),
            "funding_slot" => Some(Self::FundingSlot),
            "missing_file" => Some(Self::MissingFile),
            "allowlisted_missing" => Some(Self::AllowlistedMissing),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gap {
    pub kind: GapKind,
    pub start: UtcMicros,
    pub end: UtcMicros,
    pub detail: String,
}

pub fn book_allowlisted(day: CivilDay) -> bool {
    matches!(
        (day.year(), day.month(), day.day()),
        (2023, 2, 8) | (2023, 2, 9) | (2024, 4, 18) | (2026, 10, 8)
    )
}

pub fn missing_coverage(day: CivilDay, allow_book: bool) -> Gap {
    if allow_book && book_allowlisted(day) {
        Gap {
            kind: GapKind::AllowlistedMissing,
            start: day.start_us(),
            end: day.end_us(),
            detail: format!("bookDepth {day} is on the known-missing allowlist"),
        }
    } else {
        Gap {
            kind: GapKind::MissingFile,
            start: day.start_us(),
            end: day.end_us(),
            detail: format!("no archive for {day}"),
        }
    }
}

pub fn collapse_by<T, K>(rows: Vec<T>, key_of: impl Fn(&T) -> K) -> Result<Vec<T>, FaultReport>
where
    T: Clone + PartialEq,
    K: Ord + Clone,
{
    let mut keyed: Vec<(K, T)> = rows.into_iter().map(|row| (key_of(&row), row)).collect();
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    let mut out: Vec<(K, T)> = Vec::new();
    for (key, row) in keyed {
        if let Some((previous_key, previous)) = out.last() {
            if previous_key == &key {
                if previous != &row {
                    return Err(FaultReport {
                        name: "duplicate",
                        detail: "same key with a different payload".into(),
                    });
                }
                continue;
            }
        }
        out.push((key, row));
    }
    Ok(out.into_iter().map(|(_, row)| row).collect())
}

pub fn kline_row_faults(rows: &[KlineFact], unit: TimeUnit) -> Vec<FaultReport> {
    let mut faults = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if row.interval_us != ONE_MINUTE_US || row.open_time.micros() % ONE_MINUTE_US != 0 {
            faults.push(FaultReport {
                name: "grid",
                detail: format!("row {index} open_time is not on the 1m grid"),
            });
        }
        let span = KlineSpan {
            open_time: row.open_time,
            close_time: row.close_time,
            interval: Interval::ONE_MINUTE,
            unit,
        };
        if let Err(err) = validate_close_time(span) {
            faults.push(FaultReport {
                name: "close_time",
                detail: format!("row {index} {err}"),
            });
        }
        if !ohlc_ordered(
            row.low_ticks,
            row.open_ticks,
            row.close_ticks,
            row.high_ticks,
        ) {
            faults.push(FaultReport {
                name: "ohlc",
                detail: format!("row {index} low/open/close/high are not ordered"),
            });
        }
        if row.taker_buy_volume.ticks() > row.volume.ticks() {
            faults.push(FaultReport {
                name: "taker_buy",
                detail: format!(
                    "row {index} taker_buy {} > volume {}",
                    row.taker_buy_volume.ticks(),
                    row.volume.ticks()
                ),
            });
        }
        if row.taker_buy_quote_volume.ticks() > row.quote_volume.ticks() {
            faults.push(FaultReport {
                name: "taker_buy_quote",
                detail: format!(
                    "row {index} taker_buy_quote {} > quote_volume {}",
                    row.taker_buy_quote_volume.ticks(),
                    row.quote_volume.ticks()
                ),
            });
        }
        if faults.len() >= 8 {
            break;
        }
    }
    faults
}

fn ohlc_ordered(low: i64, open: i64, close: i64, high: i64) -> bool {
    low <= open && low <= close && open <= high && close <= high
}

pub fn kline_day_gaps(rows: &[KlineFact], day: CivilDay) -> Vec<Gap> {
    let present: BTreeSet<i64> = rows
        .iter()
        .filter(|row| crate::civil::day_of(row.open_time).ok() == Some(day))
        .map(|row| row.open_time.micros())
        .collect();
    missing_slots(
        day.start_us().micros(),
        ONE_MINUTE_US,
        MINUTES_PER_DAY,
        &present,
        GapKind::Grid,
    )
}

pub fn metrics_row_faults(rows: &[MetricsFact]) -> Vec<FaultReport> {
    let mut faults = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if row.create_time.micros() % FIVE_MINUTES_US != 0 {
            faults.push(FaultReport {
                name: "metrics_grid",
                detail: format!("row {index} create_time is not on the 5m grid"),
            });
        }
        if row.available_lag_verified {
            faults.push(FaultReport {
                name: "available_lag",
                detail: format!("row {index} marks the metrics publication lag as verified"),
            });
        }
        let expected_available = row.create_time.micros().saturating_add(FIVE_MINUTES_US);
        if row.available_ts.micros().micros() != expected_available {
            faults.push(FaultReport {
                name: "available_ts",
                detail: format!("row {index} available_ts is not create_time + 5m"),
            });
        }
        for (name, ratio) in [
            (
                "count_toptrader_long_short_ratio",
                row.count_toptrader_long_short_ratio,
            ),
            (
                "sum_toptrader_long_short_ratio",
                row.sum_toptrader_long_short_ratio,
            ),
            ("count_long_short_ratio", row.count_long_short_ratio),
            (
                "sum_taker_long_short_vol_ratio",
                row.sum_taker_long_short_vol_ratio,
            ),
        ] {
            if ratio.ticks() <= 0 {
                faults.push(FaultReport {
                    name: "ratio",
                    detail: format!("row {index} {name} is not positive"),
                });
            }
        }
        if faults.len() >= 8 {
            break;
        }
    }
    faults
}

pub fn metrics_day_gaps(rows: &[MetricsFact], day: CivilDay) -> Vec<Gap> {
    let present: BTreeSet<i64> = rows
        .iter()
        .filter(|row| crate::civil::day_of(row.create_time).ok() == Some(day))
        .map(|row| row.create_time.micros())
        .collect();
    missing_slots(
        day.start_us().micros(),
        FIVE_MINUTES_US,
        METRICS_PER_DAY,
        &present,
        GapKind::Grid,
    )
}

pub fn expected_funding_slots(month: YearMonth) -> Vec<UtcMicros> {
    let mut cursor = month.first().start_us().micros();
    let end = month.last().end_us().micros();
    let mut slots = Vec::new();
    while cursor < end {
        slots.push(UtcMicros::from_micros(cursor));
        cursor += EIGHT_HOURS_US;
    }
    slots
}

pub fn funding_window(
    rows: &[FundingFact],
    month: YearMonth,
) -> Result<Vec<Gap>, Vec<FaultReport>> {
    let expected = expected_funding_slots(month);
    let expected_set: BTreeSet<i64> = expected.iter().map(|slot| slot.micros()).collect();
    let mut present: BTreeSet<i64> = BTreeSet::new();
    let mut faults = Vec::new();
    for row in rows {
        if row.interval_hours != 8 {
            faults.push(FaultReport {
                name: "funding_interval",
                detail: format!("interval_hours is {}", row.interval_hours),
            });
        }
        if !expected_set.contains(&row.funding_time.micros()) {
            faults.push(FaultReport {
                name: "funding_extra",
                detail: format!(
                    "funding_time {} is outside {}",
                    row.funding_time.micros(),
                    month.first()
                ),
            });
        }
        present.insert(row.funding_time.micros());
    }
    if !faults.is_empty() {
        return Err(faults);
    }
    let gaps = expected
        .into_iter()
        .filter(|slot| !present.contains(&slot.micros()))
        .map(|slot| Gap {
            kind: GapKind::FundingSlot,
            start: slot,
            end: slot,
            detail: format!("missing settlement {}", slot.micros()),
        })
        .collect();
    Ok(gaps)
}

pub fn book_row_faults(rows: &mut [BookFact]) -> Vec<FaultReport> {
    let mut grouped: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        grouped
            .entry(row.timestamp.micros())
            .or_default()
            .push(index);
    }
    let v1 = level_set(false);
    let v2 = level_set(true);
    let mut faults = Vec::new();
    for (timestamp, indexes) in grouped {
        let mut seen = BTreeSet::new();
        for index in &indexes {
            let ticks = rows[*index].percentage.ticks();
            if !seen.insert(ticks) {
                faults.push(FaultReport {
                    name: "level_set",
                    detail: format!("timestamp {timestamp} repeats percentage {ticks}"),
                });
            }
        }
        let schema = if seen == v1 {
            1
        } else if seen == v2 {
            2
        } else {
            faults.push(FaultReport {
                name: "level_set",
                detail: format!(
                    "timestamp {timestamp} has {} levels and is neither schema v1 nor v2",
                    seen.len()
                ),
            });
            continue;
        };
        for index in &indexes {
            rows[*index].level_schema = schema;
        }
        let levels: Vec<&BookFact> = indexes.iter().map(|index| &rows[*index]).collect();
        if let Some(fault) = monotonic_fault(&levels, timestamp) {
            faults.push(fault);
        }
        if levels
            .iter()
            .any(|row| row.depth.ticks() == 0 && row.notional.ticks() > 0)
        {
            faults.push(FaultReport {
                name: "book_depth",
                detail: format!("timestamp {timestamp} has zero depth with a positive notional"),
            });
        }
        let priced: Vec<&BookFact> = levels
            .into_iter()
            .filter(|row| row.depth.ticks() > 0)
            .collect();
        if !implied_band_ok(&priced) {
            faults.push(FaultReport {
                name: "notional",
                detail: format!("timestamp {timestamp} implied prices exceed a 3/2 band"),
            });
        }
        if faults.len() >= 8 {
            break;
        }
    }
    faults
}

fn level_set(with_fifth: bool) -> BTreeSet<i64> {
    let mut set = BTreeSet::new();
    for step in 1..=5 {
        set.insert(step * PCT_SCALE);
        set.insert(-step * PCT_SCALE);
    }
    if with_fifth {
        set.insert(PCT_FIFTH);
        set.insert(-PCT_FIFTH);
    }
    set
}

fn monotonic_fault(levels: &[&BookFact], timestamp: i64) -> Option<FaultReport> {
    for negative in [true, false] {
        let mut side: Vec<&BookFact> = levels
            .iter()
            .copied()
            .filter(|row| (row.percentage.ticks() < 0) == negative)
            .collect();
        side.sort_by_key(|row| row.percentage.ticks().abs());
        for pair in side.windows(2) {
            if pair[1].depth.ticks() < pair[0].depth.ticks()
                || pair[1].notional.ticks() < pair[0].notional.ticks()
            {
                return Some(FaultReport {
                    name: "monotonic",
                    detail: format!("timestamp {timestamp} depth is not cumulative in |pct|"),
                });
            }
        }
    }
    None
}

fn implied_band_ok(levels: &[&BookFact]) -> bool {
    let mut min_n = 0i64;
    let mut min_d = 0i64;
    let mut max_n = 0i64;
    let mut max_d = 0i64;
    let mut any = false;
    for level in levels {
        let notional = level.notional.ticks();
        let depth = level.depth.ticks();
        if depth <= 0 || notional < 0 {
            return false;
        }
        if !any {
            min_n = notional;
            min_d = depth;
            max_n = notional;
            max_d = depth;
            any = true;
            continue;
        }
        if i128::from(notional) * i128::from(min_d) < i128::from(min_n) * i128::from(depth) {
            min_n = notional;
            min_d = depth;
        }
        if i128::from(notional) * i128::from(max_d) > i128::from(max_n) * i128::from(depth) {
            max_n = notional;
            max_d = depth;
        }
    }
    if !any {
        return false;
    }
    i128::from(2) * i128::from(max_n) * i128::from(min_d)
        <= i128::from(3) * i128::from(min_n) * i128::from(max_d)
}

pub fn book_day_gaps(rows: &[BookFact], day: CivilDay) -> Vec<Gap> {
    let mut stamps: Vec<i64> = rows
        .iter()
        .filter(|row| crate::civil::day_of(row.timestamp).ok() == Some(day))
        .map(|row| row.timestamp.micros())
        .collect();
    stamps.sort_unstable();
    stamps.dedup();
    let mut gaps = Vec::new();
    let start = day.start_us().micros();
    let end = day.end_us().micros();
    if stamps.is_empty() {
        gaps.push(Gap {
            kind: GapKind::BookSpan,
            start: day.start_us(),
            end: day.end_us(),
            detail: format!("bookDepth {day} has no snapshots"),
        });
        return gaps;
    }
    if stamps[0] - start > BOOK_GAP_LIMIT_US {
        gaps.push(span_gap(start, stamps[0]));
    }
    for pair in stamps.windows(2) {
        if pair[1] - pair[0] > BOOK_GAP_LIMIT_US {
            gaps.push(span_gap(pair[0], pair[1]));
        }
    }
    if end - stamps[stamps.len() - 1] > BOOK_GAP_LIMIT_US {
        gaps.push(span_gap(stamps[stamps.len() - 1], end));
    }
    gaps
}

fn span_gap(start: i64, end: i64) -> Gap {
    Gap {
        kind: GapKind::BookSpan,
        start: UtcMicros::from_micros(start),
        end: UtcMicros::from_micros(end),
        detail: format!("book span {start}..{end} exceeds {BOOK_GAP_LIMIT_US}us"),
    }
}

fn missing_slots(
    start: i64,
    step: i64,
    count: i64,
    present: &BTreeSet<i64>,
    kind: GapKind,
) -> Vec<Gap> {
    let mut gaps = Vec::new();
    let mut run: Option<(i64, i64)> = None;
    for index in 0..count {
        let slot = start + index * step;
        if present.contains(&slot) {
            if let Some((run_start, run_end)) = run.take() {
                gaps.push(grid_gap(kind, run_start, run_end, step));
            }
        } else if let Some((run_start, _)) = run {
            run = Some((run_start, slot));
        } else {
            run = Some((slot, slot));
        }
    }
    if let Some((run_start, run_end)) = run {
        gaps.push(grid_gap(kind, run_start, run_end, step));
    }
    gaps
}

fn grid_gap(kind: GapKind, start: i64, last: i64, step: i64) -> Gap {
    Gap {
        kind,
        start: UtcMicros::from_micros(start),
        end: UtcMicros::from_micros(last + step),
        detail: format!("missing slots {start}..={last}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::{spot_kline_clock, UM_KLINE_CLOCK};
    use crate::vision::parse::{parse_book, parse_funding, parse_klines, parse_metrics};
    use crate::vision::{KlineFact, MetricsFact, TickDomain};
    use parangsae_core::{ReceivedTs, Symbol, TimeUnit};

    fn symbol() -> Symbol {
        Symbol::new("BTCUSDT").unwrap()
    }

    fn received() -> ReceivedTs {
        ReceivedTs::from_micros(UtcMicros::from_micros(1))
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/../../fixtures/vision/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn um_fixtures_pass_row_checks_and_bad_rows_name_the_check() {
        let ok = parse_klines(
            &fixture("um_kline_2021-06-01_headerless.csv"),
            &symbol(),
            UM_KLINE_CLOCK,
            TickDomain::Price,
            received(),
        )
        .unwrap();
        assert!(kline_row_faults(&ok.rows, TimeUnit::Millis).is_empty());
        let bad_close = parse_klines(
            &fixture("um_kline_bad_close.csv"),
            &symbol(),
            UM_KLINE_CLOCK,
            TickDomain::Price,
            received(),
        )
        .unwrap();
        assert_eq!(
            kline_row_faults(&bad_close.rows, TimeUnit::Millis)[0].name,
            "close_time"
        );
        let bad_ohlc = parse_klines(
            &fixture("um_kline_bad_ohlc.csv"),
            &symbol(),
            UM_KLINE_CLOCK,
            TickDomain::Price,
            received(),
        )
        .unwrap();
        assert_eq!(
            kline_row_faults(&bad_ohlc.rows, TimeUnit::Millis)[0].name,
            "ohlc"
        );
        let bad_taker = parse_klines(
            &fixture("um_kline_bad_taker.csv"),
            &symbol(),
            UM_KLINE_CLOCK,
            TickDomain::Price,
            received(),
        )
        .unwrap();
        assert_eq!(
            kline_row_faults(&bad_taker.rows, TimeUnit::Millis)[0].name,
            "taker_buy"
        );
        let bad_quote = parse_klines(
            &fixture("um_kline_bad_taker_quote.csv"),
            &symbol(),
            UM_KLINE_CLOCK,
            TickDomain::Price,
            received(),
        )
        .unwrap();
        assert_eq!(
            kline_row_faults(&bad_quote.rows, TimeUnit::Millis)[0].name,
            "taker_buy_quote"
        );
    }

    #[test]
    fn spot_close_uses_the_declared_source_tick() {
        let day = CivilDay::new(2025, 1, 1).unwrap();
        let parsed = parse_klines(
            &fixture("spot_kline_2025-01-01_us.csv"),
            &symbol(),
            spot_kline_clock(day),
            TickDomain::Price,
            received(),
        )
        .unwrap();
        assert!(kline_row_faults(&parsed.rows, TimeUnit::Micros).is_empty());
        assert_eq!(
            kline_row_faults(&parsed.rows, TimeUnit::Millis)[0].name,
            "close_time"
        );
        let earlier = CivilDay::new(2024, 12, 31).unwrap();
        let millis = parse_klines(
            &fixture("spot_kline_2024-12-31_ms.csv"),
            &symbol(),
            spot_kline_clock(earlier),
            TickDomain::Price,
            received(),
        )
        .unwrap();
        assert!(kline_row_faults(&millis.rows, TimeUnit::Millis).is_empty());
        assert_eq!(
            kline_row_faults(&millis.rows, TimeUnit::Micros)[0].name,
            "close_time"
        );
    }

    #[test]
    fn a_complete_generated_day_has_no_kline_or_metrics_gap() {
        let day = CivilDay::new(2026, 9, 1).unwrap();
        let mut klines = Vec::new();
        for index in 0..MINUTES_PER_DAY {
            let open = day.start_us().micros() + index * ONE_MINUTE_US;
            let close = open + ONE_MINUTE_US - 1_000;
            klines.push(synthetic_kline(open, close));
        }
        assert!(kline_day_gaps(&klines, day).is_empty());
        klines.pop();
        assert_eq!(kline_day_gaps(&klines, day).len(), 1);

        let mut metrics = Vec::new();
        for index in 0..METRICS_PER_DAY {
            let create = day.start_us().micros() + index * FIVE_MINUTES_US;
            metrics.push(synthetic_metric(create));
        }
        assert!(metrics_row_faults(&metrics).is_empty());
        assert!(metrics_day_gaps(&metrics, day).is_empty());
        assert!(!metrics[0].available_lag_verified);
    }

    fn synthetic_kline(open: i64, close: i64) -> KlineFact {
        let open_time = UtcMicros::from_micros(open);
        let close_time = UtcMicros::from_micros(close);
        KlineFact {
            symbol: symbol(),
            interval_us: ONE_MINUTE_US,
            open_time,
            close_time,
            open_ticks: 1,
            high_ticks: 1,
            low_ticks: 1,
            close_ticks: 1,
            volume: parangsae_core::Qty::from_ticks(0).unwrap(),
            quote_volume: parangsae_core::Amount::from_ticks(0).unwrap(),
            count: 0,
            taker_buy_volume: parangsae_core::Qty::from_ticks(0).unwrap(),
            taker_buy_quote_volume: parangsae_core::Amount::from_ticks(0).unwrap(),
            event_ts: parangsae_core::EventTs::from_micros(open_time),
            available_ts: parangsae_core::AvailableTs::from_micros(
                close_time.checked_add_micros(1_000).unwrap(),
            ),
            received_ts: received(),
            schema_version: 1,
            tick_domain: TickDomain::Price,
        }
    }

    fn synthetic_metric(create: i64) -> MetricsFact {
        let create_time = UtcMicros::from_micros(create);
        let ratio = parangsae_core::Ratio::from_ticks(1);
        MetricsFact {
            symbol: symbol(),
            create_time,
            sum_open_interest: parangsae_core::Qty::from_ticks(1).unwrap(),
            sum_open_interest_value: parangsae_core::Amount::from_ticks(1).unwrap(),
            count_toptrader_long_short_ratio: ratio,
            sum_toptrader_long_short_ratio: ratio,
            count_long_short_ratio: ratio,
            sum_taker_long_short_vol_ratio: ratio,
            event_ts: parangsae_core::EventTs::from_micros(create_time),
            available_ts: parangsae_core::AvailableTs::from_micros(UtcMicros::from_micros(
                create + FIVE_MINUTES_US,
            )),
            received_ts: received(),
            available_lag_verified: false,
            schema_version: 4,
        }
    }

    #[test]
    fn funding_month_lengths_and_the_missing_slot_fixture() {
        let september = YearMonth::new(2026, 9).unwrap();
        let slots = expected_funding_slots(september);
        assert_eq!(slots.len(), 90);
        assert_eq!(
            slots[0].micros(),
            CivilDay::new(2026, 9, 1).unwrap().start_us().micros()
        );
        assert_eq!(
            slots[slots.len() - 1].micros(),
            CivilDay::new(2026, 9, 30).unwrap().start_us().micros() + 16 * 3_600_000_000
        );
        assert_eq!(
            expected_funding_slots(YearMonth::new(2020, 1).unwrap()).len(),
            93
        );
        let parsed =
            parse_funding(&fixture("funding_missing_slot.csv"), &symbol(), received()).unwrap();
        let gaps = funding_window(&parsed.rows, september).unwrap();
        assert!(gaps
            .iter()
            .any(|gap| gap.start.micros() == slots[1].micros()));
    }

    #[test]
    fn book_schemas_monotonic_notional_and_gap() {
        let mut v2 = parse_book(
            &fixture("book_2026-10-20_one_ts.csv"),
            &symbol(),
            received(),
        )
        .unwrap();
        assert!(book_row_faults(&mut v2.rows).is_empty());
        assert!(v2.rows.iter().all(|row| row.level_schema == 2));
        let mut v1 = parse_book(
            &fixture("book_2023-01-01_one_ts.csv"),
            &symbol(),
            received(),
        )
        .unwrap();
        assert!(book_row_faults(&mut v1.rows).is_empty());
        assert!(v1.rows.iter().all(|row| row.level_schema == 1));
        let mut bad_levels =
            parse_book(&fixture("book_bad_levels.csv"), &symbol(), received()).unwrap();
        assert_eq!(book_row_faults(&mut bad_levels.rows)[0].name, "level_set");
        let mut bad_mono =
            parse_book(&fixture("book_bad_monotonic.csv"), &symbol(), received()).unwrap();
        assert_eq!(book_row_faults(&mut bad_mono.rows)[0].name, "monotonic");
        let mut bad_notional =
            parse_book(&fixture("book_bad_notional.csv"), &symbol(), received()).unwrap();
        let notional_faults = book_row_faults(&mut bad_notional.rows);
        assert!(notional_faults.iter().any(|fault| fault.name == "notional"));
        let mut gap_rows =
            parse_book(&fixture("book_gap_real_span.csv"), &symbol(), received()).unwrap();
        assert!(book_row_faults(&mut gap_rows.rows).is_empty());
        let day = CivilDay::new(2026, 10, 20).unwrap();
        let gaps = book_day_gaps(&gap_rows.rows, day);
        assert!(gaps.iter().any(|gap| {
            gap.kind == GapKind::BookSpan
                && gap.end.micros() - gap.start.micros() > BOOK_GAP_LIMIT_US
        }));
    }

    #[test]
    fn allowlist_is_only_the_four_known_days() {
        assert!(book_allowlisted(CivilDay::new(2023, 2, 8).unwrap()));
        assert!(book_allowlisted(CivilDay::new(2026, 10, 8).unwrap()));
        assert!(!book_allowlisted(CivilDay::new(2026, 10, 7).unwrap()));
        let gap = missing_coverage(CivilDay::new(2023, 2, 9).unwrap(), true);
        assert!(!gap.kind.fails_command());
        let missing = missing_coverage(CivilDay::new(2026, 9, 1).unwrap(), true);
        assert!(missing.kind.fails_command());
    }

    #[test]
    fn identical_metrics_rows_collapse_and_disagreement_does_not() {
        let parsed = parse_metrics(
            &fixture("metrics_2020-09-01_duplicate.csv"),
            &symbol(),
            received(),
        )
        .unwrap();
        let collapsed = collapse_by(parsed.rows, |row| row.create_time.micros()).unwrap();
        assert_eq!(collapsed.len(), 1);
        let unsorted = parse_metrics(
            &fixture("metrics_2026-10-20_unsorted.csv"),
            &symbol(),
            received(),
        )
        .unwrap();
        assert!(metrics_row_faults(&unsorted.rows).is_empty());
        let sorted = collapse_by(unsorted.rows, |row| row.create_time.micros()).unwrap();
        assert!(sorted
            .windows(2)
            .all(|pair| pair[0].create_time < pair[1].create_time));
        assert!(!sorted[0].available_lag_verified);
        assert_eq!(
            sorted[0].available_ts.micros().micros(),
            sorted[0].create_time.micros() + FIVE_MINUTES_US
        );
    }
}
