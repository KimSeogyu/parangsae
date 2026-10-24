//! Causal C1 funding payloads. The backtest actor owns control and consumed keys.

use std::fmt;

use parangsae_core::{AvailableTs, Causal, DecisionTime, Ratio, UtcMicros, SCALE};
use parangsae_signals::FundingRow;

const HOUR_US: i64 = 3_600_000_000;
const SLOT_US: i64 = 8 * HOUR_US;
const RATE_COUNT: usize = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum C1MaxHold {
    Hours168,
    Hours336,
}

impl C1MaxHold {
    pub const fn hours(self) -> i64 {
        match self {
            Self::Hours168 => 168,
            Self::Hours336 => 336,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct C1SettledRate {
    row: FundingRow,
    available: AvailableTs,
}

impl C1SettledRate {
    pub const fn key(self) -> UtcMicros {
        self.row.funding_time()
    }

    pub const fn rate(self) -> Ratio {
        self.row.rate()
    }

    pub const fn interval_hours(self) -> u8 {
        self.row.interval_hours()
    }

    pub const fn available_ts(self) -> AvailableTs {
        self.available
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum C1FundingEvaluation {
    ExpectedUnavailable {
        expected_key: UtcMicros,
    },
    Latest {
        rate: C1SettledRate,
        history: C1History,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum C1History {
    Complete(C1FundingFeature),
    /// Bit k denotes missing `expected_latest - k * 8h`, for k in 0..9.
    Incomplete {
        missing_mask: u16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct C1FundingFeature {
    latest: C1SettledRate,
    oldest_key: UtcMicros,
    sum9_ticks: i128,
    available_through: AvailableTs,
}

impl C1FundingFeature {
    pub const fn latest(self) -> C1SettledRate {
        self.latest
    }

    pub const fn oldest_key(self) -> UtcMicros {
        self.oldest_key
    }

    pub const fn sum9_ticks(self) -> i128 {
        self.sum9_ticks
    }

    pub const fn available_through(self) -> AvailableTs {
        self.available_through
    }

    /// A persistence scenario over H, not expected or guaranteed future funding.
    pub fn project(&self, horizon: C1MaxHold) -> Result<C1RateProjection, C1FeatureError> {
        let numerator = self
            .sum9_ticks
            .checked_mul(i128::from(horizon.hours()))
            .ok_or(C1FeatureError::Overflow)?;
        let denominator = 72_i128.checked_mul(SCALE).ok_or(C1FeatureError::Overflow)?;
        Ok(C1RateProjection {
            numerator,
            denominator,
        })
    }
}

/// A dimensionless signed rate. Divide numerator by denominator exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct C1RateProjection {
    numerator: i128,
    denominator: i128,
}

impl C1RateProjection {
    pub const fn numerator(self) -> i128 {
        self.numerator
    }

    pub const fn denominator(self) -> i128 {
        self.denominator
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum C1KeyClass {
    Fresh,
    Duplicate,
    Superseded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum C1FalseReason {
    LatestNonPositive,
    ForecastNotAboveCost,
    NegativeBasis,
}

/// A read-only actor assessment, never cash, a fill, or dispatch authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum C1EntryAssessment {
    Pass,
    KnownFalse(C1FalseReason),
    Unavailable,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum C1FeatureError {
    AfterDecision { available: AvailableTs },
    FutureSettlement { key: UtcMicros },
    BadExpectedKey { key: UtcMicros },
    OffGrid { key: UtcMicros },
    WrongCadence { key: UtcMicros, interval_hours: u8 },
    UnexpectedKey { key: UtcMicros },
    ConflictingKey { key: UtcMicros },
    Overflow,
}

impl fmt::Display for C1FeatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AfterDecision { .. } => write!(f, "C1 funding view is after the decision"),
            Self::FutureSettlement { .. } => write!(f, "C1 funding settlement is in the future"),
            Self::BadExpectedKey { .. } => {
                write!(f, "C1 expected funding key is not on the 8h grid")
            }
            Self::OffGrid { .. } => write!(f, "C1 funding key is not on the 8h grid"),
            Self::WrongCadence { .. } => write!(f, "C1 funding interval is not 8h"),
            Self::UnexpectedKey { .. } => {
                write!(f, "C1 funding key is outside the required 9 slots")
            }
            Self::ConflictingKey { .. } => {
                write!(f, "C1 funding key has conflicting canonical facts")
            }
            Self::Overflow => write!(f, "C1 funding arithmetic overflowed"),
        }
    }
}

impl std::error::Error for C1FeatureError {}

/// Data supplies its expected key and already normalized views. No lag is added.
pub fn evaluate_c1_funding(
    decision: DecisionTime,
    expected_latest: UtcMicros,
    rows: &[Causal<'_, FundingRow>],
) -> Result<C1FundingEvaluation, C1FeatureError> {
    if expected_latest.micros().rem_euclid(SLOT_US) != 0 {
        return Err(C1FeatureError::BadExpectedKey {
            key: expected_latest,
        });
    }
    if expected_latest > decision.micros() {
        return Err(C1FeatureError::FutureSettlement {
            key: expected_latest,
        });
    }
    let oldest = expected_latest
        .micros()
        .checked_sub(8 * SLOT_US)
        .ok_or(C1FeatureError::Overflow)?;
    let mut slots: [Option<C1SettledRate>; RATE_COUNT] = [None; RATE_COUNT];
    for view in rows {
        if view.available_ts().micros() > decision.micros() {
            return Err(C1FeatureError::AfterDecision {
                available: view.available_ts(),
            });
        }
        let row = *view.value();
        let key = row.funding_time();
        if key > decision.micros() {
            return Err(C1FeatureError::FutureSettlement { key });
        }
        if key.micros().rem_euclid(SLOT_US) != 0 {
            return Err(C1FeatureError::OffGrid { key });
        }
        if row.interval_hours() != 8 {
            return Err(C1FeatureError::WrongCadence {
                key,
                interval_hours: row.interval_hours(),
            });
        }
        if key > expected_latest || key.micros() < oldest {
            return Err(C1FeatureError::UnexpectedKey { key });
        }
        let distance = expected_latest
            .micros()
            .checked_sub(key.micros())
            .ok_or(C1FeatureError::Overflow)?;
        let index = usize::try_from(distance / SLOT_US).map_err(|_| C1FeatureError::Overflow)?;
        let known = C1SettledRate {
            row,
            available: view.available_ts(),
        };
        match slots[index] {
            Some(old) if old != known => return Err(C1FeatureError::ConflictingKey { key }),
            Some(_) => {}
            None => slots[index] = Some(known),
        }
    }
    let Some(latest) = slots[0] else {
        return Ok(C1FundingEvaluation::ExpectedUnavailable {
            expected_key: expected_latest,
        });
    };
    let mut missing_mask = 0_u16;
    let mut sum9_ticks = 0_i128;
    let mut available_through = latest.available;
    for (index, slot) in slots.iter().enumerate() {
        if let Some(known) = slot {
            sum9_ticks = sum9_ticks
                .checked_add(i128::from(known.rate().ticks()))
                .ok_or(C1FeatureError::Overflow)?;
            available_through = available_through.max(known.available);
        } else {
            missing_mask |= 1_u16 << index;
        }
    }
    let history = if missing_mask != 0 {
        C1History::Incomplete { missing_mask }
    } else {
        C1History::Complete(C1FundingFeature {
            latest,
            oldest_key: UtcMicros::from_micros(oldest),
            sum9_ticks,
            available_through,
        })
    };
    Ok(C1FundingEvaluation::Latest {
        rate: latest,
        history,
    })
}

/// The actor supplies its committed stamp. This function never consumes a key.
pub fn classify_c1_key(
    candidate: &C1SettledRate,
    committed: Option<&C1SettledRate>,
) -> Result<C1KeyClass, C1FeatureError> {
    let Some(old) = committed else {
        return Ok(C1KeyClass::Fresh);
    };
    match candidate.key().cmp(&old.key()) {
        std::cmp::Ordering::Greater => Ok(C1KeyClass::Fresh),
        std::cmp::Ordering::Less => Ok(C1KeyClass::Superseded),
        std::cmp::Ordering::Equal if candidate == old => Ok(C1KeyClass::Duplicate),
        std::cmp::Ordering::Equal => Err(C1FeatureError::ConflictingKey {
            key: candidate.key(),
        }),
    }
}
