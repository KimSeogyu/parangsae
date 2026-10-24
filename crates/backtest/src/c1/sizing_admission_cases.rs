use super::*;

#[test]
fn current_net_spot_exit_below_minimum_is_a_size_refusal() {
    let mut account = initial();
    // Preserve full C0 while making only the spot wallet binding.
    account.spot_quote = Cash(2_510_100_000_000);
    account.perp_wallet = Cash(92_489_900_000_000);
    let mut prices = refs(10_000_000_000);
    prices.perp_reference = Price::from_ticks(10_100_000_000).expect("101USDT");
    prices.perp_risk = prices.perp_reference;

    let gross = Qty::from_ticks(100_000_000).expect("one grossBTC");
    let net = Qty::from_ticks(99_900_000).expect("fee-net0.999BTC");
    assert_eq!(
        amount(prices.spot_reference, gross, false).expect("gross"),
        MIN_NOTIONAL
    );
    assert_eq!(
        amount(prices.spot_reference, net, false).expect("net spot"),
        9_990_000_000
    );
    assert_eq!(
        amount(prices.perp_reference, net, false).expect("net perp"),
        10_089_900_000
    );
    assert_eq!(
        account.spot_quote.0 + account.perp_wallet.0 + account.idle.0,
        100_000_000_000_000
    );

    for scenario in [Scenario::Headline, Scenario::HigherPerpFee] {
        assert_eq!(
            quote_entry(&account, prices, scenario).expect("known finite terms"),
            EntrySizing::Refused(Refusal::NoCompatibleSize),
            "gross spot and net perp passing their minimum cannot hide a known net spot-exit refusal"
        );
    }
}

#[test]
fn richer_perp_wallet_cannot_expand_the_initial_cap_past_its_entry_fee() {
    let mut account = initial();
    account.perp_wallet = Cash(50_000_000_000_000);
    let mut prices = refs(10_000_000_000_000);
    prices.perp_reference = Price::from_ticks(11_149_000_000_000).expect("111490USDT");
    prices.perp_risk = prices.perp_reference;
    let proposed_net = Qty::from_ticks(399_600_000).expect("3.996BTC");
    let upper = PriceBand::at(prices.perp_reference)
        .expect("perp band")
        .upper;
    let margin = amount(upper, proposed_net, true).expect("native margin");
    assert_eq!(margin, 44_996_918_040_000);
    assert!(margin < EACH_CAP);

    for (scenario, expected_fee, expected_all_in) in [
        (Scenario::Headline, 22_498_459_020, 45_019_416_499_020),
        (Scenario::HigherPerpFee, 26_998_150_824, 45_023_916_190_824),
    ] {
        let fee = taker_fee(upper, proposed_net, scenario.fees().perp_bps)
            .expect("scenario entry fee")
            .0;
        assert_eq!(fee, expected_fee);
        assert_eq!(margin + fee, expected_all_in);
        assert!(expected_all_in > EACH_CAP);
        assert!(expected_all_in + PERP_CLOSE_AND_FUNDING < account.perp_wallet.0);

        let quote = sized(&account, prices, scenario);
        assert_eq!(
            quote.gross_spot.ticks(),
            300_000_000,
            "the richer wallet funds4BTC but cannot raise the original all-in entry cap"
        );
        assert_eq!(quote.net_btc.ticks(), 299_700_000);
        assert!(quote.perp_requirement.0 - PERP_CLOSE_AND_FUNDING <= EACH_CAP);
    }
}

#[test]
fn btc_buy_fee_is_inventory_not_a_second_usdt_entry_fee() {
    let mut account = initial();
    account.spot_quote = Cash(42_900_000_000_000);
    let prices = refs(10_000_000_000_000);
    let headline = sized(&account, prices, Scenario::Headline);
    let higher_fee = sized(&account, prices, Scenario::HigherPerpFee);

    for quote in [headline, higher_fee] {
        assert_eq!(quote.gross_spot.ticks(), 400_000_000);
        assert_eq!(fee_btc(quote.gross_spot.ticks()).expect("BTC fee"), 400_000);
        assert_eq!(quote.net_btc.ticks(), 399_600_000);
        assert_eq!(quote.net_btc.ticks() % SPOT_STEP, 0);
        assert_eq!(quote.net_btc.ticks() % PERP_STEP, 0);
        assert_eq!(
            quote.spot_requirement.0, account.spot_quote.0,
            "404000USDT principal plus25000reserve fits exactly; no quote-paid buy fee"
        );
    }
    assert_eq!(
        higher_fee.headline_c4, headline.headline_c4,
        "actual6bps must not replace the fixed5/2 headline estimator"
    );
    assert!(higher_fee.perp_requirement.0 > headline.perp_requirement.0);
}
