//! Funding and queued fills at the open. Delayed decisions follow the open;
//! undelayed closed-bar decisions follow the close mark.

use parangsae_core::{
    require_regular_grid, resample_series, DecisionTime, Interval, Observation, Qty, Side,
    TimeError, Timestamps, UtcMicros,
};
use parangsae_core::{AvailableTs, EventTs, ReceivedTs};
use parangsae_sim::{
    fill_price, slippage_cost, Cash, Costs, Fill, FundingEvent, Ledger, MarketOrder, SimError,
};

use crate::bar::{Bar, BarView, FundingStamp, Strategy, Tape, ONE_MINUTE_US};
use crate::cash_view::{CashVisibility, FundingDependency, VisibleEquity};
use crate::date::UtcDate;
use crate::error::EngineError;
use crate::metrics::{
    fill_notional_ticks, max_drawdown, sharpe_ticks, total_return, turnover_ticks,
};
use crate::report::{
    daily_equity, ForcedExit, ForcedExitReason, H4EntryDrop, H4EntryDropReason, H4Trace, H4Trade,
    Metrics, Report, RoundTrip,
};
use crate::signal::{
    funding_visible, ActualHold, BookState, Decision, ExecutionDirective, ExecutionPhase,
    FlatReason, FundingObs, H4Event, PositionSide, SignalOutput, TargetSignal,
};
use crate::sizing::target_qty;
use crate::tape::{sample_start, TapeError};

/// Headline fee is 5 bps. The grid reruns 2, 4, 5, and 6 with a fresh strategy.
pub const FEE_GRID_BPS: [u32; 4] = [2, 4, 5, 6];

pub fn run<S: Strategy>(
    tape: &Tape,
    strategy: &mut S,
    costs: &Costs,
    cfg: &RunConfig,
) -> Result<Report, EngineError> {
    if cfg.initial_cash.0 <= 0 {
        return Err(EngineError::Sim(SimError::Negative));
    }
    refuse_holdout(tape, cfg)?;
    validate(tape)?;
    let window = feed_window(&tape.bars, cfg)?;
    let step = funding_step(&tape.funding)?;
    require_coverage(tape, window, step, cfg)?;

    let mut ledger = Ledger::new(cfg.initial_cash);
    let mut book = Book::default();
    let mut fills = Vec::new();
    let mut funding = Vec::new();
    let mut equity_curve = Vec::with_capacity(window.len());
    let mut pending: Option<HeldOrder> = None;
    let mut funding_i = 0usize;
    let mut ruined: Option<UtcMicros> = None;
    let mut frozen: Option<Cash> = None;
    let mut dropped_orders = 0u64;
    let mut refused_fills = 0u64;
    let mut slippage_ticks = 0i64;

    let scored_bars = scored_len(window, cfg.start);
    for (index, bar) in window.iter().enumerate() {
        if bar.open_time < cfg.start {
            continue;
        }
        if ruined.is_none() {
            funding_i = settle_funding(
                &FundingPass {
                    tape,
                    gaps: &cfg.mark_gaps,
                    index: funding_i,
                },
                &mut ledger,
                &mut book,
                &mut funding,
                None,
                window,
                index,
            )?;
            settle_pending(
                &mut pending,
                &mut refused_fills,
                &mut FillSink {
                    fills: &mut fills,
                    slippage_ticks: &mut slippage_ticks,
                },
                &mut ledger,
                &mut book,
                bar,
                costs,
            )?;
        }

        let mark = resolve_mark(&tape.marks, bar.open_time, &cfg.mark_gaps)?;
        if let Some(cash) = frozen {
            equity_curve.push((bar.close_time, cash));
            continue;
        }

        let equity = ledger.equity(mark.close).map_err(EngineError::Sim)?;
        if equity.0 <= 0 {
            force_flat(
                &mut ledger,
                &mut book,
                &mut fills,
                &mut slippage_ticks,
                mark.close,
                costs,
                bar.close_time,
            )?;
            let after = ledger.equity(mark.close).map_err(EngineError::Sim)?;
            ruined = Some(bar.close_time);
            frozen = Some(after);
            equity_curve.push((bar.close_time, after));
            pending = None;
            continue;
        }

        equity_curve.push((bar.close_time, equity));
        let view = BarView::new(&window[..=index], bar.available);
        match strategy.on_bar(view) {
            Some(_) if index + 1 == window.len() => dropped_orders += 1,
            Some(order) => pending = Some(HeldOrder::new(PendingIntent::Fixed(order))),
            None => {}
        }
    }

    if pending.is_some() {
        dropped_orders += 1;
    }
    finish(
        cfg,
        tape,
        costs,
        scored_bars,
        Accum {
            ledger,
            book,
            equity_curve,
            fills,
            funding,
            ruined,
            dropped_orders,
            refused_fills,
            warmup_bars: 0,
            flat_funding_stale: 0,
            flat_funding_slot_missing: 0,
            signal_outputs: Vec::new(),
            slippage_ticks,
            n_rebalances: 0,
            n_sign_changes: 0,
            n_forced_exits: 0,
            forced_exits: Vec::new(),
            h4: None,
        },
    )
}

pub fn run_fee_grid<S, F>(
    tape: &Tape,
    mut make_strategy: F,
    base: &Costs,
    cfg: &RunConfig,
) -> Result<Vec<(parangsae_sim::FeeBps, Report)>, EngineError>
where
    S: Strategy,
    F: FnMut() -> S,
{
    let mut out = Vec::with_capacity(FEE_GRID_BPS.len());
    for bps in FEE_GRID_BPS {
        let costs = Costs {
            taker_fee: parangsae_sim::FeeBps(bps),
            slippage: base.slippage,
        };
        let mut strategy = make_strategy();
        out.push((
            parangsae_sim::FeeBps(bps),
            run(tape, &mut strategy, &costs, cfg)?,
        ));
    }
    Ok(out)
}

/// One signal's per-bar targets.
///
/// A target is sized once, from equity marked at the decision close, and that
/// quantity is held until the emitted target ticks change or `SignalOutput::accept`
/// changes. Decision equity stays frozen until sizing at the actual slipped
/// execution price. The fill itself is
/// the next 1m open whose `count` is greater than zero. One order that waits
/// through several `count = 0` minutes increments `n_refused_fills` once.
///
/// Bars from `cfg.warmup_start` up to `cfg.start` are fed through
/// [`TargetSignal::on_warmup_bar`]. That span has no decision, order, sizing,
/// fill, funding settlement, or equity point. A hold or held target built
/// there is not a position: the first scored bar decides from a flat book.
///
/// Calendar exits use the fixed scheduled minute immediately before the boundary.
/// Missing or unfillable held closes fail the whole run; later liquidity is never scanned.
pub fn run_targets<S: TargetSignal>(
    tape: &Tape,
    signal: &mut S,
    costs: &Costs,
    cfg: &RunConfig,
) -> Result<Report, EngineError> {
    if cfg.initial_cash.0 <= 0 || cfg.lot_ticks <= 0 {
        return Err(EngineError::Sim(SimError::Negative));
    }
    let delay = signal.decision_delay_us();
    if !matches!(delay, 0 | 1_000) {
        return Err(EngineError::Sim(SimError::Negative));
    }
    refuse_holdout(tape, cfg)?;
    validate(tape)?;
    let window = feed_window(&tape.bars, cfg)?;
    if let Some(bar) = window.iter().find(|bar| bar.open_time < sample_start()) {
        return Err(EngineError::BeforeSample(bar.open_time));
    }
    let step = funding_step(&tape.funding)?;
    require_coverage(tape, window, step, cfg)?;
    let resampled = resample_series(window, cfg.interval).map_err(EngineError::Resample)?;
    let width = usize::try_from(cfg.interval.micros() / ONE_MINUTE_US)
        .map_err(|_| EngineError::Sim(SimError::Overflow))?;
    let funding_rows: Vec<FundingObs> = tape
        .funding
        .iter()
        .map(|stamp| FundingObs {
            funding_time: stamp.funding_time,
            available: stamp.available,
            rate: stamp.rate,
        })
        .collect();
    let forced_opens: Vec<UtcMicros> = cfg
        .forced_flat
        .iter()
        .filter_map(|range| {
            window
                .iter()
                .find(|bar| bar.open_time >= range.start)
                .map(|bar| bar.open_time)
        })
        .collect();
    let window_flat = window_flat_open(window, cfg)?;
    let end_open = development_flat_open(window, cfg)?;

    let mut ledger = Ledger::new(cfg.initial_cash);
    let mut book = Book::default();
    let mut fills = Vec::new();
    let mut funding = Vec::new();
    let mut equity_curve = Vec::with_capacity(window.len());
    let mut pending: Option<HeldOrder> = None;
    let mut funding_i = 0usize;
    let mut ruined: Option<UtcMicros> = None;
    let mut frozen: Option<Cash> = None;
    let mut dropped_orders = 0u64;
    let mut refused_fills = 0u64;
    let mut sized: Option<(i64, i64, u64)> = None;
    let mut seen = false;
    let mut warmup_bars = 0u64;
    let mut flat_funding_stale = 0u64;
    let mut flat_funding_slot_missing = 0u64;
    let mut signal_outputs = Vec::new();
    let mut slippage_ticks = 0i64;
    let mut n_rebalances = 0u64;
    let mut n_sign_changes = 0u64;
    let mut n_forced_exits = 0u64;
    let mut forced_exits = Vec::new();
    let mut cash_visibility = CashVisibility::new(tape.funding.len());
    let trace_capacity = tape
        .funding
        .len()
        .checked_mul(parangsae_strategies::H4_OBSERVATIONS_PER_EVENT)
        .ok_or(EngineError::TraceCapacity { at: cfg.start })?;
    let mut h4 = (signal.h4_observation_capacity() > 0).then(|| H4Trace {
        scored: UtcRange {
            start: cfg.start,
            end: cfg.end,
        },
        observations: Vec::new(),
        trades: Vec::new(),
        entry_drops: Vec::new(),
    });
    book.trace_capacity = trace_capacity;

    let scored_bars = scored_len(window, cfg.start);
    for (index, bar) in window.iter().enumerate() {
        if bar.open_time < cfg.start {
            if width > 0 && (index + 1) % width == 0 && !resampled.is_empty() {
                let hour_index = (index + 1) / width - 1;
                let hour = &resampled[hour_index];
                let decision = hour
                    .close_time()
                    .checked_add_micros(1_000 + delay)
                    .ok_or(EngineError::Sim(SimError::Overflow))?;
                let bars_prefix = &resampled[..=hour_index];
                let fund_prefix = funding_visible(&funding_rows, decision);
                let bar_obs = Observation::new(decision_stamps(decision), bars_prefix);
                let fund_obs = Observation::new(decision_stamps(decision), fund_prefix);
                let view = bar_obs
                    .causal_at(DecisionTime::from_micros(decision))
                    .expect("the prefix is stamped available at the decision");
                let funding_view = fund_obs
                    .causal_at(DecisionTime::from_micros(decision))
                    .expect("the prefix is stamped available at the decision");
                if delay == 0 || decision < cfg.start {
                    signal.on_warmup_bar(view, funding_view);
                    drain_observations(signal, &mut h4, trace_capacity, decision)?;
                }
            }
            continue;
        }
        if frozen.is_none() {
            funding_i = settle_funding(
                &FundingPass {
                    tape,
                    gaps: &cfg.mark_gaps,
                    index: funding_i,
                },
                &mut ledger,
                &mut book,
                &mut funding,
                Some(&mut cash_visibility),
                window,
                index,
            )?;
            let halt = ruined.is_none()
                && control_wants_halt(
                    tape,
                    &ledger,
                    &mut cash_visibility,
                    bar.open_time,
                    index.checked_sub(1).map(|i| window[i].open_time),
                    &cfg.mark_gaps,
                )?;
            if halt {
                start_liquidation(
                    &ledger,
                    &mut pending,
                    &mut ruined,
                    &mut frozen,
                    bar.open_time,
                )?;
            }
            if window_flat == Some(bar.open_time)
                || end_open == Some(bar.open_time)
                || forced_opens.contains(&bar.open_time)
            {
                // A calendar boundary cancels risk even while the actual position is flat.
                if let Some(canceled) = pending.take() {
                    if let Some(entry) = canceled.entry {
                        record_entry_drop(
                            &mut h4,
                            H4EntryDrop {
                                event: entry.event,
                                at: bar.open_time,
                                reason: H4EntryDropReason::Terminal,
                            },
                            trace_capacity,
                        )?;
                    }
                    if ledger.position().0 == 0 {
                        dropped_orders = dropped_orders
                            .checked_add(1)
                            .ok_or(EngineError::Sim(SimError::Overflow))?;
                    }
                }
                if let Some(order) = flatten_order(&ledger)? {
                    let mut held = HeldOrder::new(PendingIntent::Fixed(order));
                    held.reason = OrderReason::Terminal;
                    held.trigger = Some(bar.open_time);
                    pending = Some(held);
                }
            }
            let before_settlement = ledger.position().0;
            let forced_exit = pending
                .as_ref()
                .is_some_and(|order| order.reason != OrderReason::Strategy);
            let forced_event = pending.as_ref().and_then(|order| {
                let reason = match order.reason {
                    OrderReason::Strategy => return None,
                    OrderReason::Terminal => ForcedExitReason::Terminal,
                    OrderReason::Ruin => ForcedExitReason::Ruin,
                };
                Some(ForcedExit {
                    trigger: order.trigger.expect("forced order has its trigger"),
                    filled_at: bar.open_time,
                    reason,
                })
            });
            let outcome = settle_pending(
                &mut pending,
                &mut refused_fills,
                &mut FillSink {
                    fills: &mut fills,
                    slippage_ticks: &mut slippage_ticks,
                },
                &mut ledger,
                &mut book,
                bar,
                costs,
            )?;
            if let Some(resolved) = outcome.sized {
                sized = Some(resolved);
            }
            if let Some(drop) = outcome.entry_drop {
                dropped_orders = dropped_orders
                    .checked_add(1)
                    .ok_or(EngineError::Sim(SimError::Overflow))?;
                record_entry_drop(&mut h4, drop, trace_capacity)?;
                sized = None;
            }
            note_settlement(
                &mut n_rebalances,
                &mut n_sign_changes,
                &mut n_forced_exits,
                forced_exit,
                before_settlement,
                ledger.position().0,
            )?;
            if before_settlement != 0 && ledger.position().0 == 0 {
                if let Some(event) = forced_event {
                    forced_exits.push(event);
                }
            }
            if ruined.is_some() && ledger.position().0 == 0 {
                frozen = Some(ledger.cash());
                pending = None;
            }
        }

        for before_close in [true, false] {
            if !before_close {
                // This is a retrospective economic curve label, never an action clock.
                let equity = if let Some(cash) = frozen {
                    cash
                } else {
                    let mark = resolve_mark(&tape.marks, bar.open_time, &cfg.mark_gaps)?;
                    ledger
                        .equity(mark.close)
                        .map_err(|_| EngineError::EconomicArithmetic { at: bar.close_time })?
                };
                equity_curve.push((bar.close_time, equity));
            }
            if ruined.is_some() {
                continue;
            }
            let hour_index = if before_close {
                if delay == 0 || width == 0 || index == 0 || index % width != 0 {
                    continue;
                }
                index / width - 1
            } else {
                if delay != 0 || width == 0 || (index + 1) % width != 0 {
                    continue;
                }
                (index + 1) / width - 1
            };
            if resampled.is_empty() {
                continue;
            }
            let hour = &resampled[hour_index];
            let decision = hour
                .close_time()
                .checked_add_micros(1_000 + delay)
                .ok_or(EngineError::Sim(SimError::Overflow))?;
            if control_wants_halt(
                tape,
                &ledger,
                &mut cash_visibility,
                decision,
                if before_close {
                    index.checked_sub(1).map(|i| window[i].open_time)
                } else {
                    Some(bar.open_time)
                },
                &cfg.mark_gaps,
            )? {
                // A zero-delay callback is the prevenue phase at the next open.
                // Its failed admission withholds this callback only. Pending and
                // final liquidation state stay unchanged until funding is booked
                // and the normal boundary control admits the complete money view.
                if delay == 0 {
                    continue;
                }
                start_liquidation(&ledger, &mut pending, &mut ruined, &mut frozen, decision)?;
                continue;
            }
            let bars_prefix = &resampled[..=hour_index];
            let fund_prefix = funding_visible(&funding_rows, decision);
            let bar_obs = Observation::new(decision_stamps(decision), bars_prefix);
            let fund_obs = Observation::new(decision_stamps(decision), fund_prefix);
            let view = bar_obs
                .causal_at(DecisionTime::from_micros(decision))
                .expect("the prefix is stamped available at the decision");
            let funding_view = fund_obs
                .causal_at(DecisionTime::from_micros(decision))
                .expect("the prefix is stamped available at the decision");
            let decision_close = hour.close();
            let visible_money = cash_visibility.project(&ledger, decision_close, decision)?;
            let sizing_equity = visible_money.equity();
            let execution = book.execution_phase(&ledger, pending.as_ref())?;
            let book_state = BookState::new(
                visible_money,
                ledger.position().0,
                decision_close.ticks(),
                execution,
            );
            let next = signal.on_bar(view, funding_view, book_state);
            drain_observations(signal, &mut h4, trace_capacity, decision)?;
            match next {
                None if !seen => warmup_bars += 1,
                None => return Err(EngineError::MissingSignal(decision)),
                Some(envelope) => {
                    let output = envelope.signal;
                    if output.bar_close_time_us != hour.close_time()
                        || output.decision_ts.micros() != decision
                    {
                        return Err(EngineError::MissingSignal(decision));
                    }
                    seen = true;
                    signal_outputs.push(output);
                    // Keep is an execution no-op even when its diagnostic target differs.
                    let entry = match envelope.directive {
                        ExecutionDirective::Keep => continue,
                        ExecutionDirective::SetTarget => None,
                        ExecutionDirective::Enter { event, hold } => {
                            if h4.is_none() {
                                return Err(EngineError::InvalidExecution);
                            }
                            if execution != ExecutionPhase::Flat
                                || pending.is_some()
                                || output.decision.target_ticks()
                                    != event.crowd_side.opposite().target_ticks()
                            {
                                return Err(EngineError::InvalidExecution);
                            }
                            Some(H4EntryPlan {
                                event,
                                hold,
                                development_end: cfg.holdout_start.min(sample_end()),
                            })
                        }
                    };
                    let inside = cfg
                        .forced_flat
                        .iter()
                        .any(|range| decision >= range.start && decision < range.end)
                        || in_window_flat(window_flat, cfg.flatten_before, bar.open_time)
                        || end_open.is_some_and(|open| {
                            bar.open_time >= open && bar.open_time <= sample_end_open()
                        });
                    match output.decision {
                        Decision::Flat(FlatReason::FundingStale) => flat_funding_stale += 1,
                        Decision::Flat(FlatReason::FundingSlotMissing) => {
                            flat_funding_slot_missing += 1
                        }
                        Decision::Target(_) => {}
                    }
                    let suppressed = inside || matches!(output.decision, Decision::Flat(_));
                    let key = (output.decision.target_ticks(), output.accept);
                    let resize = !suppressed
                        && key.0 != 0
                        && sized.is_none_or(|(target, _, accept)| (target, accept) != key);
                    let lots = held_lots(
                        &mut sized,
                        inside || matches!(output.decision, Decision::Flat(_)),
                        output.decision.target_ticks(),
                        output.accept,
                        sizing_equity.ticks(),
                        decision_close.ticks(),
                        cfg.lot_ticks,
                    )
                    .map_err(EngineError::Sim)?;
                    if book.open.as_ref().is_some_and(|open| open.h4.is_some())
                        && output.decision.target_ticks() != 0
                    {
                        return Err(EngineError::InvalidExecution);
                    }
                    // A sized intent has no provisional market side or quantity. An
                    // unchanged request retains frozen money and its refusal history.
                    if !suppressed
                        && pending.as_ref().is_some_and(|held| {
                            matches!(held.intent,
                        PendingIntent::Sized(sizing) if (sizing.target, sizing.accept) == key)
                        })
                    {
                        continue;
                    }
                    let diff = lots
                        .checked_sub(ledger.position().0)
                        .ok_or(EngineError::Sim(SimError::Overflow))?;
                    if diff == 0 && !resize {
                        pending = None;
                    } else {
                        let intent = if resize {
                            PendingIntent::Sized(FillSizing {
                                target: key.0,
                                equity: sizing_equity,
                                lot: cfg.lot_ticks,
                                accept: key.1,
                            })
                        } else {
                            let side = if diff > 0 { Side::Buy } else { Side::Sell };
                            let qty_ticks = diff
                                .checked_abs()
                                .ok_or(EngineError::Sim(SimError::Overflow))?;
                            let qty = Qty::from_ticks(qty_ticks)
                                .map_err(|_| EngineError::Sim(SimError::Negative))?;
                            PendingIntent::Fixed(MarketOrder { side, qty })
                        };
                        if resize || !pending.as_ref().is_some_and(|held| held.intent == intent) {
                            let mut held = HeldOrder::new(intent);
                            held.not_before = Some(decision);
                            held.entry = entry;
                            pending = Some(held);
                        }
                    }
                }
            }
        }
    }

    if pending
        .as_ref()
        .is_some_and(|order| order.reason == OrderReason::Terminal)
    {
        return Err(EngineError::UnfilledBoundaryClose {
            boundary: window_flat.or(end_open).unwrap_or(cfg.end),
        });
    }
    if ruined.is_none() {
        if let Some(last) = window.last() {
            let boundary = last
                .open_time
                .checked_add_micros(ONE_MINUTE_US)
                .ok_or(EngineError::Sim(SimError::Overflow))?;
            if control_wants_halt(
                tape,
                &ledger,
                &mut cash_visibility,
                boundary,
                Some(last.open_time),
                &cfg.mark_gaps,
            )? {
                start_liquidation(&ledger, &mut pending, &mut ruined, &mut frozen, boundary)?;
            }
        }
    }
    if let Some(trigger) = ruined {
        if ledger.position().0 != 0 {
            return Err(EngineError::UnfilledLiquidation { trigger });
        }
    }
    if pending.is_some() {
        dropped_orders += 1;
    }
    finish(
        cfg,
        tape,
        costs,
        scored_bars,
        Accum {
            ledger,
            book,
            equity_curve,
            fills,
            funding,
            ruined,
            dropped_orders,
            refused_fills,
            warmup_bars,
            flat_funding_stale,
            flat_funding_slot_missing,
            signal_outputs,
            slippage_ticks,
            n_rebalances,
            n_sign_changes,
            n_forced_exits,
            forced_exits,
            h4,
        },
    )
}

fn drain_observations<S: TargetSignal>(
    signal: &mut S,
    trace: &mut Option<H4Trace>,
    capacity: usize,
    at: UtcMicros,
) -> Result<(), EngineError> {
    let items = signal.take_h4_observations();
    if items.is_empty() {
        return Ok(());
    }
    let trace = trace.as_mut().ok_or(EngineError::InvalidExecution)?;
    if trace
        .observations
        .len()
        .checked_add(items.len())
        .is_none_or(|n| n > capacity)
    {
        return Err(EngineError::TraceCapacity { at });
    }
    trace.observations.extend(items);
    Ok(())
}

fn record_entry_drop(
    trace: &mut Option<H4Trace>,
    drop: H4EntryDrop,
    capacity: usize,
) -> Result<(), EngineError> {
    let trace = trace.as_mut().ok_or(EngineError::InvalidExecution)?;
    if trace.entry_drops.len() >= capacity {
        return Err(EngineError::TraceCapacity { at: drop.at });
    }
    trace.entry_drops.push(drop);
    Ok(())
}

/// Money control uses a published original mark at its actual use clock.
/// Economic curve labels never authorize a fill or a pending cancellation.
fn control_wants_halt(
    tape: &Tape,
    ledger: &Ledger,
    visibility: &mut CashVisibility,
    at: UtcMicros,
    economic_open: Option<UtcMicros>,
    gaps: &[MarkAbsence],
) -> Result<bool, EngineError> {
    let unknown_cash = visibility.unavailable_dependencies(at).next().is_some();
    let (price, economic) = if ledger.position().0 == 0 {
        // A known flat position has no price dependency; this is structural zero.
        (
            parangsae_core::Price::from_ticks(0).expect("zero price"),
            ledger.cash(),
        )
    } else {
        let original = resolve_mark(
            &tape.marks,
            economic_open.ok_or(EngineError::UnavailableControlMark { at })?,
            gaps,
        )?;
        if original.available > at || original.close_time >= at {
            return Err(EngineError::UnavailableControlMark { at });
        }
        let economic = ledger
            .equity(original.close)
            .map_err(|_| EngineError::EconomicArithmetic { at })?;
        (original.close, economic)
    };
    let visible = visibility.project(ledger, price, at)?;
    let visible_equity = if ledger.position().0 == 0 {
        visible.cash().0
    } else {
        visible.equity().ticks()
    };
    if (economic.0 <= 0) != (visible_equity <= 0) {
        return Err(if unknown_cash {
            EngineError::UnadmittedMoneyControl { at }
        } else {
            EngineError::InvalidExecution
        });
    }
    Ok(visible_equity <= 0)
}

fn start_liquidation(
    ledger: &Ledger,
    pending: &mut Option<HeldOrder>,
    ruined: &mut Option<UtcMicros>,
    frozen: &mut Option<Cash>,
    trigger: UtcMicros,
) -> Result<(), EngineError> {
    *ruined = Some(trigger);
    *pending = None;
    if let Some(order) = flatten_order(ledger)? {
        let mut close = HeldOrder::new(PendingIntent::Fixed(order));
        close.not_before = Some(
            trigger
                .checked_add_micros(1)
                .ok_or(EngineError::EconomicArithmetic { at: trigger })?,
        );
        close.reason = OrderReason::Ruin;
        close.trigger = Some(trigger);
        *pending = Some(close);
    } else {
        *frozen = Some(ledger.cash());
    }
    Ok(())
}

/// Count realized position changes once per settled order, after both fill legs.
fn note_settlement(
    n_rebalances: &mut u64,
    n_sign_changes: &mut u64,
    n_forced_exits: &mut u64,
    forced_exit: bool,
    old_lots: i64,
    new_lots: i64,
) -> Result<(), EngineError> {
    if old_lots == new_lots {
        return Ok(());
    }
    if forced_exit {
        *n_forced_exits = n_forced_exits
            .checked_add(1)
            .ok_or(EngineError::Sim(SimError::Overflow))?;
        return Ok(());
    }
    *n_rebalances = n_rebalances
        .checked_add(1)
        .ok_or(EngineError::Sim(SimError::Overflow))?;
    if old_lots != 0 && new_lots != 0 && old_lots.signum() != new_lots.signum() {
        *n_sign_changes = n_sign_changes
            .checked_add(1)
            .ok_or(EngineError::Sim(SimError::Overflow))?;
    }
    Ok(())
}

/// Lots held from the last sizing. The same target and the same `accept` do not resize.
/// A flat decision or a forced-flat window clears the hold so the next target sizes again.
fn held_lots(
    sized: &mut Option<(i64, i64, u64)>,
    suppress: bool,
    target_ticks: i64,
    accept: u64,
    equity_ticks: i64,
    mark_ticks: i64,
    lot_ticks: i64,
) -> Result<i64, SimError> {
    if suppress {
        *sized = None;
        return Ok(0);
    }
    if let Some((held_ticks, held_lots, held_accept)) = *sized {
        if held_ticks == target_ticks && held_accept == accept {
            return Ok(held_lots);
        }
    }
    let lots = target_qty(target_ticks, equity_ticks, mark_ticks, lot_ticks)?;
    *sized = Some((target_ticks, lots, accept));
    Ok(lots)
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FillSizing {
    target: i64,
    equity: VisibleEquity,
    lot: i64,
    accept: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingIntent {
    Fixed(MarketOrder),
    Sized(FillSizing),
}

struct HeldOrder {
    intent: PendingIntent,
    refused: bool,
    not_before: Option<UtcMicros>,
    reason: OrderReason,
    trigger: Option<UtcMicros>,
    entry: Option<H4EntryPlan>,
}

#[derive(Clone, Copy)]
struct H4EntryPlan {
    event: H4Event,
    hold: ActualHold,
    development_end: UtcMicros,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OrderReason {
    Strategy,
    Terminal,
    Ruin,
}

impl HeldOrder {
    fn new(intent: PendingIntent) -> Self {
        Self {
            intent,
            refused: false,
            not_before: None,
            reason: OrderReason::Strategy,
            trigger: None,
            entry: None,
        }
    }
}

struct FillSink<'a> {
    fills: &'a mut Vec<Fill>,
    slippage_ticks: &'a mut i64,
}

#[derive(Default)]
struct PendingOutcome {
    sized: Option<(i64, i64, u64)>,
    entry_drop: Option<H4EntryDrop>,
}

fn settle_pending(
    pending: &mut Option<HeldOrder>,
    refused_fills: &mut u64,
    sink: &mut FillSink<'_>,
    ledger: &mut Ledger,
    book: &mut Book,
    bar: &Bar,
    costs: &Costs,
) -> Result<PendingOutcome, EngineError> {
    let Some(mut held) = pending.take() else {
        return Ok(PendingOutcome::default());
    };
    if held.not_before.is_some_and(|time| bar.open_time < time) {
        *pending = Some(held);
        return Ok(PendingOutcome::default());
    }
    if bar.count == 0 {
        if held.reason == OrderReason::Terminal {
            return Err(EngineError::UnfilledBoundaryClose {
                boundary: bar.open_time,
            });
        }
        if !held.refused {
            *refused_fills = refused_fills
                .checked_add(1)
                .ok_or(EngineError::Sim(SimError::Overflow))?;
            held.refused = true;
        }
        *pending = Some(held);
        return Ok(PendingOutcome::default());
    }
    if let Some(entry) = held.entry {
        if ledger.position().0 != 0 {
            return Err(EngineError::InvalidExecution);
        }
        let exit = entry
            .hold
            .exit_decision(bar.open_time)
            .map_err(|_| EngineError::EconomicArithmetic { at: bar.open_time })?;
        if exit.micros() >= entry.development_end {
            return Ok(PendingOutcome {
                sized: None,
                entry_drop: Some(H4EntryDrop {
                    event: entry.event,
                    at: bar.open_time,
                    reason: H4EntryDropReason::DevelopmentBoundary,
                }),
            });
        }
    }
    let mut resolved = None;
    let order = match held.intent {
        PendingIntent::Fixed(order) => order,
        PendingIntent::Sized(sizing) => {
            let mut chosen = None;
            // A rebalance may buy or sell an existing position. Use the same adverse
            // execution-price function as the ledger, then floor to the lot grid.
            for side in [Side::Buy, Side::Sell] {
                let price = fill_price(bar.open, side, costs.slippage).map_err(EngineError::Sim)?;
                let lots = target_qty(
                    sizing.target,
                    sizing.equity.ticks(),
                    price.ticks(),
                    sizing.lot,
                )
                .map_err(EngineError::Sim)?;
                let diff = lots
                    .checked_sub(ledger.position().0)
                    .ok_or(EngineError::Sim(SimError::Overflow))?;
                if (side == Side::Buy && diff > 0) || (side == Side::Sell && diff < 0) {
                    let qty = Qty::from_ticks(
                        diff.checked_abs()
                            .ok_or(EngineError::Sim(SimError::Overflow))?,
                    )
                    .map_err(|_| EngineError::Sim(SimError::Negative))?;
                    chosen = Some((MarketOrder { side, qty }, lots));
                    break;
                }
            }
            let Some((order, lots)) = chosen else {
                return Ok(PendingOutcome {
                    sized: Some((sizing.target, ledger.position().0, sizing.accept)),
                    entry_drop: held.entry.map(|entry| H4EntryDrop {
                        event: entry.event,
                        at: bar.open_time,
                        reason: H4EntryDropReason::ZeroLots,
                    }),
                });
            };
            resolved = Some((sizing.target, lots, sizing.accept));
            order
        }
    };
    let first_fill = sink.fills.len();
    let first_slippage = *sink.slippage_ticks;
    execute(ledger, book, sink, order, bar.open, costs, bar.open_time)?;
    if let Some(entry) = held.entry {
        let open = book.open.as_mut().ok_or(EngineError::InvalidExecution)?;
        if sink.fills.len() != first_fill + 1 {
            return Err(EngineError::InvalidExecution);
        }
        open.h4 = Some(OpenH4 {
            plan: entry,
            funding: Vec::new(),
            fills: vec![sink.fills[first_fill]],
            slippage_cost: Cash(
                sink.slippage_ticks
                    .checked_sub(first_slippage)
                    .ok_or(EngineError::EconomicArithmetic { at: bar.open_time })?,
            ),
        });
    }
    Ok(PendingOutcome {
        sized: resolved,
        entry_drop: None,
    })
}

/// Fixed scheduled development-terminal open; a missing key is an error.
fn development_flat_open(
    window: &[Bar],
    cfg: &RunConfig,
) -> Result<Option<UtcMicros>, EngineError> {
    let limit = sample_end_open();
    if cfg.start > limit || cfg.end <= limit {
        return Ok(None);
    }
    require_terminal_key(window, limit)?;
    Ok(Some(limit))
}

fn require_terminal_key(window: &[Bar], key: UtcMicros) -> Result<(), EngineError> {
    if window
        .binary_search_by_key(&key, |bar| bar.open_time)
        .is_err()
    {
        return Err(EngineError::UnfilledBoundaryClose { boundary: key });
    }
    Ok(())
}

fn sample_end() -> UtcMicros {
    UtcDate::parse("2025-10-01")
        .expect("valid development boundary")
        .start_us()
}

fn sample_end_open() -> UtcMicros {
    UtcMicros::from_micros(sample_end().micros() - ONE_MINUTE_US)
}

fn in_window_flat(open: Option<UtcMicros>, limit: Option<UtcMicros>, bar_open: UtcMicros) -> bool {
    match (open, limit) {
        (Some(open), Some(limit)) => bar_open >= open && bar_open < limit,
        _ => false,
    }
}

fn decision_stamps(decision: UtcMicros) -> Timestamps {
    Timestamps {
        event_ts: EventTs::from_micros(decision),
        available_ts: AvailableTs::from_micros(decision),
        received_ts: ReceivedTs::from_micros(decision),
    }
}

fn flatten_order(ledger: &Ledger) -> Result<Option<MarketOrder>, EngineError> {
    let pos = ledger.position().0;
    if pos == 0 {
        return Ok(None);
    }
    let side = if pos > 0 { Side::Sell } else { Side::Buy };
    let lots = pos
        .checked_abs()
        .ok_or(EngineError::Sim(SimError::Overflow))?;
    let qty = Qty::from_ticks(lots).map_err(|_| EngineError::Sim(SimError::Negative))?;
    Ok(Some(MarketOrder { side, qty }))
}

/// Inclusive UTC range. `end` is exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UtcRange {
    pub start: UtcMicros,
    pub end: UtcMicros,
}

/// A mark-price hole. Only a short `known_source_absence` may carry the last mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkAbsence {
    pub first_open: UtcMicros,
    pub last_open: UtcMicros,
    pub known_source_absence: bool,
}

/// Inclusive start, exclusive end, on bar open time.
///
/// `holdout_start`, `forced_flat`, and `mark_gaps` have no default: the caller
/// writes them down, including an empty list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunConfig {
    pub initial_cash: Cash,
    /// Lookback fed to the signal. Fills, funding, and equity begin at [`Self::start`].
    pub warmup_start: UtcMicros,
    pub start: UtcMicros,
    pub end: UtcMicros,
    pub holdout_start: UtcMicros,
    pub forced_flat: Vec<UtcRange>,
    /// Flatten at the scheduled minute immediately before this instant.
    /// `None` does not add a window-end flatten.
    pub flatten_before: Option<UtcMicros>,
    pub mark_gaps: Vec<MarkAbsence>,
    pub interval: Interval,
    pub lot_ticks: i64,
}

fn validate(tape: &Tape) -> Result<(), EngineError> {
    ensure_strict(tape.bars.iter().map(|bar| bar.open_time))?;
    let opens: Vec<UtcMicros> = tape.bars.iter().map(|bar| bar.open_time).collect();
    require_regular_grid(&opens, Interval::ONE_MINUTE).map_err(|err| match err {
        TimeError::GridGap { expected, found } => EngineError::Gap {
            expected,
            got: found,
        },
        TimeError::Overflow | TimeError::CloseTime { .. } => EngineError::Sim(SimError::Overflow),
    })?;
    ensure_strict(tape.marks.iter().map(|bar| bar.open_time))?;
    ensure_strict(tape.funding.iter().map(|stamp| stamp.funding_time))?;
    let step = funding_step(&tape.funding)?;
    for stamp in &tape.funding {
        if stamp.funding_time.micros().rem_euclid(step) != 0 {
            return Err(EngineError::Tape(TapeError::OffGrid {
                funding_time: stamp.funding_time,
            }));
        }
    }
    Ok(())
}

fn ensure_strict(times: impl IntoIterator<Item = UtcMicros>) -> Result<(), EngineError> {
    let mut previous: Option<UtcMicros> = None;
    for ts in times {
        if let Some(seen) = previous {
            if ts <= seen {
                return Err(EngineError::Unsorted);
            }
        }
        previous = Some(ts);
    }
    Ok(())
}

fn funding_step(funding: &[FundingStamp]) -> Result<i64, EngineError> {
    let hours = funding
        .first()
        .map(|stamp| stamp.interval_hours)
        .unwrap_or(8);
    if hours <= 0 {
        return Err(EngineError::Tape(TapeError::FundingInterval { got: hours }));
    }
    for stamp in funding {
        if stamp.interval_hours != hours {
            return Err(EngineError::Tape(TapeError::FundingInterval {
                got: stamp.interval_hours,
            }));
        }
    }
    hours
        .checked_mul(3_600_000_000)
        .ok_or(EngineError::Sim(SimError::Overflow))
}

fn feed_window<'a>(bars: &'a [Bar], cfg: &RunConfig) -> Result<&'a [Bar], EngineError> {
    if cfg.warmup_start > cfg.start {
        return Err(EngineError::Warmup(cfg.warmup_start));
    }
    if cfg.warmup_start >= cfg.holdout_start {
        return Err(EngineError::HoldoutBar(cfg.warmup_start));
    }
    Ok(time_window(bars, cfg.warmup_start, cfg.end))
}

fn scored_len(window: &[Bar], start: UtcMicros) -> usize {
    window.iter().filter(|bar| bar.open_time >= start).count()
}

fn time_window(bars: &[Bar], start: UtcMicros, end: UtcMicros) -> &[Bar] {
    let start_i = bars
        .iter()
        .position(|bar| bar.open_time >= start)
        .unwrap_or(bars.len());
    let end_i = bars[start_i..]
        .iter()
        .position(|bar| bar.open_time >= end)
        .map(|offset| start_i + offset)
        .unwrap_or(bars.len());
    &bars[start_i..end_i]
}

fn require_coverage(
    tape: &Tape,
    window: &[Bar],
    step: i64,
    cfg: &RunConfig,
) -> Result<(), EngineError> {
    for bar in window {
        resolve_mark(&tape.marks, bar.open_time, &cfg.mark_gaps)?;
    }
    let times = expected_funding(window, step)?;
    let have: Vec<i64> = tape
        .funding
        .iter()
        .map(|stamp| stamp.funding_time.micros())
        .collect();
    for ts in times {
        if have.binary_search(&ts).is_err() {
            return Err(EngineError::MissingFunding(UtcMicros::from_micros(ts)));
        }
        resolve_mark(&tape.marks, UtcMicros::from_micros(ts), &cfg.mark_gaps)?;
    }
    Ok(())
}

fn expected_funding(window: &[Bar], step: i64) -> Result<Vec<i64>, EngineError> {
    if window.is_empty() {
        return Ok(Vec::new());
    }
    let mut grid = next_grid(bar_prev(window, 0)?, step)?;
    let last = window
        .last()
        .expect("window is non-empty")
        .open_time
        .micros();
    let mut out = Vec::new();
    while grid <= last {
        out.push(grid);
        match grid.checked_add(step) {
            Some(next) => grid = next,
            None => break,
        }
    }
    Ok(out)
}

fn bar_prev(window: &[Bar], index: usize) -> Result<i64, EngineError> {
    if index == 0 {
        window[0]
            .open_time
            .micros()
            .checked_sub(ONE_MINUTE_US)
            .ok_or(EngineError::Sim(SimError::Overflow))
    } else {
        Ok(window[index - 1].open_time.micros())
    }
}

fn next_grid(prev: i64, step: i64) -> Result<i64, EngineError> {
    if step <= 0 {
        return Err(EngineError::Sim(SimError::Overflow));
    }
    let rem = prev.rem_euclid(step);
    let delta = if rem == 0 { step } else { step - rem };
    prev.checked_add(delta)
        .ok_or(EngineError::Sim(SimError::Overflow))
}

fn resolve_mark<'a>(
    marks: &'a [Bar],
    open: UtcMicros,
    gaps: &[MarkAbsence],
) -> Result<&'a Bar, EngineError> {
    if let Ok(index) = marks.binary_search_by(|bar| bar.open_time.cmp(&open)) {
        return Ok(&marks[index]);
    }
    let Some(gap) = gaps
        .iter()
        .find(|gap| gap.first_open <= open && open <= gap.last_open)
    else {
        return Err(EngineError::MissingMark(open));
    };
    let minutes = gap_minutes(gap)?;
    if !gap.known_source_absence || minutes > 5 {
        return Err(EngineError::MissingMark(open));
    }
    let carry = marks.partition_point(|bar| bar.open_time < gap.first_open);
    if carry == 0 {
        return Err(EngineError::MissingMark(open));
    }
    Ok(&marks[carry - 1])
}

fn gap_minutes(gap: &MarkAbsence) -> Result<i64, EngineError> {
    let span = gap
        .last_open
        .micros()
        .checked_sub(gap.first_open.micros())
        .ok_or(EngineError::Sim(SimError::Overflow))?;
    if span < 0 || span % ONE_MINUTE_US != 0 {
        return Err(EngineError::MissingMark(gap.first_open));
    }
    Ok(span / ONE_MINUTE_US + 1)
}

fn refuse_holdout(tape: &Tape, cfg: &RunConfig) -> Result<(), EngineError> {
    for bar in tape.bars.iter().chain(tape.marks.iter()) {
        if bar.open_time >= cfg.holdout_start {
            return Err(EngineError::HoldoutBar(bar.open_time));
        }
    }
    Ok(())
}

fn window_flat_open(window: &[Bar], cfg: &RunConfig) -> Result<Option<UtcMicros>, EngineError> {
    let Some(end) = cfg.flatten_before else {
        return Ok(None);
    };
    let key = end
        .checked_add_micros(-ONE_MINUTE_US)
        .ok_or(EngineError::EconomicArithmetic { at: end })?;
    if key < cfg.start || key >= cfg.end {
        return Err(EngineError::UnfilledBoundaryClose { boundary: key });
    }
    require_terminal_key(window, key)?;
    Ok(Some(key))
}

struct FundingPass<'a> {
    tape: &'a Tape,
    gaps: &'a [MarkAbsence],
    index: usize,
}

fn settle_funding(
    pass: &FundingPass<'_>,
    ledger: &mut Ledger,
    book: &mut Book,
    events: &mut Vec<FundingEvent>,
    mut visibility: Option<&mut CashVisibility>,
    window: &[Bar],
    bar_index: usize,
) -> Result<usize, EngineError> {
    let prev = bar_prev(window, bar_index)?;
    let open = window[bar_index].open_time.micros();
    let tape = pass.tape;
    let mut funding_i = pass.index;
    while funding_i < tape.funding.len() && tape.funding[funding_i].funding_time.micros() <= prev {
        funding_i += 1;
    }
    while funding_i < tape.funding.len() && tape.funding[funding_i].funding_time.micros() <= open {
        let stamp = &tape.funding[funding_i];
        let mark = resolve_mark(&tape.marks, stamp.funding_time, pass.gaps)?;
        let checked_money = visibility.is_some();
        let reservation = visibility
            .as_deref_mut()
            .map(|view| view.reserve(FundingDependency::from_sources(stamp, mark)))
            .transpose()?;
        let arithmetic_error = |error| {
            if checked_money {
                EngineError::EconomicArithmetic {
                    at: stamp.funding_time,
                }
            } else {
                EngineError::Sim(error)
            }
        };
        if book
            .open
            .as_ref()
            .and_then(|open| open.h4.as_ref())
            .is_some_and(|raw| raw.funding.len() >= book.trace_capacity)
        {
            return Err(EngineError::TraceCapacity {
                at: stamp.funding_time,
            });
        }
        let mut staged_ledger = ledger.clone();
        let event = staged_ledger
            .apply_funding(stamp.rate, mark.open, stamp.funding_time)
            .map_err(arithmetic_error)?;
        let next_pnl = book
            .open
            .as_ref()
            .map(|open| {
                open.pnl
                    .checked_add(i128::from(event.amount.0))
                    .ok_or_else(|| arithmetic_error(SimError::Overflow))
            })
            .transpose()?;
        if let Some(reservation) = reservation {
            reservation.commit(event.amount);
        }
        *ledger = staged_ledger;
        if let Some(pnl) = next_pnl {
            book.open.as_mut().expect("staged open trade").pnl = pnl;
        }
        if let Some(raw) = book.open.as_mut().and_then(|open| open.h4.as_mut()) {
            raw.funding.push(event);
        }
        events.push(event);
        funding_i += 1;
    }
    Ok(funding_i)
}

fn force_flat(
    ledger: &mut Ledger,
    book: &mut Book,
    fills: &mut Vec<Fill>,
    slippage_ticks: &mut i64,
    price: parangsae_core::Price,
    costs: &Costs,
    ts: UtcMicros,
) -> Result<(), EngineError> {
    let pos = ledger.position().0;
    if pos == 0 {
        return Ok(());
    }
    let side = if pos > 0 { Side::Sell } else { Side::Buy };
    let lots = pos
        .checked_abs()
        .ok_or(EngineError::Sim(SimError::Overflow))?;
    let qty = Qty::from_ticks(lots).map_err(|_| EngineError::Sim(SimError::Negative))?;
    execute(
        ledger,
        book,
        &mut FillSink {
            fills,
            slippage_ticks,
        },
        MarketOrder { side, qty },
        price,
        costs,
        ts,
    )
}

struct Leg {
    side: Side,
    lots: i64,
    price: parangsae_core::Price,
    ts: UtcMicros,
}

fn execute(
    ledger: &mut Ledger,
    book: &mut Book,
    sink: &mut FillSink<'_>,
    order: MarketOrder,
    price: parangsae_core::Price,
    costs: &Costs,
    ts: UtcMicros,
) -> Result<(), EngineError> {
    let (close_qty, open_qty) = split_legs(ledger.position().0, order.side, order.qty.ticks())?;
    let mut staged_ledger = ledger.clone();
    let mut staged_book = Book {
        closed: Vec::with_capacity(1),
        open: book.open.clone(),
        trace_capacity: book.trace_capacity,
    };
    let mut staged_fills = Vec::with_capacity(2);
    let mut staged_slippage = *sink.slippage_ticks;
    let mut staged_sink = FillSink {
        fills: &mut staged_fills,
        slippage_ticks: &mut staged_slippage,
    };
    for lots in [close_qty, open_qty] {
        if lots > 0 {
            apply_leg(
                &mut staged_ledger,
                &mut staged_book,
                &mut staged_sink,
                costs,
                Leg {
                    side: order.side,
                    lots,
                    price,
                    ts,
                },
            )?;
        }
    }
    if staged_book.closed.iter().any(|done| done.h4.is_some())
        && book
            .closed
            .len()
            .checked_add(staged_book.closed.len())
            .is_none_or(|n| n > book.trace_capacity)
    {
        return Err(EngineError::TraceCapacity { at: ts });
    }
    *ledger = staged_ledger;
    book.open = staged_book.open;
    book.closed.extend(staged_book.closed);
    sink.fills.extend(staged_fills);
    *sink.slippage_ticks = staged_slippage;
    Ok(())
}

fn split_legs(pos: i64, side: Side, qty: i64) -> Result<(i64, i64), EngineError> {
    if qty <= 0 {
        return Err(EngineError::Sim(SimError::ZeroQty));
    }
    let reducing = match side {
        Side::Buy => pos < 0,
        Side::Sell => pos > 0,
    };
    if !reducing {
        return Ok((0, qty));
    }
    let abs_pos = pos
        .checked_abs()
        .ok_or(EngineError::Sim(SimError::Overflow))?;
    let close_qty = qty.min(abs_pos);
    Ok((close_qty, qty - close_qty))
}

fn apply_leg(
    ledger: &mut Ledger,
    book: &mut Book,
    sink: &mut FillSink<'_>,
    costs: &Costs,
    leg: Leg,
) -> Result<(), EngineError> {
    let qty = Qty::from_ticks(leg.lots).map_err(|_| EngineError::Sim(SimError::Negative))?;
    let cost = slippage_cost(leg.price, leg.side, qty, costs.slippage).map_err(EngineError::Sim)?;
    *sink.slippage_ticks = sink
        .slippage_ticks
        .checked_add(cost.0)
        .ok_or(EngineError::Sim(SimError::Overflow))?;
    let pos_before = ledger.position().0;
    let cash_before = i128::from(ledger.cash().0);
    let fill = ledger
        .apply_fill(
            MarketOrder {
                side: leg.side,
                qty,
            },
            leg.price,
            costs,
            leg.ts,
        )
        .map_err(EngineError::Sim)?;
    let delta = i128::from(ledger.cash().0) - cash_before;
    book.note_fill(pos_before, &fill, delta, cost)?;
    sink.fills.push(fill);
    Ok(())
}

struct Accum {
    ledger: Ledger,
    book: Book,
    equity_curve: Vec<(UtcMicros, Cash)>,
    fills: Vec<Fill>,
    funding: Vec<FundingEvent>,
    ruined: Option<UtcMicros>,
    dropped_orders: u64,
    refused_fills: u64,
    warmup_bars: u64,
    flat_funding_stale: u64,
    flat_funding_slot_missing: u64,
    signal_outputs: Vec<SignalOutput>,
    slippage_ticks: i64,
    n_rebalances: u64,
    n_sign_changes: u64,
    n_forced_exits: u64,
    forced_exits: Vec<ForcedExit>,
    h4: Option<H4Trace>,
}

fn finish(
    cfg: &RunConfig,
    tape: &Tape,
    costs: &Costs,
    bars: usize,
    accum: Accum,
) -> Result<Report, EngineError> {
    let Accum {
        ledger,
        book,
        equity_curve,
        fills,
        funding,
        ruined,
        dropped_orders,
        refused_fills,
        warmup_bars,
        flat_funding_stale,
        flat_funding_slot_missing,
        signal_outputs,
        slippage_ticks,
        n_rebalances,
        n_sign_changes,
        n_forced_exits,
        forced_exits,
        mut h4,
    } = accum;
    let (trades, h4_trades) = book.into_trades()?;
    if let Some(trace) = h4.as_mut() {
        trace.trades = h4_trades;
    } else if !h4_trades.is_empty() {
        return Err(EngineError::InvalidExecution);
    }
    let daily = daily_equity(&equity_curve);
    let end = equity_curve
        .last()
        .map(|(_, cash)| *cash)
        .unwrap_or(cfg.initial_cash);
    let mut levels = Vec::with_capacity(daily.len() + 1);
    levels.push(cfg.initial_cash.0);
    levels.extend(daily.iter().map(|(_, cash)| cash.0));
    let (sharpe_daily, sharpe_annual) = sharpe_ticks(&levels).map_err(EngineError::Sim)?;
    let points: Vec<Cash> = equity_curve.iter().map(|(_, cash)| *cash).collect();
    let mut notional = 0i128;
    for fill in &fills {
        notional +=
            fill_notional_ticks(fill.price.ticks(), fill.qty.ticks()).map_err(EngineError::Sim)?;
    }
    let daily_ticks: Vec<i64> = daily.iter().map(|(_, cash)| cash.0).collect();
    let turnover = turnover_ticks(notional, &daily_ticks).map_err(EngineError::Sim)?;
    let metrics = Metrics {
        days: u32::try_from(daily.len()).map_err(|_| EngineError::Sim(SimError::Overflow))?,
        total_return: total_return(cfg.initial_cash, end).map_err(EngineError::Sim)?,
        max_drawdown: max_drawdown(&points).map_err(EngineError::Sim)?,
        sharpe_daily,
        sharpe_annual,
        turnover,
        fees_paid: ledger.fees_paid(),
        funding_net: ledger.funding_net(),
        fills: u32::try_from(fills.len()).map_err(|_| EngineError::Sim(SimError::Overflow))?,
        trades: u32::try_from(trades.len()).map_err(|_| EngineError::Sim(SimError::Overflow))?,
    };
    Ok(Report {
        equity_curve,
        daily_equity: daily,
        fills,
        funding,
        trades,
        metrics,
        costs: *costs,
        bars: u64::try_from(bars).map_err(|_| EngineError::Sim(SimError::Overflow))?,
        ruined,
        dropped_orders,
        n_refused_fills: refused_fills,
        slippage_cost: Cash(slippage_ticks),
        n_rebalances,
        n_sign_changes,
        n_forced_exits,
        forced_exits,
        h4,
        warmup_bars,
        flat_funding_stale,
        flat_funding_slot_missing,
        signal_outputs,
        overlays: tape.overlays.clone(),
        mark_gaps: cfg.mark_gaps.clone(),
        forced_flat: cfg.forced_flat.clone(),
    })
}

#[derive(Default)]
struct Book {
    closed: Vec<ClosedTrade>,
    open: Option<OpenTrade>,
    trace_capacity: usize,
}

struct ClosedTrade {
    trip: RoundTrip,
    h4: Option<H4Trade>,
}

#[derive(Clone)]
struct OpenTrade {
    side: Side,
    qty: i64,
    open_ts: UtcMicros,
    pnl: i128,
    h4: Option<OpenH4>,
}

#[derive(Clone)]
struct OpenH4 {
    plan: H4EntryPlan,
    funding: Vec<FundingEvent>,
    fills: Vec<Fill>,
    slippage_cost: Cash,
}

impl Book {
    fn execution_phase(
        &self,
        ledger: &Ledger,
        pending: Option<&HeldOrder>,
    ) -> Result<ExecutionPhase, EngineError> {
        let position = ledger.position().0;
        if position == 0 {
            if self.open.is_some() {
                return Err(EngineError::InvalidExecution);
            }
            return Ok(match pending {
                Some(held) => ExecutionPhase::EntryPending {
                    side: match held.intent {
                        PendingIntent::Fixed(order) => position_side(order.side),
                        PendingIntent::Sized(sizing) if sizing.target > 0 => PositionSide::Long,
                        PendingIntent::Sized(sizing) if sizing.target < 0 => PositionSide::Short,
                        PendingIntent::Sized(_) => return Err(EngineError::InvalidExecution),
                    },
                },
                None => ExecutionPhase::Flat,
            });
        }
        let open = self.open.as_ref().ok_or(EngineError::InvalidExecution)?;
        let side = position_side(open.side);
        if (position > 0) != (side == PositionSide::Long) {
            return Err(EngineError::InvalidExecution);
        }
        Ok(match pending {
            None => ExecutionPhase::Open {
                side,
                entered_at: open.open_ts,
            },
            Some(HeldOrder {
                intent: PendingIntent::Fixed(order),
                ..
            }) if position_side(order.side) != side
                && order.qty.ticks()
                    == position
                        .checked_abs()
                        .ok_or(EngineError::InvalidExecution)? =>
            {
                ExecutionPhase::ExitPending {
                    side,
                    entered_at: open.open_ts,
                }
            }
            Some(_) => ExecutionPhase::Adjusting {
                side,
                entered_at: open.open_ts,
            },
        })
    }

    fn note_fill(
        &mut self,
        pos_before: i64,
        fill: &Fill,
        delta: i128,
        slippage: Cash,
    ) -> Result<(), EngineError> {
        if pos_before == 0 {
            self.open = Some(OpenTrade {
                side: fill.side,
                qty: fill.qty.ticks(),
                open_ts: fill.ts,
                pnl: delta,
                h4: None,
            });
            return Ok(());
        }
        let same_side = {
            let open = self.open.as_mut().ok_or(EngineError::InvalidExecution)?;
            open.pnl = open
                .pnl
                .checked_add(delta)
                .ok_or(EngineError::Sim(SimError::Overflow))?;
            if let Some(raw) = open.h4.as_mut() {
                if raw.fills.len() >= 2 {
                    return Err(EngineError::TraceCapacity { at: fill.ts });
                }
                raw.slippage_cost.0 = raw
                    .slippage_cost
                    .0
                    .checked_add(slippage.0)
                    .ok_or(EngineError::EconomicArithmetic { at: fill.ts })?;
                raw.fills.push(*fill);
            }
            fill.side == open.side
        };
        let signed = match fill.side {
            Side::Buy => fill.qty.ticks(),
            Side::Sell => -fill.qty.ticks(),
        };
        let pos_after = pos_before
            .checked_add(signed)
            .ok_or(EngineError::Sim(SimError::Overflow))?;
        if pos_after == 0 {
            let done = self.open.take().expect("checked open trip");
            self.closed.push(done.into_trip(Some(fill.ts))?);
        } else if same_side {
            let open = self.open.as_mut().expect("open trip");
            open.qty = open
                .qty
                .checked_add(fill.qty.ticks())
                .ok_or(EngineError::Sim(SimError::Overflow))?;
        }
        Ok(())
    }

    fn into_trades(self) -> Result<(Vec<RoundTrip>, Vec<H4Trade>), EngineError> {
        let mut closed = self.closed;
        if let Some(open) = self.open {
            closed.push(open.into_trip(None)?);
        }
        let mut trades = Vec::with_capacity(closed.len());
        let mut h4 = Vec::new();
        for done in closed {
            trades.push(done.trip);
            if let Some(raw) = done.h4 {
                h4.push(raw);
            }
        }
        Ok((trades, h4))
    }
}

fn position_side(side: Side) -> PositionSide {
    match side {
        Side::Buy => PositionSide::Long,
        Side::Sell => PositionSide::Short,
    }
}

impl OpenTrade {
    fn into_trip(self, close_ts: Option<UtcMicros>) -> Result<ClosedTrade, EngineError> {
        let trip = RoundTrip {
            open_ts: self.open_ts,
            close_ts,
            side: self.side,
            qty: Qty::from_ticks(self.qty).map_err(|_| EngineError::Sim(SimError::Negative))?,
            pnl: Cash(i64::try_from(self.pnl).map_err(|_| EngineError::Sim(SimError::Overflow))?),
        };
        let h4 = self.h4.map(|raw| H4Trade {
            trade: trip,
            event: raw.plan.event,
            hold: raw.plan.hold,
            slippage_cost: raw.slippage_cost,
            funding: raw.funding,
            fills: raw.fills,
        });
        Ok(ClosedTrade { trip, h4 })
    }
}

#[cfg(test)]
mod staged_trace_tests {
    use super::*;
    use parangsae_core::Price;
    use parangsae_sim::{FeeBps, SlippageBps};

    #[test]
    fn retained_h4_trade_capacity_refuses_before_cash_or_fill_commit() {
        let start = UtcDate::parse("2024-06-01")
            .expect("fixture date")
            .start_us();
        let price = Price::from_ticks(100 * 100_000_000).expect("fixture price");
        let costs = Costs {
            taker_fee: FeeBps(5),
            slippage: SlippageBps(2),
        };
        let mut ledger = Ledger::new(Cash(10_000 * 100_000_000));
        let mut book = Book {
            trace_capacity: 1,
            ..Book::default()
        };
        let mut fills = Vec::new();
        let mut slippage = 0;
        let mut refused = 0;
        let mut visibility = CashVisibility::new(0);
        for round in 0..2 {
            let at = start
                .checked_add_micros(round * 2 * ONE_MINUTE_US)
                .expect("fixture time");
            let bar = Bar {
                open_time: at,
                close_time: at
                    .checked_add_micros(ONE_MINUTE_US - 1_000)
                    .expect("fixture close"),
                available: at
                    .checked_add_micros(ONE_MINUTE_US)
                    .expect("fixture available"),
                open: price,
                high: price,
                low: price,
                close: price,
                volume: Qty::from_ticks(100_000_000).expect("fixture volume"),
                count: 1,
            };
            let equity = visibility
                .project(&ledger, price, at)
                .expect("admitted money")
                .equity();
            let mut entry = HeldOrder::new(PendingIntent::Sized(FillSizing {
                target: 100_000_000,
                equity,
                lot: 100_000,
                accept: 0,
            }));
            entry.entry = Some(H4EntryPlan {
                event: H4Event {
                    settlement: at,
                    episode: at,
                    crowd_side: PositionSide::Short,
                    gap_boundary: false,
                },
                hold: ActualHold::from_hours(72).expect("fixture hold"),
                development_end: sample_end(),
            });
            let outcome = settle_pending(
                &mut Some(entry),
                &mut refused,
                &mut FillSink {
                    fills: &mut fills,
                    slippage_ticks: &mut slippage,
                },
                &mut ledger,
                &mut book,
                &bar,
                &costs,
            )
            .expect("entry admitted");
            assert!(outcome.entry_drop.is_none());
            let before = ledger.clone();
            let before_fills = fills.clone();
            let before_slippage = slippage;
            let close_at = at.checked_add_micros(ONE_MINUTE_US).expect("fixture exit");
            let result = execute(
                &mut ledger,
                &mut book,
                &mut FillSink {
                    fills: &mut fills,
                    slippage_ticks: &mut slippage,
                },
                flatten_order(&before)
                    .expect("close shape")
                    .expect("held position"),
                price,
                &costs,
                close_at,
            );
            if round == 0 {
                result.expect("capacity permits first trip");
                assert_eq!(book.closed.len(), 1);
            } else {
                assert!(matches!(result, Err(EngineError::TraceCapacity { at }) if at == close_at));
                assert_eq!(ledger, before);
                assert_eq!(fills, before_fills);
                assert_eq!(slippage, before_slippage);
                assert_eq!(book.closed.len(), 1);
                assert_eq!(book.open.as_ref().expect("held trip retained").open_ts, at);
            }
        }
    }
}
