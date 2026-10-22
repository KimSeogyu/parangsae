//! P7 prefix invariance and P8 cost monotonicity. 100 cases each.

mod support;

use parangsae_backtest::fixture::{FlipEveryN, Recorder};
use parangsae_backtest::run;
use parangsae_core::Price;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use support::{at, btc, cfg, costs, series, tape_of};

const CASES: u32 = 100;

fn fail<T, E: std::fmt::Display>(result: Result<T, E>) -> Result<T, TestCaseError> {
    result.map_err(|err| TestCaseError::fail(err.to_string()))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(CASES))]

    /// P7. A decision at bar t ignores every later close, including after truncation.
    #[test]
    fn p7_causal_prefix(
        raw in prop::collection::vec(1i64..=400, 12),
        t in 0usize..8,
        junk in prop::collection::vec(1i64..=50, 12),
    ) {
        let mut closes = Vec::with_capacity(raw.len());
        for (index, close) in raw.into_iter().enumerate() {
            let offset = i64::try_from(index).map_err(|err| TestCaseError::fail(err.to_string()))?;
            closes.push(close + offset * 1_000);
        }
        let rows = closes.iter().map(|close| (100, *close)).collect::<Vec<_>>();
        let bars = fail(series(fail(at(12, 0))?, &rows))?;
        let tape = tape_of(bars.clone());
        let mut recorder = Recorder::default();
        fail(run(&tape, &mut recorder, &costs(0, 0), &fail(cfg(&bars, 1_000))?))?;
        prop_assert_eq!(recorder.calls.len(), bars.len());
        for (index, call) in recorder.calls.iter().enumerate() {
            prop_assert!(!call.open_us.is_empty());
            prop_assert_eq!(call.decision_us, bars[index].available.micros());
            prop_assert!(call.available_us.iter().all(|ts| *ts <= call.decision_us));
            let open = call
                .open_us
                .last()
                .copied()
                .ok_or_else(|| TestCaseError::fail("no open".to_owned()))?;
            let close = call
                .close_ticks
                .last()
                .copied()
                .ok_or_else(|| TestCaseError::fail("no close".to_owned()))?;
            prop_assert_eq!(open, bars[index].open_time.micros());
            prop_assert_eq!(close, bars[index].close.ticks());
            prop_assert!(call.open_us.iter().all(|open| *open <= bars[index].open_time.micros()));
            prop_assert!(!call.open_us.iter().any(|open| *open > bars[index].open_time.micros()));
        }

        let mut mutated = tape.clone();
        for (index, bar) in mutated.bars.iter_mut().enumerate().skip(t + 1) {
            bar.close = fail(Price::from_ticks(junk[index]))?;
        }
        for (index, bar) in mutated.marks.iter_mut().enumerate().skip(t + 1) {
            bar.close = fail(Price::from_ticks(junk[index]))?;
        }
        let mut after_mutation = Recorder::default();
        fail(run(
            &mutated,
            &mut after_mutation,
            &costs(0, 0),
            &fail(cfg(&bars, 1_000))?,
        ))?;
        prop_assert_eq!(&after_mutation.calls[..=t], &recorder.calls[..=t]);

        let mut truncated = tape.clone();
        truncated.bars.truncate(t + 1);
        truncated.marks.truncate(t + 1);
        let kept = truncated.bars.clone();
        let mut after_cut = Recorder::default();
        fail(run(
            &truncated,
            &mut after_cut,
            &costs(0, 0),
            &fail(cfg(&kept, 1_000))?,
        ))?;
        prop_assert_eq!(after_cut.calls.as_slice(), &recorder.calls[..=t]);
    }

    /// P8. A churning book with at least one fill is poorer at 5 bps than at 0.
    #[test]
    fn p8_five_bps_is_strictly_dearer(price_usdt in 20i64..=400, n in 4usize..10) {
        let rows = vec![(price_usdt, price_usdt); n];
        let bars = fail(series(fail(at(12, 0))?, &rows))?;
        let tape = tape_of(bars.clone());
        let qty = fail(btc(1))?;
        let mut costly = FlipEveryN::new(1, qty);
        let mut free = FlipEveryN::new(1, qty);
        let window = fail(cfg(&bars, 1_000_000))?;
        let with_fee = fail(run(&tape, &mut costly, &costs(5, 0), &window))?;
        let without = fail(run(&tape, &mut free, &costs(0, 0), &window))?;
        prop_assert!(!with_fee.fills.is_empty());
        let paid = with_fee
            .equity_end()
            .ok_or_else(|| TestCaseError::fail("fee run has no equity".to_owned()))?
            .0;
        let free_end = without
            .equity_end()
            .ok_or_else(|| TestCaseError::fail("free run has no equity".to_owned()))?
            .0;
        prop_assert!(paid < free_end);
    }
}
