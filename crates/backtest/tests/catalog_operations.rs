use cucumber::{then, when, World as _};
use parangsae_backtest::catalog::{self, Family, InputUniverse, Output, Request};
use parangsae_core::{
    AvailableTs, BarInterval, DecisionTime, EventTs, Observation, Price, ReceivedTs, ResampledBar,
    Symbol, Timestamps, UtcMicros,
};
use parangsae_execution::planning::{QuoteConfig, QuoteState, VolumeProfile};
use parangsae_risk::allocation::{Regime, RiskEstimates, Volatility, VolatilityRegime};
use parangsae_strategies::{
    catalog::derivatives::{CoveredOption, OptionKind},
    Strategy,
};
use std::collections::BTreeMap;
#[derive(Debug, Default, cucumber::World)]
struct World {
    checked: bool,
}
fn at(t: i64) -> DecisionTime {
    DecisionTime::from_micros(UtcMicros::from_micros(t))
}
fn obs<T>(t: i64, v: T) -> Observation<T> {
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(t)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(t)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(t)),
        },
        v,
    )
}
#[when("I inspect the eighteen source families")]
fn catalogue(w: &mut World) {
    let numbers: Vec<_> = Family::ALL.iter().map(|x| x.source_number()).collect();
    assert_eq!(numbers, (1..=18).collect::<Vec<_>>());
    assert_eq!(Family::RiskParity.role(), catalog::Role::Allocation);
    assert_eq!(Family::ExecutionAlgorithms.role(), catalog::Role::Execution);
    assert!(Family::ALL
        .iter()
        .all(|f| f.supports(InputUniverse::SuppliedOffline)));
    assert_eq!(
        Family::ALL
            .iter()
            .filter(|f| f.supports(InputUniverse::BtcUsdM))
            .count(),
        8
    );
    w.checked = true;
}
#[when("I request option assessment in the current BTC USD-M domain")]
fn unsupported(w: &mut World) {
    let price = Price::from_ticks(100).unwrap();
    let contract = obs(
        1,
        CoveredOption::new(
            OptionKind::CoveredCall,
            price,
            price,
            price,
            Price::from_ticks(0).unwrap(),
        )
        .unwrap(),
    );
    let expiry = obs(2, price);
    let result = catalog::evaluate(
        InputUniverse::BtcUsdM,
        Request::Options {
            contract: &contract.causal_at(at(2)).unwrap(),
            expiry: &expiry.causal_at(at(2)).unwrap(),
            decision: at(2),
        },
    );
    assert_eq!(
        result,
        Err(catalog::Error::UnsupportedDomain(Family::OptionStrategies))
    );
    w.checked = true;
}
#[when("the synthetic quote inventory is at its long limit")]
fn market(w: &mut World) {
    let row = obs(1, QuoteState::new(100, 102, 10, 10).unwrap());
    let out = catalog::evaluate(
        InputUniverse::SuppliedOffline,
        Request::MarketMaking {
            state: &row.causal_at(at(1)).unwrap(),
            decision: at(1),
            config: QuoteConfig::new(2, 2, 5).unwrap(),
        },
    )
    .unwrap();
    let Output::Quotes(plan) = out else {
        panic!("typed quote output")
    };
    assert_eq!(plan.bid(), None);
    assert_eq!(plan.ask(), Some((104, 2)));
    w.checked = true;
}
#[when("I split ten lots into three equal-time slices")]
fn twap(w: &mut World) {
    let out = catalog::evaluate(
        InputUniverse::BtcUsdM,
        Request::Twap {
            lots: 10,
            start_micros: 1,
            end_micros: 10,
            count: 3,
        },
    )
    .unwrap();
    let Output::Schedule(s) = out else {
        panic!("schedule")
    };
    assert_eq!(s.slices(), &[(1, 4), (4, 3), (7, 3)]);
    w.checked = true;
}
#[when("I split ten lots using a known one two one volume profile")]
fn vwap(w: &mut World) {
    let row = obs(0, VolumeProfile::new(vec![1, 2, 1]).unwrap());
    let out = catalog::evaluate(
        InputUniverse::BtcUsdM,
        Request::Vwap {
            lots: 10,
            start_micros: 1,
            end_micros: 10,
            profile: &row.causal_at(at(0)).unwrap(),
            decision: at(0),
        },
    )
    .unwrap();
    let Output::Schedule(s) = out else {
        panic!("schedule")
    };
    assert_eq!(s.slices(), &[(1, 3), (4, 5), (7, 2)]);
    w.checked = true;
}
#[when("two synthetic assets have volatility one hundred and two hundred")]
fn parity(w: &mut World) {
    let row = obs(
        1,
        RiskEstimates::new(vec![
            (Symbol::new("BTCUSDT").unwrap(), 100),
            (Symbol::new("ETHUSDT").unwrap(), 200),
        ])
        .unwrap(),
    );
    let out = catalog::evaluate(
        InputUniverse::SuppliedOffline,
        Request::RiskParity {
            estimates: &row.causal_at(at(1)).unwrap(),
            decision: at(1),
        },
    )
    .unwrap();
    let Output::Allocation(a) = out else {
        panic!("allocation")
    };
    assert_eq!(a.weights()[0].1, 66_666_667);
    assert_eq!(a.weights()[1].1, 33_333_333);
    w.checked = true;
}
#[when("causal volatility changes from high through middle to low")]
fn regime(w: &mut World) {
    let mut model = VolatilityRegime::new(100, 200).unwrap();
    for (t, v, r) in [
        (1, 250, Regime::Defensive),
        (2, 150, Regime::Defensive),
        (3, 100, Regime::Normal),
    ] {
        let row = obs(t, Volatility::new(v).unwrap());
        assert_eq!(
            catalog::evaluate(
                InputUniverse::BtcUsdM,
                Request::Regime {
                    model: &mut model,
                    volatility: &row.causal_at(at(t)).unwrap(),
                    decision: at(t)
                }
            ),
            Ok(Output::Regime(r))
        );
    }
    w.checked = true;
}
#[when("an existing fade strategy receives a zero-price bar then its correction")]
fn retry(w: &mut World) {
    let params = BTreeMap::from([
        ("l".into(), 24),
        ("k".into(), 100_000_000),
        ("hold_h".into(), 4),
    ]);
    let mut repaired = Strategy::from_params("S05", &params).unwrap();
    let mut fresh = Strategy::from_params("S05", &params).unwrap();
    let open = UtcMicros::from_micros(1_704_067_200_000_000);
    let row = |ticks| {
        let p = Price::from_ticks(ticks).unwrap();
        let b = ResampledBar::new(open, BarInterval::H1, p, p, p, p).unwrap();
        obs(b.close_time().micros() + 1000, b)
    };
    assert!(repaired.push(&row(0), None).is_err());
    assert_eq!(repaired.push(&row(100), None), fresh.push(&row(100), None));
    w.checked = true;
}
#[when("extreme inverse-volatility weights cannot remain positive")]
fn resolution(w: &mut World) {
    let vols = std::iter::repeat_n(1, 100)
        .chain(std::iter::repeat_n(2, 22))
        .chain(std::iter::repeat_n(3, 5))
        .chain([1_000_000]);
    let assets = vols
        .enumerate()
        .map(|(i, v)| (Symbol::new(&format!("A{i:03}")).unwrap(), v))
        .collect();
    let row = obs(1, RiskEstimates::new(assets).unwrap());
    assert_eq!(
        catalog::evaluate(
            InputUniverse::SuppliedOffline,
            Request::RiskParity {
                estimates: &row.causal_at(at(1)).unwrap(),
                decision: at(1)
            }
        ),
        Err(catalog::Error::Allocation(
            parangsae_risk::allocation::AllocationError::Resolution
        ))
    );
    w.checked = true;
}
#[then(
    regex = "^(each source number is unique and complete|the catalogue refuses the unsupported option domain|only the inventory-reducing ask is proposed|the schedule has quantities four three three and exact times|the schedule has quantities three five two|their quantized allocation is two thirds and one third|the defensive regime persists until the low threshold|its corrected output matches a fresh strategy|the allocation reports a typed resolution refusal)$"
)]
fn verified(w: &mut World, _label: String) {
    assert!(w.checked);
}
#[tokio::main]
async fn main() {
    World::cucumber()
        .fail_on_skipped()
        .run_and_exit("tests/features/catalog_operations.feature")
        .await;
}
