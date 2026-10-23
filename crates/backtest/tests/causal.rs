//! P7 prefix invariance and P8 cost monotonicity. 100 cases each.

mod support;

use parangsae_backtest::fixture::{FlipEveryN, Recorder};
use parangsae_backtest::run;
use parangsae_backtest::{funding_visible, FundingObs};
use parangsae_core::{Price, Ratio, UtcMicros};
use proptest::prelude::*;
use support::{at, btc, cfg, costs, series, tape_of};

const CASES: u32 = 100;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(CASES))]

    /// P7. A decision at bar t ignores every later close, including after truncation.
    #[test]
    fn p7_causal_prefix(
        raw in prop::collection::vec(1i64..=400, 12),
        t in 0usize..8,
        junk in prop::collection::vec(1i64..=50, 12),
    ) {
        let closes: Vec<i64> = raw
            .into_iter()
            .enumerate()
            .map(|(index, close)| close + i64::try_from(index).unwrap() * 1_000)
            .collect();
        let bars = series(at(12, 0), &closes.iter().map(|close| (100, *close)).collect::<Vec<_>>());
        let tape = tape_of(bars.clone());
        let mut recorder = Recorder::default();
        run(&tape, &mut recorder, &costs(0, 0), &cfg(&bars, 1_000)).unwrap();
        prop_assert_eq!(recorder.calls.len(), bars.len());
        for (index, call) in recorder.calls.iter().enumerate() {
            prop_assert!(!call.open_us.is_empty());
            prop_assert_eq!(call.decision_us, bars[index].available.micros());
            prop_assert!(call.available_us.iter().all(|ts| *ts <= call.decision_us));
            prop_assert_eq!(*call.open_us.last().unwrap(), bars[index].open_time.micros());
            prop_assert_eq!(*call.close_ticks.last().unwrap(), bars[index].close.ticks());
            prop_assert!(call.open_us.iter().all(|open| *open <= bars[index].open_time.micros()));
            prop_assert!(!call.open_us.iter().any(|open| *open > bars[index].open_time.micros()));
        }

        let mut mutated = tape.clone();
        for (index, bar) in mutated.bars.iter_mut().enumerate().skip(t + 1) {
            bar.close = Price::from_ticks(junk[index]).unwrap();
        }
        for (index, bar) in mutated.marks.iter_mut().enumerate().skip(t + 1) {
            bar.close = Price::from_ticks(junk[index]).unwrap();
        }
        let mut after_mutation = Recorder::default();
        run(&mutated, &mut after_mutation, &costs(0, 0), &cfg(&bars, 1_000)).unwrap();
        prop_assert_eq!(&after_mutation.calls[..=t], &recorder.calls[..=t]);

        let mut truncated = tape.clone();
        truncated.bars.truncate(t + 1);
        truncated.marks.truncate(t + 1);
        let kept = truncated.bars.clone();
        let mut after_cut = Recorder::default();
        run(&truncated, &mut after_cut, &costs(0, 0), &cfg(&kept, 1_000)).unwrap();
        prop_assert_eq!(after_cut.calls.as_slice(), &recorder.calls[..=t]);
    }

    /// P8. A churning book with at least one fill is poorer at 5 bps than at 0.
    #[test]
    fn p8_five_bps_is_strictly_dearer(price_usdt in 20i64..=400, n in 4usize..10) {
        let rows = vec![(price_usdt, price_usdt); n];
        let bars = series(at(12, 0), &rows);
        let tape = tape_of(bars.clone());
        let mut costly = FlipEveryN::new(1, btc(1));
        let mut free = FlipEveryN::new(1, btc(1));
        let with_fee = run(&tape, &mut costly, &costs(5, 0), &cfg(&bars, 1_000_000)).unwrap();
        let without = run(&tape, &mut free, &costs(0, 0), &cfg(&bars, 1_000_000)).unwrap();
        prop_assert!(!with_fee.fills.is_empty());
        prop_assert!(with_fee.equity_end().unwrap().0 < without.equity_end().unwrap().0);
    }

    #[test]
    fn delayed_funding_is_hidden_despite_later_available_rows(
        delays in prop::collection::vec(0i64..50, 1..20),
        decision in 0i64..70,
        future_rate in any::<i64>(),
    ) {
        let rows: Vec<_> = delays.into_iter().enumerate().map(|(index, delay)| {
            let event = i64::try_from(index).unwrap();
            FundingObs {
                funding_time: UtcMicros::from_micros(event),
                available: UtcMicros::from_micros(event + delay),
                rate: Ratio::from_ticks(100),
            }
        }).collect();
        let decision = UtcMicros::from_micros(decision);
        let visible = funding_visible(&rows, decision);
        prop_assert!(visible.iter().all(|row| row.available <= decision));
        let mut mutated = rows.clone();
        for row in &mut mutated {
            if row.available > decision {
                row.rate = Ratio::from_ticks(future_rate);
            }
        }
        prop_assert_eq!(funding_visible(&mutated, decision), visible);
    }
}
