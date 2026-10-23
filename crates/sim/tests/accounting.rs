//! P1–P6. Each property runs 100 cases. The model below does not call the ledger.

use parangsae_core::{Price, Qty, Ratio, UtcMicros, SCALE};
use parangsae_sim::{
    round_against_trader, taker_fee, Cash, Costs, FeeBps, Ledger, MarketOrder, Side, SimError,
    SlippageBps,
};
use proptest::prelude::*;

const CASES: u32 = 100;
const BPS_DENOM: i128 = 10_000;

fn as_price(ticks: i64) -> Price {
    Price::from_ticks(ticks).expect("price ticks are non-negative")
}

fn qty(ticks: i64) -> Qty {
    Qty::from_ticks(ticks).expect("qty ticks are non-negative")
}

fn costs(fee: u32, slip: u32) -> Costs {
    Costs {
        taker_fee: FeeBps(fee),
        slippage: SlippageBps(slip),
    }
}

fn order(side: Side, lots: i64) -> MarketOrder {
    MarketOrder {
        side,
        qty: qty(lots),
    }
}

/// Spec rounding: payers away from zero, receivers toward zero.
fn spec_round(num: i128, den: i128, pays: bool) -> Option<i128> {
    if den <= 0 {
        return None;
    }
    let quot = num.checked_div(den)?;
    let rem = num.checked_rem(den)?;
    if rem == 0 || !pays {
        return Some(quot);
    }
    let step = if num > 0 { 1 } else { -1 };
    quot.checked_add(step)
}

fn spec_fill_price(open: i64, side: Side, slip: u32) -> Option<i64> {
    let slip = i128::from(slip);
    let (factor, pays) = match side {
        Side::Buy => (BPS_DENOM + slip, true),
        Side::Sell => {
            if slip > 10_000 {
                return None;
            }
            (BPS_DENOM - slip, false)
        }
    };
    let ticks = spec_round(i128::from(open).checked_mul(factor)?, BPS_DENOM, pays)?;
    if ticks < 0 {
        return None;
    }
    i64::try_from(ticks).ok()
}

fn spec_fee(px: i64, lots: i64, bps: u32) -> Option<i64> {
    let num = i128::from(lots)
        .checked_mul(i128::from(px))?
        .checked_mul(i128::from(bps))?;
    let den = SCALE.checked_mul(BPS_DENOM)?;
    i64::try_from(spec_round(num, den, true)?).ok()
}

fn spec_notional(side: Side, px: i64, lots: i64) -> Option<i64> {
    let num = i128::from(px).checked_mul(i128::from(lots))?;
    let pays = matches!(side, Side::Buy);
    let magnitude = spec_round(num, SCALE, pays)?;
    let signed = if pays {
        magnitude.checked_neg()?
    } else {
        magnitude
    };
    i64::try_from(signed).ok()
}

fn spec_funding(position: i64, mark: i64, rate: i64) -> Option<i64> {
    let product = i128::from(position)
        .checked_mul(i128::from(mark))?
        .checked_mul(i128::from(rate))?;
    let num = product.checked_neg()?;
    let den = SCALE.checked_mul(SCALE)?;
    i64::try_from(spec_round(num, den, num < 0)?).ok()
}

fn floor_div(num: i128, den: i128) -> Option<i128> {
    if den <= 0 {
        return None;
    }
    let quot = num.checked_div(den)?;
    let rem = num.checked_rem(den)?;
    if rem != 0 && num < 0 {
        quot.checked_sub(1)
    } else {
        Some(quot)
    }
}

#[derive(Clone, Debug)]
struct Model {
    cash: i64,
    pos: i64,
    fees: i64,
    funding: i64,
    buy_cost: i128,
    sell_proceeds: i128,
}

impl Model {
    fn new(initial: i64) -> Self {
        Self {
            cash: initial,
            pos: 0,
            fees: 0,
            funding: 0,
            buy_cost: 0,
            sell_proceeds: 0,
        }
    }

    fn fill(&mut self, side: Side, open: i64, lots: i64, bps: u32, slip: u32) -> Option<()> {
        let px = spec_fill_price(open, side, slip)?;
        let fee = spec_fee(px, lots, bps)?;
        let notion = spec_notional(side, px, lots)?;
        let cash_delta = notion.checked_sub(fee)?;
        self.cash = self.cash.checked_add(cash_delta)?;
        self.pos = match side {
            Side::Buy => self.pos.checked_add(lots)?,
            Side::Sell => self.pos.checked_sub(lots)?,
        };
        self.fees = self.fees.checked_add(fee)?;
        match side {
            Side::Buy => self.buy_cost += i128::from(-notion),
            Side::Sell => self.sell_proceeds += i128::from(notion),
        }
        Some(())
    }

    fn fund(&mut self, mark: i64, rate: i64) -> Option<()> {
        let amount = spec_funding(self.pos, mark, rate)?;
        self.cash = self.cash.checked_add(amount)?;
        self.funding = self.funding.checked_add(amount)?;
        Some(())
    }

    fn equity(&self, mark: i64) -> Option<i64> {
        let mtm = floor_div(i128::from(self.pos).checked_mul(i128::from(mark))?, SCALE)?;
        i64::try_from(mtm.checked_add(i128::from(self.cash))?).ok()
    }
}

#[derive(Clone, Debug)]
enum Op {
    Fill {
        side: Side,
        price: i64,
        qty: i64,
        bps: u32,
        slip: u32,
    },
    Funding {
        mark: i64,
        rate: i64,
    },
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (
            any::<bool>(),
            1i64..=50_000_000_000,
            1i64..=200_000_000,
            0u32..=30,
            0u32..=15
        )
            .prop_map(|(buy, price, qty, bps, slip)| Op::Fill {
                side: if buy { Side::Buy } else { Side::Sell },
                price,
                qty,
                bps,
                slip,
            }),
        (1i64..=50_000_000_000, -500_000i64..=500_000)
            .prop_map(|(mark, rate)| Op::Funding { mark, rate }),
    ]
}

fn snap(ledger: &Ledger) -> (i64, i64, i64, i64) {
    (
        ledger.cash().0,
        ledger.position().0,
        ledger.fees_paid().0,
        ledger.funding_net().0,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(CASES))]

    /// The helper matches the signed anti-flatter rule, including exact ties.
    #[test]
    fn round_against_trader_is_the_anti_flatter_rule(
        num in -1_000_000_000_000i128..1_000_000_000_000,
        den in 1i128..100_000,
        pays in any::<bool>(),
    ) {
        let got = round_against_trader(num, den, pays).unwrap();
        let quot = num / den;
        let rem = num % den;
        if rem == 0 || !pays {
            prop_assert_eq!(got, quot);
        } else {
            let step = if num > 0 { 1 } else { -1 };
            prop_assert_eq!(got, quot + step);
        }
        let reconstructed = got * den;
        let pays_away = if num >= 0 {
            reconstructed >= num
        } else {
            reconstructed <= num
        };
        let receives_toward = if num >= 0 {
            reconstructed <= num
        } else {
            reconstructed >= num
        };
        if pays {
            prop_assert!(pays_away);
        } else {
            prop_assert!(receives_toward);
        }
        prop_assert!((reconstructed - num).abs() < den);
    }

    /// P1. Equity equals initial cash, signed notionals, fees, funding, and floored mark.
    #[test]
    fn p1_conservation(ops in prop::collection::vec(arb_op(), 0..8), mark in 1i64..=50_000_000_000) {
        let initial = 10_000 * 100_000_000;
        let mut ledger = Ledger::new(Cash(initial));
        let mut model = Model::new(initial);
        let ts = UtcMicros::from_micros(0);
        for op in ops {
            let before = snap(&ledger);
            let mut trial = model.clone();
            let model_ok = match op {
                Op::Fill { side, price, qty, bps, slip } => trial.fill(side, price, qty, bps, slip),
                Op::Funding { mark, rate } => trial.fund(mark, rate),
            };
            let ledger_res = match op {
                Op::Fill { side, price, qty, bps, slip } => ledger.apply_fill(
                    order(side, qty),
                    as_price(price),
                    &costs(bps, slip),
                    ts,
                ).map(|_| ()),
                Op::Funding { mark, rate } => ledger
                    .apply_funding(Ratio::from_ticks(rate), as_price(mark), ts)
                    .map(|_| ()),
            };
            match (model_ok, ledger_res) {
                (Some(()), Ok(())) => {
                    model = trial;
                    prop_assert_eq!(snap(&ledger), (model.cash, model.pos, model.fees, model.funding));
                }
                (None, Err(SimError::Overflow)) => {
                    prop_assert_eq!(snap(&ledger), before);
                    break;
                }
                (model_ok, ledger_res) => {
                    prop_assert!(false, "model {model_ok:?} ledger {ledger_res:?}");
                }
            }
        }
        let expected = i128::from(initial) - model.buy_cost + model.sell_proceeds
            - i128::from(model.fees)
            + i128::from(model.funding)
            + floor_div(i128::from(model.pos).checked_mul(i128::from(mark)).unwrap(), SCALE).unwrap();
        prop_assert_eq!(i128::from(ledger.equity(as_price(mark)).unwrap().0), expected);
        prop_assert_eq!(ledger.equity(as_price(mark)).unwrap().0, model.equity(mark).unwrap());
    }

    /// P2. Fee is a ceiling, never negative, and monotone in bps and notional.
    #[test]
    fn p2_fee_ceiling_and_monotone(
        px in 1i64..=1_000_000_000_000,
        lots in 1i64..=1_000_000_000_000,
        more_lots in 1i64..=1_000_000_000_000,
        more_px in 1i64..=1_000_000_000_000,
        bps in 0u32..=100,
        more_bps in 0u32..=100,
    ) {
        let fee = taker_fee(as_price(px), qty(lots), FeeBps(bps)).unwrap();
        prop_assert!(fee.0 >= 0);
        let num = i128::from(lots) * i128::from(px) * i128::from(bps);
        let den = SCALE * BPS_DENOM;
        prop_assert!(i128::from(fee.0) * den >= num);
        prop_assert!(i128::from(fee.0) * den - num < den);
        let hi_bps = bps.max(more_bps);
        let lo_bps = bps.min(more_bps);
        let hi = taker_fee(as_price(px), qty(lots), FeeBps(hi_bps)).unwrap();
        let lo = taker_fee(as_price(px), qty(lots), FeeBps(lo_bps)).unwrap();
        prop_assert!(hi.0 >= lo.0);
        let big_qty = lots.max(more_lots);
        let small_qty = lots.min(more_lots);
        let at_big = taker_fee(as_price(px), qty(big_qty), FeeBps(bps)).unwrap();
        let at_small = taker_fee(as_price(px), qty(small_qty), FeeBps(bps)).unwrap();
        prop_assert!(at_big.0 >= at_small.0);
        let big_px = px.max(more_px);
        let small_px = px.min(more_px);
        let px_big = taker_fee(as_price(big_px), qty(lots), FeeBps(bps)).unwrap();
        let px_small = taker_fee(as_price(small_px), qty(lots), FeeBps(bps)).unwrap();
        prop_assert!(px_big.0 >= px_small.0);
    }

    /// P3. A round trip at one price loses money only when a fee is charged.
    #[test]
    fn p3_no_free_round_trip(usdt in 1i64..=1_000, btc in 1i64..=3, bps in 1u32..=20) {
        let px = usdt * 100_000_000;
        let lots = btc * 100_000_000;
        let initial = 10_000 * 100_000_000;
        let ts = UtcMicros::from_micros(0);
        let mut paid = Ledger::new(Cash(initial));
        paid.apply_fill(order(Side::Buy, lots), as_price(px), &costs(bps, 0), ts).unwrap();
        paid.apply_fill(order(Side::Sell, lots), as_price(px), &costs(bps, 0), ts).unwrap();
        prop_assert!(paid.equity(as_price(px)).unwrap().0 < initial);
        prop_assert_eq!(paid.position().0, 0);
        let mut free = Ledger::new(Cash(initial));
        free.apply_fill(order(Side::Buy, lots), as_price(px), &costs(0, 0), ts).unwrap();
        free.apply_fill(order(Side::Sell, lots), as_price(px), &costs(0, 0), ts).unwrap();
        prop_assert_eq!(free.equity(as_price(px)).unwrap().0, initial);
        prop_assert_eq!(free.position().0, 0);
    }

    /// P4. k partial fills never finish richer than one fill of the same qty.
    #[test]
    fn p4_splitting_does_not_raise_equity(
        buy in any::<bool>(),
        px in 1i64..=1_000_000_000_000,
        parts in prop::collection::vec(1i64..=1_000_000, 2..5),
        bps in 0u32..=10,
        mark in 1i64..=1_000_000_000_000,
    ) {
        let side = if buy { Side::Buy } else { Side::Sell };
        let total: i64 = parts.iter().sum();
        let initial = 1_000_000 * 100_000_000;
        let ts = UtcMicros::from_micros(0);
        let mut once = Ledger::new(Cash(initial));
        once.apply_fill(order(side, total), as_price(px), &costs(bps, 0), ts).unwrap();
        let mut split = Ledger::new(Cash(initial));
        for part in parts {
            split.apply_fill(order(side, part), as_price(px), &costs(bps, 0), ts).unwrap();
        }
        prop_assert_eq!(once.position().0, split.position().0);
        prop_assert!(split.equity(as_price(mark)).unwrap().0 <= once.equity(as_price(mark)).unwrap().0);
    }

    /// P5. A payer's cash does not rise. A receiver's does not fall. Flat is zero.
    #[test]
    fn p5_funding_sign(long in any::<bool>(), rate_mag in 1i64..=1_000_000, mark in 1i64..=100_000_000_000, positive_rate in any::<bool>()) {
        let lots = 100_000_000;
        let side = if long { Side::Buy } else { Side::Sell };
        let rate = if positive_rate { rate_mag } else { -rate_mag };
        let initial = 1_000_000 * 100_000_000;
        let ts = UtcMicros::from_micros(0);
        let mut ledger = Ledger::new(Cash(initial));
        ledger.apply_fill(order(side, lots), as_price(1), &costs(0, 0), ts).unwrap();
        let before = ledger.cash().0;
        let event = ledger.apply_funding(Ratio::from_ticks(rate), as_price(mark), ts).unwrap();
        let pays = (long && rate > 0) || (!long && rate < 0);
        if pays {
            prop_assert!(ledger.cash().0 <= before);
            prop_assert!(event.amount.0 <= 0);
        } else {
            prop_assert!(ledger.cash().0 >= before);
            prop_assert!(event.amount.0 >= 0);
        }
        let mut flat = Ledger::new(Cash(initial));
        let flat_event = flat.apply_funding(Ratio::from_ticks(rate), as_price(mark), ts).unwrap();
        prop_assert_eq!(flat_event.amount.0, 0);
        prop_assert_eq!(flat.cash().0, initial);
        prop_assert_eq!(flat.position().0, 0);
    }

    /// P6. A product that does not fit is `Overflow`, and the ledger stays put.
    #[test]
    fn p6_overflow_does_not_wrap(bump in 0i64..1_000) {
        let ticks = 1_000_000_000_000_000 + bump;
        let mut ledger = Ledger::new(Cash(1_000));
        let err = ledger.apply_fill(
            order(Side::Buy, ticks),
            as_price(ticks),
            &costs(5, 0),
            UtcMicros::from_micros(0),
        ).unwrap_err();
        prop_assert_eq!(err, SimError::Overflow);
        prop_assert_eq!(ledger.cash().0, 1_000);
        prop_assert_eq!(ledger.position().0, 0);
        prop_assert_eq!(ledger.fees_paid().0, 0);

        let mut funded = Ledger::new(Cash(1_000_000));
        funded.apply_fill(
            order(Side::Buy, 100_000_000),
            as_price(1),
            &costs(0, 0),
            UtcMicros::from_micros(0),
        ).unwrap();
        let before = snap(&funded);
        let err = funded.apply_funding(
            Ratio::from_ticks(i64::MAX),
            as_price(i64::MAX),
            UtcMicros::from_micros(1),
        ).unwrap_err();
        prop_assert_eq!(err, SimError::Overflow);
        prop_assert_eq!(snap(&funded), before);
    }
}

#[test]
fn non_positive_denominator_is_overflow() {
    assert_eq!(round_against_trader(1, 0, true), Err(SimError::Overflow));
    assert_eq!(round_against_trader(1, -2, false), Err(SimError::Overflow));
}
