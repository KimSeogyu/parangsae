use cucumber::{then, when, World as _};
use parangsae_core::{
    AvailableTs, DecisionTime, EventTs, Observation, Price, Qty, ReceivedTs, Timestamps, UtcMicros,
};
use parangsae_execution::carry::*;
use parangsae_execution::{
    ApprovalTiming, ExecutionConfig, MaxAttempts, Mono, NotFoundAuthority, PositiveMicros,
};
use parangsae_sim::carry::{CarryAccountingError, FeeCharge, Market};
use parangsae_sim::{Cash, FeeBps, SlippageBps};
use std::path::Path;

const U: i64 = 100_000_000;
const NET: i64 = 99_900_000;
const AT: i64 = 1_704_067_200_000_000;

fn qty(ticks: i64) -> Qty {
    Qty::from_ticks(ticks).unwrap()
}

fn price(usdt: i64) -> Price {
    Price::from_ticks(usdt * U).unwrap()
}

fn stamp(now: u64) -> UtcMicros {
    UtcMicros::from_micros(AT + i64::try_from(now).unwrap())
}

fn observed<T>(value: T, now: u64) -> Observation<T> {
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(stamp(now)),
            available_ts: AvailableTs::from_micros(stamp(now)),
            received_ts: ReceivedTs::from_micros(stamp(now)),
        },
        value,
    )
}

fn id(stage: CarryStage) -> CarryOrderId {
    CarryOrderId {
        pair: 1,
        stage,
        attempt: 0,
    }
}

fn session(slip: u32, perp_cash: Cash) -> CarrySession {
    let age = PositiveMicros::new(1_000).unwrap();
    CarrySession {
        allocation: parangsae_sim::carry::Allocation {
            total: Cash(5_000 * U + perp_cash.0),
            spot_quote: Cash(4_000 * U),
            perp_collateral: perp_cash,
            idle: Cash(1_000 * U),
        },
        fees: CarryFees {
            spot_bps: FeeBps(10),
            spot_buy_asset: SpotFeeAsset::Btc,
            spot_sell_asset: SpotFeeAsset::Usdt,
            perp_bps: FeeBps(5),
            spot_slippage: SlippageBps(0),
            perp_slippage: SlippageBps(slip),
        },
        spot_grid: CarryGrid {
            lot: qty(100_000),
            min_notional: Cash(U),
            provenance: RuleProvenance::AssumedResearch,
        },
        perp_grid: CarryGrid {
            lot: qty(100_000),
            min_notional: Cash(U),
            provenance: RuleProvenance::AssumedResearch,
        },
        limits: CarryLimits {
            max_net_btc: qty(2 * U),
            max_unhedged_notional: Cash(20_000 * U),
            max_session_loss: Cash(500 * U),
            max_mark_age_us: 10_000,
            max_legging_us: 10_000,
            max_orders: 16,
            max_receipts: 16,
            max_funding_events: 4,
            max_batch_entries: 16,
        },
        margin: AssumedMarginPolicy {
            initial_bps: 10_000,
            maintenance_bps: 50,
            spot_close_buffer: Cash(10 * U),
            perp_close_buffer: Cash(10 * U),
            perp_funding_buffer: Cash(20 * U),
        },
        execution: ExecutionConfig::new(
            ApprovalTiming::new(age, PositiveMicros::new(100).unwrap()).unwrap(),
            NotFoundAuthority::new(5_000, 1_000).unwrap(),
            MaxAttempts::new(1).unwrap(),
            age,
        )
        .unwrap(),
        initial_spot: observed(price(100), 0),
        initial_perp: observed(price(100), 0),
        initial_decision: DecisionTime::from_micros(stamp(0)),
        initial_now: Mono::from_micros(0),
    }
}

#[derive(Debug)]
struct Fixture {
    engine: CarryEngine,
    now: u64,
    receipts: Vec<Observation<CarryFill>>,
}

impl Fixture {
    fn held(slip: u32, perp_cash: Cash) -> Self {
        let mut f = Self {
            engine: CarryEngine::new(session(slip, perp_cash)).unwrap(),
            now: 0,
            receipts: Vec::new(),
        };
        f.step(CarryAction::Plan(CarryPlan {
            pair: 1,
            gross_spot: qty(U),
            net_btc: qty(NET),
            max_spot_price: price(101),
            max_perp_price: price(101),
        }))
        .unwrap();
        f.step(CarryAction::Dispatch(id(CarryStage::SpotEntry)))
            .unwrap();
        f.fill(CarryStage::SpotEntry, U, 100).unwrap();
        f.cut(CarryStage::SpotEntry);
        f.step(CarryAction::PlanHedge { pair: 1 }).unwrap();
        f.step(CarryAction::Dispatch(id(CarryStage::PerpEntry)))
            .unwrap();
        f.fill(CarryStage::PerpEntry, NET, 100).unwrap();
        f.cut(CarryStage::PerpEntry);
        assert_eq!(f.engine.snapshot().phase, CarryPhase::Hedged);
        f
    }

    fn step(&mut self, action: CarryAction) -> Result<CarryOutcome, CarryError> {
        self.now += 1;
        self.engine.step(&CarryCommand {
            now: Mono::from_micros(self.now),
            decision: DecisionTime::from_micros(stamp(self.now)),
            spot: observed(price(100), self.now),
            perp: observed(price(100), self.now),
            action,
        })
    }

    fn fill(
        &mut self,
        stage: CarryStage,
        amount: i64,
        reference: i64,
    ) -> Result<CarryOutcome, CarryError> {
        let market = match stage {
            CarryStage::SpotEntry | CarryStage::SpotExit => Market::Spot,
            CarryStage::PerpEntry | CarryStage::PerpExit => Market::UsdMPerp,
        };
        let source = observed(
            CarryFill {
                order: id(stage),
                fill_id: u64::try_from(self.receipts.len() + 1).unwrap(),
                market,
                qty: qty(amount),
                reference_price: price(reference),
            },
            self.now + 1,
        );
        let result = self.step(CarryAction::Deliver(
            source.with_value(CarryReport::Fill(source.value().clone())),
        ));
        if result.is_ok() {
            self.receipts.push(source);
        }
        result
    }

    fn cut(&mut self, stage: CarryStage) {
        let receipts: Vec<_> = self
            .receipts
            .iter()
            .filter(|r| r.value().order == id(stage))
            .cloned()
            .collect();
        let cumulative = receipts.iter().map(|r| r.value().qty.ticks()).sum();
        self.step(CarryAction::Reconcile(observed(
            CarryTerminalCut {
                order: id(stage),
                evidence_id: 1,
                status: CarryTerminal::Filled,
                cumulative: qty(cumulative),
                receipts,
                complete: true,
            },
            self.now + 1,
        )))
        .unwrap();
    }

    fn finish(&mut self) {
        self.fill(CarryStage::PerpExit, NET, 100).unwrap();
        self.cut(CarryStage::PerpExit);
        self.step(CarryAction::PlanSpotExit {
            pair: 1,
            max_spot_price: price(101),
        })
        .unwrap();
        self.step(CarryAction::Dispatch(id(CarryStage::SpotExit)))
            .unwrap();
        self.fill(CarryStage::SpotExit, NET, 100).unwrap();
        self.cut(CarryStage::SpotExit);
    }
}

#[derive(Debug)]
struct CloseTrace {
    fixture: Fixture,
    before: CarrySnapshot,
    outcome: Result<CarryOutcome, CarryError>,
    after: CarrySnapshot,
}

fn prepare_close(slip: u32, perp_cash: Cash, upper: i64) -> CloseTrace {
    let mut fixture = Fixture::held(slip, perp_cash);
    fixture
        .step(CarryAction::BeginExit {
            pair: 1,
            max_perp_price: price(upper),
        })
        .unwrap();
    let before = fixture.engine.snapshot();
    let outcome = fixture.step(CarryAction::Dispatch(id(CarryStage::PerpExit)));
    let after = fixture.engine.snapshot();
    CloseTrace {
        fixture,
        before,
        outcome,
        after,
    }
}

fn assert_no_posting(t: &CloseTrace) {
    assert_eq!(t.after.account, t.before.account);
    assert_eq!(t.after.fills, t.before.fills);
    assert_eq!(t.after.funding, t.before.funding);
    assert_eq!(t.after.spot_quote_reserved, t.before.spot_quote_reserved);
    assert_eq!(
        t.after.perp_collateral_reserved,
        t.before.perp_collateral_reserved
    );
}

fn assert_round_trip(snapshot: &CarrySnapshot, slip: u32) {
    let (buyback, fee, realized, perp_wallet, total) = match slip {
        0 => (
            10_000_000_000,
            4_995_000,
            0,
            499_990_010_000,
            999_970_020_000,
        ),
        2 => (
            10_002_000_000,
            4_995_999,
            -3_996_000,
            499_986_014_000,
            999_966_024_000,
        ),
        _ => panic!("unexpected synthetic fee scenario"),
    };
    assert_eq!(snapshot.phase, CarryPhase::Closed);
    assert_eq!(snapshot.account.spot_quote, Cash(399_980_010_000));
    assert_eq!(snapshot.account.perp_wallet, Cash(perp_wallet));
    assert_eq!(snapshot.account.idle, Cash(1_000 * U));
    assert_eq!(snapshot.account.spot_btc, qty(0));
    assert_eq!(snapshot.account.perp_short, qty(0));
    assert_eq!(snapshot.account.perp_fees, Cash(9_990_000));
    assert_eq!(snapshot.account.realized_perp, Cash(realized));
    assert_eq!(
        snapshot.account.equity(price(100), price(100)).unwrap(),
        Cash(total)
    );
    let close = snapshot
        .fills
        .iter()
        .find(|f| f.source.value().order.stage == CarryStage::PerpExit)
        .unwrap();
    assert_eq!(close.source.value().reference_price, price(100));
    assert_eq!(close.price.ticks(), buyback);
    assert_eq!(close.fee, FeeCharge::Usdt(Cash(fee)));
}

#[test]
fn a_positive_slippage_close_fits_its_executed_upper_bound() {
    let mut t = prepare_close(2, Cash(5_000 * U), 101);
    assert_eq!(
        t.outcome,
        Ok(CarryOutcome::Dispatched(id(CarryStage::PerpExit)))
    );
    assert_no_posting(&t);
    t.fixture.finish();
    assert_round_trip(&t.fixture.engine.snapshot(), 2);
}

#[derive(Debug, Default, cucumber::World)]
struct CloseWorld {
    trace: Option<CloseTrace>,
    affordable: Option<CloseTrace>,
    slip: u32,
    refused_before: Option<CarrySnapshot>,
    refused_outcome: Option<Result<CarryOutcome, CarryError>>,
}

#[when(
    regex = r"^a held pair with (0|2) bps perpetual slippage dispatches a close capped at 101 USDT$"
)]
fn held_close(world: &mut CloseWorld, slip: u32) {
    world.slip = slip;
    world.trace = Some(prepare_close(slip, Cash(5_000 * U), 101));
}

#[then("the funded close dispatches without posting cash, fees, PnL or a priced fill")]
fn dispatched_without_posting(world: &mut CloseWorld) {
    let t = world.trace.as_ref().unwrap();
    assert_eq!(
        t.outcome,
        Ok(CarryOutcome::Dispatched(id(CarryStage::PerpExit)))
    );
    assert_no_posting(t);
    assert!(t.after.orders.last().unwrap().progress.sent());
}

#[when("the raw 100 USDT buyback and spot exit complete with priced cuts")]
fn finish_close(world: &mut CloseWorld) {
    world.trace.as_mut().unwrap().fixture.finish();
}

#[then("the executed price, fees and final wallet PnL match one slippage application")]
fn exact_economics(world: &mut CloseWorld) {
    let snapshot = world.trace.as_ref().unwrap().fixture.engine.snapshot();
    assert_round_trip(&snapshot, world.slip);
}

#[when("a raw 101 USDT buyback is delivered against the 101 USDT executed cap")]
fn out_of_bound(world: &mut CloseWorld) {
    let t = world.trace.as_mut().unwrap();
    world.refused_before = Some(t.fixture.engine.snapshot());
    world.refused_outcome = Some(t.fixture.fill(CarryStage::PerpExit, NET, 101));
}

#[then("PriceEnvelope retains the unapplied fact without changing financial evidence")]
fn refused_fill(world: &mut CloseWorld) {
    let before = world.refused_before.as_ref().unwrap();
    let after = world.trace.as_ref().unwrap().fixture.engine.snapshot();
    assert_eq!(world.refused_outcome, Some(Err(CarryError::PriceEnvelope)));
    assert_eq!(after.account, before.account);
    assert_eq!(after.orders, before.orders);
    assert_eq!(after.fills, before.fills);
    assert_eq!(after.funding, before.funding);
    assert_eq!(after.spot_quote_reserved, before.spot_quote_reserved);
    assert_eq!(
        after.perp_collateral_reserved,
        before.perp_collateral_reserved
    );
    assert!(matches!(
        after.unsupported,
        Some(UnsupportedEvidence::Fill(_))
    ));
    assert!(after.halted);
    assert_eq!(after.phase, CarryPhase::Unresolved);
}

#[when("the fee-inclusive zero-cash boundary and one tick less preflight a 300 USDT upper")]
fn unaffordable(world: &mut CloseWorld) {
    world.affordable = Some(prepare_close(2, Cash(20_001_977_001), 300));
    world.trace = Some(prepare_close(2, Cash(20_001_977_000), 300));
}

#[then("only the funded boundary dispatches and neither preflight posts or releases reserves")]
fn unaffordable_refusal(world: &mut CloseWorld) {
    let funded = world.affordable.as_ref().unwrap();
    assert_eq!(
        funded.outcome,
        Ok(CarryOutcome::Dispatched(id(CarryStage::PerpExit)))
    );
    assert_no_posting(funded);
    let t = world.trace.as_ref().unwrap();
    assert_eq!(
        funded.before.account.perp_wallet.0 - t.before.account.perp_wallet.0,
        1
    );
    assert_eq!(
        t.outcome,
        Err(CarryError::Accounting(
            CarryAccountingError::InsufficientPerpCash
        ))
    );
    assert_no_posting(t);
    assert_eq!(t.after.orders, t.before.orders);
    assert!(!t.after.orders.last().unwrap().progress.sent());
}

#[test]
fn carry_close_preflight_behavior() {
    futures::executor::block_on(
        CloseWorld::cucumber()
            .with_default_cli()
            .fail_on_skipped()
            .run_and_exit(Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/carry/close_preflight.feature"
            ))),
    );
}
