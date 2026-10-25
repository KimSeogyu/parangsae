use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use parangsae_core::{DecisionTime, Observation, Price, Qty, Side, UtcMicros, SCALE};
use parangsae_sim::carry::{
    funding_credit, notional, AccountEvent, AccountFill, CarryAccountState, CarryLedger, FeeCharge,
    Market,
};
use parangsae_sim::{fill_price, taker_fee, Cash};

use super::types::*;
use crate::{
    transition, Attempt, CellContext, HaltCause, HaltEpoch, Mono, OrderProgress, OrderState,
    ValidatedEvent,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct LegRequest {
    id: CarryOrderId,
    qty: Qty,
    max_price: Price,
}

// Created only by a successful plan and consumed only by its owning engine.
// This type has no Clone, Serialize or public constructor.
#[derive(Debug)]
struct LegAuthority {
    instance: Arc<()>,
    request: LegRequest,
    epoch: HaltEpoch,
    issued: Mono,
}

#[derive(Debug)]
struct TrackedOrder {
    request: LegRequest,
    progress: OrderProgress,
    authority: Option<LegAuthority>,
    dispatched_at: Option<DecisionTime>,
    terminal: Option<CarryTerminalCut>,
    terminal_visible_at: Option<DecisionTime>,
}

#[derive(Clone, Copy)]
struct RiskPrior {
    loss: bool,
    margin: bool,
    delta: bool,
    epoch: HaltEpoch,
    latched: bool,
}

#[derive(Clone, Copy)]
struct AssessmentContext {
    now: Mono,
    decision: DecisionTime,
    phase: CarryPhase,
    point: CarryAssessmentPoint,
}

struct RiskStamp {
    first_halt: Option<Box<CarryHaltEvidence>>,
    loss: bool,
    margin: bool,
    delta: bool,
    latch: bool,
    epoch: HaltEpoch,
}

struct Projection {
    pair: Option<CarryPlan>,
    exiting: bool,
    spot_remaining: i64,
    spot_complete: bool,
    perp_complete: bool,
    all_complete: bool,
    funding_pending: bool,
    perp_entry_remaining: i64,
}

/// Owns both wallets and all leg authority. Inputs contain data, never permits.
///
/// There is no mutable account getter:
/// ```compile_fail
/// fn change(engine: &mut parangsae_execution::carry::CarryEngine) {
///     engine.ledger.snapshot();
/// }
/// ```
#[derive(Debug)]
pub struct CarryEngine {
    session: CarrySession,
    ledger: CarryLedger,
    instance: Arc<()>,
    pair: Option<CarryPlan>,
    orders: BTreeMap<CarryOrderId, TrackedOrder>,
    fills: BTreeMap<(CarryOrderId, u64), CarryRecordedFill>,
    funding: BTreeMap<u64, CarryFundingSnapshot>,
    phase: CarryPhase,
    exiting: bool,
    hedged_at: Option<DecisionTime>,
    halted: bool,
    first_halt: Option<Box<CarryHaltEvidence>>,
    epoch: HaltEpoch,
    loss_latched: bool,
    margin_latched: bool,
    delta_latched: bool,
    unsupported: Option<UnsupportedEvidence>,
    reconciled_epoch: Option<HaltEpoch>,
    last_now: Mono,
    last_decision: DecisionTime,
    latched_this_step: bool,
    spot_reserved: Cash,
    perp_reserved: Cash,
    legging_since: Option<Mono>,
}

impl CarryEngine {
    pub fn new(session: CarrySession) -> Result<Self, CarryError> {
        validate_session(&session)?;
        let ledger = CarryLedger::new(session.allocation)?;
        let mut engine = Self {
            last_now: session.initial_now,
            last_decision: session.initial_decision,
            session,
            ledger,
            instance: Arc::new(()),
            pair: None,
            orders: BTreeMap::new(),
            fills: BTreeMap::new(),
            funding: BTreeMap::new(),
            phase: CarryPhase::Flat,
            exiting: false,
            hedged_at: None,
            halted: false,
            first_halt: None,
            epoch: HaltEpoch::ZERO,
            loss_latched: false,
            margin_latched: false,
            delta_latched: false,
            unsupported: None,
            reconciled_epoch: None,
            latched_this_step: false,
            spot_reserved: Cash(0),
            perp_reserved: Cash(0),
            legging_since: None,
        };
        let spot = *engine.session.initial_spot.value();
        let perp = *engine.session.initial_perp.value();
        engine.observe(
            spot,
            perp,
            engine.session.initial_now,
            engine.session.initial_decision,
        )?;
        Ok(engine)
    }

    pub fn snapshot(&self) -> CarrySnapshot {
        CarrySnapshot {
            account: self.ledger.snapshot(),
            phase: self.phase,
            pair: self.pair.clone(),
            orders: self
                .orders
                .values()
                .map(|o| CarryOrderSnapshot {
                    id: o.request.id,
                    qty: o.request.qty,
                    max_price: o.request.max_price,
                    progress: o.progress.clone(),
                    complete_cut: o.terminal.is_some(),
                })
                .collect(),
            fills: self.fills.values().cloned().collect(),
            funding: self.funding.values().cloned().collect(),
            spot_quote_reserved: self.spot_reserved,
            perp_collateral_reserved: self.perp_reserved,
            halted: self.halted,
            first_halt: self.first_halt.clone(),
            epoch: self.epoch,
            loss_latched: self.loss_latched,
            margin_latched: self.margin_latched,
            unsupported: self.unsupported.clone(),
            reconciled_epoch: self.reconciled_epoch,
            hedged_at: self.hedged_at,
        }
    }

    pub fn step(&mut self, command: &CarryCommand) -> Result<CarryOutcome, CarryError> {
        self.latched_this_step = false;
        if command.now < self.last_now || command.decision.micros() < self.last_decision.micros() {
            self.latch(HaltCause::PayloadError, command)?;
            return Err(CarryError::ClockWentBackwards);
        }
        if let Err(error) = admitted(
            &command.spot,
            command.decision,
            self.session.limits.max_mark_age_us,
        )
        .and_then(|()| {
            admitted(
                &command.perp,
                command.decision,
                self.session.limits.max_mark_age_us,
            )
        }) {
            self.latch(HaltCause::PayloadError, command)?;
            return Err(error);
        }
        self.last_now = command.now;
        self.last_decision = command.decision;
        if let Err(error) = self.observe(
            *command.spot.value(),
            *command.perp.value(),
            command.now,
            command.decision,
        ) {
            self.refresh_phase(command.decision);
            return Err(error);
        }
        let deadline_expired = self.legging_since.is_some_and(|start| {
            command.now.micros() - start.micros() > self.session.limits.max_legging_us
        });
        if deadline_expired && self.unsupported.is_none() {
            self.unsupported = Some(UnsupportedEvidence::LeggingDeadline);
            self.latch(HaltCause::UnknownOrder, command)?;
        }
        if deadline_expired
            && !matches!(
                command.action,
                CarryAction::Deliver(_)
                    | CarryAction::Reconcile(_)
                    | CarryAction::FundingReceipt(_)
            )
        {
            self.refresh_phase(command.decision);
            return Err(CarryError::UnsupportedFact);
        }
        let result = self.act(command);
        if let Err(error) = &result {
            if matches!(
                error,
                CarryError::ReceiptConflict
                    | CarryError::InvalidEvidence
                    | CarryError::FutureObservation
                    | CarryError::IncompleteCut
            ) {
                self.latch(HaltCause::PayloadError, command)?;
            }
            if matches!(
                error,
                CarryError::PriceEnvelope
                    | CarryError::CapitalEnvelope
                    | CarryError::Accounting(_)
                    | CarryError::InvalidEvidence
                    | CarryError::Capacity
                    | CarryError::ReceiptConflict
                    | CarryError::Overflow
                    | CarryError::Transition(_)
                    | CarryError::UnknownOrder
                    | CarryError::MissingFundingBoundary
            ) {
                let evidence = match &command.action {
                    CarryAction::Deliver(source) => match source.value() {
                        CarryReport::Fill(fill) => {
                            Some(UnsupportedEvidence::Fill(source.with_value(fill.clone())))
                        }
                        _ => None,
                    },
                    CarryAction::Reconcile(source) => Some(UnsupportedEvidence::Terminal {
                        order: source.value().order,
                        evidence_id: source.value().evidence_id,
                    }),
                    CarryAction::FundingReceipt(source) => {
                        Some(UnsupportedEvidence::Funding(source.clone()))
                    }
                    CarryAction::FundingBoundary { key, effective } => {
                        Some(UnsupportedEvidence::FundingBoundary {
                            key: *key,
                            effective: *effective,
                        })
                    }
                    _ => None,
                };
                if let Some(evidence) = evidence {
                    self.unsupported.get_or_insert(evidence);
                    self.latch(HaltCause::ReconMismatch, command)?;
                }
            }
        }
        self.refresh_phase(command.decision);
        result
    }

    fn act(&mut self, command: &CarryCommand) -> Result<CarryOutcome, CarryError> {
        match &command.action {
            CarryAction::Observe => Ok(CarryOutcome::Observed),
            CarryAction::Plan(plan) => self.plan(plan, command),
            CarryAction::PlanHedge { pair } => {
                self.may_plan(*pair)?;
                if self.exiting
                    || !self.complete(CarryStage::SpotEntry)
                    || self.has_order(CarryStage::PerpEntry)
                {
                    return Err(CarryError::Phase);
                }
                let visible = self
                    .order(CarryStage::SpotEntry)?
                    .terminal_visible_at
                    .ok_or(CarryError::Phase)?;
                if command.decision.micros() <= visible.micros() {
                    return Err(CarryError::FutureObservation);
                }
                let lots = self.ledger.snapshot().spot_btc;
                let plan = self.pair.as_ref().ok_or(CarryError::WrongPair)?;
                if lots.ticks() == 0
                    || lots.ticks() > plan.net_btc.ticks()
                    || lots.ticks() % self.session.perp_grid.lot.ticks() != 0
                {
                    return Err(CarryError::GridMismatch);
                }
                let id = order_id(*pair, CarryStage::PerpEntry);
                let max_price = plan.max_perp_price;
                self.check_entry_capital(lots, plan.max_spot_price, max_price, command)?;
                self.insert_order(id, lots, max_price, command)?;
                Ok(CarryOutcome::Planned(id))
            }
            CarryAction::Dispatch(id) => self.dispatch(*id, command),
            CarryAction::Deliver(source) => {
                visible(source, command.decision)?;
                match source.value() {
                    CarryReport::Fill(fill) => {
                        self.deliver(&source.with_value(fill.clone()), command)
                    }
                    CarryReport::Ack(id) => self.change(*id, ValidatedEvent::Ack, command),
                    CarryReport::CancelConfirmed(id) => {
                        self.change(*id, ValidatedEvent::CancelConfirmed, command)
                    }
                    CarryReport::CancelDenied(id) => {
                        self.change(*id, ValidatedEvent::CancelDenied, command)
                    }
                    CarryReport::Expired(id) => self.change(*id, ValidatedEvent::Expire, command),
                    CarryReport::Rejected(id) => self.change(*id, ValidatedEvent::Reject, command),
                }
            }
            CarryAction::Cancel(id) => self.change(*id, ValidatedEvent::CancelRequested, command),
            CarryAction::Timeout(id) => self.change(*id, ValidatedEvent::Timeout, command),
            CarryAction::Reconcile(source) => self.reconcile(source, command),
            CarryAction::BeginExit {
                pair,
                max_perp_price,
            } => {
                self.may_close(*pair)?;
                if self.outstanding() || self.exiting {
                    return Err(CarryError::Phase);
                }
                let short = self.ledger.snapshot().perp_short;
                let mut projection = self.projection(None, None, false)?;
                projection.exiting = true;
                let reserved = reservations(&self.session, &self.ledger.snapshot(), &projection)?;
                if short.ticks() == 0 {
                    self.exiting = true;
                    self.set_reservations(reserved);
                    return Ok(CarryOutcome::Waiting);
                }
                let id = order_id(*pair, CarryStage::PerpExit);
                self.insert_order(id, short, *max_perp_price, command)?;
                self.exiting = true;
                self.set_reservations(reserved);
                Ok(CarryOutcome::Planned(id))
            }
            CarryAction::PlanSpotExit {
                pair,
                max_spot_price,
            } => {
                self.may_close(*pair)?;
                let account = self.ledger.snapshot();
                if !self.exiting
                    || self.outstanding()
                    || account.perp_short.ticks() != 0
                    || self.has_order(CarryStage::SpotExit)
                {
                    return Err(CarryError::Phase);
                }
                if !self.known_perp_flat() {
                    return Err(CarryError::Phase);
                }
                if self
                    .orders
                    .values()
                    .filter(|o| market(o.request.id.stage) == Market::UsdMPerp)
                    .filter_map(|o| o.terminal_visible_at)
                    .any(|at| command.decision.micros() <= at.micros())
                {
                    return Err(CarryError::FutureObservation);
                }
                let qty = account.spot_btc;
                if qty.ticks() == 0 || qty.ticks() % self.session.spot_grid.lot.ticks() != 0 {
                    return Err(CarryError::GridMismatch);
                }
                let id = order_id(*pair, CarryStage::SpotExit);
                self.insert_order(id, qty, *max_spot_price, command)?;
                Ok(CarryOutcome::Planned(id))
            }
            CarryAction::FundingBoundary { key, effective } => {
                self.boundary(*key, *effective, command)
            }
            CarryAction::FundingReceipt(source) => self.settle(source, command),
            CarryAction::ReconcilePortfolio(expected) => {
                if self.unsupported.is_some()
                    || self.outstanding()
                    || self
                        .orders
                        .values()
                        .any(|o| o.progress.state() == OrderState::Unknown)
                {
                    return Err(CarryError::Unresolved);
                }
                if &self.ledger.snapshot() != expected {
                    return Err(CarryError::InvalidEvidence);
                }
                self.reconciled_epoch = Some(self.epoch);
                Ok(CarryOutcome::PortfolioReconciled)
            }
            CarryAction::NoExecutionEvidence { order, kind } => {
                if !self.orders.contains_key(order) {
                    return Err(CarryError::UnknownOrder);
                }
                match kind {
                    MissingExecution::ZeroCount => Ok(CarryOutcome::Waiting),
                    MissingExecution::MissingSource => {
                        self.unsupported
                            .get_or_insert(UnsupportedEvidence::MissingSource(*order));
                        self.latch(HaltCause::ReconMismatch, command)?;
                        Err(CarryError::UnsupportedFact)
                    }
                }
            }
            CarryAction::EndOfInput => {
                if self.phase == CarryPhase::Flat || self.phase == CarryPhase::Closed {
                    Ok(CarryOutcome::Closed)
                } else {
                    self.unsupported
                        .get_or_insert(UnsupportedEvidence::EndOfInput);
                    self.latch(HaltCause::ReconMismatch, command)?;
                    Err(CarryError::Unresolved)
                }
            }
            CarryAction::Halt(cause) => {
                self.latch(*cause, command)?;
                Ok(CarryOutcome::Halted(self.epoch))
            }
            CarryAction::Resume { expected_epoch } => {
                if *expected_epoch != self.epoch.get() {
                    return Err(CarryError::EpochMismatch);
                }
                if self.loss_latched {
                    return Err(CarryError::LossLatched);
                }
                if self.unsupported.is_some() || self.outstanding() {
                    return Err(CarryError::Unresolved);
                }
                if self.reconciled_epoch != Some(self.epoch) {
                    return Err(CarryError::NotReconciled);
                }
                if self.margin_latched {
                    return Err(CarryError::Unresolved);
                }
                if self.delta_latched {
                    return Err(CarryError::CapitalEnvelope);
                }
                self.halted = false;
                Ok(CarryOutcome::Resumed(self.epoch))
            }
        }
    }

    fn plan(
        &mut self,
        plan: &CarryPlan,
        command: &CarryCommand,
    ) -> Result<CarryOutcome, CarryError> {
        if self.halted {
            return Err(CarryError::Halted);
        }
        if let Some(old) = &self.pair {
            if old.pair == plan.pair {
                if old == plan {
                    return Ok(CarryOutcome::Duplicate);
                }
                self.latch(HaltCause::PayloadError, command)?;
                return Err(CarryError::ReceiptConflict);
            }
        }
        if !matches!(self.phase, CarryPhase::Flat | CarryPhase::Closed) || self.outstanding() {
            return Err(CarryError::Phase);
        }
        if plan.pair == 0
            || plan.gross_spot.ticks() == 0
            || plan.net_btc.ticks() == 0
            || plan.net_btc.ticks() > self.session.limits.max_net_btc.ticks()
        {
            return Err(CarryError::InvalidPlan);
        }
        if plan.gross_spot.ticks() % self.session.spot_grid.lot.ticks() != 0
            || plan.net_btc.ticks() % self.session.perp_grid.lot.ticks() != 0
        {
            return Err(CarryError::GridMismatch);
        }
        let fee = fee_for(
            &self.session,
            Market::Spot,
            Side::Buy,
            plan.gross_spot,
            plan.max_spot_price,
        )?;
        let net = match fee {
            FeeCharge::Btc(q) => plan
                .gross_spot
                .ticks()
                .checked_sub(q.ticks())
                .ok_or(CarryError::Overflow)?,
            FeeCharge::Usdt(_) => plan.gross_spot.ticks(),
        };
        if net != plan.net_btc.ticks() {
            return Err(CarryError::NetQuantityMismatch);
        }
        self.check_entry_capital(
            plan.net_btc,
            plan.max_spot_price,
            plan.max_perp_price,
            command,
        )?;
        let cost = spot_budget(&self.session, plan.gross_spot, plan.max_spot_price)?;
        if self.ledger.snapshot().spot_quote.0 < add(cost, self.session.margin.spot_close_buffer.0)?
        {
            return Err(CarryError::CapitalEnvelope);
        }
        let id = order_id(plan.pair, CarryStage::SpotEntry);
        let projection = Projection {
            pair: Some(plan.clone()),
            exiting: false,
            spot_remaining: plan.gross_spot.ticks(),
            spot_complete: false,
            perp_complete: true,
            all_complete: false,
            funding_pending: false,
            perp_entry_remaining: plan.net_btc.ticks(),
        };
        let reserved = reservations(&self.session, &self.ledger.snapshot(), &projection)?;
        self.insert_order(id, plan.gross_spot, plan.max_spot_price, command)?;
        self.pair = Some(plan.clone());
        self.exiting = false;
        self.hedged_at = None;
        self.set_reservations(reserved);
        Ok(CarryOutcome::Planned(id))
    }

    fn check_entry_capital(
        &self,
        net: Qty,
        spot_max: Price,
        perp_max: Price,
        command: &CarryCommand,
    ) -> Result<(), CarryError> {
        if spot_max.ticks() < command.spot.value().ticks()
            || perp_max.ticks() < command.perp.value().ticks()
            || spot_max.ticks() <= 0
            || perp_max.ticks() <= 0
        {
            return Err(CarryError::PriceEnvelope);
        }
        if notional(spot_max.max(perp_max), net, true)?
            > self.session.limits.max_unhedged_notional.0
        {
            return Err(CarryError::CapitalEnvelope);
        }
        let required = perp_budget(&self.session, net, perp_max)?;
        if self.ledger.snapshot().perp_wallet.0 < required {
            return Err(CarryError::CapitalEnvelope);
        }
        Ok(())
    }

    fn insert_order(
        &mut self,
        id: CarryOrderId,
        qty: Qty,
        max_price: Price,
        command: &CarryCommand,
    ) -> Result<(), CarryError> {
        if self.orders.len() >= self.session.limits.max_orders {
            return Err(CarryError::Capacity);
        }
        if self.orders.contains_key(&id)
            || id.attempt != 0
            || qty.ticks() == 0
            || max_price.ticks() <= 0
        {
            return Err(CarryError::InvalidPlan);
        }
        let market = market(id.stage);
        let grid = if market == Market::Spot {
            &self.session.spot_grid
        } else {
            &self.session.perp_grid
        };
        let known_price = if market == Market::Spot {
            *command.spot.value()
        } else {
            *command.perp.value()
        };
        if max_price.ticks() < known_price.ticks() {
            return Err(CarryError::PriceEnvelope);
        }
        if qty.ticks() % grid.lot.ticks() != 0
            || notional(known_price, qty, false)? < grid.min_notional.0
        {
            return Err(CarryError::GridMismatch);
        }
        let request = LegRequest { id, qty, max_price };
        let progress = OrderProgress::new_pending_attempt(
            qty,
            Attempt::FIRST,
            self.session.execution.max_attempts(),
        )
        .map_err(|_| CarryError::InvalidPlan)?;
        let authority = LegAuthority {
            instance: Arc::clone(&self.instance),
            request: request.clone(),
            epoch: self.epoch,
            issued: command.now,
        };
        self.orders.insert(
            id,
            TrackedOrder {
                request,
                progress,
                authority: Some(authority),
                dispatched_at: None,
                terminal: None,
                terminal_visible_at: None,
            },
        );
        Ok(())
    }

    fn dispatch(
        &mut self,
        id: CarryOrderId,
        command: &CarryCommand,
    ) -> Result<CarryOutcome, CarryError> {
        if self.halted && matches!(id.stage, CarryStage::SpotEntry | CarryStage::PerpEntry) {
            return Err(CarryError::Halted);
        }
        if self.unsupported.is_some() {
            return Err(CarryError::Unresolved);
        }
        let order = self.orders.get_mut(&id).ok_or(CarryError::UnknownOrder)?;
        let permit = order.authority.take().ok_or(CarryError::MissingApproval)?;
        if !Arc::ptr_eq(&permit.instance, &self.instance)
            || permit.epoch != self.epoch
            || permit.request != order.request
        {
            return Err(CarryError::MissingApproval);
        }
        if command
            .now
            .micros()
            .checked_sub(permit.issued.micros())
            .ok_or(CarryError::ClockWentBackwards)?
            > self.session.execution.timing().max_approval_age().get()
        {
            self.latch(HaltCause::RiskTrip, command)?;
            return Err(CarryError::ApprovalExpired);
        }
        let state = self.ledger.snapshot();
        let request = permit.request.clone();
        let spot = *command.spot.value();
        let perp = *command.perp.value();
        if (if market(id.stage) == Market::Spot {
            spot
        } else {
            perp
        })
        .ticks()
            > request.max_price.ticks()
        {
            return Err(CarryError::PriceEnvelope);
        }
        let grid = if market(id.stage) == Market::Spot {
            &self.session.spot_grid
        } else {
            &self.session.perp_grid
        };
        if notional(
            if market(id.stage) == Market::Spot {
                spot
            } else {
                perp
            },
            request.qty,
            false,
        )? < grid.min_notional.0
        {
            return Err(CarryError::GridMismatch);
        }
        match id.stage {
            CarryStage::SpotEntry => {
                let plan = self.pair.as_ref().ok_or(CarryError::WrongPair)?;
                self.check_entry_capital(
                    plan.net_btc,
                    plan.max_spot_price,
                    plan.max_perp_price,
                    command,
                )?;
                if state.spot_quote.0
                    < add(
                        spot_budget(&self.session, request.qty, request.max_price)?,
                        self.session.margin.spot_close_buffer.0,
                    )?
                {
                    return Err(CarryError::CapitalEnvelope);
                }
            }
            CarryStage::PerpEntry => {
                if request.qty.ticks() > state.spot_btc.ticks()
                    || !self.complete(CarryStage::SpotEntry)
                {
                    return Err(CarryError::CapitalEnvelope);
                }
                if state.perp_wallet.0 < perp_budget(&self.session, request.qty, request.max_price)?
                {
                    return Err(CarryError::CapitalEnvelope);
                }
            }
            CarryStage::PerpExit => {
                if request.qty.ticks() != state.perp_short.ticks() {
                    return Err(CarryError::CapitalEnvelope);
                }
                let account_fill = account_fill_at_executed_price(
                    &self.session,
                    &request,
                    request.qty,
                    request.max_price,
                )?;
                self.ledger.prepare(&[AccountEvent::Fill(account_fill)])?;
            }
            CarryStage::SpotExit => {
                if !self.known_perp_flat() || request.qty.ticks() > state.spot_btc.ticks() {
                    return Err(CarryError::Phase);
                }
            }
        }
        let order = self.orders.get_mut(&id).expect("known order");
        let sent = order
            .progress
            .record_dispatch(command.now)
            .map_err(CarryError::Transition)?;
        order.progress = sent;
        order.dispatched_at = Some(command.decision);
        Ok(CarryOutcome::Dispatched(id))
    }

    fn deliver(
        &mut self,
        source: &Observation<CarryFill>,
        command: &CarryCommand,
    ) -> Result<CarryOutcome, CarryError> {
        visible(source, command.decision)?;
        let fill = source.value();
        let key = (fill.order, fill.fill_id);
        if let Some(old) = self.fills.get(&key) {
            return if same_fill(&old.source, source) {
                Ok(CarryOutcome::Duplicate)
            } else {
                Err(CarryError::ReceiptConflict)
            };
        }
        if self.fills.len() >= self.session.limits.max_receipts {
            return Err(CarryError::Capacity);
        }
        let order = self
            .orders
            .get(&fill.order)
            .ok_or(CarryError::UnknownOrder)?;
        validate_fill(order, source)?;
        self.validate_boundary_order(source)?;
        let event = if order.progress.state() == OrderState::Canceled {
            ValidatedEvent::RaceFill {
                qty: fill.qty,
                fill_id: fill.fill_id,
            }
        } else {
            ValidatedEvent::Fill {
                qty: fill.qty,
                fill_id: fill.fill_id,
            }
        };
        let progress = transition(&order.progress, &event, &cell(&self.session, command.now))
            .map_err(CarryError::Transition)?;
        let accounted = account_fill(
            &self.session,
            &order.request,
            fill.qty,
            fill.reference_price,
        )?;
        let projection = self.projection(Some((fill.order, &progress, false)), None, false)?;
        let prior = self.risk_prior();
        let draft = self.ledger.prepare(&[AccountEvent::Fill(accounted)])?;
        if fill.order.stage == CarryStage::PerpEntry
            && draft.snapshot().perp_short.ticks() > draft.snapshot().spot_btc.ticks()
        {
            return Err(CarryError::CapitalEnvelope);
        }
        let stamp = assessed(
            &self.session,
            draft.snapshot(),
            command,
            prior,
            self.phase,
            CarryAssessmentPoint::Fill(fill.order),
        )?;
        let reserved = reservations(&self.session, draft.snapshot(), &projection)?;
        let recorded = CarryRecordedFill {
            source: source.clone(),
            price: accounted.price,
            fee: accounted.fee,
        };
        draft.commit();
        self.commit_guard(stamp);
        self.set_reservations(reserved);
        self.fills.insert(key, recorded.clone());
        self.orders
            .get_mut(&fill.order)
            .expect("known order")
            .progress = progress;
        self.reconciled_epoch = None;
        Ok(CarryOutcome::FillApplied(recorded))
    }

    fn change(
        &mut self,
        id: CarryOrderId,
        event: ValidatedEvent,
        command: &CarryCommand,
    ) -> Result<CarryOutcome, CarryError> {
        let order = self.orders.get(&id).ok_or(CarryError::UnknownOrder)?;
        if order.terminal.is_some() {
            return Err(CarryError::Phase);
        }
        if !order.progress.sent() && event != ValidatedEvent::CancelRequested {
            return Err(CarryError::MissingApproval);
        }
        let next = transition(&order.progress, &event, &cell(&self.session, command.now))
            .map_err(CarryError::Transition)?;
        let local = !order.progress.sent();
        let unknown = next.state() == OrderState::Unknown;
        let projection = self.projection(Some((id, &next, local)), None, false)?;
        let reserved = reservations(&self.session, &self.ledger.snapshot(), &projection)?;
        let terminal = if local {
            Some(CarryTerminalCut {
                order: id,
                evidence_id: 1,
                status: CarryTerminal::Canceled,
                cumulative: quantity(0)?,
                receipts: Vec::new(),
                complete: true,
            })
        } else {
            None
        };
        if unknown {
            self.latch(HaltCause::UnknownOrder, command)?;
        }
        let order = self.orders.get_mut(&id).expect("known order");
        order.progress = next;
        if local {
            order.authority = None;
            order.terminal = terminal;
            order.terminal_visible_at = Some(command.decision);
        }
        self.set_reservations(reserved);
        self.reconciled_epoch = None;
        Ok(CarryOutcome::StateChanged(id))
    }

    fn reconcile(
        &mut self,
        source: &Observation<CarryTerminalCut>,
        command: &CarryCommand,
    ) -> Result<CarryOutcome, CarryError> {
        visible(source, command.decision)?;
        let cut = source.value();
        if !cut.complete || cut.evidence_id == 0 {
            return Err(CarryError::IncompleteCut);
        }
        if cut.receipts.len() > self.session.limits.max_receipts
            || cut.receipts.len() > self.session.limits.max_batch_entries
        {
            return Err(CarryError::Capacity);
        }
        let order = self
            .orders
            .get(&cut.order)
            .ok_or(CarryError::UnknownOrder)?;
        if let Some(old) = &order.terminal {
            return if same_cut(old, cut) {
                Ok(CarryOutcome::Duplicate)
            } else {
                Err(CarryError::ReceiptConflict)
            };
        }
        if !order.progress.sent() {
            return Err(CarryError::MissingApproval);
        }
        let mut keys = BTreeSet::new();
        let mut cumulative = 0i64;
        let mut events = Vec::new();
        let mut records = Vec::new();
        let mut progress = order.progress.clone();
        let unknown = progress.state() == OrderState::Unknown;
        for receipt in &cut.receipts {
            visible(receipt, command.decision)?;
            let value = receipt.value();
            if value.order != cut.order || !keys.insert(value.fill_id) {
                return Err(CarryError::InvalidEvidence);
            }
            cumulative = add(cumulative, value.qty.ticks())?;
            let key = (value.order, value.fill_id);
            if let Some(old) = self.fills.get(&key) {
                if !same_fill(&old.source, receipt) {
                    return Err(CarryError::ReceiptConflict);
                }
                continue;
            }
            validate_fill(order, receipt)?;
            self.validate_boundary_order(receipt)?;
            if order.progress.state() == OrderState::Expired {
                return Err(CarryError::InvalidEvidence);
            }
            let accounted = account_fill(
                &self.session,
                &order.request,
                value.qty,
                value.reference_price,
            )?;
            if !unknown {
                let event = if progress.state() == OrderState::Canceled {
                    ValidatedEvent::RaceFill {
                        qty: value.qty,
                        fill_id: value.fill_id,
                    }
                } else {
                    ValidatedEvent::Fill {
                        qty: value.qty,
                        fill_id: value.fill_id,
                    }
                };
                progress = transition(&progress, &event, &cell(&self.session, command.now))
                    .map_err(CarryError::Transition)?;
            }
            events.push(AccountEvent::Fill(accounted));
            records.push((
                (value.order, value.fill_id),
                CarryRecordedFill {
                    source: receipt.clone(),
                    price: accounted.price,
                    fee: accounted.fee,
                },
            ));
        }
        if cumulative != cut.cumulative.ticks()
            || self
                .fills
                .keys()
                .any(|(id, fill)| *id == cut.order && !keys.contains(fill))
        {
            return Err(CarryError::IncompleteCut);
        }
        if self
            .fills
            .len()
            .checked_add(records.len())
            .is_none_or(|len| len > self.session.limits.max_receipts)
        {
            return Err(CarryError::Capacity);
        }
        progress = terminal_progress(progress, cut, &cell(&self.session, command.now))?;
        let projection = self.projection(Some((cut.order, &progress, true)), None, false)?;
        let prior = self.risk_prior();
        let draft = self.ledger.prepare(&events)?;
        if draft.snapshot().perp_short.ticks() > draft.snapshot().spot_btc.ticks() {
            return Err(CarryError::CapitalEnvelope);
        }
        let stamp = assessed(
            &self.session,
            draft.snapshot(),
            command,
            prior,
            self.phase,
            CarryAssessmentPoint::Reconciliation(cut.order),
        )?;
        let reserved = reservations(&self.session, draft.snapshot(), &projection)?;
        draft.commit();
        self.commit_guard(stamp);
        self.set_reservations(reserved);
        for (key, record) in records {
            self.fills.insert(key, record);
        }
        let order = self.orders.get_mut(&cut.order).expect("known order");
        order.progress = progress;
        order.terminal = Some(cut.clone());
        order.terminal_visible_at = Some(command.decision);
        self.reconciled_epoch = None;
        Ok(CarryOutcome::TerminalReconciled(cut.order))
    }

    fn boundary(
        &mut self,
        key: u64,
        effective: UtcMicros,
        command: &CarryCommand,
    ) -> Result<CarryOutcome, CarryError> {
        if let Some(old) = self.funding.get(&key) {
            return if old.effective == effective {
                Ok(CarryOutcome::Duplicate)
            } else {
                Err(CarryError::ReceiptConflict)
            };
        }
        if self.funding.values().any(|old| old.effective == effective) {
            return Err(CarryError::ReceiptConflict);
        }
        if key == 0 || effective != command.decision.micros() {
            return Err(CarryError::InvalidEvidence);
        }
        if self.funding.len() >= self.session.limits.max_funding_events {
            return Err(CarryError::Capacity);
        }
        if self.unsupported.is_some()
            || self.orders.values().any(|o| {
                market(o.request.id.stage) == Market::UsdMPerp
                    && o.progress.sent()
                    && o.terminal.is_none()
            })
        {
            self.unsupported
                .get_or_insert(UnsupportedEvidence::FundingBoundary { key, effective });
            self.latch(HaltCause::UnknownOrder, command)?;
            return Err(CarryError::AmbiguousFundingBoundary);
        }
        let short = self.ledger.snapshot().perp_short;
        let projection = self.projection(None, None, true)?;
        let reserved = reservations(&self.session, &self.ledger.snapshot(), &projection)?;
        self.funding.insert(
            key,
            CarryFundingSnapshot {
                key,
                effective,
                captured_short: short,
                receipt: None,
                cashflow: None,
            },
        );
        self.set_reservations(reserved);
        Ok(CarryOutcome::BoundaryCaptured { key, short })
    }

    fn settle(
        &mut self,
        source: &Observation<CarryFundingReceipt>,
        command: &CarryCommand,
    ) -> Result<CarryOutcome, CarryError> {
        visible(source, command.decision)?;
        let value = source.value();
        let boundary = self
            .funding
            .get(&value.key)
            .ok_or(CarryError::MissingFundingBoundary)?;
        if let Some(old) = &boundary.receipt {
            return if same_observation(old, source) {
                Ok(CarryOutcome::Duplicate)
            } else {
                Err(CarryError::ReceiptConflict)
            };
        }
        if value.effective != boundary.effective
            || source.timestamps().event_ts.micros() != value.effective
            || value.charge_mark.ticks() <= 0
        {
            return Err(CarryError::InvalidEvidence);
        }
        let amount = funding_credit(boundary.captured_short, value.charge_mark, value.rate)?;
        let projection = self.projection(None, Some(value.key), false)?;
        let prior = self.risk_prior();
        let draft = self.ledger.prepare(&[AccountEvent::Funding(amount)])?;
        let stamp = assessed(
            &self.session,
            draft.snapshot(),
            command,
            prior,
            self.phase,
            CarryAssessmentPoint::Funding { key: value.key },
        )?;
        let reserved = reservations(&self.session, draft.snapshot(), &projection)?;
        draft.commit();
        self.commit_guard(stamp);
        self.set_reservations(reserved);
        let boundary = self.funding.get_mut(&value.key).expect("known boundary");
        boundary.receipt = Some(source.clone());
        boundary.cashflow = Some(amount);
        self.reconciled_epoch = None;
        Ok(CarryOutcome::FundingApplied {
            key: value.key,
            amount,
        })
    }

    fn may_plan(&self, pair: u128) -> Result<(), CarryError> {
        if self.halted {
            return Err(CarryError::Halted);
        }
        if self.pair.as_ref().is_none_or(|p| p.pair != pair) {
            return Err(CarryError::WrongPair);
        }
        Ok(())
    }
    fn validate_boundary_order(&self, source: &Observation<CarryFill>) -> Result<(), CarryError> {
        if source.value().market == Market::UsdMPerp
            && self
                .funding
                .values()
                .any(|boundary| source.timestamps().event_ts.micros() <= boundary.effective)
        {
            return Err(CarryError::InvalidEvidence);
        }
        Ok(())
    }
    fn may_close(&self, pair: u128) -> Result<(), CarryError> {
        if self.unsupported.is_some() {
            return Err(CarryError::Unresolved);
        }
        if self.pair.as_ref().is_none_or(|p| p.pair != pair) {
            return Err(CarryError::WrongPair);
        }
        Ok(())
    }
    fn has_order(&self, stage: CarryStage) -> bool {
        self.order(stage).is_ok()
    }
    fn order(&self, stage: CarryStage) -> Result<&TrackedOrder, CarryError> {
        let pair = self.pair.as_ref().ok_or(CarryError::WrongPair)?.pair;
        self.orders
            .get(&order_id(pair, stage))
            .ok_or(CarryError::UnknownOrder)
    }
    fn complete(&self, stage: CarryStage) -> bool {
        self.order(stage).is_ok_and(|o| o.terminal.is_some())
    }
    fn known_perp_flat(&self) -> bool {
        self.ledger.snapshot().perp_short.ticks() == 0
            && self
                .orders
                .values()
                .filter(|o| market(o.request.id.stage) == Market::UsdMPerp)
                .all(|o| o.terminal.is_some())
    }
    fn outstanding(&self) -> bool {
        self.orders.values().any(|o| o.terminal.is_none())
    }

    fn latch(&mut self, cause: HaltCause, command: &CarryCommand) -> Result<(), CarryError> {
        self.latch_at(cause, command.now, command.decision)
    }

    fn latch_at(
        &mut self,
        cause: HaltCause,
        now: Mono,
        decision: DecisionTime,
    ) -> Result<(), CarryError> {
        if !self.latched_this_step {
            self.epoch = self.epoch.checked_next().ok_or(CarryError::Overflow)?;
        }
        self.first_halt.get_or_insert_with(|| {
            Box::new(CarryHaltEvidence {
                cause,
                epoch: self.epoch,
                now,
                decision,
                phase_before_refresh: self.phase,
                assessment: None,
            })
        });
        self.latched_this_step = true;
        self.halted = true;
        self.reconciled_epoch = None;
        for order in self.orders.values_mut() {
            order.authority = None;
        }
        Ok(())
    }

    fn risk_prior(&self) -> RiskPrior {
        RiskPrior {
            loss: self.loss_latched,
            margin: self.margin_latched,
            delta: self.delta_latched,
            epoch: self.epoch,
            latched: self.latched_this_step,
        }
    }

    fn commit_guard(&mut self, stamp: RiskStamp) {
        self.loss_latched = stamp.loss;
        self.margin_latched = stamp.margin;
        self.delta_latched = stamp.delta;
        self.epoch = stamp.epoch;
        if stamp.latch {
            if self.first_halt.is_none() {
                self.first_halt = stamp.first_halt;
            }
            self.latched_this_step = true;
            self.halted = true;
            self.reconciled_epoch = None;
            for order in self.orders.values_mut() {
                order.authority = None;
            }
        }
    }

    fn observe(
        &mut self,
        spot: Price,
        perp: Price,
        now: Mono,
        decision: DecisionTime,
    ) -> Result<(), CarryError> {
        let state = self.ledger.snapshot();
        let result = assess_values(
            &self.session,
            &state,
            spot,
            perp,
            self.risk_prior(),
            AssessmentContext {
                now,
                decision,
                phase: self.phase,
                point: CarryAssessmentPoint::Observation,
            },
        );
        match result {
            Ok(stamp) => {
                self.commit_guard(stamp);
                Ok(())
            }
            Err(error) => {
                self.unsupported
                    .get_or_insert(UnsupportedEvidence::Valuation);
                self.latch_at(HaltCause::RiskTrip, now, decision)?;
                Err(error)
            }
        }
    }

    fn refresh_phase(&mut self, decision: DecisionTime) {
        if self.unsupported.is_some()
            || self
                .orders
                .values()
                .any(|o| o.progress.state() == OrderState::Unknown)
        {
            self.phase = CarryPhase::Unresolved;
            return;
        }
        let Some(_) = &self.pair else {
            self.phase = CarryPhase::Flat;
            return;
        };
        let state = self.ledger.snapshot();
        if !self.outstanding()
            && state.spot_btc.ticks() == 0
            && state.perp_short.ticks() == 0
            && self.funding.values().all(|f| f.receipt.is_some())
        {
            self.phase = CarryPhase::Closed;
        } else if self.exiting {
            self.phase = if state.perp_short.ticks() > 0
                || (self.has_order(CarryStage::PerpExit) && !self.complete(CarryStage::PerpExit))
            {
                CarryPhase::ClosingPerp
            } else {
                CarryPhase::ClosingSpot
            };
        } else if self.complete(CarryStage::SpotEntry)
            && self.complete(CarryStage::PerpEntry)
            && state.spot_btc.ticks() > 0
            && state.spot_btc == state.perp_short
        {
            self.phase = CarryPhase::Hedged;
            self.hedged_at.get_or_insert(decision);
        } else if state.spot_btc.ticks() > 0 {
            self.phase = CarryPhase::OpeningHedge;
        } else {
            self.phase = CarryPhase::OpeningSpot;
        }
        if matches!(
            self.phase,
            CarryPhase::Flat | CarryPhase::Closed | CarryPhase::Hedged
        ) {
            self.legging_since = None;
        } else if state.spot_btc != state.perp_short
            || self
                .orders
                .values()
                .any(|o| o.progress.sent() && o.terminal.is_none())
        {
            self.legging_since.get_or_insert(self.last_now);
        }
    }

    fn projection(
        &self,
        update: Option<(CarryOrderId, &OrderProgress, bool)>,
        settled: Option<u64>,
        new_boundary: bool,
    ) -> Result<Projection, CarryError> {
        let mut projected = Projection {
            pair: self.pair.clone(),
            exiting: self.exiting,
            spot_remaining: 0,
            spot_complete: false,
            perp_complete: true,
            all_complete: true,
            funding_pending: new_boundary
                || self
                    .funding
                    .iter()
                    .any(|(key, row)| Some(*key) != settled && row.receipt.is_none()),
            perp_entry_remaining: 0,
        };
        let current_pair = self.pair.as_ref().map(|p| p.pair);
        let mut has_entry = false;
        for order in self.orders.values() {
            let (progress, complete) = if let Some((id, progress, complete)) =
                update.filter(|(id, _, _)| *id == order.request.id)
            {
                let _ = id;
                (progress, complete)
            } else {
                (&order.progress, order.terminal.is_some())
            };
            projected.all_complete &= complete;
            if market(order.request.id.stage) == Market::UsdMPerp {
                projected.perp_complete &= complete;
            }
            if Some(order.request.id.pair) != current_pair {
                continue;
            }
            if order.request.id.stage == CarryStage::SpotEntry {
                projected.spot_complete = complete;
                projected.spot_remaining = if complete {
                    0
                } else {
                    order
                        .request
                        .qty
                        .ticks()
                        .checked_sub(progress.cum().ticks())
                        .ok_or(CarryError::Overflow)?
                };
            }
            if order.request.id.stage == CarryStage::PerpEntry {
                has_entry = true;
                projected.perp_entry_remaining = if complete {
                    0
                } else {
                    order
                        .request
                        .qty
                        .ticks()
                        .checked_sub(progress.cum().ticks())
                        .ok_or(CarryError::Overflow)?
                };
            }
        }
        if !has_entry && !self.exiting {
            projected.perp_entry_remaining = self.pair.as_ref().map_or(0, |p| p.net_btc.ticks());
        }
        Ok(projected)
    }

    fn set_reservations(&mut self, reserved: (Cash, Cash)) {
        self.spot_reserved = reserved.0;
        self.perp_reserved = reserved.1;
    }
}

fn assessed(
    s: &CarrySession,
    state: &CarryAccountState,
    command: &CarryCommand,
    prior: RiskPrior,
    phase: CarryPhase,
    point: CarryAssessmentPoint,
) -> Result<RiskStamp, CarryError> {
    assess_values(
        s,
        state,
        *command.spot.value(),
        *command.perp.value(),
        prior,
        AssessmentContext {
            now: command.now,
            decision: command.decision,
            phase,
            point,
        },
    )
}

fn assess_values(
    s: &CarrySession,
    state: &CarryAccountState,
    spot: Price,
    perp: Price,
    prior: RiskPrior,
    context: AssessmentContext,
) -> Result<RiskStamp, CarryError> {
    let equity = state.equity(spot, perp)?;
    let session_loss_ticks = i128::from(s.allocation.total.0) - i128::from(equity.0);
    let loss = session_loss_ticks >= i128::from(s.limits.max_session_loss.0);
    let (margin, margin_balance_ticks, margin_required) = if state.perp_short.ticks() == 0 {
        (false, None, None)
    } else {
        let balance = i128::from(state.perp_wallet.0) + i128::from(state.unrealized_perp(perp)?.0);
        let required = add(
            bps(
                notional(perp, state.perp_short, true)?,
                s.margin.maintenance_bps,
            )?,
            add(s.margin.perp_close_buffer.0, s.margin.perp_funding_buffer.0)?,
        )?;
        (
            balance < i128::from(required),
            Some(balance),
            Some(Cash(required)),
        )
    };
    let delta = (i128::from(state.spot_btc.ticks()) - i128::from(state.perp_short.ticks())).abs();
    let delta = quantity(i64::try_from(delta).map_err(|_| CarryError::Overflow)?)?;
    let bound = delta.ticks() > s.limits.max_net_btc.ticks()
        || notional(spot.max(perp), delta, true)? > s.limits.max_unhedged_notional.0;
    // Both operands are nonnegative i64 values; their product and rounded quotient fit i128.
    // Keep the original short-circuit/fallible bound above as the authority decision.
    let scaled_delta = i128::from(spot.max(perp).ticks()) * i128::from(delta.ticks());
    let unhedged_notional_ticks = scaled_delta / SCALE + i128::from(scaled_delta % SCALE != 0);
    let latch = (loss && !prior.loss) || (margin && !prior.margin) || (bound && !prior.delta);
    let epoch = if latch && !prior.latched {
        prior.epoch.checked_next().ok_or(CarryError::Overflow)?
    } else {
        prior.epoch
    };
    let first_halt = latch.then(|| {
        Box::new(CarryHaltEvidence {
            cause: HaltCause::RiskTrip,
            epoch,
            now: context.now,
            decision: context.decision,
            phase_before_refresh: context.phase,
            assessment: Some(CarryRiskAssessment {
                point: context.point,
                account: state.clone(),
                spot,
                perp,
                equity,
                session_loss_ticks,
                max_session_loss: s.limits.max_session_loss,
                margin_balance_ticks,
                margin_required,
                unhedged_qty: delta,
                unhedged_notional_ticks,
                notional_guard_evaluated: delta.ticks() <= s.limits.max_net_btc.ticks(),
                max_net_btc: s.limits.max_net_btc,
                max_unhedged_notional: s.limits.max_unhedged_notional,
                flags: CarryRiskFlags {
                    loss,
                    margin,
                    net_quantity: delta.ticks() > s.limits.max_net_btc.ticks(),
                    unhedged_notional: unhedged_notional_ticks
                        > i128::from(s.limits.max_unhedged_notional.0),
                },
            }),
        })
    });
    Ok(RiskStamp {
        first_halt,
        loss: prior.loss || loss,
        margin: prior.margin || margin,
        delta: bound,
        latch,
        epoch,
    })
}

fn reservations(
    s: &CarrySession,
    state: &CarryAccountState,
    projected: &Projection,
) -> Result<(Cash, Cash), CarryError> {
    let Some(plan) = &projected.pair else {
        return Ok((Cash(0), Cash(0)));
    };
    if state.spot_btc.ticks() == 0
        && state.perp_short.ticks() == 0
        && projected.all_complete
        && !projected.funding_pending
    {
        return Ok((Cash(0), Cash(0)));
    }
    let spot = if projected.spot_remaining > 0 || state.spot_btc.ticks() > 0 {
        add(
            spot_budget(s, quantity(projected.spot_remaining)?, plan.max_spot_price)?,
            s.margin.spot_close_buffer.0,
        )?
    } else {
        0
    };
    let short = if projected.exiting {
        state.perp_short
    } else if projected.spot_complete {
        state.spot_btc
    } else {
        plan.net_btc
    };
    let perp = if short.ticks() > 0 {
        let principal = bps(
            notional(plan.max_perp_price, short, true)?,
            s.margin.initial_bps,
        )?;
        let remaining_fee = taker_fee(
            plan.max_perp_price,
            quantity(projected.perp_entry_remaining)?,
            s.fees.perp_bps,
        )
        .map_err(|_| CarryError::Overflow)?
        .0;
        add(
            principal,
            add(
                remaining_fee,
                add(s.margin.perp_close_buffer.0, s.margin.perp_funding_buffer.0)?,
            )?,
        )?
    } else if !projected.perp_complete {
        add(s.margin.perp_close_buffer.0, s.margin.perp_funding_buffer.0)?
    } else if projected.funding_pending {
        s.margin.perp_funding_buffer.0
    } else {
        0
    };
    Ok((Cash(spot), Cash(perp)))
}

fn validate_session(s: &CarrySession) -> Result<(), CarryError> {
    let l = s.limits;
    let m = s.margin;
    if s.spot_grid.lot.ticks() == 0
        || s.perp_grid.lot.ticks() == 0
        || s.spot_grid.min_notional.0 < 0
        || s.perp_grid.min_notional.0 < 0
        || s.fees.spot_bps.0 >= 10_000
        || s.fees.perp_bps.0 >= 10_000
        || s.fees.spot_slippage.0 >= 10_000
        || s.fees.perp_slippage.0 >= 10_000
        || s.fees.spot_sell_asset != SpotFeeAsset::Usdt
        || l.max_net_btc.ticks() == 0
        || l.max_unhedged_notional.0 <= 0
        || l.max_session_loss.0 <= 0
        || l.max_mark_age_us == 0
        || l.max_legging_us == 0
        || l.max_orders == 0
        || l.max_receipts == 0
        || l.max_funding_events == 0
        || l.max_batch_entries == 0
        || m.initial_bps == 0
        || m.initial_bps > 10_000
        || m.maintenance_bps == 0
        || m.maintenance_bps > m.initial_bps
        || m.spot_close_buffer.0 < 0
        || m.perp_close_buffer.0 < 0
        || m.perp_funding_buffer.0 < 0
    {
        return Err(CarryError::InvalidSession);
    }
    for grid in [&s.spot_grid, &s.perp_grid] {
        if let RuleProvenance::VerifiedHistorical {
            effective_from,
            source,
        } = &grid.provenance
        {
            if *effective_from > s.initial_decision.micros()
                || source.is_empty()
                || source.len() > 256
            {
                return Err(CarryError::InvalidSession);
            }
        }
    }
    admitted(&s.initial_spot, s.initial_decision, l.max_mark_age_us)?;
    admitted(&s.initial_perp, s.initial_decision, l.max_mark_age_us)?;
    Ok(())
}

fn admitted(source: &Observation<Price>, d: DecisionTime, max_age: u64) -> Result<(), CarryError> {
    visible(source, d)?;
    if source.value().ticks() <= 0 {
        return Err(CarryError::InvalidEvidence);
    }
    let age = d
        .micros()
        .micros()
        .checked_sub(source.timestamps().event_ts.micros().micros())
        .ok_or(CarryError::Overflow)?;
    if u64::try_from(age).map_err(|_| CarryError::FutureObservation)? > max_age {
        return Err(CarryError::StaleMark);
    }
    Ok(())
}
fn visible<T>(source: &Observation<T>, d: DecisionTime) -> Result<(), CarryError> {
    let times = source.timestamps();
    if source.causal_at(d).is_none()
        || times.event_ts.micros() > d.micros()
        || times.received_ts.micros() > d.micros()
    {
        return Err(CarryError::FutureObservation);
    }
    Ok(())
}
fn same_observation<T: PartialEq>(a: &Observation<T>, b: &Observation<T>) -> bool {
    a.value() == b.value()
        && a.timestamps().event_ts == b.timestamps().event_ts
        && a.timestamps().available_ts == b.timestamps().available_ts
}
fn same_fill(a: &Observation<CarryFill>, b: &Observation<CarryFill>) -> bool {
    same_observation(a, b)
}
fn same_cut(a: &CarryTerminalCut, b: &CarryTerminalCut) -> bool {
    if a.order != b.order
        || a.status != b.status
        || a.cumulative != b.cumulative
        || !b.complete
        || a.receipts.len() != b.receipts.len()
    {
        return false;
    }
    a.receipts
        .iter()
        .all(|left| b.receipts.iter().any(|right| same_fill(left, right)))
}
fn order_id(pair: u128, stage: CarryStage) -> CarryOrderId {
    CarryOrderId {
        pair,
        stage,
        attempt: 0,
    }
}
fn market(stage: CarryStage) -> Market {
    match stage {
        CarryStage::SpotEntry | CarryStage::SpotExit => Market::Spot,
        CarryStage::PerpEntry | CarryStage::PerpExit => Market::UsdMPerp,
    }
}
fn side(stage: CarryStage) -> Side {
    match stage {
        CarryStage::SpotEntry | CarryStage::PerpExit => Side::Buy,
        CarryStage::PerpEntry | CarryStage::SpotExit => Side::Sell,
    }
}
fn cell(session: &CarrySession, now: Mono) -> CellContext<'_> {
    CellContext {
        now,
        authority: session.execution.authority(),
        max_attempts: session.execution.max_attempts(),
    }
}
fn quantity(ticks: i64) -> Result<Qty, CarryError> {
    Qty::from_ticks(ticks).map_err(|_| CarryError::Overflow)
}
fn add(a: i64, b: i64) -> Result<i64, CarryError> {
    a.checked_add(b).ok_or(CarryError::Overflow)
}
fn bps(value: i64, rate: u32) -> Result<i64, CarryError> {
    let num = i128::from(value)
        .checked_mul(i128::from(rate))
        .ok_or(CarryError::Overflow)?;
    i64::try_from(num / 10_000 + i128::from(num % 10_000 != 0)).map_err(|_| CarryError::Overflow)
}
fn fee_for(
    s: &CarrySession,
    market: Market,
    side: Side,
    qty: Qty,
    price: Price,
) -> Result<FeeCharge, CarryError> {
    let btc = market == Market::Spot
        && match side {
            Side::Buy => s.fees.spot_buy_asset == SpotFeeAsset::Btc,
            Side::Sell => s.fees.spot_sell_asset == SpotFeeAsset::Btc,
        };
    if btc {
        return Ok(FeeCharge::Btc(quantity(bps(
            qty.ticks(),
            s.fees.spot_bps.0,
        )?)?));
    }
    let rate = if market == Market::Spot {
        s.fees.spot_bps
    } else {
        s.fees.perp_bps
    };
    Ok(FeeCharge::Usdt(
        taker_fee(price, qty, rate).map_err(|_| CarryError::Overflow)?,
    ))
}
fn spot_budget(s: &CarrySession, qty: Qty, max: Price) -> Result<i64, CarryError> {
    let fee = match fee_for(s, Market::Spot, Side::Buy, qty, max)? {
        FeeCharge::Usdt(v) => v.0,
        FeeCharge::Btc(_) => 0,
    };
    add(notional(max, qty, true)?, fee)
}
fn perp_budget(s: &CarrySession, qty: Qty, max: Price) -> Result<i64, CarryError> {
    let principal = bps(notional(max, qty, true)?, s.margin.initial_bps)?;
    let fee = taker_fee(max, qty, s.fees.perp_bps)
        .map_err(|_| CarryError::Overflow)?
        .0;
    add(
        principal,
        add(
            fee,
            add(s.margin.perp_close_buffer.0, s.margin.perp_funding_buffer.0)?,
        )?,
    )
}
fn validate_fill(order: &TrackedOrder, source: &Observation<CarryFill>) -> Result<(), CarryError> {
    let f = source.value();
    if order.terminal.is_some()
        || !order.progress.sent()
        || f.fill_id == 0
        || f.qty.ticks() == 0
        || f.reference_price.ticks() <= 0
        || f.market != market(order.request.id.stage)
        || order
            .dispatched_at
            .is_none_or(|d| source.timestamps().event_ts.micros() < d.micros())
    {
        return Err(CarryError::InvalidEvidence);
    }
    Ok(())
}
fn account_fill(
    s: &CarrySession,
    request: &LegRequest,
    qty: Qty,
    reference: Price,
) -> Result<AccountFill, CarryError> {
    let market = market(request.id.stage);
    let side = side(request.id.stage);
    let slip = if market == Market::Spot {
        s.fees.spot_slippage
    } else {
        s.fees.perp_slippage
    };
    let price = fill_price(reference, side, slip).map_err(|_| CarryError::Overflow)?;
    account_fill_at_executed_price(s, request, qty, price)
}
fn account_fill_at_executed_price(
    s: &CarrySession,
    request: &LegRequest,
    qty: Qty,
    price: Price,
) -> Result<AccountFill, CarryError> {
    if price.ticks() > request.max_price.ticks() {
        return Err(CarryError::PriceEnvelope);
    }
    let market = market(request.id.stage);
    let side = side(request.id.stage);
    Ok(AccountFill {
        market,
        side,
        qty,
        price,
        fee: fee_for(s, market, side, qty, price)?,
    })
}
fn terminal_progress(
    progress: OrderProgress,
    cut: &CarryTerminalCut,
    cx: &CellContext<'_>,
) -> Result<OrderProgress, CarryError> {
    let target = match cut.status {
        CarryTerminal::Filled => OrderState::Filled,
        CarryTerminal::Canceled => OrderState::Canceled,
        CarryTerminal::Rejected => OrderState::Rejected,
        CarryTerminal::Expired => OrderState::Expired,
    };
    if progress.state() == OrderState::Unknown {
        let event = match cut.status {
            CarryTerminal::Filled => ValidatedEvent::QueryFilled {
                cum: cut.cumulative,
                evidence: cut.evidence_id,
            },
            CarryTerminal::Canceled => ValidatedEvent::QueryCanceled {
                cum: cut.cumulative,
                evidence: cut.evidence_id,
            },
            CarryTerminal::Expired => ValidatedEvent::QueryExpired {
                cum: cut.cumulative,
                evidence: cut.evidence_id,
            },
            CarryTerminal::Rejected => return Err(CarryError::IncompleteCut),
        };
        return transition(&progress, &event, cx).map_err(CarryError::Transition);
    }
    if progress.cum() != cut.cumulative {
        return Err(CarryError::IncompleteCut);
    }
    if progress.state() == target {
        return Ok(progress);
    }
    let event = match cut.status {
        CarryTerminal::Canceled => ValidatedEvent::CancelConfirmed,
        CarryTerminal::Expired => ValidatedEvent::Expire,
        CarryTerminal::Rejected => ValidatedEvent::Reject,
        CarryTerminal::Filled => return Err(CarryError::IncompleteCut),
    };
    transition(&progress, &event, cx).map_err(CarryError::Transition)
}

#[cfg(test)]
mod halt_diagnostic_tests {
    use super::*;
    use crate::{ApprovalTiming, ExecutionConfig, MaxAttempts, NotFoundAuthority, PositiveMicros};
    use parangsae_core::{AvailableTs, EventTs, ReceivedTs, Timestamps};
    use parangsae_sim::carry::Allocation;
    use parangsae_sim::{FeeBps, SlippageBps};

    #[test]
    fn wide_diagnostic_does_not_add_a_narrow_notional_refusal_to_the_qty_short_circuit() {
        let qty = |v| Qty::from_ticks(v).unwrap();
        let price = |v| Price::from_ticks(v).unwrap();
        let time = UtcMicros::from_micros(0);
        let observation = |v| {
            Observation::new(
                Timestamps {
                    event_ts: EventTs::from_micros(time),
                    available_ts: AvailableTs::from_micros(time),
                    received_ts: ReceivedTs::from_micros(time),
                },
                v,
            )
        };
        let age = PositiveMicros::new(1_000).unwrap();
        let s = CarrySession {
            allocation: Allocation {
                total: Cash(10_000),
                spot_quote: Cash(4_000),
                perp_collateral: Cash(5_000),
                idle: Cash(1_000),
            },
            fees: CarryFees {
                spot_bps: FeeBps(0),
                spot_buy_asset: SpotFeeAsset::Usdt,
                spot_sell_asset: SpotFeeAsset::Usdt,
                perp_bps: FeeBps(0),
                spot_slippage: SlippageBps(0),
                perp_slippage: SlippageBps(0),
            },
            spot_grid: CarryGrid {
                lot: qty(1),
                min_notional: Cash(0),
                provenance: RuleProvenance::AssumedResearch,
            },
            perp_grid: CarryGrid {
                lot: qty(1),
                min_notional: Cash(0),
                provenance: RuleProvenance::AssumedResearch,
            },
            limits: CarryLimits {
                max_net_btc: qty(100_000_000),
                max_unhedged_notional: Cash(100),
                max_session_loss: Cash(500),
                max_mark_age_us: 1_000,
                max_legging_us: 1_000,
                max_orders: 4,
                max_receipts: 4,
                max_funding_events: 4,
                max_batch_entries: 4,
            },
            margin: AssumedMarginPolicy {
                initial_bps: 10_000,
                maintenance_bps: 1,
                spot_close_buffer: Cash(0),
                perp_close_buffer: Cash(0),
                perp_funding_buffer: Cash(0),
            },
            execution: ExecutionConfig::new(
                ApprovalTiming::new(age, PositiveMicros::new(1).unwrap()).unwrap(),
                NotFoundAuthority::new(5_000, 1_000).unwrap(),
                MaxAttempts::new(1).unwrap(),
                age,
            )
            .unwrap(),
            initial_spot: observation(price(1)),
            initial_perp: observation(price(1)),
            initial_decision: DecisionTime::from_micros(time),
            initial_now: Mono::from_micros(0),
        };
        let mut state = CarryLedger::new(s.allocation).unwrap().snapshot();
        // Isolate the existing pure guard's extreme input; no plan/fill is fabricated.
        state.spot_btc = qty(300_000_000);
        assert!(notional(price(i64::MAX), state.spot_btc, true).is_err());
        let prior = RiskPrior {
            loss: false,
            margin: false,
            delta: false,
            epoch: HaltEpoch::ZERO,
            latched: false,
        };
        let context = AssessmentContext {
            now: Mono::from_micros(0),
            decision: s.initial_decision,
            phase: CarryPhase::ClosingPerp,
            point: CarryAssessmentPoint::Observation,
        };
        let stamp = assess_values(&s, &state, price(1), price(i64::MAX), prior, context).unwrap();
        assert!(stamp.latch && stamp.delta);
        let evidence = stamp.first_halt.unwrap();
        let a = evidence.assessment.unwrap();
        assert_eq!(a.unhedged_notional_ticks, 27_670_116_110_564_327_421);
        assert!(a.flags.net_quantity && a.flags.unhedged_notional);
        assert!(!a.notional_guard_evaluated);
        assert!(!a.flags.loss && !a.flags.margin);
        assert_eq!(a.margin_balance_ticks, None);
        assert_eq!(a.margin_required, None);
    }
}
