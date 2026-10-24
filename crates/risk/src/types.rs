use std::fmt;

use parangsae_core::{Amount, Causal, DecisionTime, Price, Qty, Side, Symbol, UtcMicros};

/// Injected monotonic microseconds. This type never reads a clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mono(u64);

impl Mono {
    pub const fn from_micros(value: u64) -> Self {
        Self(value)
    }
    pub const fn micros(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HaltEpoch(u64);

impl HaltEpoch {
    pub const ZERO: Self = Self(0);
    pub const fn get(self) -> u64 {
        self.0
    }
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HaltCause {
    ReconMismatch,
    UnknownOrder,
    Kill,
    Ban,
    RiskTrip,
    IllegalTransition,
    PayloadError,
}

/// The logical intent and the attempt are both part of the immutable binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrderId {
    intent: u128,
    attempt: u8,
}

impl OrderId {
    pub fn new(intent: u128, attempt: u8) -> Result<Self, RiskError> {
        if attempt >= 16 {
            return Err(RiskError::InvalidAttempt);
        }
        Ok(Self { intent, attempt })
    }
    pub const fn intent(self) -> u128 {
        self.intent
    }
    pub const fn attempt(self) -> u8 {
        self.attempt
    }
}

/// A raw request is data. Only an [`crate::ApprovedOrder`] grants dispatch authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderRequest {
    id: OrderId,
    symbol: Symbol,
    side: Side,
    qty: Qty,
    reduce_only: bool,
}

impl OrderRequest {
    pub fn new(
        id: OrderId,
        symbol: Symbol,
        side: Side,
        qty: Qty,
        reduce_only: bool,
    ) -> Result<Self, RiskError> {
        if qty.ticks() == 0 {
            return Err(RiskError::InvalidQuantity);
        }
        Ok(Self {
            id,
            symbol,
            side,
            qty,
            reduce_only,
        })
    }
    pub const fn id(&self) -> OrderId {
        self.id
    }
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }
    pub const fn side(&self) -> Side {
        self.side
    }
    pub const fn qty(&self) -> Qty {
        self.qty
    }
    pub const fn reduce_only(&self) -> bool {
        self.reduce_only
    }
}

/// Explicit session limits. There is no default, day rollover, or automatic reset.
/// `capacity` separately bounds retained order identities, retained unique fill
/// receipts, one settlement batch, one terminal receipt list, and a reconcile cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub(crate) max_order: Qty,
    pub(crate) max_position: Qty,
    pub(crate) max_notional: Amount,
    pub(crate) max_session_loss: Amount,
    pub(crate) max_approval_age_us: u64,
    pub(crate) max_mark_age_us: u64,
    pub(crate) capacity: usize,
}

impl Limits {
    pub fn new(
        max_order: Qty,
        max_position: Qty,
        max_notional: Amount,
        max_session_loss: Amount,
        max_approval_age_us: u64,
        max_mark_age_us: u64,
        capacity: usize,
    ) -> Result<Self, RiskError> {
        if max_order.ticks() == 0
            || max_position.ticks() == 0
            || max_notional.ticks() == 0
            || max_session_loss.ticks() == 0
            || max_approval_age_us == 0
            || max_mark_age_us == 0
            || capacity == 0
        {
            return Err(RiskError::InvalidLimits);
        }
        Ok(Self {
            max_order,
            max_position,
            max_notional,
            max_session_loss,
            max_approval_age_us,
            max_mark_age_us,
            capacity,
        })
    }
    pub const fn max_approval_age_us(self) -> u64 {
        self.max_approval_age_us
    }
    pub const fn max_mark_age_us(self) -> u64 {
        self.max_mark_age_us
    }
    pub const fn max_order(self) -> Qty {
        self.max_order
    }
    pub const fn max_position(self) -> Qty {
        self.max_position
    }
    pub const fn max_notional(self) -> Amount {
        self.max_notional
    }
    pub const fn max_session_loss(self) -> Amount {
        self.max_session_loss
    }
    pub const fn capacity(self) -> usize {
        self.capacity
    }
}

/// Declared initial paper state. The equity is the frozen session loss anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitialPortfolio {
    pub symbol: Symbol,
    pub position_ticks: i64,
    pub equity_ticks: i64,
    pub valuation: Price,
}

/// Current accounting and causal valuation supplied by the owning paper engine.
/// The gate compares `position_ticks` with its own settlement history.
#[derive(Debug)]
pub struct RiskContext<'a> {
    pub now: Mono,
    pub decision: DecisionTime,
    pub mark: Causal<'a, Price>,
    pub position_ticks: i64,
    pub equity_ticks: i64,
}

/// Reported candidate accounting, checked against the gate's own position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortfolioSnapshot {
    pub position_ticks: i64,
    pub equity_ticks: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExposureState {
    Known,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReservationPhase {
    Approved,
    Submitted,
    Unknown,
    Terminal,
}

/// Economic receipt identity. The order attempt is part of the key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FillKey {
    pub order: OrderId,
    pub fill_id: u64,
}

/// A complete priced receipt. Side and symbol come from the bound request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FillReceipt {
    pub key: FillKey,
    pub qty: Qty,
    pub price: Price,
    pub fee: Amount,
    pub at: UtcMicros,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalStatus {
    Filled,
    Canceled,
    Expired,
    Rejected,
}

/// A terminal cut must report all receipt keys and the exact cumulative fill.
/// `complete` is an explicit assertion by the offline paper source. It does not
/// turn a network cancellation acknowledgment into complete fill evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalEvidence {
    pub order: OrderId,
    pub status: TerminalStatus,
    pub cumulative: Qty,
    pub receipts: Vec<FillKey>,
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Settlement {
    Fill(FillReceipt),
    Terminal(TerminalEvidence),
    Unknown(OrderId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconciledOrder {
    pub order: OrderId,
    pub cumulative: Qty,
    pub remaining: Qty,
    pub phase: ReservationPhase,
}

/// One complete paper cut. Reconciliation compares and never adopts state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reconciliation {
    pub position_ticks: i64,
    pub orders: Vec<ReconciledOrder>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExposureEnvelope {
    pub low_ticks: i128,
    pub high_ticks: i128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RiskSnapshot {
    pub position_ticks: i64,
    pub epoch: HaltEpoch,
    pub halted: bool,
    pub loss_latched: bool,
    pub exposure: ExposureState,
    pub envelope: ExposureEnvelope,
    pub version: u64,
    pub orders: Vec<ReconciledOrder>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskError {
    InvalidLimits,
    InvalidQuantity,
    InvalidAttempt,
    InvalidInitialState,
    WrongSymbol,
    ForeignApproval,
    UnknownOrder,
    IdentityConflict,
    AlreadyDispatched,
    TerminalOrder,
    InvalidRetry,
    Capacity,
    Overflow,
    ClockWentBackwards,
    FutureMark,
    StaleMark,
    PositionMismatch,
    ExposureUnknown,
    Halted,
    OrderQuantity,
    PositionLimit,
    NotionalLimit,
    NotStrictlyReducing,
    ExpiredApproval,
    StaleApproval,
    InvalidReceipt,
    ReceiptConflict,
    Overfill,
    IncompleteTerminal,
    TerminalConflict,
    ReconciliationMismatch,
    ResetRequiresReconciliation,
    LossLatched,
    EpochMismatch,
    InstrumentRules,
}

impl fmt::Display for RiskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RiskError {}
