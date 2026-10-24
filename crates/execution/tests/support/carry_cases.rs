use parangsae_core::{
    AvailableTs, DecisionTime, EventTs, Observation, Price, Qty, ReceivedTs, Timestamps, UtcMicros,
};
use parangsae_execution::carry::*;
use parangsae_execution::{
    ApprovalTiming, ExecutionConfig, MaxAttempts, Mono, NotFoundAuthority, PositiveMicros,
};
use parangsae_sim::carry::{Allocation, Market};
use parangsae_sim::{Cash, FeeBps, SlippageBps};

pub const U: i64 = 100_000_000;
pub const AT: i64 = 1_704_067_200_000_000;

pub fn qty(ticks: i64) -> Qty {
    Qty::from_ticks(ticks).unwrap()
}
pub fn price(ticks: i64) -> Price {
    Price::from_ticks(ticks).unwrap()
}
pub fn time(at: u64) -> UtcMicros {
    UtcMicros::from_micros(AT + i64::try_from(at).unwrap())
}
pub fn observed<T>(value: T, event: u64, available: u64) -> Observation<T> {
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(time(event)),
            available_ts: AvailableTs::from_micros(time(available)),
            received_ts: ReceivedTs::from_micros(time(available)),
        },
        value,
    )
}
pub fn id(stage: CarryStage) -> CarryOrderId {
    CarryOrderId {
        pair: 1,
        stage,
        attempt: 0,
    }
}
pub fn plan() -> CarryPlan {
    CarryPlan {
        pair: 1,
        gross_spot: qty(U),
        net_btc: qty(99_900_000),
        max_spot_price: price(1_200 * U),
        max_perp_price: price(1_200 * U),
    }
}
pub fn session() -> CarrySession {
    let age = PositiveMicros::new(1_000).unwrap();
    CarrySession {
        allocation: Allocation {
            total: Cash(10_000 * U),
            spot_quote: Cash(4_000 * U),
            perp_collateral: Cash(5_000 * U),
            idle: Cash(1_000 * U),
        },
        fees: CarryFees {
            spot_bps: FeeBps(10),
            spot_buy_asset: SpotFeeAsset::Btc,
            spot_sell_asset: SpotFeeAsset::Usdt,
            perp_bps: FeeBps(5),
            spot_slippage: SlippageBps(0),
            perp_slippage: SlippageBps(0),
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
            max_orders: 32,
            max_receipts: 32,
            max_funding_events: 4,
            max_batch_entries: 32,
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
        initial_spot: observed(price(1_000 * U), 0, 0),
        initial_perp: observed(price(1_000 * U), 0, 0),
        initial_decision: DecisionTime::from_micros(time(0)),
        initial_now: Mono::from_micros(0),
    }
}

#[derive(Debug)]
pub struct Harness {
    pub engine: CarryEngine,
    pub initial: CarrySession,
    pub commands: Vec<CarryCommand>,
    pub now: u64,
    pub receipts: Vec<Observation<CarryFill>>,
}
impl Harness {
    pub fn new() -> Self {
        Self::with_session(session())
    }
    pub fn with_session(session: CarrySession) -> Self {
        Self {
            engine: CarryEngine::new(session.clone()).unwrap(),
            initial: session,
            commands: Vec::new(),
            now: 0,
            receipts: Vec::new(),
        }
    }
    pub fn command(&self, action: CarryAction, spot: i64, perp: i64) -> CarryCommand {
        let now = self.now + 1;
        CarryCommand {
            now: Mono::from_micros(now),
            decision: DecisionTime::from_micros(time(now)),
            spot: observed(price(spot), now, now),
            perp: observed(price(perp), now, now),
            action,
        }
    }
    pub fn marked(
        &mut self,
        action: CarryAction,
        spot: i64,
        perp: i64,
    ) -> Result<CarryOutcome, CarryError> {
        let cmd = self.command(action, spot, perp);
        self.now += 1;
        self.commands.push(cmd.clone());
        self.engine.step(&cmd)
    }
    pub fn act(&mut self, action: CarryAction) -> Result<CarryOutcome, CarryError> {
        self.marked(action, 1_000 * U, 1_000 * U)
    }
    pub fn fill(&mut self, stage: CarryStage, fill_id: u64, lots: i64) {
        let at = self.now + 1;
        let market = match stage {
            CarryStage::SpotEntry | CarryStage::SpotExit => Market::Spot,
            CarryStage::PerpEntry | CarryStage::PerpExit => Market::UsdMPerp,
        };
        let source = observed(
            CarryFill {
                order: id(stage),
                fill_id,
                market,
                qty: qty(lots),
                reference_price: price(1_000 * U),
            },
            at,
            at,
        );
        self.act(CarryAction::Deliver(
            source.with_value(CarryReport::Fill(source.value().clone())),
        ))
        .unwrap();
        self.receipts.push(source);
    }
    pub fn cut(&mut self, stage: CarryStage, status: CarryTerminal) {
        let at = self.now + 1;
        let receipts: Vec<_> = self
            .receipts
            .iter()
            .filter(|r| r.value().order == id(stage))
            .cloned()
            .collect();
        let cumulative = receipts.iter().map(|r| r.value().qty.ticks()).sum();
        self.act(CarryAction::Reconcile(observed(
            CarryTerminalCut {
                order: id(stage),
                evidence_id: 1,
                status,
                cumulative: qty(cumulative),
                receipts,
                complete: true,
            },
            at,
            at,
        )))
        .unwrap();
    }
    pub fn spot(&mut self) {
        self.act(CarryAction::Plan(plan())).unwrap();
        self.act(CarryAction::Dispatch(id(CarryStage::SpotEntry)))
            .unwrap();
        self.fill(CarryStage::SpotEntry, 1, U);
        self.cut(CarryStage::SpotEntry, CarryTerminal::Filled);
    }
    pub fn hedged(&mut self) {
        self.spot();
        self.act(CarryAction::PlanHedge { pair: 1 }).unwrap();
        self.act(CarryAction::Dispatch(id(CarryStage::PerpEntry)))
            .unwrap();
        self.fill(CarryStage::PerpEntry, 2, 99_900_000);
        self.cut(CarryStage::PerpEntry, CarryTerminal::Filled);
        assert_eq!(self.engine.snapshot().phase, CarryPhase::Hedged);
    }
    pub fn close(&mut self) {
        self.act(CarryAction::BeginExit {
            pair: 1,
            max_perp_price: price(1_200 * U),
        })
        .unwrap();
        self.act(CarryAction::Dispatch(id(CarryStage::PerpExit)))
            .unwrap();
        self.fill(CarryStage::PerpExit, 3, 99_900_000);
        self.cut(CarryStage::PerpExit, CarryTerminal::Filled);
        self.act(CarryAction::PlanSpotExit {
            pair: 1,
            max_spot_price: price(1_200 * U),
        })
        .unwrap();
        self.act(CarryAction::Dispatch(id(CarryStage::SpotExit)))
            .unwrap();
        self.fill(CarryStage::SpotExit, 4, 99_900_000);
        self.cut(CarryStage::SpotExit, CarryTerminal::Filled);
    }
}
