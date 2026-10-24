//! Exact arithmetic for the selected conditional C1 capital and cost model.

use parangsae_core::{Price, Qty, Side, SCALE};
use parangsae_execution::carry::{CarryFees, SpotFeeAsset};
use parangsae_sim::carry::{notional, CarryAccountState};
use parangsae_sim::{fill_price, taker_fee, Cash, FeeBps, SlippageBps};

const BPS: i128 = 10_000;
const GROSS_MAX: i64 = 10_000_000_000;
const NET_MAX: i64 = 9_990_000_000;
const SPOT_STEP: i64 = 1_000;
const PERP_STEP: i64 = 100_000;
const EACH_CAP: i64 = 45_000_000_000_000;
const SPOT_CLOSE: i64 = 2_500_000_000_000;
const PERP_CLOSE_AND_FUNDING: i64 = 2_500_000_000_000;
const MIN_NOTIONAL: i64 = 10_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scenario {
    Headline,
    HigherPerpFee,
    SlippageStress,
}

impl Scenario {
    pub(super) fn fees(self) -> CarryFees {
        CarryFees {
            spot_bps: FeeBps(10),
            spot_buy_asset: SpotFeeAsset::Btc,
            spot_sell_asset: SpotFeeAsset::Usdt,
            perp_bps: FeeBps(if self == Self::HigherPerpFee { 6 } else { 5 }),
            spot_slippage: SlippageBps(if self == Self::SlippageStress { 5 } else { 2 }),
            perp_slippage: SlippageBps(if self == Self::SlippageStress { 5 } else { 2 }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SizingError {
    Price,
    Overflow,
    Account,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    Occupied,
    NativePriceEnvelope,
    NoCompatibleSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PriceBand {
    pub lower: Price,
    pub upper: Price,
}

impl PriceBand {
    pub(super) fn at(reference: Price) -> Result<Self, SizingError> {
        if reference.ticks() <= 0 {
            return Err(SizingError::Price);
        }
        let raw = i128::from(reference.ticks());
        let lower = ceil(raw.checked_mul(9_900).ok_or(SizingError::Overflow)?, BPS)?;
        let upper = raw.checked_mul(10_100).ok_or(SizingError::Overflow)? / BPS;
        Ok(Self {
            lower: price(lower)?,
            upper: price(upper)?,
        })
    }

    pub(super) fn contains(self, executed: Price) -> bool {
        self.lower <= executed && executed <= self.upper
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct EntryPrices {
    pub spot_reference: Price,
    pub perp_reference: Price,
    pub spot_risk: Price,
    pub perp_risk: Price,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EntryQuote {
    pub gross_spot: Qty,
    pub net_btc: Qty,
    pub spot_band: PriceBand,
    pub perp_band: PriceBand,
    pub headline_c4: Cash,
    pub spot_requirement: Cash,
    pub perp_requirement: Cash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EntrySizing {
    Sized(EntryQuote),
    Refused(Refusal),
}

pub(super) fn quote_entry(
    account: &CarryAccountState,
    refs: EntryPrices,
    scenario: Scenario,
) -> Result<EntrySizing, SizingError> {
    if account.spot_btc.ticks() != 0 || account.perp_short.ticks() != 0 {
        return Ok(EntrySizing::Refused(Refusal::Occupied));
    }
    let spot_band = PriceBand::at(refs.spot_reference)?;
    let perp_band = PriceBand::at(refs.perp_reference)?;
    if refs.spot_risk.ticks() <= 0 || refs.perp_risk.ticks() <= 0 {
        return Err(SizingError::Price);
    }
    if refs.spot_risk > spot_band.upper || refs.perp_risk > perp_band.upper {
        return Ok(EntrySizing::Refused(Refusal::NativePriceEnvelope));
    }
    let actual = scenario.fees();
    // On the selected spot grid, the BTC fee is exact per gross lot. Solving
    // net-grid compatibility bounds the search to at most100 gross quantities.
    let net_per_spot_lot = SPOT_STEP - fee_btc(SPOT_STEP)?;
    let common_net_step = lcm(SPOT_STEP, PERP_STEP)?;
    let k_step = common_net_step / gcd(net_per_spot_lot, common_net_step);
    let gross_step = SPOT_STEP.checked_mul(k_step).ok_or(SizingError::Overflow)?;
    for count in (1..=GROSS_MAX / gross_step).rev() {
        let gross_ticks = count.checked_mul(gross_step).ok_or(SizingError::Overflow)?;
        let net_ticks = gross_ticks
            .checked_sub(fee_btc(gross_ticks)?)
            .ok_or(SizingError::Overflow)?;
        if net_ticks <= 0
            || net_ticks > NET_MAX
            || gross_ticks % SPOT_STEP != 0
            || net_ticks % SPOT_STEP != 0
            || net_ticks % PERP_STEP != 0
        {
            continue;
        }
        let gross = quantity(gross_ticks)?;
        let net = quantity(net_ticks)?;
        let spot_principal = amount(spot_band.upper, gross, true)?;
        let perp_principal = amount(perp_band.upper, net, true)?;
        let spot_requirement = checked_add(spot_principal, SPOT_CLOSE)?;
        let perp_fee = taker_fee(perp_band.upper, net, actual.perp_bps)
            .map_err(|_| SizingError::Overflow)?
            .0;
        let perp_requirement = checked_add(
            checked_add(perp_principal, perp_fee)?,
            PERP_CLOSE_AND_FUNDING,
        )?;
        if spot_principal > EACH_CAP
            || perp_principal > EACH_CAP
            || spot_requirement > account.spot_quote.0
            || perp_requirement > account.perp_wallet.0
        {
            continue;
        }
        if [
            amount(refs.spot_reference, gross, false)?,
            amount(refs.perp_reference, net, false)?,
            amount(refs.spot_risk, gross, false)?,
            amount(refs.perp_risk, net, false)?,
        ]
        .into_iter()
        .any(|v| v < MIN_NOTIONAL)
        {
            continue;
        }
        return Ok(EntrySizing::Sized(EntryQuote {
            gross_spot: gross,
            net_btc: net,
            spot_band,
            perp_band,
            headline_c4: headline_c4(account, refs, gross, net)?,
            spot_requirement: Cash(spot_requirement),
            perp_requirement: Cash(perp_requirement),
        }));
    }
    Ok(EntrySizing::Refused(Refusal::NoCompatibleSize))
}

fn headline_c4(
    account: &CarryAccountState,
    refs: EntryPrices,
    gross: Qty,
    net: Qty,
) -> Result<Cash, SizingError> {
    let fees = Scenario::Headline.fees();
    let spot_buy = fill_price(refs.spot_reference, Side::Buy, fees.spot_slippage)
        .map_err(|_| SizingError::Overflow)?;
    let spot_sell = fill_price(refs.spot_reference, Side::Sell, fees.spot_slippage)
        .map_err(|_| SizingError::Overflow)?;
    let perp_sell = fill_price(refs.perp_reference, Side::Sell, fees.perp_slippage)
        .map_err(|_| SizingError::Overflow)?;
    let perp_buy = fill_price(refs.perp_reference, Side::Buy, fees.perp_slippage)
        .map_err(|_| SizingError::Overflow)?;
    let spot_cost = amount(spot_buy, gross, true)?
        .checked_sub(amount(spot_sell, net, false)?)
        .ok_or(SizingError::Overflow)?;
    let sell_fee = taker_fee(spot_sell, net, fees.spot_bps)
        .map_err(|_| SizingError::Overflow)?
        .0;
    let entry_fee = taker_fee(perp_sell, net, fees.perp_bps)
        .map_err(|_| SizingError::Overflow)?
        .0;
    let exit_fee = taker_fee(perp_buy, net, fees.perp_bps)
        .map_err(|_| SizingError::Overflow)?
        .0;
    let price_delta = i128::from(perp_sell.ticks())
        .checked_sub(i128::from(perp_buy.ticks()))
        .ok_or(SizingError::Overflow)?;
    let hypothetical = account
        .realized_perp_scaled
        .checked_add(
            price_delta
                .checked_mul(i128::from(net.ticks()))
                .ok_or(SizingError::Overflow)?,
        )
        .ok_or(SizingError::Overflow)?;
    let realized_delta = floor(hypothetical, SCALE)?
        .checked_sub(i128::from(account.realized_perp.0))
        .ok_or(SizingError::Overflow)?;
    let result = i128::from(spot_cost)
        .checked_add(i128::from(sell_fee))
        .and_then(|v| v.checked_add(i128::from(entry_fee)))
        .and_then(|v| v.checked_add(i128::from(exit_fee)))
        .and_then(|v| v.checked_sub(realized_delta))
        .ok_or(SizingError::Overflow)?;
    let result = i64::try_from(result).map_err(|_| SizingError::Overflow)?;
    if result < 0 {
        return Err(SizingError::Account);
    }
    Ok(Cash(result))
}

pub(super) fn forecast_above_cost(
    net: Qty,
    perp_reference: Price,
    numerator: i128,
    denominator: i128,
    c4: Cash,
) -> Result<bool, SizingError> {
    if denominator <= 0 || c4.0 < 0 {
        return Err(SizingError::Account);
    }
    let lhs = i128::from(net.ticks())
        .checked_mul(i128::from(perp_reference.ticks()))
        .and_then(|v| v.checked_mul(numerator))
        .ok_or(SizingError::Overflow)?;
    let rhs = i128::from(c4.0)
        .checked_mul(SCALE)
        .and_then(|v| v.checked_mul(denominator))
        .ok_or(SizingError::Overflow)?;
    Ok(lhs > rhs)
}

fn fee_btc(gross: i64) -> Result<i64, SizingError> {
    i64::try_from(ceil(
        i128::from(gross)
            .checked_mul(10)
            .ok_or(SizingError::Overflow)?,
        BPS,
    )?)
    .map_err(|_| SizingError::Overflow)
}
fn amount(p: Price, q: Qty, up: bool) -> Result<i64, SizingError> {
    notional(p, q, up).map_err(|_| SizingError::Overflow)
}
fn quantity(v: i64) -> Result<Qty, SizingError> {
    Qty::from_ticks(v).map_err(|_| SizingError::Overflow)
}
fn price(v: i128) -> Result<Price, SizingError> {
    Price::from_ticks(i64::try_from(v).map_err(|_| SizingError::Overflow)?)
        .map_err(|_| SizingError::Price)
}
fn checked_add(a: i64, b: i64) -> Result<i64, SizingError> {
    a.checked_add(b).ok_or(SizingError::Overflow)
}
fn ceil(n: i128, d: i128) -> Result<i128, SizingError> {
    if n < 0 || d <= 0 {
        return Err(SizingError::Account);
    }
    let q = n / d;
    q.checked_add(i128::from(n % d != 0))
        .ok_or(SizingError::Overflow)
}
fn floor(n: i128, d: i128) -> Result<i128, SizingError> {
    if d <= 0 {
        return Err(SizingError::Account);
    }
    let q = n / d;
    q.checked_sub(i128::from(n < 0 && n % d != 0))
        .ok_or(SizingError::Overflow)
}
fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}
fn lcm(a: i64, b: i64) -> Result<i64, SizingError> {
    (a / gcd(a, b)).checked_mul(b).ok_or(SizingError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parangsae_sim::carry::{Allocation, CarryLedger};

    fn initial() -> CarryAccountState {
        CarryLedger::new(Allocation {
            total: Cash(100_000_000_000_000),
            spot_quote: Cash(47_500_000_000_000),
            perp_collateral: Cash(47_500_000_000_000),
            idle: Cash(5_000_000_000_000),
        })
        .expect("selected allocation")
        .snapshot()
    }

    fn refs(ticks: i64) -> EntryPrices {
        let value = Price::from_ticks(ticks).expect("positive test price");
        EntryPrices {
            spot_reference: value,
            perp_reference: value,
            spot_risk: value,
            perp_risk: value,
        }
    }

    fn sized(account: &CarryAccountState, prices: EntryPrices, scenario: Scenario) -> EntryQuote {
        match quote_entry(account, prices, scenario).expect("representable estimate") {
            EntrySizing::Sized(quote) => quote,
            EntrySizing::Refused(reason) => panic!("expected a feasible quote, got {reason:?}"),
        }
    }

    #[test]
    fn both_executed_bounds_preserve_one_e_minus_eight_precision() {
        let band = PriceBand::at(Price::from_ticks(10_012_345_678).expect("price")).expect("band");
        assert_eq!(band.lower.ticks(), 9_912_222_222);
        assert_eq!(band.upper.ticks(), 10_112_469_134);
        assert!(band.contains(band.lower));
        assert!(band.contains(band.upper));
        assert!(!band.contains(Price::from_ticks(9_912_222_221).expect("one tick below")));
        assert!(!band.contains(Price::from_ticks(10_112_469_135).expect("one tick above")));
    }

    #[test]
    fn fee_net_and_both_exit_grids_define_at_most_one_hundred_candidates() {
        assert_eq!(fee_btc(100_000_000).expect("fee"), 100_000);
        let quote = sized(&initial(), refs(10_000_000_000), Scenario::Headline);
        assert_eq!(quote.gross_spot.ticks(), 10_000_000_000);
        assert_eq!(quote.net_btc.ticks(), 9_990_000_000);
        assert_eq!(quote.net_btc.ticks() % SPOT_STEP, 0);
        assert_eq!(quote.net_btc.ticks() % PERP_STEP, 0);
        assert_eq!(quote.headline_c4.0, 3_797_200_200);
    }

    #[test]
    fn initial_caps_do_not_grow_when_wallet_profits_grow() {
        let original = sized(&initial(), refs(10_000_000_000_000), Scenario::Headline);
        assert_eq!(original.gross_spot.ticks(), 400_000_000);
        assert_eq!(original.net_btc.ticks(), 399_600_000);
        assert_eq!(original.spot_requirement.0, 42_900_000_000_000);
        assert_eq!(original.perp_requirement.0, 42_879_779_800_000);
        let mut richer = initial();
        richer.spot_quote = Cash(500_000_000_000_000);
        richer.perp_wallet = Cash(500_000_000_000_000);
        assert_eq!(
            sized(&richer, refs(10_000_000_000_000), Scenario::Headline).gross_spot,
            original.gross_spot
        );
        let mut poorer = initial();
        poorer.spot_quote = Cash(42_700_000_000_000);
        assert_eq!(
            sized(&poorer, refs(10_000_000_000_000), Scenario::Headline)
                .gross_spot
                .ticks(),
            300_000_000
        );
    }

    #[test]
    fn actual_cost_scenarios_keep_the_headline_estimator() {
        let base = sized(&initial(), refs(10_000_000_000_000), Scenario::Headline);
        let high = sized(
            &initial(),
            refs(10_000_000_000_000),
            Scenario::HigherPerpFee,
        );
        let stress = sized(
            &initial(),
            refs(10_000_000_000_000),
            Scenario::SlippageStress,
        );
        assert_eq!(high.headline_c4, base.headline_c4);
        assert_eq!(stress.headline_c4, base.headline_c4);
        assert!(high.perp_requirement.0 > base.perp_requirement.0);
        assert_eq!(stress.perp_requirement, base.perp_requirement);
    }

    #[test]
    fn headline_cost_retains_the_existing_closed_accounts_fractional_pnl() {
        let mut account = initial();
        let zero = sized(&account, refs(10_012_350_001), Scenario::Headline);
        account.realized_perp_scaled = 99_900_000;
        let retained = sized(&account, refs(10_012_350_001), Scenario::Headline);
        assert_eq!(retained.headline_c4.0, zero.headline_c4.0 - 1);
        assert_eq!(retained.gross_spot, zero.gross_spot);
        assert_eq!(account.realized_perp_scaled, 99_900_000);
        assert_eq!(account.realized_perp, Cash(0));
    }

    #[test]
    fn money_refusal_is_not_an_economic_false_or_a_free_size() {
        let mut account = initial();
        account.spot_quote = Cash(SPOT_CLOSE);
        assert_eq!(
            quote_entry(&account, refs(10_000_000_000), Scenario::Headline).expect("finite"),
            EntrySizing::Refused(Refusal::NoCompatibleSize)
        );
        account = initial();
        account.spot_btc = Qty::from_ticks(1).expect("owned dust");
        assert_eq!(
            quote_entry(&account, refs(10_000_000_000), Scenario::Headline).expect("finite"),
            EntrySizing::Refused(Refusal::Occupied)
        );
        let mut prices = refs(10_000_000_000);
        prices.perp_risk = Price::from_ticks(10_100_000_001).expect("above native cap");
        assert_eq!(
            quote_entry(&initial(), prices, Scenario::Headline).expect("finite"),
            EntrySizing::Refused(Refusal::NativePriceEnvelope)
        );
    }

    #[test]
    fn strict_forecast_comparison_keeps_exact_rational_units() {
        let qty = Qty::from_ticks(100_000_000).expect("one BTC");
        let price = Price::from_ticks(10_000_000_000).expect("100USDT");
        assert!(!forecast_above_cost(qty, price, 1, 100, Cash(100_000_000)).expect("equality"));
        assert!(forecast_above_cost(qty, price, 1, 100, Cash(99_999_999)).expect("above"));
        assert!(
            !forecast_above_cost(qty, price, -1, 100, Cash(0)).expect("negative projected income")
        );
        assert_eq!(
            forecast_above_cost(qty, price, 1, 0, Cash(0)),
            Err(SizingError::Account)
        );
        assert_eq!(
            forecast_above_cost(qty, price, i128::MAX, 1, Cash(0)),
            Err(SizingError::Overflow)
        );
    }
}
