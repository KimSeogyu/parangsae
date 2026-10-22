//! Bar loop. Funding, then the queued fill, then the mark, then the decision.

use parangsae_core::{require_regular_grid, Interval, Qty, TimeError, UtcMicros};
use parangsae_sim::{Cash, Costs, Fill, FundingEvent, Ledger, MarketOrder, Side, SimError};

use crate::bar::{Bar, BarView, FundingStamp, Strategy, Tape, ONE_MINUTE_US};
use crate::error::EngineError;
use crate::metrics::{max_drawdown, sharpe_f64, total_return};
use crate::report::{daily_equity, Metrics, Report, RoundTrip};
use crate::tape::TapeError;

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
    validate(tape)?;
    let window = time_window(&tape.bars, cfg.start, cfg.end);
    let step = funding_step(&tape.funding)?;
    require_coverage(tape, window, step)?;

    let mut ledger = Ledger::new(cfg.initial_cash);
    let mut book = Book::default();
    let mut fills = Vec::new();
    let mut funding = Vec::new();
    let mut equity_curve = Vec::with_capacity(window.len());
    let mut pending: Option<MarketOrder> = None;
    let mut funding_i = 0usize;
    let mut ruined: Option<UtcMicros> = None;
    let mut frozen: Option<Cash> = None;
    let mut dropped_orders = 0u64;

    for (index, bar) in window.iter().enumerate() {
        if ruined.is_none() {
            funding_i = settle_funding(
                tape,
                &mut ledger,
                &mut book,
                &mut funding,
                window,
                index,
                funding_i,
            )?;
            if let Some(order) = pending.take() {
                execute(
                    &mut ledger,
                    &mut book,
                    &mut fills,
                    order,
                    bar.open,
                    costs,
                    bar.open_time,
                )?;
            }
        }

        let mark = find_mark(&tape.marks, bar.open_time)?;
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
            Some(order) => pending = Some(order),
            None => {}
        }
    }

    finish(
        cfg,
        costs,
        window.len(),
        Accum {
            ledger,
            book,
            equity_curve,
            fills,
            funding,
            ruined,
            dropped_orders,
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

/// Inclusive start, exclusive end, on bar open time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunConfig {
    pub initial_cash: Cash,
    pub start: UtcMicros,
    pub end: UtcMicros,
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

fn require_coverage(tape: &Tape, window: &[Bar], step: i64) -> Result<(), EngineError> {
    for bar in window {
        find_mark(&tape.marks, bar.open_time)?;
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
        find_mark(&tape.marks, UtcMicros::from_micros(ts))?;
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

fn find_mark(marks: &[Bar], open: UtcMicros) -> Result<&Bar, EngineError> {
    marks
        .binary_search_by(|bar| bar.open_time.cmp(&open))
        .map(|index| &marks[index])
        .map_err(|_| EngineError::MissingMark(open))
}

fn settle_funding(
    tape: &Tape,
    ledger: &mut Ledger,
    book: &mut Book,
    events: &mut Vec<FundingEvent>,
    window: &[Bar],
    index: usize,
    mut funding_i: usize,
) -> Result<usize, EngineError> {
    let prev = bar_prev(window, index)?;
    let open = window[index].open_time.micros();
    while funding_i < tape.funding.len() && tape.funding[funding_i].funding_time.micros() <= prev {
        funding_i += 1;
    }
    while funding_i < tape.funding.len() && tape.funding[funding_i].funding_time.micros() <= open {
        let stamp = &tape.funding[funding_i];
        let mark = find_mark(&tape.marks, stamp.funding_time)?;
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
        fills,
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
    fills: &mut Vec<Fill>,
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
            fills,
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
            fills,
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
    fills: &mut Vec<Fill>,
    costs: &Costs,
    leg: Leg,
) -> Result<(), EngineError> {
    let qty = Qty::from_ticks(leg.lots).map_err(|_| EngineError::Sim(SimError::Negative))?;
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
    fills.push(fill);
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
}

fn finish(
    cfg: &RunConfig,
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
    let (sharpe_daily, sharpe_annual) = sharpe_f64(&levels).map_err(EngineError::Sim)?;
    let points: Vec<Cash> = equity_curve.iter().map(|(_, cash)| *cash).collect();
    let metrics = Metrics {
        days: u32::try_from(daily.len()).map_err(|_| EngineError::Sim(SimError::Overflow))?,
        total_return: total_return(cfg.initial_cash, end).map_err(EngineError::Sim)?,
        max_drawdown: max_drawdown(&points).map_err(EngineError::Sim)?,
        sharpe_daily,
        sharpe_annual,
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
