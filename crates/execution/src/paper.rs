//! Pure offline execution through the actual risk authority and fixed-point ledger.

use crate::paper_event::{
    PaperAction, PaperCommand, PaperFill, PaperOrderSnapshot, PaperReport, PaperTerminal,
    PaperTerminalCut,
};
use crate::{
    transition, CellContext, ClientOrderId, ExecutionConfig, HaltCause, HaltEpoch, Mono,
    OrderProgress, OrderState, ProgressError, TransitionError, ValidatedEvent,
};
use parangsae_core::{Amount, DecisionTime, Observation, Price, Qty, Symbol};
use parangsae_risk::{
    ApprovedOrder, DispatchDecision, FillKey, FillReceipt, InitialPortfolio, Limits, OrderId,
    OrderRequest, PortfolioSnapshot, ReconciledOrder, Reconciliation, ReservationPhase,
    RiskContext, RiskError, RiskGate, RiskSnapshot, Settlement, TerminalEvidence, TerminalStatus,
};
use parangsae_sim::{Cash, Costs, Fill, Ledger, MarketOrder, PositionQty, SimError};
use std::collections::{BTreeMap, BTreeSet};

mod session_wire;

/// Explicit starting state. This first paper scope starts with one flat symbol.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "session_wire::Session", into = "session_wire::Session")]
pub struct PaperSession {
    pub symbol: Symbol,
    pub initial_cash: Cash,
    pub initial_equity: Cash,
    pub initial_mark: Observation<Price>,
    pub initial_decision: DecisionTime,
    pub initial_now: Mono,
    pub costs: Costs,
    pub limits: Limits,
    pub execution: ExecutionConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaperSnapshot {
    pub cash: Cash,
    pub position: PositionQty,
    pub fees_paid: Cash,
    pub funding_net: Cash,
    pub risk: RiskSnapshot,
    pub orders: Vec<PaperOrderSnapshot>,
    pub fills: Vec<Fill>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaperOutcome {
    Planned(ClientOrderId),
    Dispatched(ClientOrderId),
    Refused {
        client_order_id: ClientOrderId,
        reason: RiskError,
    },
    StateChanged {
        client_order_id: ClientOrderId,
        state: OrderState,
    },
    FillApplied(Fill),
    DuplicateReceipt,
    TerminalReconciled(ClientOrderId),
    PortfolioReconciled,
    Halted(HaltEpoch),
    Resumed(HaltEpoch),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaperError {
    InvalidSession,
    ClockWentBackwards,
    FutureReport,
    InvalidEvidence,
    ReceiptConflict,
    IncompleteCut,
    ExpiredReceiptRecoveryUnsupported,
    UnknownOrder,
    NotDispatched,
    AlreadyPlanned,
    MissingApproval,
    Capacity,
    Risk(RiskError),
    Sim(SimError),
    Progress(ProgressError),
    Transition(TransitionError),
}
impl std::fmt::Display for PaperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for PaperError {}

#[derive(Debug)]
struct TrackedOrder {
    request: OrderRequest,
    approval: Option<ApprovedOrder>,
    progress: OrderProgress,
    terminal: Option<TerminalEvidence>,
    definitive_refusal: bool,
    dispatch_decision: Option<DecisionTime>,
}
#[derive(Clone, Debug)]
struct RecordedFill {
    source: Observation<PaperFill>,
    receipt: FillReceipt,
    accounted: Fill,
}

/// Owns the gate, ledger, approvals and complete bounded receipt history.
/// No mutable authority state or public successful venue is exposed.
///
/// ```compile_fail
/// fn bypass(engine: &mut parangsae_execution::PaperEngine) {
///     engine.ledger = parangsae_sim::Ledger::new(parangsae_sim::Cash(1));
/// }
/// ```
#[derive(Debug)]
pub struct PaperEngine {
    session: PaperSession,
    risk: RiskGate,
    ledger: Ledger,
    orders: BTreeMap<String, TrackedOrder>,
    fills: BTreeMap<FillKey, RecordedFill>,
    last_now: Mono,
    last_decision: DecisionTime,
}

impl PaperEngine {
    pub fn new(session: PaperSession) -> Result<Self, PaperError> {
        if session.initial_cash.0 <= 0
            || session.initial_cash != session.initial_equity
            || session.execution.timing().max_approval_age().get()
                != session.limits.max_approval_age_us()
        {
            return Err(PaperError::InvalidSession);
        }
        let mark = session
            .initial_mark
            .causal_at(session.initial_decision)
            .ok_or(PaperError::Risk(RiskError::FutureMark))?;
        let ledger = Ledger::new(session.initial_cash);
        let risk = RiskGate::new(
            session.limits,
            InitialPortfolio {
                symbol: session.symbol.clone(),
                position_ticks: 0,
                equity_ticks: session.initial_equity.0,
                valuation: *mark.value(),
            },
            session.initial_now,
        )
        .map_err(PaperError::Risk)?;
        let last_now = session.initial_now;
        let last_decision = session.initial_decision;
        Ok(Self {
            session,
            risk,
            ledger,
            orders: BTreeMap::new(),
            fills: BTreeMap::new(),
            last_now,
            last_decision,
        })
    }

    pub fn snapshot(&self) -> PaperSnapshot {
        PaperSnapshot {
            cash: self.ledger.cash(),
            position: self.ledger.position(),
            fees_paid: self.ledger.fees_paid(),
            funding_net: self.ledger.funding_net(),
            risk: self.risk.snapshot(),
            orders: self
                .orders
                .iter()
                .map(|(text, order)| PaperOrderSnapshot {
                    client_order_id: ClientOrderId::parse(text)
                        .expect("engine stores canonical ids"),
                    progress: order.progress.clone(),
                })
                .collect(),
            fills: self.fills.values().map(|fill| fill.accounted).collect(),
        }
    }

    pub fn step(&mut self, command: &PaperCommand) -> Result<PaperOutcome, PaperError> {
        if command.now < self.last_now || command.decision < self.last_decision {
            self.risk.latch(HaltCause::PayloadError);
            return Err(PaperError::ClockWentBackwards);
        }
        self.last_now = command.now;
        self.last_decision = command.decision;
        let mark = match command.mark.causal_at(command.decision) {
            Some(mark) if mark.value().ticks() > 0 => mark,
            _ => {
                self.risk.latch(HaltCause::PayloadError);
                return Err(PaperError::Risk(RiskError::FutureMark));
            }
        };
        let equity = match self.ledger.equity(*mark.value()) {
            Ok(equity) => equity,
            Err(error) => {
                self.risk.latch(HaltCause::PayloadError);
                return Err(PaperError::Sim(error));
            }
        };
        let cx = RiskContext {
            now: command.now,
            decision: command.decision,
            mark,
            position_ticks: self.ledger.position().0,
            equity_ticks: equity.0,
        };
        let result = self.apply(&command.action, &cx);
        if matches!(
            &result,
            Err(PaperError::FutureReport
                | PaperError::InvalidEvidence
                | PaperError::ReceiptConflict
                | PaperError::IncompleteCut
                | PaperError::ExpiredReceiptRecoveryUnsupported
                | PaperError::UnknownOrder
                | PaperError::NotDispatched
                | PaperError::Capacity
                | PaperError::Sim(_)
                | PaperError::Progress(_)
                | PaperError::Transition(_))
        ) {
            self.risk.latch(HaltCause::PayloadError);
        }
        result
    }

    fn apply(
        &mut self,
        action: &PaperAction,
        cx: &RiskContext<'_>,
    ) -> Result<PaperOutcome, PaperError> {
        match action {
            PaperAction::Plan {
                client_order_id,
                symbol,
                side,
                qty,
                reduce_only,
            } => {
                if self.orders.contains_key(client_order_id.as_str()) {
                    return Err(PaperError::AlreadyPlanned);
                }
                let progress = OrderProgress::new_pending_attempt(
                    *qty,
                    client_order_id.attempt(),
                    self.session.execution.max_attempts(),
                )
                .map_err(PaperError::Progress)?;
                let request = OrderRequest::new(
                    order_id(client_order_id)?,
                    symbol.clone(),
                    *side,
                    *qty,
                    *reduce_only,
                )
                .map_err(PaperError::Risk)?;
                let approval = self
                    .risk
                    .check(request.clone(), cx)
                    .map_err(PaperError::Risk)?;
                self.orders.insert(
                    client_order_id.to_string(),
                    TrackedOrder {
                        request,
                        approval: Some(approval),
                        progress,
                        terminal: None,
                        definitive_refusal: false,
                        dispatch_decision: None,
                    },
                );
                Ok(PaperOutcome::Planned(client_order_id.clone()))
            }
            PaperAction::Dispatch { client_order_id } => self.dispatch(client_order_id, cx),
            PaperAction::Cancel { client_order_id } => {
                self.change(client_order_id, ValidatedEvent::CancelRequested, cx)
            }
            PaperAction::Deliver(report) => {
                visible(report, cx)?;
                match report.value() {
                    PaperReport::Fill(fill) => self.fill(report.with_value(fill.clone()), cx),
                    PaperReport::Ack { client_order_id } => {
                        self.change(client_order_id, ValidatedEvent::Ack, cx)
                    }
                    PaperReport::CancelConfirmed { client_order_id } => {
                        self.change(client_order_id, ValidatedEvent::CancelConfirmed, cx)
                    }
                    PaperReport::CancelDenied { client_order_id } => {
                        self.change(client_order_id, ValidatedEvent::CancelDenied, cx)
                    }
                    PaperReport::Expired { client_order_id } => {
                        self.change(client_order_id, ValidatedEvent::Expire, cx)
                    }
                    PaperReport::Rejected { client_order_id } => {
                        self.change(client_order_id, ValidatedEvent::Reject, cx)
                    }
                }
            }
            PaperAction::Timeout { client_order_id } => self.timeout(client_order_id, cx),
            PaperAction::Reconcile(cut) => self.terminal(cut, cx),
            PaperAction::ReconcilePortfolio => {
                let orders = self
                    .orders
                    .values()
                    .filter(|order| !finalized(order))
                    .map(|order| {
                        let cumulative = order.progress.cum();
                        let remaining =
                            Qty::from_ticks(order.request.qty().ticks() - cumulative.ticks())
                                .expect("validated cumulative quantity");
                        let phase = if order.progress.state() == OrderState::Unknown {
                            ReservationPhase::Unknown
                        } else if order.progress.sent() {
                            ReservationPhase::Submitted
                        } else {
                            ReservationPhase::Approved
                        };
                        ReconciledOrder {
                            order: order.request.id(),
                            cumulative,
                            remaining,
                            phase,
                        }
                    })
                    .collect();
                self.risk
                    .reconcile(
                        &Reconciliation {
                            position_ticks: self.ledger.position().0,
                            orders,
                        },
                        cx,
                    )
                    .map_err(PaperError::Risk)?;
                Ok(PaperOutcome::PortfolioReconciled)
            }
            PaperAction::Halt(cause) => Ok(PaperOutcome::Halted(self.risk.latch(*cause))),
            PaperAction::Resume => {
                let epoch = self.risk.snapshot().epoch;
                self.risk.resume(epoch, cx).map_err(PaperError::Risk)?;
                Ok(PaperOutcome::Resumed(epoch))
            }
        }
    }

    fn dispatch(
        &mut self,
        id: &ClientOrderId,
        cx: &RiskContext<'_>,
    ) -> Result<PaperOutcome, PaperError> {
        let order = self
            .orders
            .get_mut(id.as_str())
            .ok_or(PaperError::UnknownOrder)?;
        let sent = order
            .progress
            .record_dispatch(cx.now)
            .map_err(PaperError::Transition)?;
        let refused = transition(
            &order.progress,
            &ValidatedEvent::Reject,
            &cell(&self.session, cx),
        )
        .map_err(PaperError::Transition)?;
        let approval = order.approval.take().ok_or(PaperError::MissingApproval)?;
        let draft = self
            .risk
            .prepare_dispatch(approval, cx)
            .map_err(PaperError::Risk)?;
        match draft.commit() {
            DispatchDecision::Authorized(_) => {
                order.progress = sent;
                order.dispatch_decision = Some(cx.decision);
                Ok(PaperOutcome::Dispatched(id.clone()))
            }
            DispatchDecision::Refused { reason, .. } => {
                order.progress = refused;
                order.definitive_refusal = true;
                Ok(PaperOutcome::Refused {
                    client_order_id: id.clone(),
                    reason,
                })
            }
        }
    }

    fn change(
        &mut self,
        id: &ClientOrderId,
        event: ValidatedEvent,
        cx: &RiskContext<'_>,
    ) -> Result<PaperOutcome, PaperError> {
        let order = self
            .orders
            .get_mut(id.as_str())
            .ok_or(PaperError::UnknownOrder)?;
        if finalized(order) {
            return Err(PaperError::NotDispatched);
        }
        let next = transition(&order.progress, &event, &cell(&self.session, cx))
            .map_err(PaperError::Transition)?;
        if !order.progress.sent() || event == ValidatedEvent::Reject {
            if !order.progress.sent() && event != ValidatedEvent::CancelRequested {
                return Err(PaperError::NotDispatched);
            }
            let evidence = TerminalEvidence {
                order: order.request.id(),
                status: if event == ValidatedEvent::Reject {
                    TerminalStatus::Rejected
                } else {
                    TerminalStatus::Canceled
                },
                cumulative: Qty::from_ticks(0).expect("zero quantity"),
                receipts: Vec::new(),
                complete: true,
            };
            let draft = self
                .risk
                .prepare_settlement(
                    &[Settlement::Terminal(evidence.clone())],
                    cx,
                    PortfolioSnapshot {
                        position_ticks: cx.position_ticks,
                        equity_ticks: cx.equity_ticks,
                    },
                )
                .map_err(PaperError::Risk)?;
            draft.commit();
            order.approval = None;
            order.progress = next;
            order.terminal = Some(evidence);
            return Ok(PaperOutcome::StateChanged {
                client_order_id: id.clone(),
                state: order.progress.state(),
            });
        }
        self.risk.observe(cx).map_err(PaperError::Risk)?;
        order.progress = next;
        Ok(PaperOutcome::StateChanged {
            client_order_id: id.clone(),
            state: order.progress.state(),
        })
    }

    fn timeout(
        &mut self,
        id: &ClientOrderId,
        cx: &RiskContext<'_>,
    ) -> Result<PaperOutcome, PaperError> {
        let order = self
            .orders
            .get_mut(id.as_str())
            .ok_or(PaperError::UnknownOrder)?;
        if !order.progress.sent() || finalized(order) {
            return Err(PaperError::NotDispatched);
        }
        let next = transition(
            &order.progress,
            &ValidatedEvent::Timeout,
            &cell(&self.session, cx),
        )
        .map_err(PaperError::Transition)?;
        let draft = self
            .risk
            .prepare_settlement(
                &[Settlement::Unknown(order.request.id())],
                cx,
                PortfolioSnapshot {
                    position_ticks: cx.position_ticks,
                    equity_ticks: cx.equity_ticks,
                },
            )
            .map_err(PaperError::Risk)?;
        draft.commit();
        order.progress = next;
        Ok(PaperOutcome::StateChanged {
            client_order_id: id.clone(),
            state: OrderState::Unknown,
        })
    }

    fn fill(
        &mut self,
        source: Observation<PaperFill>,
        cx: &RiskContext<'_>,
    ) -> Result<PaperOutcome, PaperError> {
        let id = &source.value().client_order_id;
        let order = self
            .orders
            .get(id.as_str())
            .ok_or(PaperError::UnknownOrder)?;
        let key = FillKey {
            order: order.request.id(),
            fill_id: source.value().fill_id,
        };
        if let Some(old) = self.fills.get(&key) {
            return if same_source(&old.source, &source) {
                Ok(PaperOutcome::DuplicateReceipt)
            } else {
                Err(PaperError::ReceiptConflict)
            };
        }
        if !order.progress.sent() || finalized(order) {
            return Err(PaperError::NotDispatched);
        }
        let event = if order.progress.state() == OrderState::Canceled {
            ValidatedEvent::RaceFill {
                qty: source.value().qty,
                fill_id: source.value().fill_id,
            }
        } else {
            ValidatedEvent::Fill {
                qty: source.value().qty,
                fill_id: source.value().fill_id,
            }
        };
        let progress = transition(&order.progress, &event, &cell(&self.session, cx))
            .map_err(PaperError::Transition)?;
        let mut ledger = self.ledger.clone();
        let recorded = account(
            &mut ledger,
            &order.request,
            order.dispatch_decision,
            &source,
            &self.session.costs,
            cx,
        )?;
        if self.fills.len() >= self.session.limits.capacity() {
            return Err(PaperError::Capacity);
        }
        let resulting = portfolio(&ledger, *cx.mark.value())?;
        let draft = self
            .risk
            .prepare_settlement(&[Settlement::Fill(recorded.receipt.clone())], cx, resulting)
            .map_err(PaperError::Risk)?;
        draft.commit();
        self.ledger = ledger;
        self.orders
            .get_mut(id.as_str())
            .expect("order remained present")
            .progress = progress;
        let fill = recorded.accounted;
        self.fills.insert(key, recorded);
        Ok(PaperOutcome::FillApplied(fill))
    }

    fn terminal(
        &mut self,
        observed: &Observation<PaperTerminalCut>,
        cx: &RiskContext<'_>,
    ) -> Result<PaperOutcome, PaperError> {
        visible(observed, cx)?;
        let cut = observed.value();
        if !cut.complete || cut.evidence_id == 0 {
            return Err(PaperError::IncompleteCut);
        }
        if cut.receipts.len() > self.session.limits.capacity() {
            return Err(PaperError::Capacity);
        }
        let id = &cut.client_order_id;
        let order = self
            .orders
            .get(id.as_str())
            .ok_or(PaperError::UnknownOrder)?;
        let mut keys = BTreeSet::new();
        let mut cumulative = 0i64;
        for source in &cut.receipts {
            visible(source, cx)?;
            if source.value().client_order_id != *id || source.value().fill_id == 0 {
                return Err(PaperError::InvalidEvidence);
            }
            let key = FillKey {
                order: order.request.id(),
                fill_id: source.value().fill_id,
            };
            if !keys.insert(key) {
                return Err(PaperError::InvalidEvidence);
            }
            cumulative = cumulative
                .checked_add(source.value().qty.ticks())
                .ok_or(PaperError::InvalidEvidence)?;
        }
        if cumulative != cut.cumulative.ticks() {
            return Err(PaperError::IncompleteCut);
        }
        let new_count = keys
            .iter()
            .filter(|key| !self.fills.contains_key(key))
            .count();
        if new_count
            .checked_add(1)
            .is_none_or(|count| count > self.session.limits.capacity())
        {
            return Err(PaperError::Capacity);
        }
        if order.progress.state() == OrderState::Expired && new_count > 0 {
            return Err(PaperError::ExpiredReceiptRecoveryUnsupported);
        }
        let evidence = TerminalEvidence {
            order: order.request.id(),
            status: terminal_status(cut.status),
            cumulative: cut.cumulative,
            receipts: keys.iter().copied().collect(),
            complete: true,
        };
        let mut ledger = self.ledger.clone();
        let mut fills = self.fills.clone();
        let mut progress = order.progress.clone();
        let unknown = progress.state() == OrderState::Unknown;
        let mut settlements = Vec::new();
        for source in &cut.receipts {
            let key = FillKey {
                order: order.request.id(),
                fill_id: source.value().fill_id,
            };
            if let Some(old) = fills.get(&key) {
                if !same_source(&old.source, source) {
                    return Err(PaperError::ReceiptConflict);
                }
                continue;
            }
            if finalized(order) || !order.progress.sent() {
                return Err(PaperError::NotDispatched);
            }
            if fills.len() >= self.session.limits.capacity() {
                return Err(PaperError::Capacity);
            }
            let recorded = account(
                &mut ledger,
                &order.request,
                order.dispatch_decision,
                source,
                &self.session.costs,
                cx,
            )?;
            if !unknown {
                let event = if progress.state() == OrderState::Canceled {
                    ValidatedEvent::RaceFill {
                        qty: source.value().qty,
                        fill_id: source.value().fill_id,
                    }
                } else {
                    ValidatedEvent::Fill {
                        qty: source.value().qty,
                        fill_id: source.value().fill_id,
                    }
                };
                progress = transition(&progress, &event, &cell(&self.session, cx))
                    .map_err(PaperError::Transition)?;
            }
            settlements.push(Settlement::Fill(recorded.receipt.clone()));
            fills.insert(key, recorded);
        }
        if let Some(old) = &order.terminal {
            return if old == &evidence && settlements.is_empty() {
                Ok(PaperOutcome::DuplicateReceipt)
            } else {
                Err(PaperError::ReceiptConflict)
            };
        }
        progress = terminal_progress(progress, cut, &cell(&self.session, cx))?;
        settlements.push(Settlement::Terminal(evidence.clone()));
        let draft = self
            .risk
            .prepare_settlement(&settlements, cx, portfolio(&ledger, *cx.mark.value())?)
            .map_err(PaperError::Risk)?;
        draft.commit();
        self.ledger = ledger;
        self.fills = fills;
        let order = self
            .orders
            .get_mut(id.as_str())
            .expect("order remained present");
        order.progress = progress;
        order.terminal = Some(evidence);
        Ok(PaperOutcome::TerminalReconciled(id.clone()))
    }
}

fn order_id(id: &ClientOrderId) -> Result<OrderId, PaperError> {
    OrderId::new(id.intent().raw(), id.attempt().index()).map_err(PaperError::Risk)
}
fn finalized(order: &TrackedOrder) -> bool {
    order.terminal.is_some() || order.definitive_refusal
}
fn cell<'a>(session: &'a PaperSession, cx: &RiskContext<'_>) -> CellContext<'a> {
    CellContext {
        now: cx.now,
        authority: session.execution.authority(),
        max_attempts: session.execution.max_attempts(),
    }
}
fn visible<T>(source: &Observation<T>, cx: &RiskContext<'_>) -> Result<(), PaperError> {
    if source.timestamps().received_ts.micros() > cx.decision.micros()
        || source.causal_at(cx.decision).is_none()
    {
        return Err(PaperError::FutureReport);
    }
    Ok(())
}
fn same_source(left: &Observation<PaperFill>, right: &Observation<PaperFill>) -> bool {
    left.value() == right.value()
        && left.timestamps().event_ts == right.timestamps().event_ts
        && left.timestamps().available_ts == right.timestamps().available_ts
}
fn account(
    ledger: &mut Ledger,
    request: &OrderRequest,
    dispatch_decision: Option<DecisionTime>,
    source: &Observation<PaperFill>,
    costs: &Costs,
    cx: &RiskContext<'_>,
) -> Result<RecordedFill, PaperError> {
    visible(source, cx)?;
    let value = source.value();
    if dispatch_decision
        .is_none_or(|decision| source.timestamps().event_ts.micros() < decision.micros())
    {
        return Err(PaperError::InvalidEvidence);
    }
    if value.fill_id == 0
        || value.qty.ticks() <= 0
        || value.reference_price.ticks() <= 0
        || order_id(&value.client_order_id)? != request.id()
    {
        return Err(PaperError::InvalidEvidence);
    }
    let accounted = ledger
        .apply_fill(
            MarketOrder {
                side: request.side(),
                qty: value.qty,
            },
            value.reference_price,
            costs,
            source.timestamps().event_ts.micros(),
        )
        .map_err(PaperError::Sim)?;
    let receipt = FillReceipt {
        key: FillKey {
            order: request.id(),
            fill_id: value.fill_id,
        },
        qty: accounted.qty,
        price: accounted.price,
        fee: Amount::from_ticks(accounted.fee.0).map_err(|_| PaperError::InvalidEvidence)?,
        at: accounted.ts,
    };
    Ok(RecordedFill {
        source: source.clone(),
        receipt,
        accounted,
    })
}
fn portfolio(ledger: &Ledger, mark: Price) -> Result<PortfolioSnapshot, PaperError> {
    Ok(PortfolioSnapshot {
        position_ticks: ledger.position().0,
        equity_ticks: ledger.equity(mark).map_err(PaperError::Sim)?.0,
    })
}
fn terminal_status(status: PaperTerminal) -> TerminalStatus {
    match status {
        PaperTerminal::Filled => TerminalStatus::Filled,
        PaperTerminal::Canceled => TerminalStatus::Canceled,
        PaperTerminal::Expired => TerminalStatus::Expired,
        PaperTerminal::Rejected => TerminalStatus::Rejected,
    }
}
fn terminal_progress(
    progress: OrderProgress,
    cut: &PaperTerminalCut,
    cx: &CellContext<'_>,
) -> Result<OrderProgress, PaperError> {
    let target = match cut.status {
        PaperTerminal::Filled => OrderState::Filled,
        PaperTerminal::Canceled => OrderState::Canceled,
        PaperTerminal::Expired => OrderState::Expired,
        PaperTerminal::Rejected => OrderState::Rejected,
    };
    if progress.state() == OrderState::Unknown {
        let event = match cut.status {
            PaperTerminal::Filled => ValidatedEvent::QueryFilled {
                cum: cut.cumulative,
                evidence: cut.evidence_id,
            },
            PaperTerminal::Canceled => ValidatedEvent::QueryCanceled {
                cum: cut.cumulative,
                evidence: cut.evidence_id,
            },
            PaperTerminal::Expired => ValidatedEvent::QueryExpired {
                cum: cut.cumulative,
                evidence: cut.evidence_id,
            },
            PaperTerminal::Rejected => return Err(PaperError::IncompleteCut),
        };
        return transition(&progress, &event, cx).map_err(PaperError::Transition);
    }
    if progress.cum() != cut.cumulative {
        return Err(PaperError::IncompleteCut);
    }
    if progress.state() == target {
        return Ok(progress);
    }
    let event = match cut.status {
        PaperTerminal::Canceled => ValidatedEvent::CancelConfirmed,
        PaperTerminal::Expired => ValidatedEvent::Expire,
        PaperTerminal::Rejected => ValidatedEvent::Reject,
        PaperTerminal::Filled => return Err(PaperError::IncompleteCut),
    };
    transition(&progress, &event, cx).map_err(PaperError::Transition)
}
