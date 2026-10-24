use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use parangsae_core::instrument_spec::{
    check_paper_order, Decimal, PaperOrder, UtcDateTime, BTCUSDT_RULES, PAPER_BLOCKED,
};
use parangsae_core::{Price, Qty, Side, Symbol, SCALE};

use crate::types::*;

/// Dispatch authority. There is deliberately no Clone, Copy, Default, or parser.
///
/// ```compile_fail
/// let approval = parangsae_risk::ApprovedOrder::default();
/// ```
/// ```compile_fail
/// fn duplicate(a: parangsae_risk::ApprovedOrder) { let _ = a.clone(); }
/// ```
/// ```compile_fail
/// fn change(a: &mut parangsae_risk::ApprovedOrder) { a.epoch = 0; }
/// ```
#[derive(Debug)]
pub struct ApprovedOrder {
    issuer: Arc<()>,
    request: OrderRequest,
    ticket: u64,
    epoch: HaltEpoch,
    issued_at: Mono,
}

impl ApprovedOrder {
    pub fn request(&self) -> &OrderRequest {
        &self.request
    }
    pub const fn halt_epoch(&self) -> HaltEpoch {
        self.epoch
    }
    pub const fn approved_at(&self) -> Mono {
        self.issued_at
    }
}

/// Binding evidence after dispatch, without authority to dispatch again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpentApproval {
    pub request: OrderRequest,
    pub epoch: HaltEpoch,
    pub approved_at: Mono,
    pub dispatched_at: Mono,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchDecision {
    Authorized(SpentApproval),
    Refused {
        request: OrderRequest,
        reason: RiskError,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettlementOutcome {
    Filled(FillReceipt),
    Terminal(TerminalEvidence),
    Unknown(OrderId),
    Duplicate,
}

#[derive(Clone, Debug)]
struct Reservation {
    request: OrderRequest,
    remaining: i64,
    cumulative: i64,
    phase: ReservationPhase,
    ticket: u64,
    terminal: Option<TerminalEvidence>,
}

#[derive(Clone, Debug)]
struct RiskState {
    position: i64,
    equity: i64,
    epoch: HaltEpoch,
    halted: bool,
    loss_latched: bool,
    bound_violation: bool,
    exhausted: bool,
    exposure: ExposureState,
    last_now: Mono,
    last_decision: Option<i64>,
    version: u64,
    next_ticket: u64,
    reconciled_version: Option<u64>,
    orders: BTreeMap<OrderId, Reservation>,
    fills: BTreeMap<FillKey, FillReceipt>,
}

/// Owns the paper portfolio's risk authority. The accounting ledger remains in
/// the owning execution engine; every context must report the same position.
/// No mutable snapshot or public state restoration exists.
#[derive(Debug)]
pub struct RiskGate {
    issuer: Arc<()>,
    limits: Limits,
    symbol: Symbol,
    initial_equity: i64,
    portfolio: RiskState,
}

/// A checked dispatch draft holds the gate exclusively. Dropping the draft
/// cannot dispatch or release anything; its reservation remains conservative.
/// An explicit new check can replace an unconsumed approval after such a drop.
#[must_use]
pub struct PreparedDispatch<'a> {
    gate: &'a mut RiskGate,
    next: RiskState,
    decision: DispatchDecision,
}

impl PreparedDispatch<'_> {
    pub fn decision(&self) -> &DispatchDecision {
        &self.decision
    }
    pub fn commit(self) -> DispatchDecision {
        self.gate.commit_prepared(self.next);
        self.decision
    }
}

/// A checked settlement draft. The engine prepares its ledger and lifecycle
/// candidates before this infallible assignment. A dropped draft changes none
/// of the position, receipt records, or reservations.
#[must_use]
pub struct PreparedSettlement<'a> {
    gate: &'a mut RiskGate,
    next: RiskState,
    outcomes: Vec<SettlementOutcome>,
}

impl PreparedSettlement<'_> {
    pub fn outcomes(&self) -> &[SettlementOutcome] {
        &self.outcomes
    }
    pub fn position_ticks(&self) -> i64 {
        self.next.position
    }
    pub fn commit(self) -> Vec<SettlementOutcome> {
        self.gate.commit_prepared(self.next);
        self.outcomes
    }
}

impl RiskGate {
    pub fn new(limits: Limits, initial: InitialPortfolio, now: Mono) -> Result<Self, RiskError> {
        if initial.symbol.as_str() != "BTCUSDT"
            || initial.equity_ticks <= 0
            || initial.valuation.ticks() == 0
        {
            return Err(RiskError::InvalidInitialState);
        }
        let state = RiskState {
            position: initial.position_ticks,
            equity: initial.equity_ticks,
            epoch: HaltEpoch::ZERO,
            halted: false,
            loss_latched: false,
            bound_violation: false,
            exhausted: false,
            exposure: ExposureState::Known,
            last_now: now,
            last_decision: None,
            version: 0,
            next_ticket: 0,
            reconciled_version: None,
            orders: BTreeMap::new(),
            fills: BTreeMap::new(),
        };
        check_envelope(envelope(&state, None), initial.valuation, limits)
            .map_err(|_| RiskError::InvalidInitialState)?;
        Ok(Self {
            issuer: Arc::new(()),
            limits,
            symbol: initial.symbol,
            initial_equity: initial.equity_ticks,
            portfolio: state,
        })
    }

    pub const fn limits(&self) -> Limits {
        self.limits
    }
    pub const fn halt_epoch(&self) -> HaltEpoch {
        self.portfolio.epoch
    }
    pub const fn is_halted(&self) -> bool {
        self.portfolio.halted || self.portfolio.loss_latched || self.portfolio.exhausted
    }
    pub fn snapshot(&self) -> RiskSnapshot {
        RiskSnapshot {
            position_ticks: self.portfolio.position,
            epoch: self.portfolio.epoch,
            halted: self.is_halted(),
            loss_latched: self.portfolio.loss_latched,
            exposure: self.portfolio.exposure,
            envelope: envelope(&self.portfolio, None),
            version: self.portfolio.version,
            orders: outstanding(&self.portfolio),
        }
    }

    /// Every explicit latch invalidates approvals, including repeated causes.
    /// Epoch exhaustion permanently refuses authority instead of wrapping.
    pub fn latch(&mut self, cause: HaltCause) -> HaltEpoch {
        latch_state(&mut self.portfolio, cause);
        self.portfolio.epoch
    }

    pub fn check(
        &mut self,
        request: OrderRequest,
        cx: &RiskContext<'_>,
    ) -> Result<ApprovedOrder, RiskError> {
        self.observe(cx)?;
        self.validate_request(&request)?;
        check_retry(&self.portfolio, &request, self.limits.capacity)?;
        allows(&self.portfolio, &request, *cx.mark.value(), self.limits)?;
        instrument(&request, cx)?;
        let ticket = self
            .portfolio
            .next_ticket
            .checked_add(1)
            .ok_or(RiskError::Overflow)?;
        let version = self
            .portfolio
            .version
            .checked_add(1)
            .ok_or(RiskError::Overflow)?;
        self.portfolio.next_ticket = ticket;
        self.portfolio.version = version;
        self.portfolio.reconciled_version = None;
        self.portfolio.orders.insert(
            request.id(),
            Reservation {
                remaining: request.qty().ticks(),
                cumulative: 0,
                phase: ReservationPhase::Approved,
                request: request.clone(),
                ticket,
                terminal: None,
            },
        );
        Ok(ApprovedOrder {
            issuer: Arc::clone(&self.issuer),
            request,
            ticket,
            epoch: self.portfolio.epoch,
            issued_at: cx.now,
        })
    }

    /// A foreign token is rejected without touching this gate. A legitimate
    /// unsafe token produces a staged definitive refusal of its unsent order.
    /// The caller can prepare the matching lifecycle refusal before commit.
    pub fn prepare_dispatch(
        &mut self,
        approval: ApprovedOrder,
        cx: &RiskContext<'_>,
    ) -> Result<PreparedDispatch<'_>, RiskError> {
        if !Arc::ptr_eq(&approval.issuer, &self.issuer) {
            return Err(RiskError::ForeignApproval);
        }
        let record = self
            .portfolio
            .orders
            .get(&approval.request.id())
            .ok_or(RiskError::UnknownOrder)?;
        if record.request != approval.request {
            return Err(RiskError::IdentityConflict);
        }
        if record.phase != ReservationPhase::Approved {
            return Err(RiskError::AlreadyDispatched);
        }
        if record.ticket != approval.ticket {
            return Err(RiskError::StaleApproval);
        }
        let mut next = self.portfolio.clone();
        let observed = observe_state(&mut next, self.limits, self.initial_equity, cx);
        let checked = observed.and_then(|()| {
            if approval.epoch != next.epoch {
                return Err(RiskError::StaleApproval);
            }
            let age = cx
                .now
                .micros()
                .checked_sub(approval.issued_at.micros())
                .ok_or(RiskError::ClockWentBackwards)?;
            if age > self.limits.max_approval_age_us {
                return Err(RiskError::ExpiredApproval);
            }
            allows(&next, &approval.request, *cx.mark.value(), self.limits)?;
            instrument(&approval.request, cx)
        });
        let record = next
            .orders
            .get_mut(&approval.request.id())
            .ok_or(RiskError::UnknownOrder)?;
        let decision = match checked {
            Ok(()) => {
                record.phase = ReservationPhase::Submitted;
                DispatchDecision::Authorized(SpentApproval {
                    request: approval.request,
                    epoch: next.epoch,
                    approved_at: approval.issued_at,
                    dispatched_at: cx.now,
                })
            }
            Err(reason) => {
                record.phase = ReservationPhase::Terminal;
                record.remaining = 0;
                DispatchDecision::Refused {
                    request: approval.request,
                    reason,
                }
            }
        };
        bump_version(&mut next)?;
        Ok(PreparedDispatch {
            gate: self,
            next,
            decision,
        })
    }

    /// Validate priced fills, complete terminal cuts, or Unknown entry. Errors
    /// latch the gate but never change economic state or release a reservation.
    pub fn prepare_settlement(
        &mut self,
        settlements: &[Settlement],
        cx: &RiskContext<'_>,
        resulting: PortfolioSnapshot,
    ) -> Result<PreparedSettlement<'_>, RiskError> {
        let mut next = self.portfolio.clone();
        let result = (|| {
            observe_state(&mut next, self.limits, self.initial_equity, cx)?;
            let mut outcomes = Vec::with_capacity(settlements.len());
            for settlement in settlements {
                outcomes.push(prepare_settlement_state(
                    &mut next,
                    settlement.clone(),
                    self.limits,
                    cx,
                )?);
            }
            if next.position != resulting.position_ticks {
                return Err(RiskError::PositionMismatch);
            }
            observe_limits(
                &mut next,
                self.limits,
                self.initial_equity,
                *cx.mark.value(),
                resulting.equity_ticks,
            );
            if outcomes
                .iter()
                .any(|outcome| *outcome != SettlementOutcome::Duplicate)
            {
                bump_version(&mut next)?;
            }
            Ok(outcomes)
        })();
        match result {
            Ok(outcomes) => Ok(PreparedSettlement {
                gate: self,
                next,
                outcomes,
            }),
            Err(error) => {
                let cause = if error == RiskError::IncompleteTerminal {
                    HaltCause::UnknownOrder
                } else {
                    HaltCause::PayloadError
                };
                self.latch(cause);
                Err(error)
            }
        }
    }

    /// Record a causal mark and current equity. A configured limit breach
    /// latches risk; it is not a promise that future fills cannot lose more.
    pub fn observe(&mut self, cx: &RiskContext<'_>) -> Result<(), RiskError> {
        let result = observe_state(&mut self.portfolio, self.limits, self.initial_equity, cx);
        if let Err(error) = result {
            let cause = if error == RiskError::PositionMismatch {
                HaltCause::ReconMismatch
            } else {
                HaltCause::PayloadError
            };
            self.latch(cause);
        }
        result
    }

    /// A complete matching paper cut establishes known exposure, never repairs
    /// portfolio state, and never clears a latch.
    pub fn reconcile(
        &mut self,
        cut: &Reconciliation,
        cx: &RiskContext<'_>,
    ) -> Result<(), RiskError> {
        self.observe(cx)?;
        let mut given = cut.orders.clone();
        given.sort_by_key(|row| row.order);
        if cut.position_ticks != self.portfolio.position
            || given != outstanding(&self.portfolio)
            || given
                .iter()
                .any(|row| row.phase == ReservationPhase::Unknown)
        {
            self.latch(HaltCause::ReconMismatch);
            return Err(RiskError::ReconciliationMismatch);
        }
        self.portfolio.exposure = ExposureState::Known;
        bump_version(&mut self.portfolio)?;
        self.portfolio.reconciled_version = Some(self.portfolio.version);
        Ok(())
    }

    /// Explicit resume after a complete matching cut. Session loss stays latched.
    pub fn resume(&mut self, epoch: HaltEpoch, cx: &RiskContext<'_>) -> Result<(), RiskError> {
        if epoch != self.portfolio.epoch {
            return Err(RiskError::EpochMismatch);
        }
        if self.portfolio.exposure != ExposureState::Known
            || self.portfolio.reconciled_version != Some(self.portfolio.version)
        {
            return Err(RiskError::ResetRequiresReconciliation);
        }
        self.observe(cx)?;
        if epoch != self.portfolio.epoch {
            return Err(RiskError::EpochMismatch);
        }
        if self.portfolio.loss_latched {
            return Err(RiskError::LossLatched);
        }
        if self.portfolio.bound_violation || self.portfolio.exhausted {
            return Err(RiskError::Halted);
        }
        self.portfolio.halted = false;
        bump_version(&mut self.portfolio)?;
        Ok(())
    }

    fn commit_prepared(&mut self, next: RiskState) {
        self.portfolio = next;
    }

    fn validate_request(&self, request: &OrderRequest) -> Result<(), RiskError> {
        if request.symbol() != &self.symbol {
            return Err(RiskError::WrongSymbol);
        }
        Ok(())
    }
}

fn latch_state(state: &mut RiskState, cause: HaltCause) {
    state.halted = true;
    state.reconciled_version = None;
    match state.epoch.checked_next() {
        Some(epoch) => state.epoch = epoch,
        None => state.exhausted = true,
    }
    match cause {
        HaltCause::ReconMismatch
        | HaltCause::UnknownOrder
        | HaltCause::IllegalTransition
        | HaltCause::PayloadError => state.exposure = ExposureState::Unknown,
        HaltCause::Kill | HaltCause::Ban | HaltCause::RiskTrip => {}
    }
    match state.version.checked_add(1) {
        Some(version) => state.version = version,
        None => state.exhausted = true,
    }
}

fn bump_version(state: &mut RiskState) -> Result<(), RiskError> {
    let version = state.version.checked_add(1).ok_or(RiskError::Overflow)?;
    state.version = version;
    state.reconciled_version = None;
    Ok(())
}

fn envelope(state: &RiskState, except: Option<OrderId>) -> ExposureEnvelope {
    let mut low = i128::from(state.position);
    let mut high = low;
    for (id, record) in &state.orders {
        if Some(*id) == except || record.phase == ReservationPhase::Terminal {
            continue;
        }
        match record.request.side() {
            Side::Buy => high += i128::from(record.remaining),
            Side::Sell => low -= i128::from(record.remaining),
        }
    }
    ExposureEnvelope {
        low_ticks: low,
        high_ticks: high,
    }
}

/// Upper bounds round upward, unlike the core's truncating notional function.
pub fn upper_notional(position_ticks: i128, price: Price) -> Result<i128, RiskError> {
    let magnitude = position_ticks.checked_abs().ok_or(RiskError::Overflow)?;
    let product = magnitude
        .checked_mul(i128::from(price.ticks()))
        .ok_or(RiskError::Overflow)?;
    let quotient = product / SCALE;
    quotient
        .checked_add(i128::from(product % SCALE != 0))
        .ok_or(RiskError::Overflow)
}

fn check_envelope(bounds: ExposureEnvelope, price: Price, limits: Limits) -> Result<(), RiskError> {
    if price.ticks() == 0 {
        return Err(RiskError::InvalidInitialState);
    }
    let low = bounds.low_ticks.checked_abs().ok_or(RiskError::Overflow)?;
    let high = bounds.high_ticks.checked_abs().ok_or(RiskError::Overflow)?;
    if low > i128::from(limits.max_position.ticks())
        || high > i128::from(limits.max_position.ticks())
    {
        return Err(RiskError::PositionLimit);
    }
    if upper_notional(bounds.low_ticks, price)? > i128::from(limits.max_notional.ticks())
        || upper_notional(bounds.high_ticks, price)? > i128::from(limits.max_notional.ticks())
    {
        return Err(RiskError::NotionalLimit);
    }
    Ok(())
}

fn allows(
    state: &RiskState,
    request: &OrderRequest,
    mark: Price,
    limits: Limits,
) -> Result<(), RiskError> {
    if state.exhausted {
        return Err(RiskError::Overflow);
    }
    if state.exposure != ExposureState::Known {
        return Err(RiskError::ExposureUnknown);
    }
    if request.qty().ticks() > limits.max_order.ticks() {
        return Err(RiskError::OrderQuantity);
    }
    let mut bounds = envelope(state, Some(request.id()));
    let qty = i128::from(request.qty().ticks());
    let strict = match request.side() {
        Side::Buy => bounds.high_ticks < 0 && bounds.high_ticks + qty <= 0,
        Side::Sell => bounds.low_ticks > 0 && bounds.low_ticks - qty >= 0,
    };
    if request.reduce_only() && !strict {
        return Err(RiskError::NotStrictlyReducing);
    }
    if (state.halted || state.loss_latched) && !request.reduce_only() {
        return Err(RiskError::Halted);
    }
    // Known, strict protection cannot worsen any outstanding fill ordering.
    // A moving mark may already breach the cap; refusing every close would
    // preserve that breach. The existing latch is deliberately retained.
    if request.reduce_only() {
        return Ok(());
    }
    match request.side() {
        Side::Buy => bounds.high_ticks += qty,
        Side::Sell => bounds.low_ticks -= qty,
    }
    check_envelope(bounds, mark, limits)
}

fn instrument(request: &OrderRequest, cx: &RiskContext<'_>) -> Result<(), RiskError> {
    check_paper_order(
        BTCUSDT_RULES,
        PAPER_BLOCKED,
        &PaperOrder {
            symbol: request.symbol().as_str(),
            at: UtcDateTime::from_micros(cx.decision.micros()),
            price: Decimal::from_ticks(cx.mark.value().ticks()),
            qty: Decimal::from_ticks(request.qty().ticks()),
            market: true,
            reduce_only: request.reduce_only(),
        },
    )
    .map_err(|_| RiskError::InstrumentRules)
}

fn check_retry(
    state: &RiskState,
    request: &OrderRequest,
    capacity: usize,
) -> Result<(), RiskError> {
    if let Some(current) = state.orders.get(&request.id()) {
        if current.request != *request {
            return Err(RiskError::IdentityConflict);
        }
        return match current.phase {
            ReservationPhase::Approved => Ok(()),
            ReservationPhase::Submitted | ReservationPhase::Unknown => {
                Err(RiskError::AlreadyDispatched)
            }
            ReservationPhase::Terminal => Err(RiskError::TerminalOrder),
        };
    }
    if state.orders.len() >= capacity {
        return Err(RiskError::Capacity);
    }
    let previous: Vec<_> = state
        .orders
        .values()
        .filter(|row| row.request.id().intent() == request.id().intent())
        .collect();
    let Some(first) = previous.iter().min_by_key(|row| row.request.id().attempt()) else {
        return if request.id().attempt() == 0 {
            Ok(())
        } else {
            Err(RiskError::InvalidRetry)
        };
    };
    let max_attempt = previous
        .iter()
        .map(|row| row.request.id().attempt())
        .max()
        .ok_or(RiskError::InvalidRetry)?;
    let cumulative: i128 = previous.iter().map(|row| i128::from(row.cumulative)).sum();
    let expected = i128::from(first.request.qty().ticks()) - cumulative;
    if previous
        .iter()
        .any(|row| row.phase != ReservationPhase::Terminal)
        || request.id().attempt() != max_attempt.checked_add(1).ok_or(RiskError::Overflow)?
        || request.side() != first.request.side()
        || request.symbol() != first.request.symbol()
        || request.reduce_only() != first.request.reduce_only()
        || i128::from(request.qty().ticks()) != expected
    {
        return Err(RiskError::InvalidRetry);
    }
    Ok(())
}

fn observe_state(
    state: &mut RiskState,
    limits: Limits,
    initial_equity: i64,
    cx: &RiskContext<'_>,
) -> Result<(), RiskError> {
    if cx.now < state.last_now
        || state
            .last_decision
            .is_some_and(|at| cx.decision.micros().micros() < at)
    {
        return Err(RiskError::ClockWentBackwards);
    }
    let mark_at = cx.mark.available_ts().micros().micros();
    let decision = cx.decision.micros().micros();
    let age = i128::from(decision) - i128::from(mark_at);
    if age < 0 {
        return Err(RiskError::FutureMark);
    }
    if age > i128::from(limits.max_mark_age_us) {
        return Err(RiskError::StaleMark);
    }
    if cx.mark.value().ticks() == 0 {
        return Err(RiskError::InvalidInitialState);
    }
    if cx.position_ticks != state.position {
        return Err(RiskError::PositionMismatch);
    }
    state.last_now = cx.now;
    state.last_decision = Some(decision);
    observe_limits(
        state,
        limits,
        initial_equity,
        *cx.mark.value(),
        cx.equity_ticks,
    );
    Ok(())
}

fn observe_limits(
    state: &mut RiskState,
    limits: Limits,
    initial_equity: i64,
    mark: Price,
    equity: i64,
) {
    state.equity = equity;
    let loss = i128::from(initial_equity) - i128::from(equity);
    if loss >= i128::from(limits.max_session_loss.ticks()) && !state.loss_latched {
        state.loss_latched = true;
        latch_state(state, HaltCause::RiskTrip);
    }
    let breached = check_envelope(envelope(state, None), mark, limits).is_err();
    if breached && !state.bound_violation {
        latch_state(state, HaltCause::RiskTrip);
    }
    state.bound_violation = breached;
}

fn outstanding(state: &RiskState) -> Vec<ReconciledOrder> {
    state
        .orders
        .iter()
        .filter(|(_, row)| row.phase != ReservationPhase::Terminal)
        .map(|(id, row)| ReconciledOrder {
            order: *id,
            cumulative: Qty::from_ticks(row.cumulative).expect("checked nonnegative cumulative"),
            remaining: Qty::from_ticks(row.remaining).expect("checked nonnegative remaining"),
            phase: row.phase,
        })
        .collect()
}

fn prepare_settlement_state(
    state: &mut RiskState,
    settlement: Settlement,
    limits: Limits,
    cx: &RiskContext<'_>,
) -> Result<SettlementOutcome, RiskError> {
    if let Settlement::Fill(fill) = &settlement {
        if let Some(old) = state.fills.get(&fill.key) {
            return if old == fill {
                Ok(SettlementOutcome::Duplicate)
            } else {
                Err(RiskError::ReceiptConflict)
            };
        }
    }
    if let Settlement::Terminal(cut) = &settlement {
        if let Some(old) = state
            .orders
            .get(&cut.order)
            .and_then(|row| row.terminal.as_ref())
        {
            return if old == cut {
                Ok(SettlementOutcome::Duplicate)
            } else {
                Err(RiskError::TerminalConflict)
            };
        }
    }
    let outcome = match settlement {
        Settlement::Fill(fill) => {
            if fill.key.fill_id == 0
                || fill.qty.ticks() == 0
                || fill.price.ticks() == 0
                || fill.at > cx.decision.micros()
            {
                return Err(RiskError::InvalidReceipt);
            }
            if state.fills.len() >= limits.capacity {
                return Err(RiskError::Capacity);
            }
            let row = state
                .orders
                .get_mut(&fill.key.order)
                .ok_or(RiskError::UnknownOrder)?;
            match row.phase {
                ReservationPhase::Submitted | ReservationPhase::Unknown => {}
                ReservationPhase::Approved => return Err(RiskError::AlreadyDispatched),
                ReservationPhase::Terminal => return Err(RiskError::TerminalConflict),
            }
            let remaining = row
                .remaining
                .checked_sub(fill.qty.ticks())
                .ok_or(RiskError::Overflow)?;
            if remaining < 0 {
                return Err(RiskError::Overfill);
            }
            let cumulative = row
                .cumulative
                .checked_add(fill.qty.ticks())
                .ok_or(RiskError::Overflow)?;
            let position = match row.request.side() {
                Side::Buy => state.position.checked_add(fill.qty.ticks()),
                Side::Sell => state.position.checked_sub(fill.qty.ticks()),
            }
            .ok_or(RiskError::Overflow)?;
            row.remaining = remaining;
            row.cumulative = cumulative;
            state.position = position;
            state.fills.insert(fill.key, fill.clone());
            SettlementOutcome::Filled(fill)
        }
        Settlement::Terminal(cut) => {
            let row = state
                .orders
                .get(&cut.order)
                .ok_or(RiskError::UnknownOrder)?;
            let wanted: BTreeSet<_> = state
                .fills
                .keys()
                .filter(|key| key.order == cut.order)
                .copied()
                .collect();
            let given: BTreeSet<_> = cut.receipts.iter().copied().collect();
            if !cut.complete
                || wanted != given
                || given.len() != cut.receipts.len()
                || cut.cumulative.ticks() != row.cumulative
            {
                return Err(RiskError::IncompleteTerminal);
            }
            if row.phase == ReservationPhase::Terminal {
                return Err(RiskError::TerminalConflict);
            }
            match cut.status {
                TerminalStatus::Filled if row.remaining != 0 => {
                    return Err(RiskError::IncompleteTerminal)
                }
                TerminalStatus::Rejected if row.cumulative != 0 => {
                    return Err(RiskError::TerminalConflict)
                }
                TerminalStatus::Filled
                | TerminalStatus::Canceled
                | TerminalStatus::Expired
                | TerminalStatus::Rejected => {}
            }
            let row = state
                .orders
                .get_mut(&cut.order)
                .ok_or(RiskError::UnknownOrder)?;
            row.remaining = 0;
            row.phase = ReservationPhase::Terminal;
            row.terminal = Some(cut.clone());
            SettlementOutcome::Terminal(cut)
        }
        Settlement::Unknown(id) => {
            let row = state.orders.get_mut(&id).ok_or(RiskError::UnknownOrder)?;
            match row.phase {
                ReservationPhase::Submitted => row.phase = ReservationPhase::Unknown,
                ReservationPhase::Unknown => return Ok(SettlementOutcome::Duplicate),
                ReservationPhase::Approved | ReservationPhase::Terminal => {
                    return Err(RiskError::TerminalConflict)
                }
            }
            latch_state(state, HaltCause::UnknownOrder);
            SettlementOutcome::Unknown(id)
        }
    };
    Ok(outcome)
}
