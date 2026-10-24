use parangsae_core::{
    Amount, AvailableTs, DecisionTime, EventTs, Observation, Price, Qty, ReceivedTs, Side, Symbol,
    Timestamps, UtcMicros,
};
use parangsae_execution::{
    ApprovalTiming, Attempt, ClientOrderId, ExecutionConfig, HaltCause, IntentId, MaxAttempts,
    Mono, NotFoundAuthority, PaperAction, PaperCommand, PaperFill, PaperReport, PaperSession,
    PaperTerminal, PaperTerminalCut, PositiveMicros,
};
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};

pub const UNIT: i64 = 100_000_000;
pub const AT: i64 = 1_704_067_200_000_000;

pub fn id(raw: u128) -> ClientOrderId {
    ClientOrderId::from_parts(IntentId::from_raw(raw), Attempt::FIRST)
}
pub fn qty(ticks: i64) -> Qty {
    Qty::from_ticks(ticks).unwrap()
}
pub fn price(ticks: i64) -> Price {
    Price::from_ticks(ticks).unwrap()
}
pub fn observation<T>(value: T, at: u64) -> Observation<T> {
    let at = UtcMicros::from_micros(AT + i64::try_from(at).unwrap());
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(at),
            available_ts: AvailableTs::from_micros(at),
            received_ts: ReceivedTs::from_micros(at),
        },
        value,
    )
}
pub fn session() -> PaperSession {
    let age = PositiveMicros::new(10_000).unwrap();
    PaperSession {
        symbol: Symbol::new("BTCUSDT").unwrap(),
        initial_cash: Cash(10_000 * UNIT),
        initial_equity: Cash(10_000 * UNIT),
        initial_mark: observation(price(30_000 * UNIT), 0),
        initial_decision: DecisionTime::from_micros(UtcMicros::from_micros(AT)),
        initial_now: Mono::from_micros(0),
        costs: Costs {
            taker_fee: FeeBps(5),
            slippage: SlippageBps(2),
        },
        limits: parangsae_risk::Limits::new(
            qty(1_000_000),
            qty(10_000_000),
            Amount::from_ticks(10_000 * UNIT).unwrap(),
            Amount::from_ticks(100 * UNIT).unwrap(),
            age.get(),
            60_000_000,
            128,
        )
        .unwrap(),
        execution: ExecutionConfig::new(
            ApprovalTiming::new(age, PositiveMicros::new(100).unwrap()).unwrap(),
            NotFoundAuthority::new(5000, 1000).unwrap(),
            MaxAttempts::new(3).unwrap(),
            age,
        )
        .unwrap(),
    }
}
pub fn command(now: u64, action: PaperAction) -> PaperCommand {
    PaperCommand {
        now: Mono::from_micros(now),
        decision: DecisionTime::from_micros(UtcMicros::from_micros(
            AT + i64::try_from(now).unwrap(),
        )),
        mark: observation(price(30_000 * UNIT), 0),
        action,
    }
}
pub fn plan(now: u64, raw: u128) -> PaperCommand {
    command(
        now,
        PaperAction::Plan {
            client_order_id: id(raw),
            symbol: Symbol::new("BTCUSDT").unwrap(),
            side: Side::Buy,
            qty: qty(400_000),
            reduce_only: false,
        },
    )
}
pub fn fill(at: u64, number: u64, ticks: i64) -> Observation<PaperFill> {
    observation(
        PaperFill {
            client_order_id: id(1),
            fill_id: number,
            qty: qty(ticks),
            reference_price: price(30_000 * UNIT),
        },
        at,
    )
}
pub fn deliver(at: u64, fill: &Observation<PaperFill>) -> PaperCommand {
    command(
        at,
        PaperAction::Deliver(fill.with_value(PaperReport::Fill(fill.value().clone()))),
    )
}
pub fn cut(at: u64, status: PaperTerminal, receipts: Vec<Observation<PaperFill>>) -> PaperCommand {
    let cumulative = receipts
        .iter()
        .map(|receipt| receipt.value().qty.ticks())
        .sum();
    command(
        at,
        PaperAction::Reconcile(observation(
            PaperTerminalCut {
                client_order_id: id(1),
                evidence_id: at,
                status,
                cumulative: qty(cumulative),
                receipts,
                complete: true,
            },
            at,
        )),
    )
}
pub fn prefix() -> Vec<PaperCommand> {
    vec![
        plan(1, 1),
        command(
            2,
            PaperAction::Dispatch {
                client_order_id: id(1),
            },
        ),
        command(
            3,
            PaperAction::Deliver(observation(
                PaperReport::Ack {
                    client_order_id: id(1),
                },
                3,
            )),
        ),
    ]
}
pub fn full_fill() -> (PaperSession, Vec<PaperCommand>) {
    let mut commands = prefix();
    let receipt = fill(4, 1, 400_000);
    commands.extend([
        deliver(4, &receipt),
        cut(5, PaperTerminal::Filled, vec![receipt]),
        command(6, PaperAction::ReconcilePortfolio),
    ]);
    (session(), commands)
}
pub fn partial() -> (PaperSession, Vec<PaperCommand>) {
    let mut commands = prefix();
    let first = fill(4, 1, 100_000);
    let second = fill(5, 2, 300_000);
    commands.extend([
        deliver(4, &first),
        deliver(5, &second),
        deliver(6, &first),
        cut(7, PaperTerminal::Filled, vec![first, second]),
        command(8, PaperAction::ReconcilePortfolio),
    ]);
    (session(), commands)
}
pub fn cancel_race() -> (PaperSession, Vec<PaperCommand>) {
    let mut commands = prefix();
    let first = fill(4, 1, 100_000);
    let race = fill(7, 2, 100_000);
    commands.extend([
        deliver(4, &first),
        command(
            5,
            PaperAction::Cancel {
                client_order_id: id(1),
            },
        ),
        command(
            6,
            PaperAction::Deliver(observation(
                PaperReport::CancelConfirmed {
                    client_order_id: id(1),
                },
                6,
            )),
        ),
        deliver(7, &race),
        cut(8, PaperTerminal::Canceled, vec![first, race]),
        command(9, PaperAction::ReconcilePortfolio),
    ]);
    (session(), commands)
}
pub fn unknown() -> (PaperSession, Vec<PaperCommand>) {
    let mut commands = prefix();
    commands.extend([
        command(
            4,
            PaperAction::Timeout {
                client_order_id: id(1),
            },
        ),
        cut(10, PaperTerminal::Filled, vec![fill(5, 1, 400_000)]),
        command(11, PaperAction::ReconcilePortfolio),
        command(12, PaperAction::Resume),
    ]);
    (session(), commands)
}
pub fn halted() -> (PaperSession, Vec<PaperCommand>) {
    (
        session(),
        vec![
            plan(1, 1),
            command(2, PaperAction::Halt(HaltCause::RiskTrip)),
            command(
                3,
                PaperAction::Dispatch {
                    client_order_id: id(1),
                },
            ),
            command(4, PaperAction::ReconcilePortfolio),
            command(5, PaperAction::Resume),
        ],
    )
}
