//! Guards for the BTCUSDT instrument table. Each guard has a mutant that it rejects.
//!
//! The archived JSON fixtures are the BTCUSDT filter fields cited in the
//! instrument-constants note (serverTime plus the filters this table stores).
//! They are not the full exchangeInfo documents: those bodies are not in the tree.

use std::fmt::Write as _;

use parangsae_core::instrument_spec::{
    audit_continuity, audit_grid, audit_provenance, audit_snapshot, check_paper_order,
    classify_print, rule_at, Boundary, BracketPolicy, Confidence, CoverageGap, Decimal, Field,
    GridSample, InstrumentRule, LookupError, OrderError, PaperBlock, PaperOrder, PrintFit,
    RuleValue, SnapshotError, SnapshotFields, UtcDate, UtcDateTime, BACKTEST_START_US,
    BNB_DISCOUNT_START, BTCUSDT_RULES, FEE_FAQ_2020, FEE_FAQ_2020_US, FEE_FAQ_2021,
    FEE_FAQ_2021_US, FEE_FAQ_2023, FEE_FAQ_2023_US, FEE_FAQ_2024, FEE_FAQ_2024_US, FEE_MAKER,
    FEE_TAKER_4, FEE_TAKER_5, HOURS_8, KNOWN_COVERAGE_GAPS, LISTING, LISTING_US, MARKET_MAX_CHANGE,
    NOTIONAL_1, NOTIONAL_100, NOTIONAL_1_AT, NOTIONAL_50, NOTIONAL_50_AT, PAPER_BLOCKED,
    PRECISION_2, QTY_1000, QTY_120, STEP_0_001, TAKER_FEE_CHANGE, TICK_0_01, TICK_0_10,
    TICK_COARSEN_GRACE_DAYS, TICK_COARSEN_GRACE_US, TICK_SWITCH, UNRESOLVED_ITEMS, VIP0_FEE_START,
    WB2021, WB2021_US, WB2022, WB2022_US,
};
use parangsae_core::{require_declared_unit, TimeUnit, UtcMicros};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestError, TestRunner};

const CSV: &str = include_str!("../../../fixtures/instrument/btcusdt_um_rules.csv");
const GRID: &str = include_str!("../../../fixtures/instrument/grid_prints.csv");
const WB2021_JSON: &str =
    include_str!("../../../fixtures/instrument/wb_20210104195101_BTCUSDT.json");
const WB2022_JSON: &str =
    include_str!("../../../fixtures/instrument/wb_20220318130953_BTCUSDT.json");
const WB2021_BAD: &str =
    include_str!("../../../fixtures/instrument/wb_20210104195101_BTCUSDT_bad_tick.json");
const KLINE_2021: &str =
    include_str!("../../../fixtures/vision/um_kline_2021-06-01_headerless.csv");
const KLINE_2026: &str = include_str!("../../../fixtures/vision/um_kline_2026-10-20_header.csv");
const FUNDING: &str = include_str!("../../../fixtures/vision/funding_2026-09-01.csv");

fn at(
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> Result<UtcDateTime, String> {
    UtcDateTime::from_ymd_hms(year, month, day, hour, minute, second)
        .ok_or_else(|| format!("bad civil time {year}-{month}-{day} {hour}:{minute}:{second}"))
}

fn instant(text: &str) -> Result<UtcDateTime, String> {
    let (date, clock) = text
        .split_once('T')
        .ok_or_else(|| format!("not an instant: {text}"))?;
    let clock = clock
        .strip_suffix('Z')
        .ok_or_else(|| format!("instant is not UTC: {text}"))?;
    let mut date_parts = date.split('-');
    let year = date_parts
        .next()
        .ok_or("year")?
        .parse::<i32>()
        .map_err(|err| err.to_string())?;
    let month = date_parts
        .next()
        .ok_or("month")?
        .parse::<u8>()
        .map_err(|err| err.to_string())?;
    let day = date_parts
        .next()
        .ok_or("day")?
        .parse::<u8>()
        .map_err(|err| err.to_string())?;
    let mut clock_parts = clock.split(':');
    let hour = clock_parts
        .next()
        .ok_or("hour")?
        .parse::<u8>()
        .map_err(|err| err.to_string())?;
    let minute = clock_parts
        .next()
        .ok_or("minute")?
        .parse::<u8>()
        .map_err(|err| err.to_string())?;
    let second = clock_parts
        .next()
        .ok_or("second")?
        .parse::<u8>()
        .map_err(|err| err.to_string())?;
    at(year, month, day, hour, minute, second)
}

fn decimal_ticks(rule: &InstrumentRule) -> Result<i64, String> {
    match rule.value {
        RuleValue::Value(decimal) => Ok(decimal.ticks()),
        RuleValue::Absent | RuleValue::Token(_) | RuleValue::EpochMillis(_) => {
            Err(format!("{} is not a decimal", rule.field))
        }
    }
}

fn lookup_ticks(field: Field, at: UtcDateTime, policy: BracketPolicy) -> Result<i64, String> {
    let rule = rule_at("BTCUSDT", field, at, policy).map_err(|err| err.to_string())?;
    decimal_ticks(rule)
}

fn parse_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(ch);
            }
        } else if ch == '"' {
            quoted = true;
        } else if ch == ',' {
            row.push(std::mem::take(&mut cell));
        } else if ch == '\n' {
            row.push(std::mem::take(&mut cell));
            if row.iter().any(|part| !part.is_empty()) {
                rows.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
        } else if ch != '\r' {
            cell.push(ch);
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        if row.iter().any(|part| !part.is_empty()) {
            rows.push(row);
        }
    }
    Ok(rows)
}

fn latest_source_date(cell: &str) -> Result<UtcDate, String> {
    let token = if let Some((_, right)) = cell.split_once('+') {
        right
    } else if let Some((_, right)) = cell.split_once("..") {
        right
    } else {
        cell
    };
    let mut parts = token.split('-');
    let year = parts
        .next()
        .ok_or("source year")?
        .parse::<i32>()
        .map_err(|err| err.to_string())?;
    let month = parts
        .next()
        .ok_or("source month")?
        .parse::<u8>()
        .map_err(|err| err.to_string())?;
    let day = match parts.next() {
        Some(day) => day.parse::<u8>().map_err(|err| err.to_string())?,
        None => 1,
    };
    if parts.next().is_some() {
        return Err(format!("source date has extra parts: {cell}"));
    }
    UtcDate::from_ymd(year, month, day).ok_or_else(|| format!("bad source date {cell}"))
}

fn csv_confidence(cell: &str) -> Result<Confidence, String> {
    match cell {
        "verified-archive" | "verified-doc+verified-archive" => Ok(Confidence::VerifiedArchive),
        "verified-doc" => Ok(Confidence::VerifiedDoc),
        "corroborated-from-data" => Ok(Confidence::CorroboratedFromData),
        "unverified" => Ok(Confidence::Unverified),
        other => Err(format!("unknown confidence {other}")),
    }
}

fn csv_value(field: &str, cell: &str) -> Result<RuleValue, String> {
    match (field, cell) {
        ("contract_type", "PERPETUAL") => Ok(RuleValue::Token("PERPETUAL")),
        ("min_notional", "NONE") => Ok(RuleValue::Absent),
        ("onboard_date_ms", raw) => {
            let millis = raw.parse::<i64>().map_err(|err| err.to_string())?;
            Ok(RuleValue::EpochMillis(millis))
        }
        _ => Decimal::parse(cell)
            .map(RuleValue::Value)
            .map_err(|err| err.to_string()),
    }
}

fn csv_bound(cell: &str) -> Result<Option<Boundary>, String> {
    if cell.is_empty() {
        return Ok(None);
    }
    let bound = match cell {
        "UNKNOWN(<=2022-03-18T12:00:38Z)"
        | "UNKNOWN(>2021-01-04T19:51:02Z;<=2022-03-18T12:00:38Z)" => MARKET_MAX_CHANGE,
        "UNKNOWN(<=2021-01-28)" => VIP0_FEE_START,
        "UNKNOWN(>2023-06-21;<=2024-02-20)" => TAKER_FEE_CHANGE,
        "UNKNOWN(<=2020-09-30)" => BNB_DISCOUNT_START,
        "2021-01-04T19:51:02Z" => Boundary::Exact(WB2021),
        other => Boundary::Exact(instant(other)?),
    };
    Ok(Some(bound))
}

fn wayback_us(url: &str) -> Result<i64, String> {
    let marker = "/web/";
    let pos = url
        .find(marker)
        .ok_or_else(|| format!("no wayback stamp in {url}"))?;
    let rest = &url[pos + marker.len()..];
    let digits: String = rest.chars().take(14).collect();
    if digits.len() != 14 || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(format!("wayback stamp is not 14 digits in {url}"));
    }
    let year: i32 = digits[0..4]
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    let month: u8 = digits[4..6]
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    let day: u8 = digits[6..8]
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    let hour: u8 = digits[8..10]
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    let minute: u8 = digits[10..12]
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    let second: u8 = digits[12..14]
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    Ok(at(year, month, day, hour, minute, second)?
        .micros()
        .micros())
}

fn json_i64(json: &str, key: &str) -> Result<i64, String> {
    let needle = format!("\"{key}\": ");
    let pos = json.find(&needle).ok_or_else(|| format!("missing {key}"))?;
    let rest = &json[pos + needle.len()..];
    let end = rest
        .find(|ch: char| !ch.is_ascii_digit() && ch != '-')
        .unwrap_or(rest.len());
    rest[..end]
        .trim()
        .parse::<i64>()
        .map_err(|err| err.to_string())
}

fn json_string<'a>(json: &'a str, key: &str) -> Result<&'a str, String> {
    let needle = format!("\"{key}\": \"");
    let pos = json.find(&needle).ok_or_else(|| format!("missing {key}"))?;
    let rest = &json[pos + needle.len()..];
    let end = rest
        .find('"')
        .ok_or_else(|| format!("unterminated {key}"))?;
    Ok(&rest[..end])
}

fn filter_object<'a>(json: &'a str, kind: &str) -> Option<&'a str> {
    let needle = format!("\"filterType\": \"{kind}\"");
    let pos = json.find(&needle)?;
    let start = json[..pos].rfind('{')?;
    let end = json[pos..].find('}')? + pos;
    Some(&json[start..=end])
}

fn snapshot(json: &'static str) -> Result<SnapshotFields, String> {
    let price = filter_object(json, "PRICE_FILTER").ok_or("PRICE_FILTER")?;
    let lot = filter_object(json, "LOT_SIZE").ok_or("LOT_SIZE")?;
    let market = filter_object(json, "MARKET_LOT_SIZE").ok_or("MARKET_LOT_SIZE")?;
    let min_notional = match filter_object(json, "MIN_NOTIONAL") {
        Some(object) => {
            Some(Decimal::parse(json_string(object, "notional")?).map_err(|err| err.to_string())?)
        }
        None => None,
    };
    let server = UtcDateTime::from_epoch_millis(json_i64(json, "serverTime")?)
        .ok_or("serverTime does not fit")?;
    Ok(SnapshotFields {
        symbol: "BTCUSDT",
        server_time: server,
        tick_size: Decimal::parse(json_string(price, "tickSize")?)
            .map_err(|err| err.to_string())?,
        step_size: Decimal::parse(json_string(lot, "stepSize")?).map_err(|err| err.to_string())?,
        min_qty: Decimal::parse(json_string(lot, "minQty")?).map_err(|err| err.to_string())?,
        lot_max_qty: Decimal::parse(json_string(lot, "maxQty")?).map_err(|err| err.to_string())?,
        market_max_qty: Decimal::parse(json_string(market, "maxQty")?)
            .map_err(|err| err.to_string())?,
        min_notional,
        price_precision: Decimal::from_ticks(
            json_i64(json, "pricePrecision")?
                .checked_mul(Decimal::SCALE)
                .ok_or("precision overflow")?,
        ),
        quantity_precision: Decimal::from_ticks(
            json_i64(json, "quantityPrecision")?
                .checked_mul(Decimal::SCALE)
                .ok_or("precision overflow")?,
        ),
    })
}

fn kline_samples(text: &str, header: bool) -> Result<Vec<GridSample<'static>>, String> {
    let mut samples = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() || (header && index == 0) {
            continue;
        }
        let mut cols = line.split(',');
        let open_ms: i64 = cols
            .next()
            .ok_or("open time")?
            .parse()
            .map_err(|err: std::num::ParseIntError| err.to_string())?;
        let prices: Vec<Decimal> = (0..4)
            .map(|_| {
                let raw = cols.next().ok_or("price column")?;
                Decimal::parse(raw).map_err(|err| err.to_string())
            })
            .collect::<Result<_, _>>()?;
        let volume = Decimal::parse(cols.next().ok_or("volume")?).map_err(|err| err.to_string())?;
        let at = UtcDateTime::from_epoch_millis(open_ms).ok_or("kline open time")?;
        for (nth, price) in prices.into_iter().enumerate() {
            samples.push(GridSample {
                symbol: "BTCUSDT",
                at,
                price,
                qty: if nth == 3 { Some(volume) } else { None },
            });
        }
    }
    if samples.is_empty() {
        return Err("kline fixture is empty".to_owned());
    }
    Ok(samples)
}

fn order_at(
    at: UtcDateTime,
    price: &str,
    qty: &str,
    market: bool,
    reduce_only: bool,
) -> Result<PaperOrder<'static>, String> {
    Ok(PaperOrder {
        symbol: "BTCUSDT",
        at,
        price: Decimal::parse(price).map_err(|err| err.to_string())?,
        qty: Decimal::parse(qty).map_err(|err| err.to_string())?,
        market,
        reduce_only,
    })
}

fn tick_rules() -> Result<(InstrumentRule, InstrumentRule), String> {
    let mut found = (None, None);
    for rule in BTCUSDT_RULES {
        if rule.field != Field::TickSize {
            continue;
        }
        match rule.value {
            RuleValue::Value(decimal) if decimal.ticks() == TICK_0_01 => found.0 = Some(*rule),
            RuleValue::Value(decimal) if decimal.ticks() == TICK_0_10 => found.1 = Some(*rule),
            RuleValue::Value(_)
            | RuleValue::Absent
            | RuleValue::Token(_)
            | RuleValue::EpochMillis(_) => {}
        }
    }
    match found {
        (Some(old), Some(new)) => Ok((old, new)),
        _ => Err("tick rows missing".to_owned()),
    }
}

#[test]
fn table_matches_the_seed_csv() -> Result<(), String> {
    let rows = parse_csv(CSV)?;
    let (header, body) = rows.split_first().ok_or("csv is empty")?;
    if header.first().map(String::as_str) != Some("symbol") {
        return Err("csv header is not the seed header".to_owned());
    }
    if body.len() != BTCUSDT_RULES.len() {
        return Err(format!(
            "csv has {} rows, table has {}",
            body.len(),
            BTCUSDT_RULES.len()
        ));
    }
    for (csv, rule) in body.iter().zip(BTCUSDT_RULES.iter()) {
        let symbol = csv.first().ok_or("symbol")?;
        let field = csv.get(1).ok_or("field")?;
        let value = csv.get(2).ok_or("value")?;
        let valid_from = csv.get(3).ok_or("valid_from")?;
        let valid_to = csv.get(4).ok_or("valid_to")?;
        let confidence = csv.get(6).ok_or("confidence")?;
        let source = csv.get(7).ok_or("source")?;
        let source_date = csv.get(8).ok_or("source_date")?;
        let retrieved = csv.get(9).ok_or("retrieved")?;
        if symbol != rule.symbol || field != rule.field.as_str() {
            return Err(format!("row identity {field} != {}", rule.field));
        }
        if source != rule.source {
            return Err(format!("{field} source drifted"));
        }
        if csv_confidence(confidence)? != rule.confidence {
            return Err(format!("{field} confidence drifted"));
        }
        if csv_value(field, value)? != rule.value {
            return Err(format!("{field} value drifted: {value}"));
        }
        let from = csv_bound(valid_from)?.ok_or_else(|| format!("{field} has no start"))?;
        let to = csv_bound(valid_to)?;
        if from != rule.valid_from || to != rule.valid_to {
            return Err(format!(
                "{field} bounds drifted ({valid_from} .. {valid_to})"
            ));
        }
        if latest_source_date(source_date)? != rule.source_date {
            return Err(format!("{field} source date drifted: {source_date}"));
        }
        if retrieved != "2026-10-22" || rule.retrieved != UtcDate::known(2026, 10, 22) {
            return Err(format!("{field} retrieved drifted"));
        }
        if rule.venue != "binance-usdm" || rule.revision != 1 {
            return Err(format!("{field} venue or revision drifted"));
        }
    }
    Ok(())
}

#[test]
fn observation_instants_match_wayback_and_server_time() -> Result<(), String> {
    let server_2021 = require_declared_unit(1_609_789_862_039, TimeUnit::Millis)
        .map_err(|err| err.to_string())?;
    let server_2022 = require_declared_unit(1_647_604_838_598, TimeUnit::Millis)
        .map_err(|err| err.to_string())?;
    if server_2021 != WB2021.micros() || server_2021.micros() != WB2021_US {
        return Err(format!("WB2021 {} != {}", server_2021, WB2021.micros()));
    }
    if server_2022 != WB2022.micros() || server_2022.micros() != WB2022_US {
        return Err(format!("WB2022 {} != {}", server_2022, WB2022.micros()));
    }
    let maker = BTCUSDT_RULES
        .iter()
        .find(|rule| rule.field == Field::FeeMakerVip0)
        .ok_or("maker")?;
    if wayback_us(maker.source)? != FEE_FAQ_2021_US {
        return Err("maker FAQ stamp".to_owned());
    }
    let taker_old = BTCUSDT_RULES
        .iter()
        .find(|rule| {
            rule.field == Field::FeeTakerVip0
                && matches!(rule.value, RuleValue::Value(d) if d.ticks() == FEE_TAKER_4)
        })
        .ok_or("taker 0.0004")?;
    let taker_new = BTCUSDT_RULES
        .iter()
        .find(|rule| {
            rule.field == Field::FeeTakerVip0
                && matches!(rule.value, RuleValue::Value(d) if d.ticks() == FEE_TAKER_5)
        })
        .ok_or("taker 0.0005")?;
    if wayback_us(taker_old.source)? != FEE_FAQ_2023_US {
        return Err("2023 FAQ stamp".to_owned());
    }
    if wayback_us(taker_new.source)? != FEE_FAQ_2024_US {
        return Err("2024 FAQ stamp".to_owned());
    }
    let bnb = BTCUSDT_RULES
        .iter()
        .find(|rule| rule.field == Field::FeeBnbDiscount)
        .ok_or("bnb")?;
    if wayback_us(bnb.source)? != FEE_FAQ_2020_US {
        return Err("2020 FAQ stamp".to_owned());
    }
    if VIP0_FEE_START
        != (Boundary::Bracketed {
            earliest: LISTING,
            latest: FEE_FAQ_2021,
        })
    {
        return Err("VIP0 start bracket".to_owned());
    }
    if TAKER_FEE_CHANGE
        != (Boundary::Bracketed {
            earliest: FEE_FAQ_2023,
            latest: FEE_FAQ_2024,
        })
    {
        return Err("taker change bracket".to_owned());
    }
    if MARKET_MAX_CHANGE
        != (Boundary::Bracketed {
            earliest: WB2021,
            latest: WB2022,
        })
    {
        return Err("market max bracket".to_owned());
    }
    let grace = TICK_COARSEN_GRACE_DAYS
        .checked_mul(86_400)
        .and_then(|secs| secs.checked_mul(1_000_000))
        .ok_or("grace overflow")?;
    if grace != TICK_COARSEN_GRACE_US {
        return Err(format!("grace {TICK_COARSEN_GRACE_US} != {grace}"));
    }
    Ok(())
}

#[test]
fn guard_continuity_accepts_the_table_and_rejects_mutants() -> Result<(), String> {
    audit_continuity(BTCUSDT_RULES, KNOWN_COVERAGE_GAPS).map_err(|err| err.to_string())?;
    for gap in KNOWN_COVERAGE_GAPS {
        if !BTCUSDT_RULES.iter().any(|rule| rule.field == gap.field) {
            return Err(format!("gap list names missing field {}", gap.field));
        }
        if gap.reason.is_empty() {
            return Err(format!("gap {} has an empty reason", gap.field));
        }
    }
    let (mut old, new) = tick_rules()?;
    let broken_end = at(2022, 2, 14, 0, 0, 0)?;
    old.valid_to = Some(Boundary::Exact(broken_end));
    let gap = audit_continuity(&[old, new], &[]);
    if gap
        != Err(
            parangsae_core::instrument_spec::ContinuityError::BrokenLink {
                field: Field::TickSize,
            },
        )
    {
        return Err(format!("gap mutant was not a broken link: {gap:?}"));
    }
    let mut contract = *BTCUSDT_RULES
        .iter()
        .find(|rule| rule.field == Field::ContractType)
        .ok_or("contract")?;
    contract.valid_from = Boundary::Exact(WB2021);
    let unlisted = audit_continuity(&[contract], &[]);
    if unlisted
        != Err(
            parangsae_core::instrument_spec::ContinuityError::UnlistedGap {
                field: Field::ContractType,
            },
        )
    {
        return Err(format!("late start was not an unlisted gap: {unlisted:?}"));
    }
    let stale_list = [CoverageGap {
        field: Field::TickSize,
        known_from: LISTING,
        reason: "mutant: tick already covers 2020",
    }];
    let (old, new) = tick_rules()?;
    let stale = audit_continuity(&[old, new], &stale_list);
    if stale
        != Err(parangsae_core::instrument_spec::ContinuityError::StaleGap {
            field: Field::TickSize,
        })
    {
        return Err(format!("covered field stayed on the gap list: {stale:?}"));
    }
    Ok(())
}

#[test]
fn guard_provenance_accepts_the_table_and_rejects_mutants() -> Result<(), String> {
    audit_provenance(BTCUSDT_RULES, PAPER_BLOCKED).map_err(|err| err.to_string())?;
    let base = BTCUSDT_RULES[0];
    let mut empty = base;
    empty.source = "";
    let err = audit_provenance(&[empty], &[]);
    if err
        != Err(parangsae_core::instrument_spec::ProvenanceError::EmptySource { field: base.field })
    {
        return Err(format!("empty source was accepted: {err:?}"));
    }
    let mut http = base;
    http.source = "http://example.com/exchangeInfo";
    let err = audit_provenance(&[http], &[]);
    if err
        != Err(
            parangsae_core::instrument_spec::ProvenanceError::SourceNotHttps { field: base.field },
        )
    {
        return Err(format!("http source was accepted: {err:?}"));
    }
    let mut early = base;
    early.retrieved = UtcDate::known(2000, 1, 1);
    let err = audit_provenance(&[early], &[]);
    if err
        != Err(
            parangsae_core::instrument_spec::ProvenanceError::RetrievedBeforeSource {
                field: base.field,
            },
        )
    {
        return Err(format!("retrieved-before-source was accepted: {err:?}"));
    }
    let mut rules: Vec<InstrumentRule> = BTCUSDT_RULES.to_vec();
    for rule in &mut rules {
        if rule.field == Field::TickSize {
            rule.confidence = Confidence::Unverified;
        }
    }
    let err = audit_provenance(&rules, &[]);
    if err
        != Err(
            parangsae_core::instrument_spec::ProvenanceError::UnverifiedFill {
                field: Field::TickSize,
            },
        )
    {
        return Err(format!("unverified tick was accepted: {err:?}"));
    }
    let blocked = [PaperBlock {
        symbol: "BTCUSDT",
        field: Field::TickSize,
        reason: "mutant unverified tick must not be used for paper",
    }];
    audit_provenance(&rules, &blocked).map_err(|err| err.to_string())?;
    let order = order_at(at(2024, 6, 1, 0, 0, 0)?, "60000.0", "0.002", false, false)?;
    let blocked_order = check_paper_order(&rules, &blocked, &order);
    if blocked_order
        != Err(OrderError::PaperBlocked {
            field: Field::TickSize,
        })
    {
        return Err(format!(
            "blocked tick did not stop the paper order: {blocked_order:?}"
        ));
    }
    Ok(())
}

#[test]
fn guard_grid_accepts_archive_slices_and_rejects_off_grid_prints() -> Result<(), String> {
    let y2021 = kline_samples(KLINE_2021, false)?;
    let y2026 = kline_samples(KLINE_2026, true)?;
    let old = audit_grid(&y2021).map_err(|report| format!("2021 kline off grid: {report:?}"))?;
    let new = audit_grid(&y2026).map_err(|report| format!("2026 kline off grid: {report:?}"))?;
    if old.grace != 0 || new.grace != 0 || old.violations != 0 || new.violations != 0 {
        return Err(format!(
            "archive slices used grace or failed: {old:?} {new:?}"
        ));
    }
    let mut grace_rows = Vec::new();
    for line in GRID.lines().skip(1) {
        if line.is_empty() {
            continue;
        }
        let mut cols = line.split(',');
        let label = cols.next().ok_or("label")?;
        let at = instant(cols.next().ok_or("at")?)?;
        let price = Decimal::parse(cols.next().ok_or("price")?).map_err(|err| err.to_string())?;
        let qty = Decimal::parse(cols.next().ok_or("qty")?).map_err(|err| err.to_string())?;
        let class = cols.next().ok_or("class")?;
        let got = classify_print(BTCUSDT_RULES, "BTCUSDT", at, price, Some(qty));
        match (class, got) {
            ("active", Ok(PrintFit::Active)) => {}
            ("grace", Ok(PrintFit::Grace { previous_tick })) => {
                if previous_tick.ticks() != TICK_0_01 {
                    return Err(format!("{label} grace tick {}", previous_tick.ticks()));
                }
                grace_rows.push(GridSample {
                    symbol: "BTCUSDT",
                    at,
                    price,
                    qty: Some(qty),
                });
            }
            ("violation", Err(_)) => {}
            (expected, other) => {
                return Err(format!("{label} expected {expected}, got {other:?}"));
            }
        }
        if class == "violation" && label == "old_tick_at_switch" {
            match classify_print(BTCUSDT_RULES, "BTCUSDT", at, price, Some(qty)) {
                Err(parangsae_core::instrument_spec::GridError::OffGrid { tick, .. })
                    if tick.ticks() == TICK_0_10 => {}
                other => {
                    return Err(format!(
                        "switch instant did not require the new tick: {other:?}"
                    ))
                }
            }
        }
    }
    if grace_rows.len() != 2 {
        return Err(format!("expected 2 grace prints, got {}", grace_rows.len()));
    }
    let grace = audit_grid(&grace_rows).map_err(|report| format!("grace rejected: {report:?}"))?;
    if grace.grace != 2 {
        return Err(format!("grace counter did not fire: {grace:?}"));
    }
    let off = GridSample {
        symbol: "BTCUSDT",
        at: at(2021, 6, 1, 0, 0, 0)?,
        price: Decimal::parse("37244.365").map_err(|err| err.to_string())?,
        qty: Some(Decimal::parse("0.001").map_err(|err| err.to_string())?),
    };
    let bad = audit_grid(&[off]);
    if bad.is_ok() {
        return Err("off-tick print was accepted".to_owned());
    }
    Ok(())
}

#[test]
fn guard_orders_pass_filters_and_mutants_fail() -> Result<(), String> {
    let june = at(2021, 6, 1, 0, 0, 0)?;
    check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(june, "37244.36", "0.001", false, false)?,
    )
    .map_err(|err| err.to_string())?;
    let absent = at(2021, 1, 10, 0, 0, 0)?;
    check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(absent, "100.01", "0.001", false, false)?,
    )
    .map_err(|err| format!("absent min notional rejected a small order: {err}"))?;
    let min_starts = check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(NOTIONAL_1_AT, "100.01", "0.001", false, false)?,
    );
    match min_starts {
        Err(OrderError::BelowMinNotional { min, .. }) if min.ticks() == NOTIONAL_1 => {}
        other => return Err(format!("min notional 1 did not fire: {other:?}")),
    }
    let year_2024 = at(2024, 6, 1, 0, 0, 0)?;
    let tiny = check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(year_2024, "20000.00", "0.001", false, false)?,
    );
    match tiny {
        Err(OrderError::BelowMinNotional { min, .. }) if min.ticks() == NOTIONAL_100 => {}
        other => return Err(format!("min notional 100 did not fire: {other:?}")),
    }
    check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(year_2024, "20000.00", "0.001", false, true)?,
    )
    .map_err(|err| format!("reduce-only was charged min notional: {err}"))?;
    let below_min_qty = check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(june, "37244.36", "0.0001", false, false)?,
    );
    match below_min_qty {
        Err(OrderError::BelowMinQty { min, .. }) if min.ticks() == STEP_0_001 => {}
        other => return Err(format!("min qty did not fire: {other:?}")),
    }
    let off_step = check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(june, "37244.36", "0.0015", false, false)?,
    );
    if !matches!(off_step, Err(OrderError::OffStep { .. })) {
        return Err(format!("off-step qty was accepted: {off_step:?}"));
    }
    let off_tick = check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(year_2024, "20000.01", "0.010", false, false)?,
    );
    match off_tick {
        Err(OrderError::OffTick { tick, .. }) if tick.ticks() == TICK_0_10 => {}
        other => return Err(format!("off-tick order was accepted: {other:?}")),
    }
    let over_lot = check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(june, "37244.30", "1000.001", false, false)?,
    );
    match over_lot {
        Err(OrderError::AboveMaxQty { max, .. }) if max.ticks() == QTY_1000 => {}
        other => return Err(format!("lot max did not fire: {other:?}")),
    }
    check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(year_2024, "60000.0", "0.002", true, false)?,
    )
    .map_err(|err| format!("market order inside the cap failed: {err}"))?;
    let over_market = check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(year_2024, "60000.0", "120.001", true, false)?,
    );
    match over_market {
        Err(OrderError::AboveMaxQty { max, .. }) if max.ticks() == QTY_120 => {}
        other => return Err(format!("market max 120 did not fire: {other:?}")),
    }
    let inside = at(2021, 6, 1, 12, 0, 0)?;
    let conservative = check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(inside, "40000.01", "500", true, false)?,
    );
    match conservative {
        Err(OrderError::AboveMaxQty { max, .. }) if max.ticks() == QTY_120 => {}
        other => return Err(format!("conservative market cap did not fire: {other:?}")),
    }
    let early_2020 = at(2020, 6, 1, 0, 0, 0)?;
    let uncovered = check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(early_2020, "10000.00", "0.001", false, false)?,
    );
    match uncovered {
        Err(OrderError::Lookup(LookupError::NoCoverage {
            field: Field::LotMaxQty,
        })) => {}
        other => return Err(format!("2020 order invented a lot max: {other:?}")),
    }
    // 0.10 is on the 2024 tick. A stored min price of 556.80 would reject it.
    check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &order_at(year_2024, "0.10", "1000", false, false)?,
    )
    .map_err(|err| format!("unknown min price was applied: {err}"))?;
    Ok(())
}

#[test]
fn guard_snapshots_match_and_the_bad_tick_fires() -> Result<(), String> {
    audit_snapshot(BTCUSDT_RULES, &snapshot(WB2021_JSON)?).map_err(|err| err.to_string())?;
    audit_snapshot(BTCUSDT_RULES, &snapshot(WB2022_JSON)?).map_err(|err| err.to_string())?;
    let bad = audit_snapshot(BTCUSDT_RULES, &snapshot(WB2021_BAD)?);
    match bad {
        Err(SnapshotError::Tick { found, expected })
            if found.ticks()
                == Decimal::parse("0.50")
                    .map_err(|err| err.to_string())?
                    .ticks()
                && expected.ticks() == TICK_0_01 => {}
        other => return Err(format!("bad tick snapshot did not fire: {other:?}")),
    }
    Ok(())
}

#[test]
fn guard_reject_lists_only_the_documented_brackets() -> Result<(), String> {
    let windows = [
        (Field::MarketMaxQty, WB2021, WB2022),
        (Field::FeeTakerVip0, FEE_FAQ_2023, FEE_FAQ_2024),
        (Field::FeeMakerVip0, LISTING, FEE_FAQ_2021),
        (Field::FeeTakerVip0, LISTING, FEE_FAQ_2021),
        (Field::FeeBnbDiscount, LISTING, FEE_FAQ_2020),
    ];
    for (field, start, end) in windows {
        let mid = start.micros().micros() + (end.micros().micros() - start.micros().micros()) / 2;
        let at = UtcDateTime::from_micros(UtcMicros::from_micros(mid));
        match rule_at("BTCUSDT", field, at, BracketPolicy::Reject) {
            Err(LookupError::Unresolved { field: got }) if got == field => {}
            other => return Err(format!("{field} midpoint did not reject: {other:?}")),
        }
        for edge in [start, end] {
            if matches!(
                rule_at("BTCUSDT", field, edge, BracketPolicy::Reject),
                Err(LookupError::Unresolved { .. })
            ) {
                return Err(format!("{field} endpoint {edge} was treated as unresolved"));
            }
        }
    }
    let mut fields = Vec::new();
    for rule in BTCUSDT_RULES {
        if !fields.contains(&rule.field) {
            fields.push(rule.field);
        }
    }
    let end = at(2026, 10, 22, 0, 0, 0)?.micros().micros();
    let mut cursor = BACKTEST_START_US;
    let mut minutes = 0u64;
    while cursor < end {
        let at = UtcDateTime::from_micros(UtcMicros::from_micros(cursor));
        for field in &fields {
            let unresolved = matches!(
                rule_at("BTCUSDT", *field, at, BracketPolicy::Reject),
                Err(LookupError::Unresolved { .. })
            );
            let documented = windows.iter().any(|(window_field, start, stop)| {
                *window_field == *field
                    && start.micros().micros() < cursor
                    && cursor < stop.micros().micros()
            });
            if unresolved != documented {
                return Err(format!(
                    "{field} at {cursor} unresolved={unresolved} documented={documented}"
                ));
            }
        }
        minutes = minutes.saturating_add(1);
        cursor = cursor
            .checked_add(60_000_000)
            .ok_or("minute cursor overflow")?;
    }
    if minutes < 3_000_000 {
        return Err(format!("bracket scan covered only {minutes} minutes"));
    }
    Ok(())
}

#[test]
fn unresolved_items_match_the_note() -> Result<(), String> {
    let documented = [
        ("exchange_info_2026_10", "maxPrice"),
        ("market_lot_max_qty_date", "1000"),
        ("vip0_taker_fee_date", "0.0004"),
        ("vip0_fees_before_2021_01_28", "2019-09"),
        ("tick_size_announcement_url", "binance.com"),
        ("min_notional_deadline_instants", "2023-11-02"),
        ("funding_interval_funding_info", "fundingInfo"),
    ];
    if UNRESOLVED_ITEMS.len() != documented.len() {
        return Err(format!(
            "unresolved list has {} items, the note has {}",
            UNRESOLVED_ITEMS.len(),
            documented.len()
        ));
    }
    for (item, (id, phrase)) in UNRESOLVED_ITEMS.iter().zip(documented.iter()) {
        if item.id != *id || !item.summary.contains(phrase) {
            return Err(format!(
                "unresolved item {} / {:?} does not match {id} ({phrase})",
                item.id, item.summary
            ));
        }
    }
    Ok(())
}

#[test]
fn known_points_and_policies() -> Result<(), String> {
    let before = TICK_SWITCH.checked_add_micros(-1).ok_or("before switch")?;
    if lookup_ticks(Field::TickSize, before, BracketPolicy::Reject)? != TICK_0_01 {
        return Err("tick before the switch".to_owned());
    }
    if lookup_ticks(Field::TickSize, TICK_SWITCH, BracketPolicy::Reject)? != TICK_0_10 {
        return Err("tick at the switch".to_owned());
    }
    let june = at(2021, 6, 1, 0, 0, 0)?;
    if lookup_ticks(Field::MarketMaxQty, WB2021, BracketPolicy::Reject)? != QTY_1000 {
        return Err("market max at the 2021 snapshot".to_owned());
    }
    if lookup_ticks(Field::MarketMaxQty, WB2022, BracketPolicy::Reject)? != QTY_120 {
        return Err("market max at the 2022 snapshot".to_owned());
    }
    if lookup_ticks(Field::MarketMaxQty, june, BracketPolicy::Conservative)? != QTY_120 {
        return Err("conservative market max".to_owned());
    }
    if lookup_ticks(Field::MarketMaxQty, june, BracketPolicy::Earliest)? != QTY_120 {
        return Err("earliest market max".to_owned());
    }
    if lookup_ticks(Field::MarketMaxQty, june, BracketPolicy::Latest)? != QTY_1000 {
        return Err("latest market max".to_owned());
    }
    let september = at(2023, 9, 1, 0, 0, 0)?;
    if lookup_ticks(Field::FeeTakerVip0, september, BracketPolicy::Conservative)? != FEE_TAKER_5 {
        return Err("conservative taker fee".to_owned());
    }
    if lookup_ticks(Field::FeeTakerVip0, september, BracketPolicy::Earliest)? != FEE_TAKER_5 {
        return Err("earliest taker fee".to_owned());
    }
    if lookup_ticks(Field::FeeTakerVip0, september, BracketPolicy::Latest)? != FEE_TAKER_4 {
        return Err("latest taker fee".to_owned());
    }
    let year_2020 = at(2020, 6, 1, 0, 0, 0)?;
    if lookup_ticks(Field::FeeMakerVip0, year_2020, BracketPolicy::Conservative)? != FEE_MAKER {
        return Err("conservative maker fee inside the leading bracket".to_owned());
    }
    match rule_at(
        "BTCUSDT",
        Field::FeeMakerVip0,
        year_2020,
        BracketPolicy::Latest,
    ) {
        Err(LookupError::NoCoverage {
            field: Field::FeeMakerVip0,
        }) => {}
        other => return Err(format!("latest leading fee: {other:?}")),
    }
    match rule_at(
        "BTCUSDT",
        Field::FeeMakerVip0,
        LISTING,
        BracketPolicy::Reject,
    ) {
        Err(LookupError::NoCoverage { .. }) => {}
        other => return Err(format!("listing instant of the fee bracket: {other:?}")),
    }
    if lookup_ticks(Field::MinNotional, NOTIONAL_1_AT, BracketPolicy::Reject)? != NOTIONAL_1 {
        return Err("min notional at 2021-01-25 07:00".to_owned());
    }
    let just_before = NOTIONAL_1_AT
        .checked_add_micros(-1)
        .ok_or("before notional")?;
    match rule_at(
        "BTCUSDT",
        Field::MinNotional,
        just_before,
        BracketPolicy::Reject,
    ) {
        Ok(rule) if rule.value == RuleValue::Absent => {}
        other => {
            return Err(format!(
                "notional was not absent before the rule: {other:?}"
            ))
        }
    }
    if lookup_ticks(Field::MinNotional, NOTIONAL_50_AT, BracketPolicy::Reject)? != NOTIONAL_50 {
        return Err("min notional 50".to_owned());
    }
    match rule_at(
        "ETHUSDT",
        Field::TickSize,
        june,
        BracketPolicy::Conservative,
    ) {
        Err(LookupError::UnknownSymbol) => {}
        other => return Err(format!("unknown symbol: {other:?}")),
    }
    let funding_at = UtcDateTime::from_epoch_millis(1_788_220_800_005).ok_or("funding calc")?;
    if lookup_ticks(
        Field::FundingIntervalHours,
        funding_at,
        BracketPolicy::Reject,
    )? != HOURS_8
    {
        return Err("funding interval".to_owned());
    }
    let mut saw_eight = false;
    for line in FUNDING.lines().skip(1) {
        let mut cols = line.split(',');
        let _calc = cols.next().ok_or("calc")?;
        let hours = cols.next().ok_or("hours")?;
        if hours != "8" {
            return Err(format!("funding fixture interval {hours}"));
        }
        saw_eight = true;
    }
    if !saw_eight {
        return Err("funding fixture empty".to_owned());
    }
    Ok(())
}

#[test]
fn tick_is_not_derived_from_price_precision() -> Result<(), String> {
    let later = at(2024, 6, 1, 0, 0, 0)?;
    let tick = lookup_ticks(Field::TickSize, later, BracketPolicy::Reject)?;
    let precision = rule_at(
        "BTCUSDT",
        Field::PricePrecision,
        later,
        BracketPolicy::Reject,
    )
    .map_err(|err| err.to_string())?;
    match precision.value {
        RuleValue::Value(decimal)
            if decimal.whole_units() == Some(2) && decimal.ticks() == PRECISION_2 => {}
        RuleValue::Value(decimal) => {
            return Err(format!("price precision {decimal:?}"));
        }
        RuleValue::Absent => return Err("price precision absent".to_owned()),
        RuleValue::Token(token) => return Err(format!("price precision {token}")),
        RuleValue::EpochMillis(millis) => return Err(format!("price precision {millis}")),
    }
    if tick == TICK_0_01 {
        return Err("tick matched 10^(-pricePrecision) after the tick change".to_owned());
    }
    if tick != TICK_0_10 {
        return Err(format!("tick {tick}"));
    }
    let before = at(2021, 6, 1, 0, 0, 0)?;
    if lookup_ticks(Field::PricePrecision, before, BracketPolicy::Reject)? != PRECISION_2 {
        return Err("precision changed with the tick".to_owned());
    }
    if lookup_ticks(Field::TickSize, before, BracketPolicy::Reject)? != TICK_0_01 {
        return Err("2021 tick".to_owned());
    }
    Ok(())
}

fn fields() -> [Field; 14] {
    [
        Field::ContractType,
        Field::OnboardDateMs,
        Field::TickSize,
        Field::StepSize,
        Field::MinQty,
        Field::LotMaxQty,
        Field::MarketMaxQty,
        Field::MinNotional,
        Field::PricePrecision,
        Field::QuantityPrecision,
        Field::FundingIntervalHours,
        Field::FeeMakerVip0,
        Field::FeeTakerVip0,
        Field::FeeBnbDiscount,
    ]
}

fn conservative_domain(field: Field) -> i64 {
    match field {
        Field::LotMaxQty
        | Field::MarketMaxQty
        | Field::PricePrecision
        | Field::QuantityPrecision => WB2021_US,
        Field::FeeMakerVip0 | Field::FeeTakerVip0 | Field::FeeBnbDiscount => LISTING_US + 1,
        Field::ContractType
        | Field::OnboardDateMs
        | Field::TickSize
        | Field::StepSize
        | Field::MinQty
        | Field::MinNotional
        | Field::FundingIntervalHours => LISTING_US,
    }
}

#[test]
fn conservative_lookup_covers_each_day_of_the_backtest() -> Result<(), String> {
    let end = at(2026, 10, 22, 0, 0, 0)?.micros().micros();
    for field in fields() {
        let mut cursor = conservative_domain(field);
        if cursor < BACKTEST_START_US {
            cursor = BACKTEST_START_US;
        }
        while cursor < end {
            let at = UtcDateTime::from_micros(UtcMicros::from_micros(cursor));
            if rule_at("BTCUSDT", field, at, BracketPolicy::Conservative).is_err() {
                return Err(format!("{field} missing at {cursor}"));
            }
            cursor = cursor.checked_add(86_400_000_000).ok_or("day overflow")?;
        }
    }
    Ok(())
}

#[test]
fn proptest_lookup_totality_and_monotonic_validity() -> Result<(), String> {
    let mut runner = TestRunner::new(Config {
        cases: 64,
        failure_persistence: None,
        ..Config::default()
    });
    let end = at(2026, 10, 22, 0, 0, 0)?.micros().micros();
    let lo = LISTING_US - 86_400_000_000;
    let totality = runner.run(&(lo..end), |micros| {
        let at = UtcDateTime::from_micros(UtcMicros::from_micros(micros));
        for field in fields() {
            let got = rule_at("BTCUSDT", field, at, BracketPolicy::Conservative);
            if micros >= conservative_domain(field) {
                prop_assert!(got.is_ok(), "{field} {micros} {got:?}");
            } else {
                prop_assert!(
                    matches!(got, Err(LookupError::NoCoverage { .. })),
                    "{field} {micros} {got:?}"
                );
            }
            let rejected = rule_at("BTCUSDT", field, at, BracketPolicy::Reject);
            if let Ok(certain) = rejected {
                for policy in [
                    BracketPolicy::Conservative,
                    BracketPolicy::Earliest,
                    BracketPolicy::Latest,
                ] {
                    match rule_at("BTCUSDT", field, at, policy) {
                        Ok(other) => prop_assert_eq!(other.value, certain.value),
                        Err(err) => {
                            return Err(TestCaseError::fail(format!("{field} {policy:?} {err}")))
                        }
                    }
                }
            }
        }
        Ok(())
    });
    if let Err(err) = totality {
        return Err(format!("totality: {err:?}"));
    }
    let monotonic = runner.run(&(lo..end, lo..end), |(left, right)| {
        let (earlier, later) = if left <= right {
            (left, right)
        } else {
            (right, left)
        };
        let earlier_at = UtcDateTime::from_micros(UtcMicros::from_micros(earlier));
        let later_at = UtcDateTime::from_micros(UtcMicros::from_micros(later));
        for field in fields() {
            for policy in [
                BracketPolicy::Conservative,
                BracketPolicy::Earliest,
                BracketPolicy::Latest,
                BracketPolicy::Reject,
            ] {
                let first = rule_at("BTCUSDT", field, earlier_at, policy);
                let second = rule_at("BTCUSDT", field, later_at, policy);
                if let (Ok(first), Ok(second)) = (first, second) {
                    prop_assert!(
                        first.valid_from.earliest() <= second.valid_from.earliest(),
                        "{field} {policy:?} went backwards"
                    );
                }
            }
        }
        Ok(())
    });
    match monotonic {
        Ok(()) => Ok(()),
        Err(TestError::Abort(reason)) => Err(format!("monotonic abort: {reason}")),
        Err(TestError::Fail(reason, input)) => {
            let mut message = format!("monotonic fail: {reason}");
            let _ = write!(&mut message, " at {input:?}");
            Err(message)
        }
    }
}
