//! Eighteen source families expressed as distinct offline research capabilities.
//!
//! This is not eighteen independent alpha claims. Momentum/trend and
//! pairs/statistical-arbitrage overlap; allocation and execution are not alpha.
//! Existing S/H/C study IDs and their frozen protocols are deliberately unchanged.
pub mod derivatives;
pub mod models;
pub mod panel;
pub mod price;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Momentum,
    MeanReversion,
    StatisticalArbitrage,
    PairsTrading,
    TrendFollowing,
    MarketMaking,
    EventDriven,
    FactorInvesting,
    CarryTrade,
    VolatilityArbitrage,
    OptionStrategies,
    CrossSectionalAlpha,
    ShortTermReversal,
    BreakoutTrading,
    RiskParity,
    RegimeSwitching,
    MachineLearningAlpha,
    ExecutionAlgorithms,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Signal,
    ModelSignal,
    Allocation,
    Execution,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputUniverse {
    /// Current single-instrument bar research domain. Multi-asset, corporate
    /// events, options, and order-book execution features are explicitly refused.
    BtcUsdM,
    /// Caller-supplied offline typed inputs, including synthetic fixtures.
    /// This label is NOT evidence of historical provenance or market support.
    SuppliedOffline,
}
impl Family {
    pub const ALL: [Self; 18] = [
        Self::Momentum,
        Self::MeanReversion,
        Self::StatisticalArbitrage,
        Self::PairsTrading,
        Self::TrendFollowing,
        Self::MarketMaking,
        Self::EventDriven,
        Self::FactorInvesting,
        Self::CarryTrade,
        Self::VolatilityArbitrage,
        Self::OptionStrategies,
        Self::CrossSectionalAlpha,
        Self::ShortTermReversal,
        Self::BreakoutTrading,
        Self::RiskParity,
        Self::RegimeSwitching,
        Self::MachineLearningAlpha,
        Self::ExecutionAlgorithms,
    ];
    pub const fn source_number(self) -> u8 {
        match self {
            Self::Momentum => 1,
            Self::MeanReversion => 2,
            Self::StatisticalArbitrage => 3,
            Self::PairsTrading => 4,
            Self::TrendFollowing => 5,
            Self::MarketMaking => 6,
            Self::EventDriven => 7,
            Self::FactorInvesting => 8,
            Self::CarryTrade => 9,
            Self::VolatilityArbitrage => 10,
            Self::OptionStrategies => 11,
            Self::CrossSectionalAlpha => 12,
            Self::ShortTermReversal => 13,
            Self::BreakoutTrading => 14,
            Self::RiskParity => 15,
            Self::RegimeSwitching => 16,
            Self::MachineLearningAlpha => 17,
            Self::ExecutionAlgorithms => 18,
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::Momentum => "Momentum",
            Self::MeanReversion => "Mean Reversion",
            Self::StatisticalArbitrage => "Statistical Arbitrage",
            Self::PairsTrading => "Pairs Trading",
            Self::TrendFollowing => "Trend Following",
            Self::MarketMaking => "Market Making",
            Self::EventDriven => "Event-Driven",
            Self::FactorInvesting => "Factor Investing",
            Self::CarryTrade => "Carry Trade",
            Self::VolatilityArbitrage => "Volatility Arbitrage",
            Self::OptionStrategies => "Option Strategies",
            Self::CrossSectionalAlpha => "Cross-Sectional Alpha",
            Self::ShortTermReversal => "Short-Term Reversal",
            Self::BreakoutTrading => "Breakout Trading",
            Self::RiskParity => "Risk Parity",
            Self::RegimeSwitching => "Regime Switching",
            Self::MachineLearningAlpha => "Machine Learning Alpha",
            Self::ExecutionAlgorithms => "Execution Algorithms",
        }
    }
    pub const fn role(self) -> Role {
        match self {
            Self::MarketMaking | Self::ExecutionAlgorithms => Role::Execution,
            Self::RiskParity | Self::RegimeSwitching => Role::Allocation,
            Self::StatisticalArbitrage | Self::VolatilityArbitrage | Self::MachineLearningAlpha => {
                Role::ModelSignal
            }
            Self::Momentum
            | Self::MeanReversion
            | Self::PairsTrading
            | Self::TrendFollowing
            | Self::EventDriven
            | Self::FactorInvesting
            | Self::CarryTrade
            | Self::OptionStrategies
            | Self::CrossSectionalAlpha
            | Self::ShortTermReversal
            | Self::BreakoutTrading => Role::Signal,
        }
    }
    /// Domain compatibility only; never authorizes a study, source or execution.
    pub const fn supports(self, universe: InputUniverse) -> bool {
        match universe {
            InputUniverse::SuppliedOffline => true,
            InputUniverse::BtcUsdM => matches!(
                self,
                Self::Momentum
                    | Self::MeanReversion
                    | Self::TrendFollowing
                    | Self::ShortTermReversal
                    | Self::BreakoutTrading
                    | Self::RegimeSwitching
                    | Self::MachineLearningAlpha
                    | Self::ExecutionAlgorithms
            ),
        }
    }
}
