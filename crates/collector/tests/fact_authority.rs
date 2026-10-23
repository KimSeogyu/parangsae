//! Public collector behavior under generated whole-row ownership. All storage is temporary.
use parangsae_collector::civil::CivilDay;
use parangsae_collector::store::{self, WriteCounts};
use parangsae_collector::vision::{
    BookFact, FundingFact, KlineFact, MetricsFact, TickDomain, SCHEMA_BOOK, SCHEMA_FUNDING,
    SCHEMA_KLINE_HEADER, SCHEMA_KLINE_HEADERLESS, SCHEMA_METRICS,
};
use parangsae_core::{Amount, AvailableTs, EventTs, Qty, Ratio, ReceivedTs, Symbol, UtcMicros};

fn day() -> CivilDay {
    CivilDay::new(2026, 9, 1).unwrap()
}
fn at(received: i64) -> ReceivedTs {
    ReceivedTs::from_micros(UtcMicros::from_micros(received))
}
fn kline(minute: i64, value: i64, received: i64) -> KlineFact {
    let open = day()
        .start_us()
        .checked_add_micros(minute * 60_000_000)
        .unwrap();
    let close = open.checked_add_micros(59_999_000).unwrap();
    KlineFact {
        symbol: Symbol::new("BTCUSDT").unwrap(),
        interval_us: 60_000_000,
        open_time: open,
        close_time: close,
        open_ticks: value,
        high_ticks: value,
        low_ticks: value,
        close_ticks: value,
        volume: Qty::from_ticks(1).unwrap(),
        quote_volume: Amount::from_ticks(2).unwrap(),
        count: 3,
        taker_buy_volume: Qty::from_ticks(4).unwrap(),
        taker_buy_quote_volume: Amount::from_ticks(5).unwrap(),
        event_ts: EventTs::from_micros(open),
        available_ts: AvailableTs::from_micros(close.checked_add_micros(1_000).unwrap()),
        received_ts: at(received),
        schema_version: SCHEMA_KLINE_HEADERLESS,
        tick_domain: TickDomain::Price,
    }
}
fn funding(value: i64, received: i64) -> FundingFact {
    let time = day().start_us();
    FundingFact {
        symbol: Symbol::new("BTCUSDT").unwrap(),
        raw_calc_time: time,
        funding_time: time,
        interval_hours: 8,
        rate: Ratio::from_ticks(value),
        event_ts: EventTs::from_micros(time),
        available_ts: AvailableTs::from_micros(time),
        received_ts: at(received),
        schema_version: SCHEMA_FUNDING,
    }
}
fn metrics(value: i64, received: i64) -> MetricsFact {
    let time = day().start_us();
    MetricsFact {
        symbol: Symbol::new("BTCUSDT").unwrap(),
        create_time: time,
        sum_open_interest: Qty::from_ticks(value).unwrap(),
        sum_open_interest_value: Amount::from_ticks(2).unwrap(),
        count_toptrader_long_short_ratio: Ratio::from_ticks(3),
        sum_toptrader_long_short_ratio: Ratio::from_ticks(4),
        count_long_short_ratio: Ratio::from_ticks(5),
        sum_taker_long_short_vol_ratio: Ratio::from_ticks(6),
        event_ts: EventTs::from_micros(time),
        available_ts: AvailableTs::from_micros(time),
        received_ts: at(received),
        available_lag_verified: true,
        schema_version: SCHEMA_METRICS,
    }
}
fn book(value: i64, received: i64) -> BookFact {
    let time = day().start_us();
    BookFact {
        symbol: Symbol::new("BTCUSDT").unwrap(),
        timestamp: time,
        percentage: Ratio::from_ticks(1),
        depth: Qty::from_ticks(value).unwrap(),
        notional: Amount::from_ticks(2).unwrap(),
        level_schema: 1,
        event_ts: EventTs::from_micros(time),
        available_ts: AvailableTs::from_micros(time),
        received_ts: at(received),
        schema_version: SCHEMA_BOOK,
    }
}

#[test]
fn a_conflict_rolls_back_new_keys_and_keeps_complete_persisted_rows_and_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let original = kline(0, 10, 1);
    store::upsert_klines(temp.path(), "um-kline", vec![original.clone()]).unwrap();
    let path = store::fact_path(temp.path(), "um-kline", "BTCUSDT", day());
    let before = std::fs::read(&path).unwrap();
    let result = store::upsert_klines(
        temp.path(),
        "um-kline",
        vec![
            kline(1, 20, 2),
            kline(0, 11, 3),
            kline(0, 12, 4),
            kline(0, 10, 5),
        ],
    )
    .unwrap();
    assert_eq!(
        result,
        WriteCounts {
            inserted: 0,
            unchanged: 1,
            conflicts: 2
        }
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let loaded = store::load_klines(temp.path(), "um-kline", "BTCUSDT", day()).unwrap();
    assert_eq!(loaded, vec![original]);
    assert_eq!(loaded[0].received_ts, at(1));
    assert_eq!(loaded[0].schema_version, SCHEMA_KLINE_HEADERLESS);
    let conflicts = std::fs::read_to_string(temp.path().join("conflicts/um-kline.jsonl")).unwrap();
    let reports: Vec<serde_json::Value> = conflicts
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(reports.len(), 2);
    assert!(reports[0]["stored"].as_str().unwrap().contains("open=10"));
    assert!(reports[0]["rejected"].as_str().unwrap().contains("open=11"));
    assert!(reports[1]["rejected"].as_str().unwrap().contains("open=12"));
}

#[test]
fn a_conflict_on_a_provisionally_inserted_key_creates_no_parquet_partition() {
    let temp = tempfile::tempdir().unwrap();
    let result = store::upsert_klines(
        temp.path(),
        "um-kline",
        vec![kline(0, 10, 1), kline(1, 20, 2), kline(0, 11, 3)],
    )
    .unwrap();
    assert_eq!(
        result,
        WriteCounts {
            inserted: 0,
            unchanged: 0,
            conflicts: 1
        }
    );
    assert!(!store::fact_path(temp.path(), "um-kline", "BTCUSDT", day()).exists());
    assert!(
        store::load_klines(temp.path(), "um-kline", "BTCUSDT", day())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("conflicts/um-kline.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn known_kline_schema_equivalence_preserves_first_raw_schema_receipt_and_file_in_both_directions() {
    for (first, incoming) in [
        (SCHEMA_KLINE_HEADERLESS, SCHEMA_KLINE_HEADER),
        (SCHEMA_KLINE_HEADER, SCHEMA_KLINE_HEADERLESS),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut original = kline(0, 10, 1);
        original.schema_version = first;
        store::upsert_klines(temp.path(), "um-kline", vec![original.clone()]).unwrap();
        let path = store::fact_path(temp.path(), "um-kline", "BTCUSDT", day());
        let bytes = std::fs::read(&path).unwrap();
        let mut repeat = kline(0, 10, 4);
        repeat.schema_version = incoming;
        assert_eq!(
            store::upsert_klines(temp.path(), "um-kline", vec![repeat]).unwrap(),
            WriteCounts {
                inserted: 0,
                unchanged: 1,
                conflicts: 0
            }
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let loaded = store::load_klines(temp.path(), "um-kline", "BTCUSDT", day()).unwrap();
        assert_eq!(loaded, vec![original]);
        assert_eq!(loaded[0].schema_version, first);
        assert_eq!(loaded[0].received_ts, at(1));
    }
}

#[test]
fn every_closed_fact_family_uses_generated_equality_and_retains_complete_first_rows() {
    let temp = tempfile::tempdir().unwrap();
    store::upsert_funding(temp.path(), "funding-rate", vec![funding(10, 1)]).unwrap();
    assert_eq!(
        store::upsert_funding(temp.path(), "funding-rate", vec![funding(10, 4)])
            .unwrap()
            .unchanged,
        1
    );
    assert_eq!(
        store::upsert_funding(
            temp.path(),
            "funding-rate",
            vec![funding(11, 2), funding(12, 3), funding(10, 4)]
        )
        .unwrap(),
        WriteCounts {
            inserted: 0,
            unchanged: 1,
            conflicts: 2
        }
    );
    let stored = store::load_funding(temp.path(), "funding-rate", "BTCUSDT", day()).unwrap();
    assert_eq!(stored, vec![funding(10, 1)]);
    assert_eq!(stored[0].received_ts, at(1));
    let mut schema_change = funding(10, 5);
    schema_change.schema_version = 99;
    assert_eq!(
        store::upsert_funding(temp.path(), "funding-rate", vec![schema_change])
            .unwrap()
            .conflicts,
        1
    );

    store::upsert_metrics(temp.path(), "metrics", vec![metrics(10, 1)]).unwrap();
    assert_eq!(
        store::upsert_metrics(temp.path(), "metrics", vec![metrics(10, 4)])
            .unwrap()
            .unchanged,
        1
    );
    assert_eq!(
        store::upsert_metrics(temp.path(), "metrics", vec![metrics(11, 2)])
            .unwrap()
            .conflicts,
        1
    );
    let stored = store::load_metrics(temp.path(), "metrics", "BTCUSDT", day()).unwrap();
    assert_eq!(stored, vec![metrics(10, 1)]);
    assert_eq!(stored[0].received_ts, at(1));
    let mut flag_change = metrics(10, 5);
    flag_change.available_lag_verified = false;
    assert_eq!(
        store::upsert_metrics(temp.path(), "metrics", vec![flag_change])
            .unwrap()
            .conflicts,
        1
    );

    store::upsert_book(temp.path(), "book", vec![book(10, 1)]).unwrap();
    assert_eq!(
        store::upsert_book(temp.path(), "book", vec![book(10, 4)])
            .unwrap()
            .unchanged,
        1
    );
    assert_eq!(
        store::upsert_book(temp.path(), "book", vec![book(11, 2)])
            .unwrap()
            .conflicts,
        1
    );
    let stored = store::load_book(temp.path(), "book", "BTCUSDT", day()).unwrap();
    assert_eq!(stored, vec![book(10, 1)]);
    assert_eq!(stored[0].received_ts, at(1));
}

#[test]
fn a_conflict_log_failure_does_not_replace_the_partition() {
    let temp = tempfile::tempdir().unwrap();
    store::upsert_klines(temp.path(), "um-kline", vec![kline(0, 10, 1)]).unwrap();
    let path = store::fact_path(temp.path(), "um-kline", "BTCUSDT", day());
    let before = std::fs::read(&path).unwrap();
    std::fs::write(temp.path().join("conflicts"), b"not a directory").unwrap();
    assert!(store::upsert_klines(
        temp.path(),
        "um-kline",
        vec![kline(1, 20, 2), kline(0, 11, 3)]
    )
    .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let stored = store::load_klines(temp.path(), "um-kline", "BTCUSDT", day()).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].received_ts, at(1));
}

#[test]
fn unknown_kline_schemas_are_equal_only_to_the_identical_raw_schema() {
    for (first, incoming, is_equal) in [
        (1, 99, false),
        (99, 1, false),
        (99, 99, true),
        (0, 99, false),
        (u32::MAX, u32::MAX, true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut original = kline(0, 10, 1);
        original.schema_version = first;
        store::upsert_klines(temp.path(), "um-kline", vec![original]).unwrap();
        let path = store::fact_path(temp.path(), "um-kline", "BTCUSDT", day());
        let before = std::fs::read(&path).unwrap();
        let mut repeat = kline(0, 10, 2);
        repeat.schema_version = incoming;
        let result = store::upsert_klines(temp.path(), "um-kline", vec![repeat]).unwrap();
        assert_eq!(
            result,
            WriteCounts {
                inserted: 0,
                unchanged: u64::from(is_equal),
                conflicts: u64::from(!is_equal)
            }
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let loaded = store::load_klines(temp.path(), "um-kline", "BTCUSDT", day()).unwrap();
        assert_eq!(loaded[0].schema_version, first);
        assert_eq!(loaded[0].received_ts, at(1));
    }
}

#[test]
fn identical_rejected_rows_still_emit_two_conflict_observations_when_state_stutters() {
    let temp = tempfile::tempdir().unwrap();
    store::upsert_klines(temp.path(), "um-kline", vec![kline(0, 10, 1)]).unwrap();
    let path = store::fact_path(temp.path(), "um-kline", "BTCUSDT", day());
    let before = std::fs::read(&path).unwrap();
    let rejected = kline(0, 11, 2);
    let result =
        store::upsert_klines(temp.path(), "um-kline", vec![rejected.clone(), rejected]).unwrap();
    assert_eq!(
        result,
        WriteCounts {
            inserted: 0,
            unchanged: 0,
            conflicts: 2
        }
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let reports = std::fs::read_to_string(temp.path().join("conflicts/um-kline.jsonl")).unwrap();
    let lines: Vec<_> = reports.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], lines[1]);
    let persisted = store::load_klines(temp.path(), "um-kline", "BTCUSDT", day()).unwrap();
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].open_ticks, 10);
    assert_eq!(persisted[0].received_ts, at(1));
}
