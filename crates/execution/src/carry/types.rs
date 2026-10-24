use parangsae_core::{DecisionTime, Observation, Price, Qty, Ratio, UtcMicros};
use parangsae_sim::carry::{
    Allocation, CarryAccountState, CarryAccountingError, FeeCharge, Market,
};
use parangsae_sim::{Cash, FeeBps, SlippageBps};

use crate::{ExecutionConfig, HaltCause, HaltEpoch, Mono, OrderProgress, TransitionError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleProvenance {
    AssumedResearch,
    VerifiedHistorical {
        effective_from: UtcMicros,
        source: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarryGrid {
    pub lot: Qty,
    pub min_notional: Cash,
    pub provenance: RuleProvenance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotFeeAsset {
    Usdt,
    Btc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Buy fees may consume BTC or USDT; this first engine requires USDT sell fees.
pub struct CarryFees {
    pub spot_bps: FeeBps,
    pub spot_buy_asset: SpotFeeAsset,
    pub spot_sell_asset: SpotFeeAsset,
    pub perp_bps: FeeBps,
    pub spot_slippage: SlippageBps,
    pub perp_slippage: SlippageBps,
}

/// Explicit conditional scenario; this is not a historical margin certificate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssumedMarginPolicy {
    pub initial_bps: u32,
    pub maintenance_bps: u32,
    pub spot_close_buffer: Cash,
    pub perp_close_buffer: Cash,
    pub perp_funding_buffer: Cash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CarryLimits {
    pub max_net_btc: Qty,
    pub max_unhedged_notional: Cash,
    pub max_session_loss: Cash,
    pub max_mark_age_us: u64,
    pub max_legging_us: u64,
    pub max_orders: usize,
    pub max_receipts: usize,
    pub max_funding_events: usize,
    pub max_batch_entries: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarrySession {
    pub allocation: Allocation,
    pub fees: CarryFees,
    pub spot_grid: CarryGrid,
    pub perp_grid: CarryGrid,
    pub limits: CarryLimits,
    pub margin: AssumedMarginPolicy,
    pub execution: ExecutionConfig,
    pub initial_spot: Observation<Price>,
    pub initial_perp: Observation<Price>,
    pub initial_decision: DecisionTime,
    pub initial_now: Mono,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CarryStage {
    SpotEntry,
    PerpEntry,
    PerpExit,
    SpotExit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CarryOrderId {
    pub pair: u128,
    pub stage: CarryStage,
    pub attempt: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarryPlan {
    pub pair: u128,
    pub gross_spot: Qty,
    pub net_btc: Qty,
    pub max_spot_price: Price,
    pub max_perp_price: Price,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarryCommand {
    pub now: Mono,
    pub decision: DecisionTime,
    pub spot: Observation<Price>,
    pub perp: Observation<Price>,
    pub action: CarryAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CarryAction {
    Plan(CarryPlan),
    PlanHedge {
        pair: u128,
    },
    Dispatch(CarryOrderId),
    Deliver(Observation<CarryReport>),
    Cancel(CarryOrderId),
    Timeout(CarryOrderId),
    Reconcile(Observation<CarryTerminalCut>),
    BeginExit {
        pair: u128,
        max_perp_price: Price,
    },
    PlanSpotExit {
        pair: u128,
        max_spot_price: Price,
    },
    FundingBoundary {
        key: u64,
        effective: UtcMicros,
    },
    FundingReceipt(Observation<CarryFundingReceipt>),
    ReconcilePortfolio(CarryAccountState),
    NoExecutionEvidence {
        order: CarryOrderId,
        kind: MissingExecution,
    },
    EndOfInput,
    Halt(HaltCause),
    Resume {
        expected_epoch: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingExecution {
    ZeroCount,
    MissingSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarryFill {
    pub order: CarryOrderId,
    pub fill_id: u64,
    pub market: Market,
    pub qty: Qty,
    pub reference_price: Price,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CarryReport {
    Ack(CarryOrderId),
    Fill(CarryFill),
    CancelConfirmed(CarryOrderId),
    CancelDenied(CarryOrderId),
    Expired(CarryOrderId),
    Rejected(CarryOrderId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarryTerminal {
    Filled,
    Canceled,
    Rejected,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarryTerminalCut {
    pub order: CarryOrderId,
    pub evidence_id: u64,
    pub status: CarryTerminal,
    pub cumulative: Qty,
    pub receipts: Vec<Observation<CarryFill>>,
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarryFundingReceipt {
    pub key: u64,
    pub effective: UtcMicros,
    pub rate: Ratio,
    /// Declared charge-mark proxy; it is not claimed to be an observed account charge mark.
    pub charge_mark: Price,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarryPhase {
    Flat,
    OpeningSpot,
    OpeningHedge,
    Hedged,
    ClosingPerp,
    ClosingSpot,
    Unresolved,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarryOrderSnapshot {
    pub id: CarryOrderId,
    pub qty: Qty,
    pub max_price: Price,
    pub progress: OrderProgress,
    pub complete_cut: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarryRecordedFill {
    pub source: Observation<CarryFill>,
    pub price: Price,
    pub fee: FeeCharge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarryFundingSnapshot {
    pub key: u64,
    pub effective: UtcMicros,
    pub captured_short: Qty,
    pub receipt: Option<Observation<CarryFundingReceipt>>,
    pub cashflow: Option<Cash>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnsupportedEvidence {
    Valuation,
    Fill(Observation<CarryFill>),
    Terminal {
        order: CarryOrderId,
        evidence_id: u64,
    },
    FundingBoundary {
        key: u64,
        effective: UtcMicros,
    },
    Funding(Observation<CarryFundingReceipt>),
    MissingSource(CarryOrderId),
    LeggingDeadline,
    EndOfInput,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarrySnapshot {
    pub account: CarryAccountState,
    pub phase: CarryPhase,
    pub pair: Option<CarryPlan>,
    pub orders: Vec<CarryOrderSnapshot>,
    pub fills: Vec<CarryRecordedFill>,
    pub funding: Vec<CarryFundingSnapshot>,
    pub spot_quote_reserved: Cash,
    pub perp_collateral_reserved: Cash,
    pub halted: bool,
    pub epoch: HaltEpoch,
    pub loss_latched: bool,
    pub margin_latched: bool,
    pub unsupported: Option<UnsupportedEvidence>,
    pub reconciled_epoch: Option<HaltEpoch>,
    pub hedged_at: Option<DecisionTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CarryOutcome {
    Planned(CarryOrderId),
    Dispatched(CarryOrderId),
    StateChanged(CarryOrderId),
    FillApplied(CarryRecordedFill),
    TerminalReconciled(CarryOrderId),
    BoundaryCaptured { key: u64, short: Qty },
    FundingApplied { key: u64, amount: Cash },
    Duplicate,
    Waiting,
    PortfolioReconciled,
    Halted(HaltEpoch),
    Resumed(HaltEpoch),
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CarryError {
    InvalidSession,
    InvalidPlan,
    GridMismatch,
    NetQuantityMismatch,
    CapitalEnvelope,
    PriceEnvelope,
    UnknownOrder,
    WrongPair,
    Phase,
    MissingApproval,
    ApprovalExpired,
    Halted,
    EpochMismatch,
    NotReconciled,
    LossLatched,
    Unresolved,
    Capacity,
    FutureObservation,
    StaleMark,
    ClockWentBackwards,
    ReceiptConflict,
    InvalidEvidence,
    IncompleteCut,
    AmbiguousFundingBoundary,
    MissingFundingBoundary,
    UnsupportedFact,
    Transition(TransitionError),
    Accounting(CarryAccountingError),
    Overflow,
}

impl From<CarryAccountingError> for CarryError {
    fn from(value: CarryAccountingError) -> Self {
        Self::Accounting(value)
    }
}
