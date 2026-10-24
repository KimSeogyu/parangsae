use super::*;
use cucumber::{then, when, World as _};

const UNIT: i64 = 100_000_000;
const NET_QTY: i64 = 9_990_000_000;

#[derive(Clone, Copy, Debug)]
struct CostOracle {
    scenario: Scenario,
    buy: i64,
    sell: i64,
    entry_fee: i64,
    exit_fee: i64,
    spot_sell_fee: i64,
    spot_wallet: i64,
    perp_wallet: i64,
    total: i64,
    realized: i64,
}

fn oracle(name: &str) -> CostOracle {
    match name {
        "headline" => CostOracle {
            scenario: Scenario::Headline,
            buy: 10_002_000_000,
            sell: 9_998_000_000,
            entry_fee: 499_400_100,
            exit_fee: 499_599_900,
            spot_sell_fee: 998_800_200,
            spot_wallet: 47_495_202_799_600,
            perp_wallet: 47_497_182_820_000,
            total: 99_992_385_619_600,
            realized: -799_200_000,
        },
        "higher-fee" => CostOracle {
            scenario: Scenario::HigherPerpFee,
            buy: 10_002_000_000,
            sell: 9_998_000_000,
            entry_fee: 599_280_120,
            exit_fee: 599_519_880,
            spot_sell_fee: 998_800_200,
            spot_wallet: 47_495_202_799_600,
            perp_wallet: 47_496_783_220_000,
            total: 99_991_986_019_600,
            realized: -799_200_000,
        },
        "stress" => CostOracle {
            scenario: Scenario::SlippageStress,
            buy: 10_005_000_000,
            sell: 9_995_000_000,
            entry_fee: 499_250_250,
            exit_fee: 499_749_750,
            spot_sell_fee: 998_500_500,
            spot_wallet: 47_494_003_999_000,
            perp_wallet: 47_495_984_020_000,
            total: 99_989_988_019_000,
            realized: -1_998_000_000,
        },
        _ => panic!("unknown selected cost fixture"),
    }
}

fn run_cost(expected: CostOracle) -> ReplayResult {
    let source = two_cycles_source(false);
    C1Replay::new(&source, expected.scenario, window())
        .unwrap()
        .run()
        .unwrap()
}

fn assert_cost(value: &ReplayResult, expected: CostOracle) {
    let snapshot = value.snapshot.as_ref().unwrap();
    assert_eq!(value.pairs.len(), 2);
    assert_eq!(snapshot.fills.len(), 8);
    assert_eq!(snapshot.funding.len(), 2);
    assert_eq!(value.daily.len(), 9);
    assert_eq!(value.final_equity, expected.total);
    assert_eq!(snapshot.account.spot_quote.0, expected.spot_wallet);
    assert_eq!(snapshot.account.perp_wallet.0, expected.perp_wallet);
    assert_eq!(snapshot.account.idle.0, 50_000 * UNIT);
    assert_eq!(snapshot.account.spot_btc.ticks(), 0);
    assert_eq!(snapshot.account.perp_short.ticks(), 0);
    assert_eq!(snapshot.account.spot_btc_fees.ticks(), 20_000_000);
    assert_eq!(
        snapshot.account.spot_quote_fees.0,
        expected.spot_sell_fee * 2
    );
    assert_eq!(
        snapshot.account.perp_fees.0,
        (expected.entry_fee + expected.exit_fee) * 2
    );
    assert_eq!(snapshot.account.realized_perp.0, expected.realized);
    assert_eq!(
        snapshot.account.realized_perp_scaled,
        i128::from(expected.realized) * i128::from(UNIT)
    );
    assert_eq!(snapshot.account.perp_entry_cost_scaled, 0);
    assert_eq!(snapshot.account.funding_net.0, -19_980_000);
    assert_eq!(snapshot.phase, CarryPhase::Closed);
    assert!(!snapshot.halted);
    assert!(!snapshot.loss_latched);
    assert!(!snapshot.margin_latched);
    for fill in &snapshot.fills {
        let (quantity, price, fee) = match fill.source.value().order.stage {
            CarryStage::SpotEntry => (
                10_000_000_000,
                expected.buy,
                FeeCharge::Btc(Qty::from_ticks(10_000_000).unwrap()),
            ),
            CarryStage::PerpEntry => (
                NET_QTY,
                expected.sell,
                FeeCharge::Usdt(Cash(expected.entry_fee)),
            ),
            CarryStage::PerpExit => (
                NET_QTY,
                expected.buy,
                FeeCharge::Usdt(Cash(expected.exit_fee)),
            ),
            CarryStage::SpotExit => (
                NET_QTY,
                expected.sell,
                FeeCharge::Usdt(Cash(expected.spot_sell_fee)),
            ),
        };
        assert_eq!(fill.source.value().qty.ticks(), quantity);
        assert_eq!(fill.source.value().reference_price.ticks(), 100 * UNIT);
        assert_eq!(fill.price.ticks(), price);
        assert_eq!(fill.fee, fee);
    }
    for (funding, effective) in snapshot
        .funding
        .iter()
        .zip([START + 2 * SLOT, START + 5 * SLOT])
    {
        assert_eq!(funding.effective.micros(), effective);
        assert_eq!(funding.captured_short.ticks(), NET_QTY);
        assert_eq!(funding.cashflow, Some(Cash(-9_990_000)));
        let source = funding.receipt.as_ref().unwrap();
        assert_eq!(source.value().rate.ticks(), -1_000);
        assert_eq!(source.value().charge_mark.ticks(), 100 * UNIT);
    }
    for pair in &value.pairs {
        assert_eq!(pair.gross_spot, 10_000_000_000);
        assert_eq!(pair.net_btc, NET_QTY);
        assert_eq!(pair.headline_c4, 3_797_200_200);
    }
    assert_dependent_decisions(value);
}

fn risk_source() -> C1SourceView {
    let settlement = START + 2 * SLOT;
    let mut bars = Vec::new();
    let mut key = START - MINUTE;
    while key <= START + 44 * HOUR {
        for source in [PriceSource::Spot, PriceSource::UmTrade, PriceSource::UmMark] {
            let price = if source != PriceSource::Spot && key >= settlement - MINUTE {
                700 * UNIT
            } else {
                100 * UNIT
            };
            bars.push(bar(source, key, price));
        }
        key += MINUTE;
    }
    let rates = (-8..=5)
        .map(|index| {
            let key = START + index * SLOT;
            let rate = if matches!(index, 0 | 2 | 3 | 5) {
                -1_000
            } else {
                50_000
            };
            (key, rate, key + MINUTE)
        })
        .collect();
    source_with_rates(rates, bars)
}

fn run_risk() -> ReplayResult {
    let source = risk_source();
    C1Replay::new(&source, Scenario::Headline, window())
        .unwrap()
        .run()
        .unwrap()
}

fn assert_risk_before_payment(value: &ReplayResult) {
    let settlement = START + 2 * SLOT;
    let exit = settlement + MS;
    assert_eq!(value.pairs.len(), 1);
    let pair = &value.pairs[0];
    assert_eq!(pair.exit_reason, Some(ExitReason::Risk));
    assert_eq!(pair.exit_decision, Some(exit));
    assert_eq!(pair.closing_deadline, Some(exit + LEGGING));
    assert!(value.records.iter().any(|record| matches!(record,
        ActorRecord::RiskStop { at, loss: true, margin: false } if *at == settlement)));
    let close_order = value.records.iter().find_map(|record| match record {
        ActorRecord::Order {
            at,
            stage: "perp_exit",
            deadline,
            ..
        } => Some((*at, *deadline)),
        _ => None,
    });
    assert_eq!(close_order, Some((exit, exit + LEGGING)));
    let payment = value.records.iter().find_map(|record| match record {
        ActorRecord::FundingReceipt {
            at,
            effective,
            cashflow,
            ..
        } if *effective == settlement => Some((*at, *cashflow)),
        _ => None,
    });
    assert_eq!(payment, Some((settlement + MINUTE, -69_930_000)));
    assert!(close_order.unwrap().0 < payment.unwrap().0);
    let snapshot = value.snapshot.as_ref().unwrap();
    assert_eq!(snapshot.funding.len(), 1);
    assert_eq!(snapshot.funding[0].captured_short.ticks(), NET_QTY);
    assert_eq!(snapshot.account.funding_net.0, -69_930_000);
    assert_eq!(snapshot.phase, CarryPhase::Closed);
    assert!(snapshot.halted);
    assert!(snapshot.loss_latched);
    assert!(pair.closed_observed_at.unwrap() > payment.unwrap().0);
    assert!(pair.closed_observed_at.unwrap() <= exit + LEGGING);
    assert!(matches!(value.control.values.get("mode"),
        Some(FiniteValue::String(mode)) if mode == "Stopped"));
}

fn no_hedge_liquidity_source() -> C1SourceView {
    let first_hedge_open = START + 9 * HOUR + 3 * MINUTE;
    let tempting_late_open = START + 9 * HOUR + 11 * MINUTE;
    let mut bars = Vec::new();
    let mut key = START - MINUTE;
    while key <= START + 44 * HOUR {
        for source in [PriceSource::Spot, PriceSource::UmTrade, PriceSource::UmMark] {
            let mut row = bar(source, key, 100 * UNIT);
            if source == PriceSource::UmTrade && key >= first_hedge_open && key < tempting_late_open
            {
                row.1.count = 0;
                row.1.volume = Qty::from_ticks(0).unwrap();
            }
            bars.push(row);
        }
        key += MINUTE;
    }
    let rates = (-8..=5)
        .map(|index| {
            let key = START + index * SLOT;
            let rate = if matches!(index, 0 | 2 | 3 | 5) {
                -1_000
            } else {
                50_000
            };
            (key, rate, key + MINUTE)
        })
        .collect();
    source_with_rates(rates, bars)
}

fn run_no_liquidity() -> ReplayFailure {
    let source = no_hedge_liquidity_source();
    C1Replay::new(&source, Scenario::Headline, window())
        .unwrap()
        .run()
        .unwrap_err()
}

fn assert_original_deadline(value: &ReplayFailure) {
    let entry = START + 9 * HOUR + MS;
    let deadline = entry + LEGGING;
    assert!(matches!(&value.error,
        ReplayError::UnknownAtDeadline { at, stage }
            if *at == deadline + MS && *stage == "unfinished_pair"));
    assert_eq!(value.stop_error, None);
    assert!(matches!(value.prefix.control.values.get("mode"),
        Some(FiniteValue::String(mode)) if mode == "Stopped"));
    assert_eq!(value.prefix.pairs.len(), 1);
    let pair = &value.prefix.pairs[0];
    assert_eq!(pair.opening_deadline, deadline);
    assert_eq!(pair.closing_deadline, None);
    assert_eq!(pair.closed_observed_at, None);
    let snapshot = value.prefix.snapshot.as_ref().unwrap();
    assert_eq!(snapshot.account.spot_quote.0, 46_499_800_000_000);
    assert_eq!(snapshot.account.spot_btc.ticks(), NET_QTY);
    assert_eq!(snapshot.account.spot_btc_fees.ticks(), 10_000_000);
    assert_eq!(snapshot.account.perp_wallet.0, 47_500_000_000_000);
    assert_eq!(snapshot.account.perp_short.ticks(), 0);
    assert_eq!(snapshot.account.perp_fees.0, 0);
    assert_eq!(snapshot.account.funding_net.0, 0);
    assert_eq!(snapshot.fills.len(), 1);
    assert_eq!(snapshot.orders.len(), 2);
    assert_ne!(snapshot.phase, CarryPhase::Closed);
    assert!(snapshot.spot_quote_reserved.0 > 0);
    assert!(snapshot.perp_collateral_reserved.0 > 0);
    let hedge = snapshot
        .orders
        .iter()
        .find(|o| o.id.stage == CarryStage::PerpEntry)
        .unwrap();
    assert!(hedge.progress.sent());
    assert_eq!(hedge.progress.cum().ticks(), 0);
    assert!(!hedge.complete_cut);
    assert!(value.prefix.records.iter().any(|record| matches!(record,
        ActorRecord::Order { stage: "perp_entry", deadline: original, .. }
            if *original == deadline)));
    assert!(value.prefix.daily.is_empty());
}

#[test]
fn risk_reduction_dispatches_before_a_retained_payment_becomes_visible() {
    assert_risk_before_payment(&run_risk());
}

#[test]
fn selected_cost_paths_match_independent_full_account_oracles() {
    for name in ["headline", "higher-fee", "stress"] {
        let expected = oracle(name);
        assert_cost(&run_cost(expected), expected);
    }
}

#[test]
fn a_paid_spot_leg_cannot_wait_for_liquidity_beyond_its_original_deadline() {
    assert_original_deadline(&run_no_liquidity());
}

#[derive(Debug, Default, cucumber::World)]
struct ActorRiskWorld {
    risk: Option<ReplayResult>,
    cost: Option<(CostOracle, ReplayResult)>,
    no_liquidity: Option<ReplayFailure>,
}

#[when("a retained settlement is unpaid when a known loss stops the paired actor")]
fn risk(world: &mut ActorRiskWorld) {
    world.risk = Some(run_risk());
}

#[then("the actual close order precedes the late payment and preserves its captured quantity and deadline")]
fn risk_result(world: &mut ActorRiskWorld) {
    assert_risk_before_payment(world.risk.as_ref().unwrap());
}

#[when(regex = "^the affordable two-cycle actor runs the (headline|higher-fee|stress) cost path$")]
fn costs(world: &mut ActorRiskWorld, name: String) {
    let expected = oracle(&name);
    world.cost = Some((expected, run_cost(expected)));
}

#[then("every executed leg fee payment and final wallet matches the independent cost oracle")]
fn cost_result(world: &mut ActorRiskWorld) {
    let (expected, result) = world.cost.as_ref().unwrap();
    assert_cost(result, *expected);
}

#[when("the paid spot leg has no hedge liquidity until after the original 600 second deadline")]
fn no_liquidity(world: &mut ActorRiskWorld) {
    world.no_liquidity = Some(run_no_liquidity());
}

#[then("the replay stays technical with paid inventory and its undrained order instead of using the late valid bar")]
fn no_liquidity_result(world: &mut ActorRiskWorld) {
    assert_original_deadline(world.no_liquidity.as_ref().unwrap());
}

#[tokio::test]
async fn c1_actor_risk_feature() {
    ActorRiskWorld::cucumber()
        .with_default_cli()
        .fail_on_skipped()
        .run_and_exit(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/features/c1_actor_risk.feature"
        ))
        .await;
}
