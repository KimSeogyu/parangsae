//! Five distinct closed-bar hypotheses; not fills, sizing, or evidence of alpha.
//! Threshold comparisons are strict: equality is neutral. No calibrated defaults.
use parangsae_core::{BarInterval, Qty, ResampledBar};
use parangsae_signals::CausalWindow;

pub const MAX_LOOKBACK: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PriceBar {
    bar: ResampledBar,
    volume: Qty,
}
impl PriceBar {
    pub fn new(bar: ResampledBar, volume: Qty) -> Result<Self, Error> {
        if bar.low().ticks() <= 0 {
            return Err(Error::Price);
        }
        Ok(Self { bar, volume })
    }
    pub const fn bar(&self) -> ResampledBar {
        self.bar
    }
    pub const fn volume(&self) -> Qty {
        self.volume
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceRule {
    Momentum {
        lookback: usize,
        threshold_bps: u16,
    },
    MeanReversion {
        lookback: usize,
        threshold_bps: u16,
    },
    TrendFollowing {
        fast: usize,
        slow: usize,
    },
    ShortTermReversal {
        lookback: usize,
        threshold_bps: u16,
        volume_multiple: u16,
    },
    Breakout {
        lookback: usize,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Long,
    Short,
    Neutral,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    Warmup,
    Stale,
    Gap,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assessment {
    Unavailable(Unavailable),
    Ready(Direction),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Config,
    Price,
    Clock,
    Duplicate,
    Interval,
    Overflow,
}

/// Required history includes the candidate. A momentum lookback of N means N
/// intervals (N+1 bars); mean reversion and breakout use N *prior* bars.
impl PriceRule {
    pub fn required_rows(self) -> Result<usize, Error> {
        let n = match self {
            Self::Momentum {
                lookback,
                threshold_bps,
            }
            | Self::MeanReversion {
                lookback,
                threshold_bps,
            } => {
                if threshold_bps > 10_000 {
                    return Err(Error::Config);
                }
                lookback.checked_add(1).ok_or(Error::Config)?
            }
            Self::TrendFollowing { fast, slow } => {
                if fast == 0 || fast >= slow {
                    return Err(Error::Config);
                }
                slow
            }
            Self::ShortTermReversal {
                lookback,
                threshold_bps,
                volume_multiple,
            } => {
                if threshold_bps > 10_000 || volume_multiple == 0 {
                    return Err(Error::Config);
                }
                lookback.checked_add(1).ok_or(Error::Config)?
            }
            Self::Breakout { lookback } => lookback.checked_add(1).ok_or(Error::Config)?,
        };
        if !(2..=MAX_LOOKBACK).contains(&n) {
            return Err(Error::Config);
        }
        Ok(n)
    }
}

/// Evaluate at the candidate bar's canonical close+1ms. Later rows are invisible
/// via CausalWindow; all rows retained by that window must have canonical clocks,
/// one interval and unique increasing keys. Missing history is not a flat signal.
pub fn evaluate(rule: PriceRule, window: &CausalWindow<'_, PriceBar>) -> Result<Assessment, Error> {
    let count = rule.required_rows()?;
    if window.cap().get() < count || window.cap().get() > MAX_LOOKBACK {
        return Err(Error::Config);
    }
    let mut previous: Option<i64> = None;
    let mut interval: Option<BarInterval> = None;
    for view in window.iter() {
        let bar = view.value().bar;
        if interval.is_some_and(|i| i != bar.interval()) {
            return Err(Error::Interval);
        }
        interval = Some(bar.interval());
        if view.available_ts().micros().micros()
            != bar
                .close_time()
                .micros()
                .checked_add(1_000)
                .ok_or(Error::Overflow)?
        {
            return Err(Error::Clock);
        }
        let open = bar.open_time().micros();
        if previous.is_some_and(|p| p == open) {
            return Err(Error::Duplicate);
        }
        if previous.is_some_and(|p| p > open) {
            return Err(Error::Clock);
        }
        previous = Some(open);
    }
    if window.len() < count {
        return Ok(Assessment::Unavailable(Unavailable::Warmup));
    }
    let rows: Vec<_> = window
        .iter()
        .skip(window.len() - count)
        .map(|v| v.value())
        .collect();
    let current = rows[count - 1];
    if current
        .bar
        .close_time()
        .micros()
        .checked_add(1_000)
        .ok_or(Error::Overflow)?
        != window.decision().micros().micros()
    {
        return Ok(Assessment::Unavailable(Unavailable::Stale));
    }
    for pair in rows.windows(2) {
        if pair[0]
            .bar
            .open_time()
            .micros()
            .checked_add(current.bar.interval().micros())
            .ok_or(Error::Overflow)?
            != pair[1].bar.open_time().micros()
        {
            return Ok(Assessment::Unavailable(Unavailable::Gap));
        }
    }
    let price = i128::from(current.bar.close().ticks());
    let prior = &rows[..count - 1];
    let direction = match rule {
        PriceRule::Momentum { threshold_bps, .. } => relative_direction(
            price,
            i128::from(rows[0].bar.close().ticks()),
            threshold_bps,
        )?,
        PriceRule::MeanReversion { threshold_bps, .. } => {
            let sum = prior.iter().try_fold(0_i128, |sum, r| {
                sum.checked_add(i128::from(r.bar.close().ticks()))
                    .ok_or(Error::Overflow)
            })?;
            reverse(relative_direction(
                price
                    .checked_mul(i128::try_from(prior.len()).map_err(|_| Error::Overflow)?)
                    .ok_or(Error::Overflow)?,
                sum,
                threshold_bps,
            )?)
        }
        PriceRule::TrendFollowing { fast, slow } => {
            let fast_sum: i128 = rows
                .iter()
                .skip(slow - fast)
                .map(|r| i128::from(r.bar.close().ticks()))
                .sum();
            let slow_sum: i128 = rows.iter().map(|r| i128::from(r.bar.close().ticks())).sum();
            sign(
                fast_sum * i128::try_from(slow).map_err(|_| Error::Overflow)?
                    - slow_sum * i128::try_from(fast).map_err(|_| Error::Overflow)?,
            )
        }
        PriceRule::ShortTermReversal {
            threshold_bps,
            volume_multiple,
            ..
        } => {
            let volume: i128 = prior.iter().map(|r| i128::from(r.volume.ticks())).sum();
            let candidate = i128::from(current.volume.ticks())
                * i128::try_from(prior.len()).map_err(|_| Error::Overflow)?;
            if volume == 0 || candidate <= volume * i128::from(volume_multiple) {
                Direction::Neutral
            } else {
                reverse(relative_direction(
                    price,
                    i128::from(prior[prior.len() - 1].bar.close().ticks()),
                    threshold_bps,
                )?)
            }
        }
        PriceRule::Breakout { .. } => {
            let high = prior
                .iter()
                .map(|r| r.bar.high().ticks())
                .max()
                .ok_or(Error::Config)?;
            let low = prior
                .iter()
                .map(|r| r.bar.low().ticks())
                .min()
                .ok_or(Error::Config)?;
            if current.bar.close().ticks() > high {
                Direction::Long
            } else if current.bar.close().ticks() < low {
                Direction::Short
            } else {
                Direction::Neutral
            }
        }
    };
    Ok(Assessment::Ready(direction))
}
fn relative_direction(value: i128, base: i128, threshold: u16) -> Result<Direction, Error> {
    let difference = value
        .checked_sub(base)
        .and_then(|d| d.checked_mul(10_000))
        .ok_or(Error::Overflow)?;
    let bound = base
        .checked_mul(i128::from(threshold))
        .ok_or(Error::Overflow)?;
    Ok(if difference > bound {
        Direction::Long
    } else if difference < -bound {
        Direction::Short
    } else {
        Direction::Neutral
    })
}
fn sign(value: i128) -> Direction {
    if value > 0 {
        Direction::Long
    } else if value < 0 {
        Direction::Short
    } else {
        Direction::Neutral
    }
}
fn reverse(direction: Direction) -> Direction {
    match direction {
        Direction::Long => Direction::Short,
        Direction::Short => Direction::Long,
        Direction::Neutral => Direction::Neutral,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parangsae_core::{
        AvailableTs, DecisionTime, EventTs, Observation, Price, ReceivedTs, Timestamps, UtcMicros,
    };
    use std::num::NonZeroUsize;
    const H: i64 = 3_600_000_000;
    fn observations(prices: &[i64]) -> Vec<Observation<PriceBar>> {
        prices
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let open = i64::try_from(i).expect("index") * H;
                let price = Price::from_ticks(*p).expect("price");
                let bar = ResampledBar::new(
                    UtcMicros::from_micros(open),
                    BarInterval::H1,
                    price,
                    price,
                    price,
                    price,
                )
                .expect("bar");
                let t = UtcMicros::from_micros(open + H);
                Observation::new(
                    Timestamps {
                        event_ts: EventTs::from_micros(t),
                        available_ts: AvailableTs::from_micros(t),
                        received_ts: ReceivedTs::from_micros(t),
                    },
                    PriceBar::new(bar, Qty::from_ticks(10).expect("qty")).expect("input"),
                )
            })
            .collect()
    }
    fn assessed(
        rule: PriceRule,
        rows: &[Observation<PriceBar>],
        n: i64,
    ) -> Result<Assessment, Error> {
        let decision = DecisionTime::from_micros(UtcMicros::from_micros(n * H));
        let window = CausalWindow::from_views(
            decision,
            NonZeroUsize::new(16).expect("cap"),
            rows.iter().filter_map(|r| r.causal_at(decision)),
        )
        .expect("window");
        evaluate(rule, &window)
    }
    #[test]
    fn momentum_and_reversion_have_opposite_observable_directions() {
        let rows = observations(&[100, 100, 120]);
        assert_eq!(
            assessed(
                PriceRule::Momentum {
                    lookback: 2,
                    threshold_bps: 100
                },
                &rows,
                3
            ),
            Ok(Assessment::Ready(Direction::Long))
        );
        assert_eq!(
            assessed(
                PriceRule::MeanReversion {
                    lookback: 2,
                    threshold_bps: 100
                },
                &rows,
                3
            ),
            Ok(Assessment::Ready(Direction::Short))
        );
    }

    #[test]
    fn trend_breakout_and_volume_confirmed_reversal_are_distinct() {
        let rows = observations(&[100, 110, 120]);
        assert_eq!(
            assessed(PriceRule::TrendFollowing { fast: 1, slow: 3 }, &rows, 3),
            Ok(Assessment::Ready(Direction::Long))
        );
        assert_eq!(
            assessed(PriceRule::Breakout { lookback: 2 }, &rows, 3),
            Ok(Assessment::Ready(Direction::Long))
        );
        let rule = PriceRule::ShortTermReversal {
            lookback: 2,
            threshold_bps: 100,
            volume_multiple: 2,
        };
        assert_eq!(
            assessed(rule, &rows, 3),
            Ok(Assessment::Ready(Direction::Neutral))
        );
        let mut active = rows;
        active[2] = active[2].with_value(
            PriceBar::new(
                active[2].value().bar(),
                Qty::from_ticks(21).expect("volume"),
            )
            .expect("bar"),
        );
        assert_eq!(
            assessed(rule, &active, 3),
            Ok(Assessment::Ready(Direction::Short))
        );
    }
    #[test]
    fn exact_threshold_and_previous_channel_are_neutral() {
        let rows = observations(&[100, 100, 101]);
        assert_eq!(
            assessed(
                PriceRule::Momentum {
                    lookback: 2,
                    threshold_bps: 100
                },
                &rows,
                3
            ),
            Ok(Assessment::Ready(Direction::Neutral))
        );
        assert_eq!(
            assessed(
                PriceRule::MeanReversion {
                    lookback: 2,
                    threshold_bps: 100
                },
                &rows,
                3
            ),
            Ok(Assessment::Ready(Direction::Neutral))
        );
        let equal = observations(&[100, 110, 110]);
        assert_eq!(
            assessed(PriceRule::Breakout { lookback: 2 }, &equal, 3),
            Ok(Assessment::Ready(Direction::Neutral))
        );
    }
    #[test]
    fn unavailable_is_not_neutral_and_duplicate_is_error() {
        let rule = PriceRule::Momentum {
            lookback: 2,
            threshold_bps: 0,
        };
        let rows = observations(&[100, 110, 120, 130]);
        assert_eq!(
            assessed(rule, &rows[..2], 2),
            Ok(Assessment::Unavailable(Unavailable::Warmup))
        );
        assert_eq!(
            assessed(rule, &rows[..3], 4),
            Ok(Assessment::Unavailable(Unavailable::Stale))
        );
        let gap = vec![rows[0].clone(), rows[2].clone(), rows[3].clone()];
        assert_eq!(
            assessed(rule, &gap, 4),
            Ok(Assessment::Unavailable(Unavailable::Gap))
        );
        let duplicate = vec![rows[0].clone(), rows[1].clone(), rows[1].clone()];
        assert_eq!(assessed(rule, &duplicate, 2), Err(Error::Duplicate));
        let mut bad = rows;
        let mut ts = bad[0].timestamps();
        ts.available_ts = AvailableTs::from_micros(UtcMicros::from_micros(H - 1));
        bad[0] = Observation::new(ts, *bad[0].value());
        assert_eq!(assessed(rule, &bad, 4), Err(Error::Clock));
    }
    #[test]
    fn invalid_configs_and_full_width_prices_do_not_wrap() {
        assert_eq!(
            PriceRule::Momentum {
                lookback: 0,
                threshold_bps: 0
            }
            .required_rows(),
            Err(Error::Config)
        );
        assert_eq!(
            PriceRule::TrendFollowing { fast: 2, slow: 2 }.required_rows(),
            Err(Error::Config)
        );
        assert_eq!(
            PriceRule::Breakout {
                lookback: usize::MAX
            }
            .required_rows(),
            Err(Error::Config)
        );
        let rows = observations(&[i64::MAX - 2, i64::MAX - 1, i64::MAX]);
        assert_eq!(
            assessed(PriceRule::TrendFollowing { fast: 1, slow: 3 }, &rows, 3),
            Ok(Assessment::Ready(Direction::Long))
        );
    }
    proptest::proptest! {
        #[test]
        fn later_price_mutation_never_changes_an_earlier_decision(future in 1_i64..i64::MAX) {
            let baseline=observations(&[100,110,120]);
            let changed=observations(&[100,110,120,future]);
            for rule in [PriceRule::Momentum{lookback:2,threshold_bps:0},PriceRule::MeanReversion{lookback:2,threshold_bps:0},PriceRule::TrendFollowing{fast:1,slow:3},PriceRule::Breakout{lookback:2},PriceRule::ShortTermReversal{lookback:2,threshold_bps:0,volume_multiple:1}] {
                proptest::prop_assert_eq!(assessed(rule,&baseline,3),assessed(rule,&changed,3));
            }
        }
    }
}
