//! Tape reader. Committed fixtures stay tiny; rejects are written to a tempdir.

#[path = "parquet_fix.rs"]
mod parquet_fix;

use std::path::PathBuf;

use parangsae_backtest::fixture::AlwaysFlat;
use parangsae_backtest::{run, RunConfig, Tape, TapeError, UtcDate};
use parangsae_core::Symbol;
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};
use parquet_fix::{write_day, Day, DAY, SYMBOL};

fn symbol() -> Symbol {
    Symbol::new(SYMBOL).unwrap()
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
    let mut flat = AlwaysFlat;
    let report = run(
        tape,
        &mut flat,
        &costs(),
        &RunConfig {
            initial_cash: Cash(1_000 * 100_000_000),
            start: tape.bars[0].open_time,
            end: UtcDate::parse(DAY).unwrap().next().start_us(),
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
    let tape = Tape::load(&root, &symbol(), day(), day()).unwrap();
    assert_three_bars(&tape);
}

#[test]
fn tempdir_fixture_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    write_day(dir.path(), &Day::good()).unwrap();
    let tape = Tape::load(dir.path(), &symbol(), day(), day()).unwrap();
    assert_three_bars(&tape);
}

#[test]
fn reversed_range_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let err = Tape::load(dir.path(), &symbol(), day().next(), day()).unwrap_err();
    assert_eq!(err, TapeError::Range);
}

#[test]
fn missing_day_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let err = Tape::load(dir.path(), &symbol(), day(), day()).unwrap_err();
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
                schema_version: 1,
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
        let err = Tape::load(dir.path(), &symbol(), day(), day()).unwrap_err();
        match label {
            "interval" => assert!(matches!(err, TapeError::Interval { .. }), "{err:?}"),
            "schema" => assert!(matches!(err, TapeError::SchemaVersion { .. }), "{err:?}"),
            "scale" => assert!(matches!(err, TapeError::Scale { .. }), "{err:?}"),
            "symbol" => assert!(matches!(err, TapeError::SymbolMismatch { .. }), "{err:?}"),
            "price" => assert!(matches!(err, TapeError::BadPrice { .. }), "{err:?}"),
            "null" => assert!(matches!(err, TapeError::NullColumn { .. }), "{err:?}"),
            "column" => assert!(matches!(err, TapeError::MissingColumn { .. }), "{err:?}"),
            "funding-schema" => {
                assert!(
                    matches!(err, TapeError::SchemaVersion { got: 1, .. }),
                    "{err:?}"
                )
            }
            _ => panic!("label {label}"),
        }
    }
}

#[test]
#[ignore]
fn write_workspace_fixture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/backtest");
    write_day(&root, &Day::good()).unwrap();
}
