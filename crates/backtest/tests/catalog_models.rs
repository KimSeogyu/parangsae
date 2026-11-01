//! Executable supplied-data observers. No tape, study, order or credentials.
use cucumber::{given, then, when, World as _};
use parangsae_backtest::catalog::{
    evaluate as dispatch, Error, Family, InputUniverse, Output, Request,
};
use parangsae_core::{
    AvailableTs, DecisionTime, EventTs, Observation, Price, ReceivedTs, Symbol, Timestamps,
    UtcMicros,
};
use parangsae_signals::CausalWindow;
use parangsae_strategies::catalog::{derivatives::*, models::*};
use std::num::NonZeroUsize;

#[derive(Debug, Default, cucumber::World)]
struct ModelWorld {
    case: String,
    outcome: Option<Result<Output, Error>>,
}
fn at(t: i64) -> DecisionTime {
    DecisionTime::from_micros(UtcMicros::from_micros(t))
}
fn obs<T>(t: i64, value: T) -> Observation<T> {
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
fn symbol(s: &str) -> Symbol {
    Symbol::new(s).expect("synthetic symbol")
}
fn price(n: i64) -> Price {
    Price::from_ticks(n).expect("nonnegative fixture price")
}

#[given(regex = r"^the supplied-data model fixture is (.+)$")]
fn fixture(world: &mut ModelWorld, case: String) {
    world.case = case;
}

#[when("the catalogue model public API is evaluated")]
fn evaluate(world: &mut ModelWorld) {
    world.outcome = Some(match world.case.as_str() {
        "two-factor residuals" => {
            let assets = vec![
                FactorAsset::new(symbol("A"), 0, [10_000, 0]).expect("asset"),
                FactorAsset::new(symbol("B"), 0, [0, 10_000]).expect("asset"),
                FactorAsset::new(symbol("C"), 0, [10_000, 20_000]).expect("asset"),
            ];
            let model = obs(10, FactorModel::new(at(9), assets).expect("model"));
            let snapshot = obs(
                20,
                FactorSnapshot::new(
                    at(20),
                    [100, 50],
                    vec![(symbol("A"), 150), (symbol("B"), 80), (symbol("C"), 190)],
                )
                .expect("snapshot"),
            );
            dispatch(
                InputUniverse::SuppliedOffline,
                Request::StatisticalArbitrage {
                    model: &model.causal_at(at(20)).expect("visible"),
                    snapshot: &snapshot.causal_at(at(20)).expect("visible"),
                    decision: at(20),
                    min_gap: 0,
                },
            )
        }
        "announced surprise" | "unavailable announcement" | "stale announcement" => {
            let release = obs(
                20,
                EventRelease::new(at(20), at(10), 150, 100).expect("event"),
            );
            let decision = match world.case.as_str() {
                "unavailable announcement" => at(19),
                "stale announcement" => at(30),
                _ => at(20),
            };
            // Deliberately attempt to reuse a later-gated view at an earlier decision.
            dispatch(
                InputUniverse::SuppliedOffline,
                Request::Event {
                    release: &release.causal_at(at(30)).expect("view"),
                    decision,
                    threshold: 10,
                    max_age_micros: 5,
                },
            )
        }
        "net carry" | "overflowing carry" => {
            let income = if world.case == "overflowing carry" {
                i64::MIN
            } else {
                90
            };
            let terms = obs(1, CarryTerms::new(income, 20, 10).expect("terms"));
            dispatch(
                InputUniverse::SuppliedOffline,
                Request::Carry {
                    terms: &terms.causal_at(at(1)).expect("view"),
                    decision: at(1),
                },
            )
        }
        "implied versus forecast variance" => {
            let terms = obs(1, VolatilityTerms::new(100, 140, 20).expect("terms"));
            dispatch(
                InputUniverse::SuppliedOffline,
                Request::Volatility {
                    terms: &terms.causal_at(at(1)).expect("view"),
                    decision: at(1),
                },
            )
        }
        "covered call expiry scenario"
        | "expiry predates contract"
        | "expiry after decision"
        | "USD-M options unsupported" => {
            let contract = obs(
                2,
                CoveredOption::new(
                    OptionKind::CoveredCall,
                    price(110),
                    price(100),
                    price(5),
                    price(1),
                )
                .expect("option"),
            );
            let expiry = obs(
                if world.case == "expiry predates contract" {
                    1
                } else {
                    3
                },
                price(150),
            );
            let decision = if world.case == "expiry after decision" {
                at(2)
            } else {
                at(3)
            };
            dispatch(
                if world.case == "USD-M options unsupported" {
                    InputUniverse::BtcUsdM
                } else {
                    InputUniverse::SuppliedOffline
                },
                Request::Options {
                    contract: &contract.causal_at(at(3)).expect("view"),
                    expiry: &expiry.causal_at(at(3)).expect("view"),
                    decision,
                },
            )
        }
        "fitted later prediction" | "in-sample prediction" => {
            let rows = [
                obs(2, LabeledSample::new(at(1), at(2), 1, 3).expect("sample")),
                obs(4, LabeledSample::new(at(3), at(4), 2, 5).expect("sample")),
                obs(6, LabeledSample::new(at(5), at(6), 3, 7).expect("sample")),
            ];
            let window = CausalWindow::from_views(
                at(6),
                NonZeroUsize::new(3).expect("cap"),
                rows.iter().filter_map(|r| r.causal_at(at(6))),
            )
            .expect("window");
            let model = RidgeModel::fit(&window, 0).expect("fit line");
            let t = if world.case == "in-sample prediction" {
                6
            } else {
                7
            };
            let predictor = obs(t, Predictor::new(at(t), 4).expect("predictor"));
            dispatch(
                InputUniverse::SuppliedOffline,
                Request::MachineLearning {
                    model: &model,
                    predictor: &predictor.causal_at(at(7)).expect("view"),
                    decision: at(7),
                    threshold: 0,
                },
            )
        }
        case => panic!("unrecognized fixture: {case}"),
    });
}

#[then(regex = r"^the model result is (.+)$")]
fn observe_result(world: &mut ModelWorld, result: String) {
    let expected = match result.as_str() {
        "long C short A with residual gap 60" => {
            Ok(Output::StatisticalArbitrage(StatArbAssessment::Pair {
                long: symbol("C"),
                short: symbol("A"),
                residual_gap: 60,
            }))
        }
        "long surprise 50" => Ok(Output::Event(EventAssessment {
            surprise: 50,
            direction: ModelDirection::Long,
        })),
        "event future refusal" => Err(Error::Model(ModelError::AfterDecision)),
        "event stale refusal" => Err(Error::Model(ModelError::Stale)),
        "positive carry 60" => Ok(Output::Carry(CarryAssessment::Positive { net_bps: 60 })),
        "carry overflow refusal" => Err(Error::Derivative(DerivativeError::Overflow)),
        "long delta-hedged variance edge 40" => Ok(Output::Volatility(
            VolatilityAssessment::LongOptionDeltaHedged { variance_edge: 40 },
        )),
        "capped option pnl 14 requiring 100" => Ok(Output::Options(OptionPayoff {
            pnl_ticks: 14,
            required_collateral_ticks: 100,
        })),
        "unsupported USD-M options refusal" => {
            Err(Error::UnsupportedDomain(Family::OptionStrategies))
        }
        "expiry chronology refusal" => Err(Error::Derivative(DerivativeError::InvalidTerms)),
        "expiry visibility refusal" => Err(Error::Derivative(DerivativeError::AfterDecision)),
        "learned score 9 long" => Ok(Output::Prediction(Prediction {
            score: 9,
            direction: ModelDirection::Long,
        })),
        "training cutoff refusal" => Err(Error::Model(ModelError::TrainingCutoff)),
        result => panic!("unrecognized result: {result}"),
    };
    assert_eq!(world.outcome.as_ref(), Some(&expected));
}

#[tokio::main]
async fn main() {
    ModelWorld::cucumber()
        .fail_on_skipped()
        .run_and_exit(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/features/catalog_models.feature"
        ))
        .await;
}
