//! Tape reader. Committed fixtures stay tiny; rejects are written to a tempdir.

#[path = "parquet_fix.rs"]
mod parquet_fix;

use std::path::PathBuf;

use std::fs;
use std::path::Path;

use parangsae_backtest::fixture::AlwaysFlat;
use parangsae_backtest::{
    funding_available, run, Overlay, RunConfig, Tape, TapeConfig, TapeError, UtcDate,
};
use parangsae_core::{Interval, Symbol, UtcMicros};
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};
use parquet_fix::{write_day, write_overlay, write_overlay_row, Day, OverlayRow, DAY, SYMBOL};
use sha2::{Digest, Sha256};

fn symbol() -> Symbol {
    Symbol::new(SYMBOL).unwrap()
}

fn load_cfg(root: &std::path::Path) -> TapeConfig {
    TapeConfig {
        holdout_start: UtcDate::parse("2025-10-01").unwrap().start_us(),
        overlays: Vec::new(),
        funding_root: root.to_path_buf(),
    }
}

fn day() -> UtcDate {
    UtcDate::parse(DAY).unwrap()
}

fn costs() -> Costs {
    Costs {
        taker_fee: FeeBps(5),
        slippage: SlippageBps(0),
    }
}

fn assert_three_bars(tape: &Tape) {
    let start = day().start_us().micros();
    assert_eq!(tape.bars.len(), 3);
    assert_eq!(tape.marks.len(), 3);
    assert_eq!(tape.funding.len(), 1);
    assert_eq!(tape.bars[0].open_time.micros(), start);
    assert_eq!(tape.bars[0].open.ticks(), 100 * 100_000_000);
    assert_eq!(tape.bars[1].close.ticks(), 101 * 100_000_000);
    assert_eq!(tape.bars[2].close.ticks(), 102 * 100_000_000);
    assert_eq!(tape.bars[0].close_time.micros(), start + 60_000_000 - 1_000);
    assert_eq!(
        tape.bars[0].available.micros(),
        tape.bars[0].close_time.micros() + 1_000
    );
    assert_eq!(tape.marks[0].close.ticks(), 200 * 100_000_000);
    assert_eq!(tape.marks[2].close.ticks(), 202 * 100_000_000);
    assert_eq!(tape.funding[0].funding_time.micros(), start);
    assert_eq!(tape.funding[0].rate.ticks(), 7697);
    assert_eq!(tape.funding[0].interval_hours, 8);
    assert_eq!(tape.funding[0].available.micros(), start + 60_000_000);
    assert_eq!(tape.bars[0].count, 1);
    let mut flat = AlwaysFlat;
    let report = run(
        tape,
        &mut flat,
        &costs(),
        &RunConfig {
            initial_cash: Cash(1_000 * 100_000_000),
            start: tape.bars[0].open_time,
            end: UtcDate::parse(DAY).unwrap().next().start_us(),
            holdout_start: UtcDate::parse("2025-10-01").unwrap().start_us(),
            forced_flat: Vec::new(),
            mark_gaps: Vec::new(),
            interval: Interval::ONE_HOUR,
            lot_ticks: 100_000,
        },
    )
    .unwrap();
    assert_eq!(report.bars, 3);
    assert_eq!(report.funding.len(), 1);
    assert_eq!(report.funding[0].amount.0, 0);
}

#[test]
fn committed_fixture_loads_three_bars() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/backtest");
    let tape = Tape::load(&root, &symbol(), day(), day(), &load_cfg(&root)).unwrap();
    assert_three_bars(&tape);
}

#[test]
fn tempdir_fixture_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    write_day(dir.path(), &Day::good()).unwrap();
    let tape = Tape::load(dir.path(), &symbol(), day(), day(), &load_cfg(dir.path())).unwrap();
    assert_three_bars(&tape);
}

#[test]
fn reversed_range_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let err = Tape::load(
        dir.path(),
        &symbol(),
        day().next(),
        day(),
        &load_cfg(dir.path()),
    )
    .unwrap_err();
    assert_eq!(err, TapeError::Range);
}

#[test]
fn missing_day_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let err = Tape::load(dir.path(), &symbol(), day(), day(), &load_cfg(dir.path())).unwrap_err();
    assert!(matches!(err, TapeError::MissingFile { .. }));
}

#[test]
fn bad_interval_schema_scale_symbol_and_price_are_rejected() {
    let cases = [
        (
            Day {
                interval_us: 1,
                ..Day::good()
            },
            "interval",
        ),
        (
            Day {
                schema_version: 9,
                ..Day::good()
            },
            "schema",
        ),
        (
            Day {
                scale: "1",
                ..Day::good()
            },
            "scale",
        ),
        (
            Day {
                row_symbol: "ETHUSDT",
                ..Day::good()
            },
            "symbol",
        ),
        (
            Day {
                close_sign: -1,
                ..Day::good()
            },
            "price",
        ),
        (
            Day {
                null_open: true,
                ..Day::good()
            },
            "null",
        ),
        (
            Day {
                omit_open: true,
                ..Day::good()
            },
            "column",
        ),
        (
            Day {
                funding_schema: 1,
                ..Day::good()
            },
            "funding-schema",
        ),
    ];
    for (day_spec, label) in cases {
        let dir = tempfile::tempdir().unwrap();
        write_day(dir.path(), &day_spec).unwrap();
        let err =
            Tape::load(dir.path(), &symbol(), day(), day(), &load_cfg(dir.path())).unwrap_err();
        match label {
            "interval" => assert!(matches!(err, TapeError::Interval { .. }), "{err:?}"),
            "schema" => assert!(matches!(err, TapeError::UnknownSchema { .. }), "{err:?}"),
            "scale" => assert!(matches!(err, TapeError::Scale { .. }), "{err:?}"),
            "symbol" => assert!(matches!(err, TapeError::SymbolMismatch { .. }), "{err:?}"),
            "price" => assert!(matches!(err, TapeError::BadPrice { .. }), "{err:?}"),
            "null" => assert!(matches!(err, TapeError::NullColumn { .. }), "{err:?}"),
            "column" => assert!(matches!(err, TapeError::MissingColumn { .. }), "{err:?}"),
            "funding-schema" => {
                assert!(
                    matches!(err, TapeError::UnknownSchema { got: 1, .. }),
                    "{err:?}"
                )
            }
            _ => panic!("label {label}"),
        }
    }
}

#[test]
fn t6_funding_available_uses_the_later_of_stored_and_slot_plus_60s() {
    let slot = 1_700_000_000_000_000i64;
    let lag = 60_000_000;
    assert_eq!(funding_available(slot, slot, slot).unwrap(), slot + lag);
    assert_eq!(
        funding_available(slot + lag, slot, slot).unwrap(),
        slot + lag
    );
    assert_eq!(
        funding_available(slot + lag + 5, slot, slot).unwrap(),
        slot + lag + 5
    );
    let err = funding_available(slot, slot, slot + lag + 1).unwrap_err();
    assert!(matches!(
        err,
        TapeError::FundingBeforeEvent {
            funding_time
        } if funding_time == UtcMicros::from_micros(slot)
    ));
}

#[test]
fn t6_parquet_applies_the_max_rule_once() {
    let stored = Day {
        funding_available_delta: 0,
        funding_event_delta: 0,
        ..Day::good()
    };
    let dir = tempfile::tempdir().unwrap();
    write_day(dir.path(), &stored).unwrap();
    let tape = Tape::load(dir.path(), &symbol(), day(), day(), &load_cfg(dir.path())).unwrap();
    let start = day().start_us().micros();
    assert_eq!(tape.funding[0].available.micros(), start + 60_000_000);

    let lagged = Day {
        funding_available_delta: 60_000_000,
        funding_event_delta: 0,
        ..Day::good()
    };
    let dir = tempfile::tempdir().unwrap();
    write_day(dir.path(), &lagged).unwrap();
    let tape = Tape::load(dir.path(), &symbol(), day(), day(), &load_cfg(dir.path())).unwrap();
    assert_eq!(tape.funding[0].available.micros(), start + 60_000_000);

    let early = Day {
        funding_available_delta: 0,
        funding_event_delta: 60_000_000 + 1,
        ..Day::good()
    };
    let dir = tempfile::tempdir().unwrap();
    write_day(dir.path(), &early).unwrap();
    let err = Tape::load(dir.path(), &symbol(), day(), day(), &load_cfg(dir.path())).unwrap_err();
    assert!(
        matches!(err, TapeError::FundingBeforeEvent { .. }),
        "{err:?}"
    );
}

#[test]
fn schema_1_klines_match_schema_2_bars() {
    let mut schema_1 = Day::good();
    schema_1.schema_version = 1;
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    write_day(left.path(), &schema_1).unwrap();
    write_day(right.path(), &Day::good()).unwrap();
    let a = Tape::load(left.path(), &symbol(), day(), day(), &load_cfg(left.path())).unwrap();
    let b = Tape::load(
        right.path(),
        &symbol(),
        day(),
        day(),
        &load_cfg(right.path()),
    )
    .unwrap();
    assert_eq!(a.bars, b.bars);
    assert_eq!(a.marks, b.marks);
    assert_eq!(a.funding, b.funding);
}

#[test]
fn before_sample_date_is_refused_before_the_poison_file_is_read() {
    let dir = tempfile::tempdir().unwrap();
    for dataset in ["um-kline", "mark-price-kline", "funding-rate"] {
        poison(dir.path(), dataset, "2020-12-17");
    }
    let day = UtcDate::parse("2020-12-17").unwrap();
    let err = Tape::load(dir.path(), &symbol(), day, day, &load_cfg(dir.path())).unwrap_err();
    assert_eq!(err, TapeError::BeforeSampleRequested);

    let both = tempfile::tempdir().unwrap();
    for date in ["2020-12-17", "2020-12-18"] {
        for dataset in ["um-kline", "mark-price-kline", "funding-rate"] {
            poison(both.path(), dataset, date);
        }
    }
    let start = UtcDate::parse("2020-12-17").unwrap();
    let end = UtcDate::parse("2020-12-18").unwrap();
    let err = Tape::load(both.path(), &symbol(), start, end, &load_cfg(both.path())).unwrap_err();
    assert_eq!(err, TapeError::BeforeSampleRequested);
}

#[test]
fn load_refuses_a_holdout_or_pre_sample_row_inside_an_in_sample_file() {
    let holdout = UtcDate::parse("2025-10-01").unwrap().start_us().micros();
    let before = UtcDate::parse("2020-12-17").unwrap().start_us().micros();
    let start = day().start_us().micros();
    let grid = |shifted: i64| vec![start, start + 60_000_000, shifted];

    let mut um = Day::good();
    um.um_open_us = Some(grid(holdout));
    let dir = tempfile::tempdir().unwrap();
    write_day(dir.path(), &um).unwrap();
    let err = Tape::load(dir.path(), &symbol(), day(), day(), &load_cfg(dir.path())).unwrap_err();
    assert_eq!(
        err,
        TapeError::HoldoutRow {
            at: UtcMicros::from_micros(holdout)
        }
    );

    let mut mark = Day::good();
    mark.mark_open_us = Some(grid(before));
    let dir = tempfile::tempdir().unwrap();
    write_day(dir.path(), &mark).unwrap();
    let err = Tape::load(dir.path(), &symbol(), day(), day(), &load_cfg(dir.path())).unwrap_err();
    assert_eq!(
        err,
        TapeError::BeforeSample {
            at: UtcMicros::from_micros(before)
        }
    );

    let mut funding = Day::good();
    funding.funding_time_us = Some(holdout);
    let dir = tempfile::tempdir().unwrap();
    write_day(dir.path(), &funding).unwrap();
    let err = Tape::load(dir.path(), &symbol(), day(), day(), &load_cfg(dir.path())).unwrap_err();
    assert_eq!(
        err,
        TapeError::HoldoutRow {
            at: UtcMicros::from_micros(holdout)
        }
    );
}

#[test]
fn h1_holdout_date_is_refused_before_the_poison_file_is_read() {
    let dir = tempfile::tempdir().unwrap();
    poison(dir.path(), "um-kline", "2025-10-01");
    poison(dir.path(), "mark-price-kline", "2025-10-01");
    poison(dir.path(), "funding-rate", "2025-10-01");
    let day = UtcDate::parse("2025-10-01").unwrap();
    let err = Tape::load(dir.path(), &symbol(), day, day, &load_cfg(dir.path())).unwrap_err();
    assert_eq!(err, TapeError::HoldoutRequested);

    let both = tempfile::tempdir().unwrap();
    for date in ["2025-09-30", "2025-10-01"] {
        poison(both.path(), "um-kline", date);
        poison(both.path(), "mark-price-kline", date);
        poison(both.path(), "funding-rate", date);
    }
    let start = UtcDate::parse("2025-09-30").unwrap();
    let end = UtcDate::parse("2025-10-01").unwrap();
    let err = Tape::load(both.path(), &symbol(), start, end, &load_cfg(both.path())).unwrap_err();
    assert_eq!(err, TapeError::HoldoutRequested);
}

#[test]
fn o1_overlay_row_replaces_the_stored_row() {
    let dir = tempfile::tempdir().unwrap();
    let scale = 100_000_000;
    let mut stored = Day::good();
    // Minute 1 is a stored filler: flat print, no trades. The other two are not.
    stored.volumes = Some(vec![2 * scale, 0, 2 * scale]);
    stored.counts = Some(vec![1, 0, 1]);
    write_day(dir.path(), &stored).unwrap();
    let start = day().start_us().micros();
    let path = dir.path().join("overlay.parquet");
    let close = 777 * scale;
    let mut row = OverlayRow::matching(start + 60_000_000, close);
    row.volume_ticks = scale;
    row.count = Some(7);
    write_overlay_row(&path, &row).unwrap();
    let mut cfg = load_cfg(dir.path());
    cfg.overlays.push(Overlay {
        dataset: "um-kline".to_owned(),
        path: path.clone(),
        sha256: file_sha(&path),
    });
    let tape = Tape::load(dir.path(), &symbol(), day(), day(), &cfg).unwrap();
    assert_eq!(tape.bars[1].close.ticks(), close);
    assert_eq!(tape.bars[0].close.ticks(), 100 * scale);
    assert_eq!(tape.bars[2].close.ticks(), 102 * scale);
    // Replacement is applied before filler status is read. The stored minute
    // was a filler; the overlay row has trades, so the loaded bar is a fill bar.
    assert!(!tape.bars[1].is_filler());
    assert_eq!(tape.bars[1].volume.ticks(), scale);
    assert_eq!(tape.bars[1].count, 7);
    assert_eq!(tape.bars[1].close_time.micros(), row.close_time_us);
    assert_eq!(tape.bars[1].available.micros(), row.available_us);
    assert_eq!(tape.bars[0].volume.ticks(), 2 * scale);
    assert_eq!(tape.bars[0].count, 1);
    assert!(!tape.bars[0].is_filler());
    assert_eq!(tape.overlays.len(), 1);
    assert_eq!(tape.overlays[0].dataset, "um-kline");
    assert_eq!(tape.overlays[0].sha256.len(), 64);
}

#[test]
fn overlay_sha_replaces_the_scale_check() {
    let start = day().start_us().micros();
    let open = start + 60_000_000;
    let close = 777 * 100_000_000;
    let cases = [
        (None, 1u32, "missing scale, schema 1"),
        (Some("1"), 2u32, "wrong scale, schema 2"),
    ];
    for (scale, schema_version, label) in cases {
        let dir = tempfile::tempdir().unwrap();
        write_day(dir.path(), &Day::good()).unwrap();
        let path = dir.path().join("overlay.parquet");
        let mut row = OverlayRow::matching(open, close);
        row.scale = scale;
        row.schema_version = schema_version;
        write_overlay_row(&path, &row).unwrap();
        let mut cfg = load_cfg(dir.path());
        cfg.overlays.push(Overlay {
            dataset: "um-kline".to_owned(),
            path: path.clone(),
            sha256: file_sha(&path),
        });
        let tape = Tape::load(dir.path(), &symbol(), day(), day(), &cfg)
            .unwrap_or_else(|err| panic!("{label}: {err}"));
        assert_eq!(tape.bars[1].close.ticks(), close, "{label}");
        assert_eq!(tape.bars[0].close.ticks(), 100 * 100_000_000, "{label}");
    }
}

#[test]
fn overlay_checks_symbol_interval_and_schema() {
    let start = day().start_us().micros();
    for fault in [
        OverlayFault::Symbol,
        OverlayFault::Interval,
        OverlayFault::Schema,
    ] {
        let dir = tempfile::tempdir().unwrap();
        write_day(dir.path(), &Day::good()).unwrap();
        let path = dir.path().join("overlay.parquet");
        let mut row = OverlayRow::matching(start, 100 * 100_000_000);
        fault.edit(&mut row);
        write_overlay_row(&path, &row).unwrap();
        let mut cfg = load_cfg(dir.path());
        cfg.overlays.push(Overlay {
            dataset: "um-kline".to_owned(),
            path: path.clone(),
            sha256: file_sha(&path),
        });
        let err = Tape::load(dir.path(), &symbol(), day(), day(), &cfg).unwrap_err();
        assert!(fault.matches(&err), "{fault:?}: {err:?}");
    }
}

#[derive(Clone, Copy, Debug)]
enum OverlayFault {
    Symbol,
    Interval,
    Schema,
}

impl OverlayFault {
    fn edit(self, row: &mut OverlayRow) {
        match self {
            Self::Symbol => row.symbol = "ETHUSDT",
            Self::Interval => row.interval_us = 1,
            Self::Schema => row.schema_version = 9,
        }
    }

    fn matches(self, err: &TapeError) -> bool {
        match self {
            Self::Symbol => matches!(err, TapeError::SymbolMismatch { .. }),
            Self::Interval => matches!(err, TapeError::Interval { got: 1 }),
            Self::Schema => matches!(err, TapeError::UnknownSchema { got: 9, .. }),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum OverlayClock {
    CloseTime,
    Available,
}

#[test]
fn overlay_clock_must_match_the_stored_row() {
    let start = day().start_us().micros();
    let open = start + 60_000_000;
    for fault in [OverlayClock::CloseTime, OverlayClock::Available] {
        let dir = tempfile::tempdir().unwrap();
        write_day(dir.path(), &Day::good()).unwrap();
        let path = dir.path().join("overlay.parquet");
        let mut row = OverlayRow::matching(open, 777 * 100_000_000);
        match fault {
            OverlayClock::CloseTime => row.close_time_us += 1_000,
            OverlayClock::Available => row.available_us += 1_000,
        }
        write_overlay_row(&path, &row).unwrap();
        let mut cfg = load_cfg(dir.path());
        cfg.overlays.push(Overlay {
            dataset: "um-kline".to_owned(),
            path: path.clone(),
            sha256: file_sha(&path),
        });
        let err = Tape::load(dir.path(), &symbol(), day(), day(), &cfg).unwrap_err();
        assert!(
            matches!(err, TapeError::OverlayShape { .. }),
            "{fault:?}: {err:?}"
        );
    }
}

#[test]
fn o2_overlay_sha_mismatch_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    write_day(dir.path(), &Day::good()).unwrap();
    let path = dir.path().join("overlay.parquet");
    let start = day().start_us().micros();
    write_overlay(&path, start, 100 * 100_000_000).unwrap();
    let mut cfg = load_cfg(dir.path());
    cfg.overlays.push(Overlay {
        dataset: "um-kline".to_owned(),
        path,
        sha256: [0u8; 32],
    });
    let err = Tape::load(dir.path(), &symbol(), day(), day(), &cfg).unwrap_err();
    assert!(matches!(err, TapeError::OverlaySha { .. }), "{err:?}");
}

#[test]
fn o3_overlay_open_outside_the_grid_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    write_day(dir.path(), &Day::good()).unwrap();
    let path = dir.path().join("overlay.parquet");
    let start = day().start_us().micros();
    write_overlay(&path, start + 10 * 60_000_000, 100 * 100_000_000).unwrap();
    let mut cfg = load_cfg(dir.path());
    cfg.overlays.push(Overlay {
        dataset: "um-kline".to_owned(),
        path: path.clone(),
        sha256: file_sha(&path),
    });
    let err = Tape::load(dir.path(), &symbol(), day(), day(), &cfg).unwrap_err();
    assert!(matches!(err, TapeError::OverlayShape { .. }), "{err:?}");
}

fn poison(root: &Path, dataset: &str, date: &str) {
    let path = root
        .join("parquet")
        .join(dataset)
        .join(format!("symbol={SYMBOL}"))
        .join(format!("date={date}"))
        .join("part.parquet");
    fs::create_dir_all(path.parent().expect("parent")).unwrap();
    fs::write(&path, b"PANIC_IF_READ").unwrap();
}

fn file_sha(path: &Path) -> [u8; 32] {
    let bytes = fs::read(path).unwrap();
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[test]
#[ignore]
fn write_workspace_fixture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/backtest");
    write_day(&root, &Day::good()).unwrap();
}
