// Included only in wfbatch::tests. No market files or research ledger are opened.
use std::collections::BTreeMap;

use cucumber::{given, then, when, World as _};
use parangsae_core::{Price, Qty, Ratio, Side, UtcMicros};
use parangsae_sim::{Cash, Costs, FeeBps, Ledger, MarketOrder, SlippageBps};
use parangsae_strategies::PositionSide;

use crate::report::ForcedExitReason;
use crate::walkforward::{
    FamilyOutput, FoldOutcome, Segment, SegmentRun, WalkForward, WindowReplay, WindowResult,
};

const DAY: i64 = 86_400_000_000;
const INITIAL: i64 = 100_000_000_000;
const SCENARIOS: [(u32, u32); 5] = [(2, 2), (4, 2), (5, 2), (6, 2), (5, 5)];

fn synthetic_window(case: &str, fold: usize, fee: u32, slip: u32) -> WindowResult {
    let forced = match case {
        "forced_terminal" => Some(ForcedExitReason::Terminal),
        "forced_ruin" => Some(ForcedExitReason::Ruin),
        _ => None,
    };
    let (_, mut report) = crate::h4_report::tests::raw_fixture(forced);
    let offset = i64::try_from(fold).unwrap() * 4 * DAY;
    let ts = |value: i64| UtcMicros::from_micros(value + offset);
    let long = case == "concentration" && fold == 1;
    let side = if long { Side::Buy } else { Side::Sell };
    let closing_side = if long { Side::Sell } else { Side::Buy };
    let (entry_open, exit_open, rate_ticks) = if case == "concentration" {
        (1_000, 1_000, [50_140_000, 19_860_000, 25_140_000][fold])
    } else if case == "undefined_gross" {
        (100, 100, 0)
    } else {
        (100, 89, 2_000_000)
    };
    let price = |usdt: i64| Price::from_ticks(usdt * 100_000_000).unwrap();
    let costs = Costs {
        taker_fee: FeeBps(fee),
        slippage: SlippageBps(slip),
    };
    let qty = Qty::from_ticks(100_000_000).unwrap();
    let open_at = ts(86_460_000_000);
    let close_at = ts(if forced == Some(ForcedExitReason::Terminal) {
        431_940_000_000
    } else {
        349_260_000_000
    });
    let mut ledger = Ledger::new(Cash(INITIAL));
    let entry = ledger
        .apply_fill(
            MarketOrder { side, qty },
            price(entry_open),
            &costs,
            open_at,
        )
        .unwrap();
    let funding = ledger
        .apply_funding(
            Ratio::from_ticks(rate_ticks),
            price(entry_open),
            ts(172_800_000_000),
        )
        .unwrap();
    let exit = ledger
        .apply_fill(
            MarketOrder {
                side: closing_side,
                qty,
            },
            price(exit_open),
            &costs,
            close_at,
        )
        .unwrap();
    let net = ledger.cash().0 - INITIAL;
    let slip_ticks = parangsae_sim::slippage_cost(price(entry_open), side, qty, costs.slippage)
        .unwrap()
        .0
        + parangsae_sim::slippage_cost(price(exit_open), closing_side, qty, costs.slippage)
            .unwrap()
            .0;
    let trace = report.h4.as_mut().unwrap();
    trace.scored.start = ts(DAY);
    trace.scored.end = ts(5 * DAY);
    for observation in &mut trace.observations {
        observation.event.settlement = ts(0);
        observation.event.episode = ts(0);
        observation.event.crowd_side = if long {
            PositionSide::Short
        } else {
            PositionSide::Long
        };
        observation.observed_at = parangsae_core::DecisionTime::from_micros(ts(observation
            .observed_at
            .micros()
            .micros()));
    }
    let raw = &mut trace.trades[0];
    raw.event = trace.observations[0].event;
    raw.trade.open_ts = open_at;
    raw.trade.close_ts = Some(close_at);
    raw.trade.side = side;
    raw.trade.pnl = Cash(net);
    raw.fills = vec![entry, exit];
    raw.funding = vec![funding];
    raw.slippage_cost = Cash(slip_ticks);
    report.trades = vec![raw.trade];
    report.fills = raw.fills.clone();
    report.funding = raw.funding.clone();
    report.costs = costs;
    let metrics: &mut crate::report::Metrics = &mut report.metrics;
    metrics.fees_paid = ledger.fees_paid();
    metrics.funding_net = ledger.funding_net();
    report.slippage_cost = Cash(slip_ticks);
    for forced in &mut report.forced_exits {
        forced.trigger = ts(forced.trigger.micros());
        forced.filled_at = close_at;
    }
    report.equity_curve = (0..4)
        .map(|day| {
            (
                ts((day + 2) * DAY - 1_000),
                Cash(INITIAL + if day == 3 { net } else { 0 }),
            )
        })
        .collect();
    report.daily_equity = crate::report::daily_equity(&report.equity_curve);
    let points: Vec<_> = std::iter::once(Cash(INITIAL))
        .chain(report.equity_curve.iter().map(|(_, value)| *value))
        .collect();
    report.metrics.max_drawdown = crate::metrics::max_drawdown(&points).unwrap();
    report.metrics.total_return =
        crate::metrics::total_return(Cash(INITIAL), ledger.cash()).unwrap();
    crate::walkforward::window_result(Cash(INITIAL), &report).unwrap()
}

fn native_window(fee: u32, slip: u32) -> WindowResult {
    use crate::bar::{Bar, FundingStamp, Tape};
    let event = crate::date::UtcDate::parse("2022-01-03")
        .unwrap()
        .start_us()
        .micros();
    let warmup = event - 2 * DAY;
    let end = event + 5 * DAY;
    let bars: Vec<_> = (warmup..end)
        .step_by(60_000_000)
        .map(|open| {
            let usdt = if open < event {
                100
            } else if open < event + 3 * DAY {
                90
            } else {
                80
            };
            let price = Price::from_ticks(usdt * 100_000_000).unwrap();
            Bar {
                open_time: UtcMicros::from_micros(open),
                close_time: UtcMicros::from_micros(open + 59_999_000),
                available: UtcMicros::from_micros(open + 60_000_000),
                open: price,
                high: price,
                low: price,
                close: price,
                volume: Qty::from_ticks(100_000_000).unwrap(),
                count: 1,
            }
        })
        .collect();
    let funding = ((event - 366 * DAY)..end)
        .step_by(28_800_000_000usize)
        .map(|at| FundingStamp {
            funding_time: UtcMicros::from_micros(at),
            available: UtcMicros::from_micros(at + 60_000_000),
            rate: Ratio::from_ticks(if at == event { 1_000_000 } else { 10_000 }),
            interval_hours: 8,
        })
        .collect();
    let tape = Tape::from_parts(bars.clone(), bars, funding);
    crate::sweep::FoldRunner::new(&tape)
        .run_scored(
            &crate::WindowConfig {
                family: "H4".to_owned(),
                initial_cash: INITIAL,
                params: BTreeMap::from([
                    ("q".to_owned(), 10_000_000),
                    ("confirm_h".to_owned(), 24),
                    ("sum_settlements".to_owned(), 3),
                    ("hold_h".to_owned(), 72),
                ]),
            },
            warmup,
            event,
            end,
            &Costs {
                taker_fee: FeeBps(fee),
                slippage: SlippageBps(slip),
            },
        )
        .unwrap()
}

fn row(window: WindowResult, fold: u32) -> SegmentRun {
    let annual = crate::metrics::sharpe_ticks(&window.equity_ticks)
        .unwrap()
        .1
        .map(|ratio| ratio.ticks());
    SegmentRun {
        grid_index: 0,
        run_id: format!("synthetic-H4-f{fold}"),
        trial_id: "synthetic-no-research-trial".to_owned(),
        config_id: "synthetic-config".to_owned(),
        start_us: window.h4.as_ref().unwrap().scored_start_us,
        daily_returns: window.daily_returns,
        equity_ticks: window.equity_ticks,
        net_result_ticks: window.net_result_ticks,
        round_trips: window.round_trips,
        sharpe_annual_ticks: annual,
        fill_notional_ticks: window.fill_notional_ticks,
        max_dd_ticks: window.max_dd_ticks,
        accounting: window.accounting,
        h4: window.h4,
    }
}

fn family(case: &str) -> WalkForward {
    let count = if case == "concentration" { 3 } else { 1 };
    let mut folds = Vec::new();
    for index in 0..count {
        let mut rows: BTreeMap<_, _> = SCENARIOS
            .into_iter()
            .map(|(fee, slip)| {
                let window = if case == "native_adapter" {
                    native_window(fee, slip)
                } else {
                    synthetic_window(case, index, fee, slip)
                };
                ((fee, slip), row(window, u32::try_from(index + 1).unwrap()))
            })
            .collect();
        let headline = rows.remove(&(5, 2)).unwrap();
        let mut cost_replays = Vec::new();
        for ((fee_bps, slippage_bps), run) in rows {
            for segment in [Segment::Train, Segment::Test] {
                cost_replays.push(WindowReplay {
                    segment,
                    fee_bps,
                    slippage_bps,
                    run: run.clone(),
                });
            }
        }
        folds.push(FoldOutcome {
            fold_index: u32::try_from(index + 1).unwrap(),
            train: vec![headline.clone()],
            test: vec![headline],
            selected_index: 0,
            selected_config_id: "synthetic-config".to_owned(),
            selected_flat: false,
            cost_replays,
        });
    }
    let mut fee_replays = Vec::new();
    for (fee, slip) in SCENARIOS {
        let mut selected = folds.clone();
        if (fee, slip) != (5, 2) {
            for fold in &mut selected {
                fold.test[0] = fold
                    .cost_replays
                    .iter()
                    .find(|r| {
                        r.segment == Segment::Test && r.fee_bps == fee && r.slippage_bps == slip
                    })
                    .unwrap()
                    .run
                    .clone();
            }
        }
        fee_replays.push(
            crate::walkforward::replay_from_selection::<String>(fee, slip, &selected).unwrap(),
        );
    }
    let headline = fee_replays
        .iter()
        .find(|r| (r.fee_bps, r.slippage_bps) == (5, 2))
        .unwrap();
    let output = FamilyOutput {
        family: "H4".to_owned(),
        folds,
        stitched_oos_daily_returns: headline.daily_returns.clone(),
        stitched_oos_equity: headline.equity_ticks.clone(),
        oos_round_trips: headline.round_trips,
        n_trials: 1,
        fee_replays,
        stay_flat_shadow: None,
        h2_member_sharpes: Vec::new(),
    };
    let mut result = WalkForward {
        families: vec![output],
        n_trials: 1,
    };
    if case == "lost_provenance" {
        result.families[0].folds[0].test[0].h4 = None;
    }
    result
}

#[derive(Debug, Default, cucumber::World)]
struct H4OutputWorld {
    case: String,
    output: Option<tempfile::TempDir>,
    result: Option<WalkForward>,
    json: Option<serde_json::Value>,
    error: Option<String>,
}

#[given(regex = r"^the synthetic H4 report case (\w+)$")]
fn given_case(world: &mut H4OutputWorld, case: String) {
    world.result = Some(family(&case));
    world.case = case;
}

#[when("the H4 output files are written")]
fn write_files(world: &mut H4OutputWorld) {
    let dir = tempfile::tempdir().unwrap();
    let result = world.result.as_ref().unwrap();
    match crate::wfbatch::write_oos(
        dir.path(),
        result,
        &BTreeMap::from([("H4".to_owned(), 366)]),
        INITIAL,
    ) {
        Err(error) => world.error = Some(error),
        Ok(()) => {
            world.json = Some(
                serde_json::from_str(
                    &std::fs::read_to_string(dir.path().join("H4-diagnostics.json")).unwrap(),
                )
                .unwrap(),
            );
            let grid = crate::walkforward::FamilyGrid {
                family: "H4".to_owned(),
                slug: "H4".to_owned(),
                prereg_sha256: "0".repeat(64),
                folds: Vec::new(),
                points: vec![crate::walkforward::GridPoint {
                    config: (),
                    params: BTreeMap::new(),
                    attempt: 0,
                }],
            };
            crate::table::write_tables(
                dir.path(),
                &[grid],
                result,
                "synthetic-pin",
                "synthetic-read-path",
                &dir.path().join("absent-synthetic-ledger"),
            )
            .unwrap();
        }
    }
    world.output = Some(dir);
}

#[then(regex = r"^the H4 diagnostic values and existing CSV columns match (\w+)$")]
fn check_files(world: &mut H4OutputWorld, case: String) {
    assert_eq!(world.case, case);
    let dir = world.output.as_ref().unwrap().path();
    if case == "lost_provenance" {
        assert!(world
            .error
            .as_ref()
            .unwrap()
            .contains("missing H4 diagnostics"));
        assert!(!dir.join("H4.json").exists());
        return;
    }
    assert!(world.error.is_none(), "{:?}", world.error);
    let json = world.json.as_ref().unwrap();
    let selected = &json["selected_cost_scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["fee_bps"] == 5 && r["slippage_bps"] == 2)
        .unwrap()["selected"];
    assert_eq!(selected["episode_independence_claimed"], false);
    match case.as_str() {
        "signed_cash" | "forced_terminal" | "forced_ruin" => {
            assert_eq!(selected["cash"]["net_ticks"], "1286770110");
            assert_eq!(selected["cash"]["fees_ticks"], "9449890");
            assert_eq!(selected["cash"]["slippage_ticks"], "3780000");
            assert_eq!(selected["cash"]["funding_ticks"], "200000000");
            assert_eq!(selected["cash"]["gross_price_ticks"], "1100000000");
            assert_eq!(
                selected["funding_test"]["funding_over_gross"]["numerator"],
                "200000000"
            );
            assert_eq!(
                selected["funding_test"]["funding_over_gross"]["denominator"],
                "1100000000"
            );
            assert_eq!(selected["crowded_short_fades"]["entries"], 0);
            assert_eq!(
                selected["crowded_long_fades"]["strategy_round_trips"],
                u64::from(case == "signed_cash")
            );
            assert_eq!(
                selected["crowded_long_fades"]["forced_round_trips"],
                u64::from(case != "signed_cash")
            );
            if case != "signed_cash" {
                let raw = json["windows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["segment"] == "test" && r["fee_bps"] == 5 && r["slippage_bps"] == 2)
                    .unwrap();
                let trade = &raw["diagnostics"]["trades"][0];
                assert_eq!(
                    trade["forced_reason"],
                    if case == "forced_terminal" {
                        "terminal"
                    } else {
                        "ruin"
                    }
                );
                let trigger = trade["forced_trigger_us"].as_i64().unwrap();
                let close = trade["close_us"].as_i64().unwrap();
                assert!(trigger <= close);
                if case == "forced_ruin" {
                    assert!(trigger < close);
                }
            }
        }
        "undefined_gross" => {
            assert_eq!(selected["cash"]["net_ticks"], "-14000000");
            assert_eq!(selected["cash"]["gross_price_ticks"], "0");
            assert!(selected["funding_test"]["funding_over_gross"].is_null());
            assert_eq!(selected["funding_test"]["original_literal_flag"], false);
        }
        "concentration" => {
            assert_eq!(selected["cash"]["net_ticks"], "55000000000");
            assert_eq!(
                selected["folds"][2]["compounded_increment_ticks"],
                "30000000000"
            );
            assert_eq!(
                selected["folds"][2]["compounded_gain_share"]["denominator"],
                "50000000000"
            );
            assert_eq!(
                selected["folds"][2]["reset_cash_share_sensitivity"]["numerator"],
                "25000000000"
            );
            assert_eq!(
                selected["folds"][2]["reset_cash_share_sensitivity"]["denominator"],
                "55000000000"
            );
            assert_eq!(
                selected["crowded_long_reset_cash_share"]["numerator"],
                "75000000000"
            );
            assert_eq!(
                selected["crowded_long_reset_cash_share"]["denominator"],
                "55000000000"
            );
            assert_eq!(selected["original_side_falsifier"], true);
        }
        "native_adapter" => {
            assert!(
                selected["crowded_long_fades"]["strategy_round_trips"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
            let window = json["windows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["segment"] == "test" && r["fee_bps"] == 5 && r["slippage_bps"] == 2)
                .unwrap();
            assert!(!window["diagnostics"]["trades"]
                .as_array()
                .unwrap()
                .is_empty());
            assert!(!window["diagnostics"]["trades"][0]["funding"]
                .as_array()
                .unwrap()
                .is_empty());
        }
        _ => panic!("unknown case"),
    }
    let csv = std::fs::read_to_string(dir.join("table_H4.csv")).unwrap();
    assert_eq!(csv.lines().next().unwrap(), "trial_id,config_id,slug,params,pinned_root,read_path_sha,window,start,end,fee_bps,slip_bps,funding_on,days_T,total_return,sharpe_annual,sharpe_denominator,sharpe_daily,skew,kurt,max_dd,turnover_ann,n_trades,avg_hold_h,exposure_pct,n_refused_fills,fees_paid,slippage_cost,funding_net,fee_sens,breakeven_fee_bps,n_global,sr_var_cross_trial,dsr_hurdle_sr_ann,dsr,pbo,gate_pass,selected_grid_index,selected_config_id,S_j,n_rebalances,n_sign_changes,n_forced_exits,h2_member_sharpes,h2_member_median,h2_gate2prime");
}

#[tokio::test]
async fn h4_reporting_gherkin() {
    H4OutputWorld::cucumber()
        .with_default_cli()
        .run_and_exit(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/h4_reporting.feature"),
        )
        .await;
}
