//! Point-in-time supplied panels and synchronized pairs. These kernels do not
//! establish source provenance, survivorship-free universes, borrow or fills.
use super::price::{Direction, PriceBar, MAX_LOOKBACK};
use parangsae_core::{Causal, DecisionTime, Symbol, UtcMicros, SCALE};
use parangsae_signals::CausalWindow;

pub const MAX_PANEL: usize = 256;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Config,
    DuplicateSymbol,
    Future,
    Stale,
    Clock,
    Duplicate,
    Gap,
    Overflow,
}

/// Four externally admitted characteristics in explicitly caller-defined units.
/// Characteristic zero is the direct alpha score used by `rank`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelRow {
    symbol: Symbol,
    characteristics: [i64; 4],
}
impl PanelRow {
    pub fn new(symbol: Symbol, characteristics: [i64; 4]) -> Self {
        Self {
            symbol,
            characteristics,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    as_of: UtcMicros,
    rows: Vec<PanelRow>,
}
impl Panel {
    pub fn new(as_of: UtcMicros, rows: Vec<PanelRow>) -> Result<Self, Error> {
        if !(2..=MAX_PANEL).contains(&rows.len()) {
            return Err(Error::Config);
        }
        for (i, row) in rows.iter().enumerate() {
            if rows[..i].iter().any(|r| r.symbol == row.symbol) {
                return Err(Error::DuplicateSymbol);
            }
        }
        Ok(Self { as_of, rows })
    }
    pub const fn as_of(&self) -> UtcMicros {
        self.as_of
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactorWeights([i64; 4]);
impl FactorWeights {
    /// Fixed, supplied loadings, not fitted on the cross-section being traded.
    pub fn new(weights: [i64; 4]) -> Result<Self, Error> {
        if weights.iter().all(|w| *w == 0)
            || weights
                .iter()
                .any(|w| !(-1_000_000..=1_000_000).contains(w))
        {
            return Err(Error::Config);
        }
        Ok(Self(weights))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leg {
    symbol: Symbol,
    weight_ticks: i64,
}
impl Leg {
    pub const fn symbol(&self) -> &Symbol {
        &self.symbol
    }
    pub const fn weight_ticks(&self) -> i64 {
        self.weight_ticks
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allocation {
    legs: Vec<Leg>,
}
impl Allocation {
    /// Empty means neutral due to a tied selection boundary. Nonempty always has
    /// +1 unit long and -1 unit short; these are target weights, not orders.
    pub fn legs(&self) -> &[Leg] {
        &self.legs
    }
}

/// Exact-decision snapshot only. A late/superseded panel cannot silently be reused.
pub fn rank(
    view: &Causal<'_, Panel>,
    decision: DecisionTime,
    per_side: usize,
) -> Result<Allocation, Error> {
    check_snapshot(view, decision)?;
    allocate(
        view.value()
            .rows
            .iter()
            .map(|r| (&r.symbol, i128::from(r.characteristics[0])))
            .collect(),
        per_side,
    )
}
pub fn factor(
    view: &Causal<'_, Panel>,
    decision: DecisionTime,
    weights: &FactorWeights,
    per_side: usize,
) -> Result<Allocation, Error> {
    check_snapshot(view, decision)?;
    let scores = view
        .value()
        .rows
        .iter()
        .map(|r| {
            let score = r
                .characteristics
                .iter()
                .zip(weights.0)
                .try_fold(0_i128, |s, (x, w)| {
                    s.checked_add(
                        i128::from(*x)
                            .checked_mul(i128::from(w))
                            .ok_or(Error::Overflow)?,
                    )
                    .ok_or(Error::Overflow)
                })?;
            Ok((&r.symbol, score))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    allocate(scores, per_side)
}
fn check_snapshot(view: &Causal<'_, Panel>, decision: DecisionTime) -> Result<(), Error> {
    if view.available_ts().micros() > decision.micros() || view.value().as_of > decision.micros() {
        return Err(Error::Future);
    }
    if view.value().as_of != decision.micros() {
        return Err(Error::Stale);
    }
    if view.available_ts().micros() < view.value().as_of {
        return Err(Error::Clock);
    }
    Ok(())
}
fn allocate(mut scores: Vec<(&Symbol, i128)>, per_side: usize) -> Result<Allocation, Error> {
    if per_side == 0 || per_side > scores.len() / 2 {
        return Err(Error::Config);
    }
    scores.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.as_str().cmp(b.0.as_str())));
    let len = scores.len();
    // Refuse arbitrary selection between identically scored instruments.
    if scores[per_side - 1].1 == scores[per_side].1
        || scores[len - per_side - 1].1 == scores[len - per_side].1
    {
        return Ok(Allocation { legs: Vec::new() });
    }
    let denominator = i64::try_from(per_side).map_err(|_| Error::Overflow)?;
    let base = 100_000_000_i64 / denominator;
    let remainder = usize::try_from(100_000_000_i64 % denominator).map_err(|_| Error::Overflow)?;
    let mut legs = Vec::with_capacity(per_side * 2);
    for (side, selected) in [
        (1_i64, &scores[len - per_side..]),
        (-1_i64, &scores[..per_side]),
    ] {
        // Canonical symbol ordering assigns rounding ticks independently of source order.
        let mut selected = selected.to_vec();
        selected.sort_by_key(|(symbol, _)| symbol.as_str());
        for (i, (symbol, _)) in selected.into_iter().enumerate() {
            legs.push(Leg {
                symbol: symbol.clone(),
                weight_ticks: side * (base + i64::from(i < remainder)),
            });
        }
    }
    Ok(Allocation { legs })
}

/// Synchronized left/right closes with one common, producer-supplied availability.
/// Both instruments must share units/currency compatible with the supplied hedge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairBar {
    left_symbol: Symbol,
    right_symbol: Symbol,
    left: PriceBar,
    right: PriceBar,
}
impl PairBar {
    pub fn new(
        left_symbol: Symbol,
        right_symbol: Symbol,
        left: PriceBar,
        right: PriceBar,
    ) -> Result<Self, Error> {
        if left_symbol == right_symbol {
            return Err(Error::DuplicateSymbol);
        }
        if left.bar().open_time() != right.bar().open_time()
            || left.bar().interval() != right.bar().interval()
        {
            return Err(Error::Clock);
        }
        Ok(Self {
            left_symbol,
            right_symbol,
            left,
            right,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairRule {
    lookback: usize,
    hedge_ratio_ticks: i64,
    threshold_ticks: i64,
}
impl PairRule {
    /// Positive frozen beta (scale 1e8); threshold is in left-price ticks.
    /// Beta fitting/cointegration testing is deliberately not performed here.
    pub fn new(
        lookback: usize,
        hedge_ratio_ticks: i64,
        threshold_ticks: i64,
    ) -> Result<Self, Error> {
        if !(1..MAX_LOOKBACK).contains(&lookback)
            || !(1..=10_000_000_000).contains(&hedge_ratio_ticks)
            || threshold_ticks < 0
        {
            return Err(Error::Config);
        }
        Ok(Self {
            lookback,
            hedge_ratio_ticks,
            threshold_ticks,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairAssessment {
    Unavailable,
    Ready {
        left: Direction,
        right: Direction,
        hedge_ratio_ticks: i64,
    },
}
/// Fade the residual relative to its prior mean. No live shorting or pair fills.
pub fn pairs(rule: PairRule, window: &CausalWindow<'_, PairBar>) -> Result<PairAssessment, Error> {
    let count = rule.lookback + 1;
    if window.cap().get() < count || window.cap().get() > MAX_LOOKBACK {
        return Err(Error::Config);
    }
    if window.len() < count {
        return Ok(PairAssessment::Unavailable);
    }
    let rows: Vec<_> = window.iter().skip(window.len() - count).collect();
    let first = rows[0].value();
    let mut previous: Option<i64> = None;
    for view in &rows {
        let row = view.value();
        let bar = row.left.bar();
        if row.left_symbol != first.left_symbol || row.right_symbol != first.right_symbol {
            return Err(Error::Config);
        }
        if bar.interval() != first.left.bar().interval() {
            return Err(Error::Clock);
        }
        if view.available_ts().micros().micros()
            != bar
                .close_time()
                .micros()
                .checked_add(1_000)
                .ok_or(Error::Overflow)?
        {
            return Err(Error::Clock);
        }
        if let Some(open) = previous {
            if open == bar.open_time().micros() {
                return Err(Error::Duplicate);
            }
            if open
                .checked_add(bar.interval().micros())
                .ok_or(Error::Overflow)?
                != bar.open_time().micros()
            {
                return Err(Error::Gap);
            }
        }
        previous = Some(bar.open_time().micros());
    }
    let last = rows[count - 1];
    if last.available_ts().micros() != window.decision().micros() {
        return Ok(PairAssessment::Unavailable);
    }
    let residual = |r: &PairBar| {
        i128::from(r.left.bar().close().ticks()) * SCALE
            - i128::from(r.right.bar().close().ticks()) * i128::from(rule.hedge_ratio_ticks)
    };
    let sum = rows[..count - 1].iter().try_fold(0_i128, |s, r| {
        s.checked_add(residual(r.value())).ok_or(Error::Overflow)
    })?;
    let n = i128::try_from(rule.lookback).map_err(|_| Error::Overflow)?;
    let difference = residual(last.value())
        .checked_mul(n)
        .and_then(|v| v.checked_sub(sum))
        .ok_or(Error::Overflow)?;
    let threshold = i128::from(rule.threshold_ticks)
        .checked_mul(SCALE)
        .and_then(|v| v.checked_mul(n))
        .ok_or(Error::Overflow)?;
    let (left, right) = if difference > threshold {
        (Direction::Short, Direction::Long)
    } else if difference < -threshold {
        (Direction::Long, Direction::Short)
    } else {
        (Direction::Neutral, Direction::Neutral)
    };
    Ok(PairAssessment::Ready {
        left,
        right,
        hedge_ratio_ticks: rule.hedge_ratio_ticks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use parangsae_core::{AvailableTs, EventTs, Observation, ReceivedTs, Timestamps};
    fn at(t: i64) -> DecisionTime {
        DecisionTime::from_micros(UtcMicros::from_micros(t))
    }
    fn observed<T>(t: i64, value: T) -> Observation<T> {
        let t = UtcMicros::from_micros(t);
        Observation::new(
            Timestamps {
                event_ts: EventTs::from_micros(t),
                available_ts: AvailableTs::from_micros(t),
                received_ts: ReceivedTs::from_micros(t),
            },
            value,
        )
    }
    fn panel() -> Panel {
        Panel::new(
            UtcMicros::from_micros(1),
            vec![
                PanelRow::new(Symbol::new("AAA").expect("symbol"), [10, -10, 0, 0]),
                PanelRow::new(Symbol::new("BBB").expect("symbol"), [0, 0, 0, 0]),
                PanelRow::new(Symbol::new("CCC").expect("symbol"), [-10, 10, 0, 0]),
            ],
        )
        .expect("panel")
    }
    #[test]
    fn rank_and_factor_choose_their_own_scores_and_conserve_exposure() {
        let row = observed(1, panel());
        let view = row.causal_at(at(1)).expect("causal");
        let ranked = rank(&view, at(1), 1).expect("rank");
        assert_eq!(ranked.legs()[0].symbol().as_str(), "AAA");
        assert_eq!(ranked.legs()[0].weight_ticks(), 100_000_000);
        assert_eq!(ranked.legs()[1].symbol().as_str(), "CCC");
        assert_eq!(ranked.legs()[1].weight_ticks(), -100_000_000);
        let factor = factor(
            &view,
            at(1),
            &FactorWeights::new([0, 1, 0, 0]).expect("weights"),
            1,
        )
        .expect("factor");
        assert_eq!(factor.legs()[0].symbol().as_str(), "CCC");
    }

    #[test]
    fn tied_boundaries_and_stale_or_future_panels_are_not_arbitrary_trades() {
        let rows = vec![
            PanelRow::new(Symbol::new("AAA").expect("symbol"), [1, 0, 0, 0]),
            PanelRow::new(Symbol::new("BBB").expect("symbol"), [1, 0, 0, 0]),
        ];
        let p = observed(
            1,
            Panel::new(UtcMicros::from_micros(1), rows).expect("panel"),
        );
        let view = p.causal_at(at(2)).expect("view");
        assert!(rank(&view, at(1), 1).expect("tie").legs().is_empty());
        assert_eq!(rank(&view, at(2), 1), Err(Error::Stale));
        assert_eq!(rank(&view, at(0), 1), Err(Error::Future));
        let late = observed(2, panel());
        assert_eq!(
            rank(&late.causal_at(at(2)).expect("view"), at(1), 1),
            Err(Error::Future)
        );
        let same = PanelRow::new(Symbol::new("AAA").expect("symbol"), [0; 4]);
        assert_eq!(
            Panel::new(UtcMicros::from_micros(1), vec![same.clone(), same]),
            Err(Error::DuplicateSymbol)
        );
        assert_eq!(FactorWeights::new([0; 4]), Err(Error::Config));
    }
    fn paired(left_prices: &[i64]) -> Vec<Observation<PairBar>> {
        use parangsae_core::{BarInterval, Price, Qty, ResampledBar};
        left_prices
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let open = i64::try_from(i).expect("index") * 3_600_000_000;
                let bar = |p| {
                    let p = Price::from_ticks(p).expect("price");
                    PriceBar::new(
                        ResampledBar::new(
                            UtcMicros::from_micros(open),
                            BarInterval::H1,
                            p,
                            p,
                            p,
                            p,
                        )
                        .expect("bar"),
                        Qty::from_ticks(10).expect("qty"),
                    )
                    .expect("bar")
                };
                observed(
                    open + 3_600_000_000,
                    PairBar::new(
                        Symbol::new("AAA").expect("symbol"),
                        Symbol::new("BBB").expect("symbol"),
                        bar(*p),
                        bar(100),
                    )
                    .expect("pair"),
                )
            })
            .collect()
    }
    fn paired_result(
        rows: &[Observation<PairBar>],
        decision: i64,
        rule: PairRule,
    ) -> Result<PairAssessment, Error> {
        let d = at(decision * 3_600_000_000);
        let window = CausalWindow::from_views(
            d,
            std::num::NonZeroUsize::new(16).expect("cap"),
            rows.iter().filter_map(|r| r.causal_at(d)),
        )
        .expect("window");
        pairs(rule, &window)
    }
    #[test]
    fn pairs_fade_prior_residual_without_looking_at_candidate_in_baseline() {
        let rule = PairRule::new(2, 100_000_000, 10).expect("rule");
        let result = paired_result(&paired(&[100, 100, 120]), 3, rule);
        assert_eq!(
            result,
            Ok(PairAssessment::Ready {
                left: Direction::Short,
                right: Direction::Long,
                hedge_ratio_ticks: 100_000_000
            })
        );
        assert_eq!(
            paired_result(&paired(&[100, 100, 110]), 3, rule),
            Ok(PairAssessment::Ready {
                left: Direction::Neutral,
                right: Direction::Neutral,
                hedge_ratio_ticks: 100_000_000
            })
        );
        assert_eq!(
            paired_result(&paired(&[100, 100, 80]), 3, rule),
            Ok(PairAssessment::Ready {
                left: Direction::Long,
                right: Direction::Short,
                hedge_ratio_ticks: 100_000_000
            })
        );
        assert_eq!(
            paired_result(&paired(&[100, 100]), 3, rule),
            Ok(PairAssessment::Unavailable)
        );
        let rows = paired(&[100, 100, 120, 120]);
        let gap = vec![rows[0].clone(), rows[2].clone(), rows[3].clone()];
        assert_eq!(paired_result(&gap, 4, rule), Err(Error::Gap));
        let dup = vec![rows[0].clone(), rows[1].clone(), rows[1].clone()];
        assert_eq!(paired_result(&dup, 2, rule), Err(Error::Duplicate));
    }
    proptest::proptest! {
        #[test]
        fn rank_is_order_independent_and_exactly_market_neutral(per_side in 1_usize..20) {
            let rows:Vec<_>=(0..(per_side*2+1)).map(|i|PanelRow::new(Symbol::new(&format!("S{i:03}")).expect("symbol"),[i64::try_from(i).expect("score"),0,0,0])).collect();
            let mut reversed=rows.clone();reversed.reverse();
            let a=observed(1,Panel::new(UtcMicros::from_micros(1),rows).expect("panel"));
            let b=observed(1,Panel::new(UtcMicros::from_micros(1),reversed).expect("panel"));
            let allocation=rank(&a.causal_at(at(1)).expect("view"),at(1),per_side).expect("allocation");
            proptest::prop_assert_eq!(&allocation,&rank(&b.causal_at(at(1)).expect("view"),at(1),per_side).expect("allocation"));
            proptest::prop_assert_eq!(allocation.legs().iter().map(|r|r.weight_ticks()).sum::<i64>(),0);
            proptest::prop_assert_eq!(allocation.legs().iter().map(|r|r.weight_ticks().abs()).sum::<i64>(),200_000_000);
        }
        #[test]
        fn future_pair_prices_do_not_leak(future in 1_i64..i64::MAX) {
            let rule=PairRule::new(2,100_000_000,0).expect("rule");
            proptest::prop_assert_eq!(paired_result(&paired(&[100,100,120]),3,rule),paired_result(&paired(&[100,100,120,future]),3,rule));
        }
    }
}
