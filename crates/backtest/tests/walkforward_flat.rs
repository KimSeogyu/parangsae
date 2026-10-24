//! Cucumber regression for the public walk-forward replay accounting.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use cucumber::{then, when, World as _};
use parangsae_backtest::{
    daily_return_tick, fold_schedule, run_walk_forward, selection_costs, FamilyGrid, FeeReplay,
    GridPoint, ScheduleSpec, SweepPins, UtcDate, WalkContext, WindowAccounting, WindowResult,
    SELECTION_FEE_BPS, SELECTION_SLIPPAGE_BPS,
};
use parangsae_sim::Cash;

const DAY_US: i64 = 86_400_000_000;
const INITIAL: i64 = 100_000_000_000;

#[derive(Debug, Default, cucumber::World)]
struct ReplayWorld {
    replays: Vec<FeeReplay>,
}

impl ReplayWorld {
    fn selected(&self, replays: &str) -> impl Iterator<Item = &FeeReplay> {
        let selection = match replays {
            "selection cost replays" => true,
            "other fee replays" => false,
            _ => panic!("unknown replay group: {replays}"),
        };
        self.replays.iter().filter(move |replay| {
            (replay.fee_bps == SELECTION_FEE_BPS && replay.slippage_bps == SELECTION_SLIPPAGE_BPS)
                == selection
        })
    }
}

fn activity() -> WindowAccounting {
    WindowAccounting {
        fees_paid_ticks: 7,
        funding_net_ticks: -11,
        position_us: 2 * DAY_US,
        window_us: 2 * DAY_US,
        hold_us_sum: 13,
        n_refused_fills: 17,
        n_dropped_entries: 19,
        slippage_cost_ticks: 23,
        n_rebalances: 29,
        n_sign_changes: 31,
        n_forced_exits: 37,
        ..WindowAccounting::ZERO
    }
}

fn stitch(first_invested: bool) -> Vec<FeeReplay> {
    let date = |text| UtcDate::parse(text).unwrap();
    let folds = fold_schedule(&ScheduleSpec {
        sample_start: date("2021-01-01"),
        holdout_start: date("2021-01-08"),
        warmup_days: 0,
        train_days: 2,
        purge_days: 1,
        test_days: 2,
        step_days: 2,
        remainder_own_fold_days: 1,
    })
    .unwrap();
    assert_eq!(folds.len(), 2);
    let first_train_end = folds[0].train.end.micros();
    let holdout_start = folds.last().unwrap().test.end;
    let grid = FamilyGrid {
        family: "H1".to_owned(),
        slug: String::new(),
        prereg_sha256: String::new(),
        folds,
        points: vec![GridPoint {
            config: (),
            params: BTreeMap::new(),
            attempt: 1,
        }],
    };
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("fixture.txt"), b"alpha").unwrap();
    let pin_path = dir.path().join("pins.json");
    fs::write(
        &pin_path,
        concat!(
            "{\"schema\":\"sweep-runner-inputs-v2\",\"objects\":[{",
            "\"path\":\"fixture.txt\",",
            "\"sha256\":\"8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8\"}]}"
        ),
    )
    .unwrap();
    let pins = SweepPins::load(&pin_path).unwrap();
    let trials_path = dir.path().join("trials.jsonl");
    let output = run_walk_forward(
        &[grid],
        &WalkContext {
            pins: &pins,
            trials_path: &trials_path,
            initial_cash: Cash(INITIAL),
            holdout_start,
        },
        &selection_costs(),
        |_, _, start, end, _| {
            assert_eq!(end - start, 2 * DAY_US);
            let extra = if end == first_train_end && first_invested {
                1_000
            } else {
                -1_000
            };
            let equity_ticks = vec![INITIAL, INITIAL, INITIAL + extra];
            Ok::<_, String>(WindowResult {
                daily_returns: equity_ticks
                    .windows(2)
                    .map(|pair| daily_return_tick(pair[0], pair[1]).unwrap())
                    .collect(),
                equity_ticks,
                net_result_ticks: extra,
                round_trips: 3,
                fill_notional_ticks: 41,
                max_dd_ticks: 0,
                accounting: activity(),
            })
        },
    )
    .unwrap();
    let family = output.families.into_iter().next().unwrap();
    assert_eq!(family.folds[0].selected_flat, !first_invested);
    assert!(family.folds[1].selected_flat);
    assert_eq!(family.fee_replays.len(), 5);
    family.fee_replays
}

#[when("one invested fold and one equal FLAT fold are stitched")]
fn mixed(world: &mut ReplayWorld) {
    world.replays = stitch(true);
}

#[when("two equal FLAT folds are stitched")]
fn flat(world: &mut ReplayWorld) {
    world.replays = stitch(false);
}

#[then(
    regex = r"^the (selection cost replays|other fee replays) have (\d+) percent exposure across 4 days$"
)]
fn exposure(world: &mut ReplayWorld, replays: String, percent: i64) {
    for replay in world.selected(&replays) {
        let accounting = replay.accounting;
        let exposure = if accounting.window_us == 0 {
            0
        } else {
            accounting.position_us * 100 / accounting.window_us
        };
        assert_eq!(exposure, percent);
        assert_eq!(accounting.window_us, 4 * DAY_US);
        assert_eq!(replay.daily_returns.len(), 4);
    }
}

#[then(
    regex = r"^the (selection cost replays|other fee replays) retain only the invested fold's activity$"
)]
fn invested_activity(world: &mut ReplayWorld, replays: String) {
    for replay in world.selected(&replays) {
        assert_eq!(
            replay.accounting,
            WindowAccounting {
                window_us: 4 * DAY_US,
                ..activity()
            }
        );
        assert_eq!(replay.round_trips, 3);
        assert_eq!(replay.turnover_ticks, 3);
        assert_eq!(&replay.daily_returns[2..], &[0, 0]);
        assert_eq!(replay.net_result_ticks, -1_000);
    }
}

#[then(regex = r"^the (selection cost replays|other fee replays) have no activity$")]
fn no_activity(world: &mut ReplayWorld, replays: String) {
    for replay in world.selected(&replays) {
        assert_eq!(
            replay.accounting,
            WindowAccounting {
                window_us: 4 * DAY_US,
                ..WindowAccounting::ZERO
            }
        );
        assert_eq!(replay.round_trips, 0);
        assert_eq!(replay.net_result_ticks, 0);
        assert_eq!(replay.turnover_ticks, 0);
        assert_eq!(replay.daily_returns, vec![0; 4]);
        assert_eq!(replay.equity_ticks, vec![INITIAL; 6]);
    }
}

#[tokio::main]
async fn main() {
    let feature = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/walkforward_flat.feature");
    ReplayWorld::cucumber().run_and_exit(feature).await;
}
