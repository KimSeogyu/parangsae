//! Bar loop. Funding, then the queued fill, then the mark, then the decision.

use parangsae_core::{
    require_regular_grid, resample_series, DecisionTime, Interval, Observation, Qty, Side,
    TimeError, Timestamps, UtcMicros,
};
use parangsae_core::{AvailableTs, EventTs, ReceivedTs};
use parangsae_sim::{
    slippage_cost, Cash, Costs, Fill, FundingEvent, Ledger, MarketOrder, SimError,
};

use crate::bar::{Bar, BarView, FundingStamp, Strategy, Tape, ONE_MINUTE_US};
use crate::date::UtcDate;
use crate::error::EngineError;
use crate::metrics::{
    fill_notional_ticks, max_drawdown, sharpe_ticks, total_return, turnover_ticks,
};
use crate::report::{daily_equity, Metrics, Report, RoundTrip};
use crate::signal::{
    funding_visible, BookState, Decision, FlatReason, FundingObs, SignalOutput, TargetSignal,
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
            Some(order) => pending = Some(HeldOrder::new(order)),
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
/// changes. The decision close stands in for the fill price. The fill itself is
/// the next 1m open whose `count` is greater than zero. One order that waits
/// through several `count = 0` minutes increments `n_refused_fills` once.
///
/// Bars from `cfg.warmup_start` up to `cfg.start` are fed through
/// [`TargetSignal::on_warmup_bar`]. That span has no decision, order, sizing,
/// fill, funding settlement, or equity point. A hold or held target built
/// there is not a position: the first scored bar decides from a flat book.
///
/// `cfg.flatten_before` flattens at the last scored minute before that instant
/// with `count > 0`. A window that contains 2025-09-30 23:59 UTC also flattens
/// at the last scored minute at or before that open with `count > 0`.
pub fn run_targets<S: TargetSignal>(
    tape: &Tape,
    signal: &mut S,
    costs: &Costs,
    cfg: &RunConfig,
) -> Result<Report, EngineError> {
    if cfg.initial_cash.0 <= 0 || cfg.lot_ticks <= 0 {
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
                .find(|bar| bar.open_time >= range.start && !bar.is_filler())
                .map(|bar| bar.open_time)
        })
        .collect();
    let window_flat = window_flat_open(window, cfg);
    let end_open = last_tradable_open(window, cfg.start);

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

    let scored_bars = scored_len(window, cfg.start);
    for (index, bar) in window.iter().enumerate() {
        if bar.open_time < cfg.start {
            if width > 0 && (index + 1) % width == 0 && !resampled.is_empty() {
                let hour_index = (index + 1) / width - 1;
                let hour = &resampled[hour_index];
                let decision = hour
                    .close_time()
                    .checked_add_micros(1_000)
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
                signal.on_warmup_bar(view, funding_view);
            }
            continue;
        }
        let mut flattened_here = false;
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
                window,
                index,
            )?;
            if window_flat == Some(bar.open_time)
                || end_open == Some(bar.open_time)
                || forced_opens.contains(&bar.open_time)
            {
                if let Some(order) = flatten_order(&ledger)? {
                    pending = Some(HeldOrder::new(order));
                    n_forced_exits = n_forced_exits
                        .checked_add(1)
                        .ok_or(EngineError::Sim(SimError::Overflow))?;
                    flattened_here = true;
                }
            }
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

        if width == 0 || (index + 1) % width != 0 || resampled.is_empty() {
            continue;
        }
        let hour_index = (index + 1) / width - 1;
        let hour = &resampled[hour_index];
        let decision = hour
            .close_time()
            .checked_add_micros(1_000)
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
        let decision_close = hour.close();
        let sizing_equity = ledger.equity(decision_close).map_err(EngineError::Sim)?;
        let book_state = BookState {
            equity_ticks: sizing_equity.0,
            position_lots: ledger.position().0,
            mark_ticks: decision_close.ticks(),
        };
        match signal.on_bar(view, funding_view, book_state) {
            None if !seen => warmup_bars += 1,
            None => return Err(EngineError::MissingSignal(decision)),
            Some(output) if output.bar_close_time_us != hour.close_time() => {
                return Err(EngineError::MissingSignal(decision));
            }
            Some(output) => {
                seen = true;
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
                let lots = held_lots(
                    &mut sized,
                    inside || matches!(output.decision, Decision::Flat(_)),
                    output.decision.target_ticks(),
                    output.accept,
                    sizing_equity.0,
                    decision_close.ticks(),
                    cfg.lot_ticks,
                )
                .map_err(EngineError::Sim)?;
                signal_outputs.push(output);
                let diff = lots
                    .checked_sub(ledger.position().0)
                    .ok_or(EngineError::Sim(SimError::Overflow))?;
                if diff == 0 {
                    pending = None;
                } else {
                    let side = if diff > 0 { Side::Buy } else { Side::Sell };
                    let qty_ticks = diff
                        .checked_abs()
                        .ok_or(EngineError::Sim(SimError::Overflow))?;
                    let qty = Qty::from_ticks(qty_ticks)
                        .map_err(|_| EngineError::Sim(SimError::Negative))?;
                    if window[index + 1..].iter().any(|next| next.count > 0) {
                        let order = MarketOrder { side, qty };
                        // An unchanged order retains its one-refusal history.
                        if !pending.as_ref().is_some_and(|held| held.order == order) {
                            pending = Some(HeldOrder::new(order));
                        }
                        if !flattened_here {
                            note_target_change(
                                &mut n_rebalances,
                                &mut n_sign_changes,
                                &mut n_forced_exits,
                                inside,
                                ledger.position().0,
                                lots,
                            )?;
                        }
                    } else {
                        pending = None;
                        dropped_orders += 1;
                    }
                }
            }
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
        },
    )
}

fn note_target_change(
    n_rebalances: &mut u64,
    n_sign_changes: &mut u64,
    n_forced_exits: &mut u64,
    inside_forced: bool,
    old_lots: i64,
    new_lots: i64,
) -> Result<(), EngineError> {
    if inside_forced {
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

struct HeldOrder {
    order: MarketOrder,
    refused: bool,
}

impl HeldOrder {
    fn new(order: MarketOrder) -> Self {
        Self {
            order,
            refused: false,
        }
    }
}

struct FillSink<'a> {
    fills: &'a mut Vec<Fill>,
    slippage_ticks: &'a mut i64,
}

fn settle_pending(
    pending: &mut Option<HeldOrder>,
    refused_fills: &mut u64,
    sink: &mut FillSink<'_>,
    ledger: &mut Ledger,
    book: &mut Book,
    bar: &Bar,
    costs: &Costs,
) -> Result<(), EngineError> {
    let Some(mut held) = pending.take() else {
        return Ok(());
    };
    if bar.count == 0 {
        if !held.refused {
            *refused_fills = refused_fills
                .checked_add(1)
                .ok_or(EngineError::Sim(SimError::Overflow))?;
            held.refused = true;
        }
        *pending = Some(held);
        return Ok(());
    }
    execute(
        ledger,
        book,
        sink,
        held.order,
        bar.open,
        costs,
        bar.open_time,
    )
}

/// Last open at or before 2025-09-30 23:59 UTC whose `count` can fill.
///
/// A window that never reaches that minute is left alone. A `count = 0` bar
/// at 23:59 walks back to the previous open that can fill.
fn last_tradable_open(window: &[Bar], start: UtcMicros) -> Option<UtcMicros> {
    let limit = sample_end_open();
    if !window.iter().any(|bar| bar.open_time == limit) {
        return None;
    }
    window
        .iter()
        .rev()
        .find(|bar| bar.open_time >= start && bar.open_time <= limit && bar.count > 0)
        .map(|bar| bar.open_time)
}

fn sample_end_open() -> UtcMicros {
    let holdout = UtcDate::parse("2025-10-01")
        .expect("2025-10-01 is a valid date")
        .start_us();
    UtcMicros::from_micros(holdout.micros() - ONE_MINUTE_US)
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
    /// Flatten on the last scored minute before this instant with `count > 0`.
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

fn window_flat_open(window: &[Bar], cfg: &RunConfig) -> Option<UtcMicros> {
    let end = cfg.flatten_before?;
    window
        .iter()
        .rfind(|bar| bar.open_time >= cfg.start && bar.open_time < end && bar.count > 0)
        .map(|bar| bar.open_time)
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
        let event = ledger
            .apply_funding(stamp.rate, mark.open, stamp.funding_time)
            .map_err(EngineError::Sim)?;
        book.on_funding(i128::from(event.amount.0))?;
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
    if close_qty > 0 {
        apply_leg(
            ledger,
            book,
            sink,
            costs,
            Leg {
                side: order.side,
                lots: close_qty,
                price,
                ts,
            },
        )?;
    }
    if open_qty > 0 {
        apply_leg(
            ledger,
            book,
            sink,
            costs,
            Leg {
                side: order.side,
                lots: open_qty,
                price,
                ts,
            },
        )?;
    }
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
    book.note_fill(pos_before, &fill, delta)?;
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
    } = accum;
    let trades = book.into_trades()?;
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
    closed: Vec<RoundTrip>,
    open: Option<OpenTrade>,
}

struct OpenTrade {
    side: Side,
    qty: i64,
    open_ts: UtcMicros,
    pnl: i128,
}

impl Book {
    fn on_funding(&mut self, delta: i128) -> Result<(), EngineError> {
        if let Some(open) = self.open.as_mut() {
            open.pnl = open
                .pnl
                .checked_add(delta)
                .ok_or(EngineError::Sim(SimError::Overflow))?;
        }
        Ok(())
    }

    fn note_fill(&mut self, pos_before: i64, fill: &Fill, delta: i128) -> Result<(), EngineError> {
        if pos_before == 0 {
            self.open = Some(OpenTrade {
                side: fill.side,
                qty: fill.qty.ticks(),
                open_ts: fill.ts,
                pnl: delta,
            });
            return Ok(());
        }
        let same_side = {
            let open = self
                .open
                .as_mut()
                .expect("a non-zero position has an open trip");
            open.pnl = open
                .pnl
                .checked_add(delta)
                .ok_or(EngineError::Sim(SimError::Overflow))?;
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
            let done = self.open.take().expect("open trip");
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

    fn into_trades(self) -> Result<Vec<RoundTrip>, EngineError> {
        let mut trades = self.closed;
        if let Some(open) = self.open {
            trades.push(open.into_trip(None)?);
        }
        Ok(trades)
    }
}

impl OpenTrade {
    fn into_trip(self, close_ts: Option<UtcMicros>) -> Result<RoundTrip, EngineError> {
        Ok(RoundTrip {
            open_ts: self.open_ts,
            close_ts,
            side: self.side,
            qty: Qty::from_ticks(self.qty).map_err(|_| EngineError::Sim(SimError::Negative))?,
            pnl: Cash(i64::try_from(self.pnl).map_err(|_| EngineError::Sim(SimError::Overflow))?),
        })
    }
}
