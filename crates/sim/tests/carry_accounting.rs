use parangsae_core::{Price, Qty, Side};
use parangsae_sim::carry::{
    AccountEvent, AccountFill, Allocation, CarryAccountingError, CarryLedger, FeeCharge, Market,
};
use parangsae_sim::Cash;
use proptest::prelude::*;

const U: i64 = 100_000_000;

fn price(value: i64) -> Price {
    Price::from_ticks(value).unwrap()
}
fn qty(value: i64) -> Qty {
    Qty::from_ticks(value).unwrap()
}
fn allocated() -> CarryLedger {
    CarryLedger::new(Allocation {
        total: Cash(10_000 * U),
        spot_quote: Cash(4_000 * U),
        perp_collateral: Cash(5_000 * U),
        idle: Cash(1_000 * U),
    })
    .unwrap()
}
fn fill(market: Market, side: Side, lots: i64, p: i64, fee: FeeCharge) -> AccountEvent {
    AccountEvent::Fill(AccountFill {
        market,
        side,
        qty: qty(lots),
        price: price(p),
        fee,
    })
}
fn apply(ledger: &mut CarryLedger, events: &[AccountEvent]) {
    ledger.prepare(events).unwrap().commit();
}

#[test]
fn one_capital_pool_cannot_seed_two_full_wallets() {
    assert_eq!(
        CarryLedger::new(Allocation {
            total: Cash(10_000 * U),
            spot_quote: Cash(10_000 * U),
            perp_collateral: Cash(10_000 * U),
            idle: Cash(0),
        })
        .unwrap_err(),
        CarryAccountingError::InvalidAllocation
    );
}

#[test]
fn opening_a_short_credits_no_notional_to_any_wallet() {
    let mut ledger = allocated();
    apply(
        &mut ledger,
        &[fill(
            Market::UsdMPerp,
            Side::Sell,
            U / 10,
            30_100 * U,
            FeeCharge::Usdt(Cash(150_500_000)),
        )],
    );
    let state = ledger.snapshot();
    assert_eq!(state.perp_wallet, Cash(499_849_500_000));
    assert_eq!(state.spot_quote, Cash(4_000 * U));
    assert_eq!(state.idle, Cash(1_000 * U));
    assert_eq!(state.perp_short, qty(U / 10));
    assert_eq!(
        state.equity(price(30_000 * U), price(30_100 * U)).unwrap(),
        Cash(999_849_500_000)
    );
}

#[test]
fn four_executions_reconcile_basis_pnl_and_every_fee() {
    let mut ledger = allocated();
    apply(
        &mut ledger,
        &[
            fill(
                Market::Spot,
                Side::Buy,
                U / 10,
                30_000 * U,
                FeeCharge::Usdt(Cash(3 * U)),
            ),
            fill(
                Market::UsdMPerp,
                Side::Sell,
                U / 10,
                30_100 * U,
                FeeCharge::Usdt(Cash(150_500_000)),
            ),
            fill(
                Market::UsdMPerp,
                Side::Buy,
                U / 10,
                31_100 * U,
                FeeCharge::Usdt(Cash(155_500_000)),
            ),
            fill(
                Market::Spot,
                Side::Sell,
                U / 10,
                31_000 * U,
                FeeCharge::Usdt(Cash(310_000_000)),
            ),
        ],
    );
    let state = ledger.snapshot();
    assert_eq!(state.spot_btc, qty(0));
    assert_eq!(state.perp_short, qty(0));
    assert_eq!(state.realized_perp, Cash(-100 * U));
    assert_eq!(state.spot_quote_fees, Cash(610_000_000));
    assert_eq!(state.perp_fees, Cash(306_000_000));
    assert_eq!(
        state.equity(price(31_000 * U), price(31_100 * U)).unwrap(),
        Cash(999_084_000_000)
    );
}

#[test]
fn btc_fee_is_owned_inventory_not_a_quote_cash_fee() {
    let mut ledger = allocated();
    apply(
        &mut ledger,
        &[fill(
            Market::Spot,
            Side::Buy,
            10_010_000,
            30_000 * U,
            FeeCharge::Btc(qty(10_000)),
        )],
    );
    let state = ledger.snapshot();
    assert_eq!(state.spot_btc, qty(10_000_000));
    assert_eq!(state.spot_quote, Cash(997 * U));
    assert_eq!(state.spot_btc_fees, qty(10_000));
    assert_eq!(state.spot_quote_fees, Cash(0));
    let before = state;
    assert_eq!(
        ledger
            .prepare(&[fill(
                Market::Spot,
                Side::Sell,
                10_000_000,
                30_000 * U,
                FeeCharge::Btc(qty(1))
            )])
            .unwrap_err(),
        CarryAccountingError::InsufficientBtc
    );
    assert_eq!(ledger.snapshot(), before);
}

#[test]
fn invalid_late_batch_entry_changes_no_wallet_or_fee() {
    let mut ledger = allocated();
    let before = ledger.snapshot();
    assert_eq!(
        ledger
            .prepare(&[
                fill(
                    Market::Spot,
                    Side::Buy,
                    U / 10,
                    30_000 * U,
                    FeeCharge::Usdt(Cash(3 * U))
                ),
                AccountEvent::Funding(Cash(-50_000 * U)),
            ])
            .unwrap_err(),
        CarryAccountingError::InsufficientPerpCash
    );
    assert_eq!(ledger.snapshot(), before);
}

#[test]
fn partial_closes_use_entry_cost_and_never_buy_a_long() {
    let mut ledger = allocated();
    apply(
        &mut ledger,
        &[
            fill(
                Market::UsdMPerp,
                Side::Sell,
                U / 10,
                30_000 * U,
                FeeCharge::Usdt(Cash(0)),
            ),
            fill(
                Market::UsdMPerp,
                Side::Sell,
                U / 10,
                32_000 * U,
                FeeCharge::Usdt(Cash(0)),
            ),
            fill(
                Market::UsdMPerp,
                Side::Buy,
                U / 20,
                31_000 * U,
                FeeCharge::Usdt(Cash(0)),
            ),
            fill(
                Market::UsdMPerp,
                Side::Buy,
                3 * U / 20,
                33_000 * U,
                FeeCharge::Usdt(Cash(0)),
            ),
        ],
    );
    let before = ledger.snapshot();
    assert_eq!(before.perp_short, qty(0));
    assert_eq!(before.realized_perp, Cash(-300 * U));
    assert_eq!(before.perp_wallet, Cash(4_700 * U));
    assert_eq!(
        ledger
            .prepare(&[fill(
                Market::UsdMPerp,
                Side::Buy,
                1,
                U,
                FeeCharge::Usdt(Cash(0))
            )])
            .unwrap_err(),
        CarryAccountingError::NotShort
    );
    assert_eq!(ledger.snapshot(), before);
}

#[test]
fn odd_price_partial_closes_preserve_the_final_scaled_basis() {
    let mut ledger = allocated();
    let entry = 3i128 * 1_000_000_003 + 4i128 * 1_000_000_019;
    let exits = 2i128 * 1_000_000_007 + 5i128 * 1_000_000_023;
    apply(
        &mut ledger,
        &[
            fill(
                Market::UsdMPerp,
                Side::Sell,
                3,
                1_000_000_003,
                FeeCharge::Usdt(Cash(0)),
            ),
            fill(
                Market::UsdMPerp,
                Side::Sell,
                4,
                1_000_000_019,
                FeeCharge::Usdt(Cash(0)),
            ),
            fill(
                Market::UsdMPerp,
                Side::Buy,
                2,
                1_000_000_007,
                FeeCharge::Usdt(Cash(0)),
            ),
        ],
    );
    let middle = ledger.snapshot();
    let whole_pnl = entry - exits;
    let after = fill(
        Market::UsdMPerp,
        Side::Buy,
        5,
        1_000_000_023,
        FeeCharge::Usdt(Cash(0)),
    );
    apply(&mut ledger, &[after]);
    let final_state = ledger.snapshot();
    assert_eq!(final_state.perp_entry_cost_scaled, 0);
    assert_eq!(final_state.realized_perp_scaled, whole_pnl);
    assert_eq!(final_state.realized_perp, Cash(-1));
    assert_eq!(final_state.perp_wallet, Cash(5_000 * U - 1));
    let mark = price(1_000_000_023);
    let exact_middle = (entry - exits).div_euclid(i128::from(U));
    let marked = i128::from(middle.realized_perp.0 + middle.unrealized_perp(mark).unwrap().0);
    // Two separately floored components can lose at most one additional cash tick.
    assert!((exact_middle - marked).abs() <= 1);
}

#[test]
fn both_funding_signs_change_only_the_futures_wallet() {
    let mut ledger = allocated();
    apply(
        &mut ledger,
        &[
            AccountEvent::Funding(Cash(123)),
            AccountEvent::Funding(Cash(-45)),
        ],
    );
    let state = ledger.snapshot();
    assert_eq!(state.perp_wallet, Cash(5_000 * U + 78));
    assert_eq!(state.funding_net, Cash(78));
    assert_eq!(state.spot_quote, Cash(4_000 * U));
    assert_eq!(state.idle, Cash(1_000 * U));
}

proptest! {
    #[test]
    fn constant_price_split_roundtrips_preserve_the_one_capital_pool(
        lots in 2i64..100_000_000,
        units in 1i64..90_000,
        partition in 1i64..100_000_000,
    ) {
        let mut ledger = CarryLedger::new(Allocation {
            total: Cash(300_000 * U), spot_quote: Cash(100_000 * U),
            perp_collateral: Cash(100_000 * U), idle: Cash(100_000 * U),
        }).unwrap();
        let first = 1 + partition % (lots - 1);
        let p = units * U;
        apply(&mut ledger, &[
            fill(Market::Spot, Side::Buy, lots, p, FeeCharge::Usdt(Cash(0))),
            fill(Market::UsdMPerp, Side::Sell, first, p, FeeCharge::Usdt(Cash(0))),
            fill(Market::UsdMPerp, Side::Sell, lots - first, p, FeeCharge::Usdt(Cash(0))),
            fill(Market::UsdMPerp, Side::Buy, lots - first, p, FeeCharge::Usdt(Cash(0))),
            fill(Market::UsdMPerp, Side::Buy, first, p, FeeCharge::Usdt(Cash(0))),
            fill(Market::Spot, Side::Sell, lots, p, FeeCharge::Usdt(Cash(0))),
        ]);
        let state = ledger.snapshot();
        prop_assert_eq!(state.equity(price(p), price(p)).unwrap(), Cash(300_000 * U));
        prop_assert_eq!(state.spot_btc, qty(0));
        prop_assert_eq!(state.perp_short, qty(0));
        prop_assert_eq!(state.realized_perp, Cash(0));
    }

    #[test]
    fn mixed_tick_entries_and_partial_exits_reconcile_exact_final_cashflow(
        lots in 2i64..1_000_000,
        partition in 1i64..1_000_000,
        closing_partition in 1i64..1_000_000,
        p1 in 1i64..1_000_000_000_000,
        p2 in 1i64..1_000_000_000_000,
        p3 in 1i64..1_000_000_000_000,
        p4 in 1i64..1_000_000_000_000,
    ) {
        let mut ledger = allocated();
        let q1 = 1 + partition % (lots - 1);
        let q3 = 1 + closing_partition % (lots - 1);
        apply(&mut ledger, &[
            fill(Market::UsdMPerp, Side::Sell, q1, p1, FeeCharge::Usdt(Cash(0))),
            fill(Market::UsdMPerp, Side::Sell, lots - q1, p2, FeeCharge::Usdt(Cash(0))),
            fill(Market::UsdMPerp, Side::Buy, q3, p3, FeeCharge::Usdt(Cash(0))),
        ]);
        let middle = ledger.snapshot();
        let all_entries = i128::from(q1) * i128::from(p1) + i128::from(lots - q1) * i128::from(p2);
        let all_exits = i128::from(q3) * i128::from(p3) + i128::from(lots - q3) * i128::from(p4);
        let exact_cash = (all_entries - all_exits).div_euclid(i128::from(U));
        let independently_marked = (all_entries - i128::from(q3) * i128::from(p3) - i128::from(lots - q3) * i128::from(p4)).div_euclid(i128::from(U));
        let separately_rounded = i128::from(middle.realized_perp.0 + middle.unrealized_perp(price(p4)).unwrap().0);
        prop_assert!((independently_marked - separately_rounded).abs() <= 1);
        apply(&mut ledger, &[fill(Market::UsdMPerp, Side::Buy, lots - q3, p4, FeeCharge::Usdt(Cash(0)))]);
        let final_state = ledger.snapshot();
        prop_assert_eq!(final_state.perp_entry_cost_scaled, 0);
        prop_assert_eq!(final_state.realized_perp_scaled, all_entries - all_exits);
        prop_assert_eq!(i128::from(final_state.perp_wallet.0), i128::from(5_000 * U) + exact_cash);
    }
}
