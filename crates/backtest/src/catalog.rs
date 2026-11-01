//! Typed, pure dispatcher for the 18-family offline research catalogue.
//!
//! Unlike `run`, this API never reads a tape, simulates fills, starts a trial or
//! produces returns. Its heterogeneous outputs are proposals/features, not
//! interchangeable position targets or execution permissions. Training remains
//! explicit through `RidgeModel::fit` on a separate causal labeled window.
use parangsae_core::{Causal, DecisionTime, Price};
use parangsae_execution::planning;
use parangsae_risk::allocation;
use parangsae_signals::CausalWindow;
use parangsae_strategies::catalog::{derivatives, models, panel, price};
pub use parangsae_strategies::catalog::{Family, InputUniverse, Role};

pub enum Request<'a, 'data> {
    Price {
        rule: price::PriceRule,
        window: &'a CausalWindow<'data, price::PriceBar>,
    },
    StatisticalArbitrage {
        model: &'a Causal<'data, models::FactorModel>,
        snapshot: &'a Causal<'data, models::FactorSnapshot>,
        decision: DecisionTime,
        min_gap: i64,
    },
    Pairs {
        rule: panel::PairRule,
        window: &'a CausalWindow<'data, panel::PairBar>,
    },
    MarketMaking {
        state: &'a Causal<'data, planning::QuoteState>,
        decision: DecisionTime,
        config: planning::QuoteConfig,
    },
    Event {
        release: &'a Causal<'data, models::EventRelease>,
        decision: DecisionTime,
        threshold: i64,
        max_age_micros: i64,
    },
    Factor {
        panel: &'a Causal<'data, panel::Panel>,
        decision: DecisionTime,
        weights: &'a panel::FactorWeights,
        per_side: usize,
    },
    Carry {
        terms: &'a Causal<'data, derivatives::CarryTerms>,
        decision: DecisionTime,
    },
    Volatility {
        terms: &'a Causal<'data, derivatives::VolatilityTerms>,
        decision: DecisionTime,
    },
    Options {
        contract: &'a Causal<'data, derivatives::CoveredOption>,
        expiry: &'a Causal<'data, Price>,
        decision: DecisionTime,
    },
    CrossSection {
        panel: &'a Causal<'data, panel::Panel>,
        decision: DecisionTime,
        per_side: usize,
    },
    RiskParity {
        estimates: &'a Causal<'data, allocation::RiskEstimates>,
        decision: DecisionTime,
    },
    Regime {
        model: &'a mut allocation::VolatilityRegime,
        volatility: &'a Causal<'data, allocation::Volatility>,
        decision: DecisionTime,
    },
    MachineLearning {
        model: &'a models::RidgeModel,
        predictor: &'a Causal<'data, models::Predictor>,
        decision: DecisionTime,
        threshold: i64,
    },
    Twap {
        lots: u64,
        start_micros: i64,
        end_micros: i64,
        count: usize,
    },
    Vwap {
        lots: u64,
        start_micros: i64,
        end_micros: i64,
        profile: &'a Causal<'data, planning::VolumeProfile>,
        decision: DecisionTime,
    },
}
impl Request<'_, '_> {
    pub const fn family(&self) -> Family {
        match self {
            Self::Price { rule, .. } => match rule {
                price::PriceRule::Momentum { .. } => Family::Momentum,
                price::PriceRule::MeanReversion { .. } => Family::MeanReversion,
                price::PriceRule::TrendFollowing { .. } => Family::TrendFollowing,
                price::PriceRule::ShortTermReversal { .. } => Family::ShortTermReversal,
                price::PriceRule::Breakout { .. } => Family::BreakoutTrading,
            },
            Self::StatisticalArbitrage { .. } => Family::StatisticalArbitrage,
            Self::Pairs { .. } => Family::PairsTrading,
            Self::MarketMaking { .. } => Family::MarketMaking,
            Self::Event { .. } => Family::EventDriven,
            Self::Factor { .. } => Family::FactorInvesting,
            Self::Carry { .. } => Family::CarryTrade,
            Self::Volatility { .. } => Family::VolatilityArbitrage,
            Self::Options { .. } => Family::OptionStrategies,
            Self::CrossSection { .. } => Family::CrossSectionalAlpha,
            Self::RiskParity { .. } => Family::RiskParity,
            Self::Regime { .. } => Family::RegimeSwitching,
            Self::MachineLearning { .. } => Family::MachineLearningAlpha,
            Self::Twap { .. } | Self::Vwap { .. } => Family::ExecutionAlgorithms,
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    Price(price::Assessment),
    StatisticalArbitrage(models::StatArbAssessment),
    Pairs(panel::PairAssessment),
    Quotes(planning::QuotePlan),
    Event(models::EventAssessment),
    Panel(panel::Allocation),
    Carry(derivatives::CarryAssessment),
    Volatility(derivatives::VolatilityAssessment),
    Options(derivatives::OptionPayoff),
    Allocation(allocation::Allocation),
    Regime(allocation::Regime),
    Prediction(models::Prediction),
    Schedule(planning::Schedule),
}
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    UnsupportedDomain(Family),
    Price(price::Error),
    Panel(panel::Error),
    Model(models::ModelError),
    Derivative(derivatives::DerivativeError),
    Allocation(allocation::AllocationError),
    Planning(planning::PlanningError),
}
/// Refuse an unsupported current-market family *before* evaluating or mutating
/// its model. SuppliedOffline is explicitly a caller-input research sandbox,
/// never an assertion that options/equities/order-book trading is implemented.
pub fn evaluate(universe: InputUniverse, request: Request<'_, '_>) -> Result<Output, Error> {
    let family = request.family();
    if !family.supports(universe) {
        return Err(Error::UnsupportedDomain(family));
    }
    match request {
        Request::Price { rule, window } => price::evaluate(rule, window)
            .map(Output::Price)
            .map_err(Error::Price),
        Request::StatisticalArbitrage {
            model,
            snapshot,
            decision,
            min_gap,
        } => models::statistical_arbitrage(model, snapshot, decision, min_gap)
            .map(Output::StatisticalArbitrage)
            .map_err(Error::Model),
        Request::Pairs { rule, window } => panel::pairs(rule, window)
            .map(Output::Pairs)
            .map_err(Error::Panel),
        Request::MarketMaking {
            state,
            decision,
            config,
        } => planning::quotes(state, decision, config)
            .map(Output::Quotes)
            .map_err(Error::Planning),
        Request::Event {
            release,
            decision,
            threshold,
            max_age_micros,
        } => models::event_driven(release, decision, threshold, max_age_micros)
            .map(Output::Event)
            .map_err(Error::Model),
        Request::Factor {
            panel,
            decision,
            weights,
            per_side,
        } => panel::factor(panel, decision, weights, per_side)
            .map(Output::Panel)
            .map_err(Error::Panel),
        Request::Carry { terms, decision } => derivatives::carry(terms, decision)
            .map(Output::Carry)
            .map_err(Error::Derivative),
        Request::Volatility { terms, decision } => {
            derivatives::volatility_arbitrage(terms, decision)
                .map(Output::Volatility)
                .map_err(Error::Derivative)
        }
        Request::Options {
            contract,
            expiry,
            decision,
        } => derivatives::option_payoff(contract, expiry, decision)
            .map(Output::Options)
            .map_err(Error::Derivative),
        Request::CrossSection {
            panel,
            decision,
            per_side,
        } => panel::rank(panel, decision, per_side)
            .map(Output::Panel)
            .map_err(Error::Panel),
        Request::RiskParity {
            estimates,
            decision,
        } => allocation::inverse_volatility(estimates, decision)
            .map(Output::Allocation)
            .map_err(Error::Allocation),
        Request::Regime {
            model,
            volatility,
            decision,
        } => model
            .observe(volatility, decision)
            .map(Output::Regime)
            .map_err(Error::Allocation),
        Request::MachineLearning {
            model,
            predictor,
            decision,
            threshold,
        } => model
            .predict(predictor, decision, threshold)
            .map(Output::Prediction)
            .map_err(Error::Model),
        Request::Twap {
            lots,
            start_micros,
            end_micros,
            count,
        } => planning::twap(lots, start_micros, end_micros, count)
            .map(Output::Schedule)
            .map_err(Error::Planning),
        Request::Vwap {
            lots,
            start_micros,
            end_micros,
            profile,
            decision,
        } => planning::vwap(lots, start_micros, end_micros, profile, decision)
            .map(Output::Schedule)
            .map_err(Error::Planning),
    }
}
