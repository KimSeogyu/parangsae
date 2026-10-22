//! Cucumber runner for `specs/features/signals/signals_v0.feature`.

#[path = "support/mod.rs"]
mod support;

use std::path::PathBuf;

use cucumber::{given, then, when, World as _};
use parangsae_core::{BarInterval, Observation, Ratio, ReceivedTs, ResampledBar};
use parangsae_signals::{
    drive, drive_feature, drive_funding_z, funding_observation, Decision, FeatureOutput,
    FlatReason, FundingRow, FundingZ, Rank, RankError, SignalError, SignalOutput, Tsmom, VolRegime,
    PROD_MAX_AGE_US,
};
use support::{
    at, bar_ending_at, bar_obs, fund_obs, gap_funding, hour, toy_funding, toy_tsmom_bars,
    toy_vol_bars, us, usdt_ticks,
};

#[derive(Debug, Default, cucumber::World)]
struct SignalWorld {
    bars: Vec<Observation<ResampledBar>>,
    funding: Vec<Observation<FundingRow>>,
    signal: Option<Result<Option<SignalOutput>, SignalError>>,
    feature: Option<Result<Option<FeatureOutput>, SignalError>>,
    rank: Option<Result<Rank, RankError>>,
}

#[given("the tsmom toy closes")]
fn tsmom_toy(world: &mut SignalWorld) {
    world.bars = toy_tsmom_bars();
}

#[given("the volregime toy closes")]
fn vol_toy(world: &mut SignalWorld) {
    let closes = [
        10_000_000_000,
        11_000_000_000,
        9_900_000_000,
        9_900_000_000,
        10_890_000_000,
        9_801_000_000,
    ];
    world.bars = toy_vol_bars(&closes);
}

#[given("the funding z toy rows")]
fn funding_toy(world: &mut SignalWorld) {
    world.funding = toy_funding();
}

#[given("the funding gap rows")]
fn funding_gap(world: &mut SignalWorld) {
    world.funding = gap_funding();
}

#[given("the funding z toy rows with the last interval at 4 hours")]
fn funding_interval(world: &mut SignalWorld) {
    let mut rows = toy_funding();
    let last = rows.pop().expect("toy row");
    rows.push(fund_obs(last.value().funding_time().micros(), -10_000, 4));
    world.funding = rows;
}

#[given(regex = r"^a funding row at hour (\d+) with rate (\d+)$")]
fn one_funding(world: &mut SignalWorld, h: i64, rate: i64) {
    world.funding = vec![funding_observation(
        us(hour(h)),
        Ratio::from_ticks(rate),
        8,
        ReceivedTs::from_micros(us(hour(h))),
    )
    .expect("funding observation")];
}

#[given("10 rising hourly closes")]
fn rising(world: &mut SignalWorld) {
    world.bars = (0..10)
        .map(|index| bar_obs(hour(index), BarInterval::H1, usdt_ticks(40_000 + index)))
        .collect();
}

#[given("hourly closes 40000 40000 40000 20000")]
fn drop_closes(world: &mut SignalWorld) {
    world.bars = [40_000, 40_000, 40_000, 20_000]
        .into_iter()
        .enumerate()
        .map(|(index, px)| {
            bar_obs(
                hour(i64::try_from(index).expect("index")),
                BarInterval::H1,
                usdt_ticks(px),
            )
        })
        .collect();
}

#[given("one hourly bar with close ticks 0")]
fn zero_close(world: &mut SignalWorld) {
    world.bars = vec![bar_obs(hour(0), BarInterval::H1, 0)];
}

fn tsmom_at(world: &mut SignalWorld, decision_us: i64) {
    let signal = Tsmom::<3>::new(BarInterval::H1, 0.05).expect("params");
    world.signal = Some(drive(&signal, &world.bars, at(decision_us)));
}

#[when(regex = r"^tsmom is evaluated at hour (\d+)$")]
fn tsmom_hour(world: &mut SignalWorld, h: i64) {
    tsmom_at(world, hour(h));
}

#[when(regex = r"^tsmom is evaluated 1 microsecond before hour (\d+)$")]
fn tsmom_before(world: &mut SignalWorld, h: i64) {
    tsmom_at(world, hour(h) - 1);
}

fn funding_at(world: &mut SignalWorld, h: i64) {
    let signal = FundingZ::<4>::new(BarInterval::H1, 2.0, PROD_MAX_AGE_US).expect("params");
    let bars = vec![bar_ending_at(hour(h), BarInterval::H1, usdt_ticks(40_000))];
    world.signal = Some(drive_funding_z(&signal, &bars, &world.funding, at(hour(h))));
}

#[when(regex = r"^funding z is evaluated at hour (\d+)$")]
fn funding_hour(world: &mut SignalWorld, h: i64) {
    funding_at(world, h);
}

#[when(regex = r"^volregime is evaluated at hour (\d+)$")]
fn vol_hour(world: &mut SignalWorld, h: i64) {
    let feature = VolRegime::<2, 3>::new(BarInterval::H1).expect("params");
    world.feature = Some(drive_feature(&feature, &world.bars, at(hour(h))));
}

#[when("a rank is computed for NaN")]
fn rank_nan(world: &mut SignalWorld) {
    world.rank = Some(Rank::of(f64::NAN, &[0.0]));
}

#[then(regex = r"^the target ticks are (-?\d+)$")]
fn target_ticks(world: &mut SignalWorld, ticks: i64) {
    match world.signal.as_ref() {
        Some(Ok(Some(out))) => match out.decision {
            Decision::Target(target) => assert_eq!(target.ticks(), ticks, "{out:?}"),
            Decision::Flat(reason) => panic!("flat {reason:?} for target {ticks}"),
        },
        other => panic!("expected target {ticks}, got {other:?}"),
    }
}

#[then("there is no signal output")]
fn no_output(world: &mut SignalWorld) {
    assert_eq!(world.signal.as_ref(), Some(&Ok(None)));
}

#[then(regex = r"^the signal error is (\w+)$")]
fn signal_error(world: &mut SignalWorld, name: String) {
    let err = match world.signal.as_ref() {
        Some(Err(err)) => err,
        other => panic!("expected {name}, got {other:?}"),
    };
    match name.as_str() {
        "NoTriggeringBar" => assert!(matches!(err, SignalError::NoTriggeringBar), "{err:?}"),
        "InvalidFeature" => assert!(matches!(err, SignalError::InvalidFeature), "{err:?}"),
        "FundingIntervalChange" => {
            assert!(
                matches!(
                    err,
                    SignalError::FundingIntervalChange {
                        interval_hours: 4,
                        ..
                    }
                ),
                "{err:?}"
            );
        }
        other => panic!("unchecked error name {other}"),
    }
}

#[then(regex = r"^the decision is flat (\w+)$")]
fn flat_reason(world: &mut SignalWorld, name: String) {
    let reason = match name.as_str() {
        "FundingSlotMissing" => FlatReason::FundingSlotMissing,
        "FundingStale" => FlatReason::FundingStale,
        other => panic!("unchecked flat reason {other}"),
    };
    match world.signal.as_ref() {
        Some(Ok(Some(out))) => {
            assert_eq!(out.decision, Decision::Flat(reason), "{out:?}");
            assert_eq!(out.decision.position_ticks(), 0);
        }
        other => panic!("expected flat {reason:?}, got {other:?}"),
    }
}

#[then(regex = r"^the feature rank ticks are (\d+)$")]
fn feature_ticks(world: &mut SignalWorld, ticks: i64) {
    match world.feature.as_ref() {
        Some(Ok(Some(out))) => assert_eq!(out.value.ticks(), ticks, "{out:?}"),
        other => panic!("expected rank {ticks}, got {other:?}"),
    }
}

#[then("the rank error is NonFinite")]
fn rank_non_finite(world: &mut SignalWorld) {
    assert_eq!(world.rank, Some(Err(RankError::NonFinite)));
}

#[then(regex = r"^the funding row is visible at hour (\d+) plus (\d+) microseconds$")]
fn funding_visible(world: &mut SignalWorld, h: i64, plus: i64) {
    let row = world.funding.first().expect("funding row");
    assert!(row.causal_at(at(hour(h) + plus)).is_some());
}

#[then(regex = r"^the funding row is not visible at hour (\d+) plus (\d+) microseconds$")]
fn funding_hidden(world: &mut SignalWorld, h: i64, plus: i64) {
    let row = world.funding.first().expect("funding row");
    assert!(row.causal_at(at(hour(h) + plus)).is_none());
}

#[then(
    regex = r"^the funding row is not visible 1 microsecond before hour (\d+) plus (\d+) microseconds$"
)]
fn funding_hidden_early(world: &mut SignalWorld, h: i64, plus: i64) {
    let row = world.funding.first().expect("funding row");
    assert!(row.causal_at(at(hour(h) + plus - 1)).is_none());
}

#[then(regex = r"^tsmom emits a target on hours (\d+) through (\d+)$")]
fn emits_range(world: &mut SignalWorld, start: i64, end: i64) {
    let signal = Tsmom::<3>::new(BarInterval::H1, 0.05).expect("params");
    for h in start..=end {
        match drive(&signal, &world.bars, at(hour(h))) {
            Ok(Some(out)) => assert!(matches!(out.decision, Decision::Target(_)), "{out:?}"),
            other => panic!("hour {h} did not emit a target: {other:?}"),
        }
    }
}

#[tokio::main]
async fn main() {
    let features = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../specs/features/signals/signals_v0.feature");
    support::anchors();
    SignalWorld::cucumber().run_and_exit(features).await;
}
