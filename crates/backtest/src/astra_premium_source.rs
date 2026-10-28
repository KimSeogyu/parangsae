//! Checked premium projection for the declared archived closed-minute model.
//!
//! This adapter does not read files or admit a snapshot. Source bounds, object
//! identity, scale, projection types/nulls and full input quality remain the admission
//! layer's responsibility. It never uses legacy H5 count/sample semantics.

use std::fmt;

use parangsae_core::{
    AvailableTs, Causal, DecisionTime, EventTs, Observation, Ratio, ReceivedTs, Symbol, Timestamps,
    UtcMicros,
};
use parangsae_signals::premium_input::{PremiumInputError, PremiumMinuteInput};

const MINUTE_US: i64 = 60_000_000;
const MILLIS_US: i64 = 1_000;

/// The ten explicitly projected stored fields. Opaque count/volume is absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PremiumSourceProjection {
    pub symbol: Symbol,
    pub interval_us: i64,
    pub open_time: UtcMicros,
    pub close_time: UtcMicros,
    pub close_ticks: i64,
    pub event_ts: EventTs,
    pub stored_available_ts: AvailableTs,
    pub recorded_received_ts: ReceivedTs,
    pub schema_version: u32,
    pub tick_domain: String,
}

/// Astra protocol v1 section 2: a complete minute is model-available at its end.
/// This names an archive convention, not measured publisher or venue latency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PremiumModelClock {
    AstraProtocolV1ClosedMinuteEnd,
}

/// A separate model clock. It is not an original `ReceivedTs` or fill receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeledPremiumDelivery {
    clock: PremiumModelClock,
    known_at: UtcMicros,
}

impl ModeledPremiumDelivery {
    pub const fn clock(self) -> PremiumModelClock {
        self.clock
    }

    pub const fn known_at(self) -> UtcMicros {
        self.known_at
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PremiumClockField {
    Event,
    Close,
    StoredAvailable,
}

/// Projection failure; malformed facts are never clamped, carried or rewritten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PremiumSourceError {
    SymbolMismatch,
    IntervalMismatch,
    UnknownSchema { got: u32 },
    TickDomainMismatch,
    Minute(PremiumInputError),
    ClockMismatch { field: PremiumClockField },
    Overflow,
}

impl fmt::Display for PremiumSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SymbolMismatch => write!(f, "premium source symbol does not match the request"),
            Self::IntervalMismatch => write!(f, "premium source interval is not one minute"),
            Self::UnknownSchema { got } => write!(f, "premium source schema {got} is unsupported"),
            Self::TickDomainMismatch => write!(f, "premium source domain is not ratio"),
            Self::Minute(error) => error.fmt(f),
            Self::ClockMismatch { field } => write!(f, "premium source {field:?} clock mismatch"),
            Self::Overflow => write!(f, "premium source minute-end overflow"),
        }
    }
}

impl std::error::Error for PremiumSourceError {}

/// One validated projection, retaining its original observation clocks.
///
/// A causal view here means the explicitly named archive model admitted the
/// value. It does not establish historical first-public or reception evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AstraPremiumObservation {
    observation: Observation<PremiumMinuteInput>,
    delivery: ModeledPremiumDelivery,
    schema_version: u32,
}

impl AstraPremiumObservation {
    pub fn from_projection(
        expected_symbol: &Symbol,
        row: PremiumSourceProjection,
        clock: PremiumModelClock,
    ) -> Result<Self, PremiumSourceError> {
        if &row.symbol != expected_symbol {
            return Err(PremiumSourceError::SymbolMismatch);
        }
        if row.interval_us != MINUTE_US {
            return Err(PremiumSourceError::IntervalMismatch);
        }
        if !matches!(row.schema_version, 1 | 2) {
            return Err(PremiumSourceError::UnknownSchema {
                got: row.schema_version,
            });
        }
        if row.tick_domain != "ratio" {
            return Err(PremiumSourceError::TickDomainMismatch);
        }
        let value = PremiumMinuteInput::try_new(row.open_time, Ratio::from_ticks(row.close_ticks))
            .map_err(PremiumSourceError::Minute)?;
        let end = row
            .open_time
            .checked_add_micros(MINUTE_US)
            .ok_or(PremiumSourceError::Overflow)?;
        let close = end
            .checked_add_micros(-MILLIS_US)
            .ok_or(PremiumSourceError::Overflow)?;
        for (actual, expected, field) in [
            (
                row.event_ts.micros(),
                row.open_time,
                PremiumClockField::Event,
            ),
            (row.close_time, close, PremiumClockField::Close),
            (
                row.stored_available_ts.micros(),
                end,
                PremiumClockField::StoredAvailable,
            ),
        ] {
            if actual != expected {
                return Err(PremiumSourceError::ClockMismatch { field });
            }
        }
        let known_at = match clock {
            PremiumModelClock::AstraProtocolV1ClosedMinuteEnd => end,
        };
        Ok(Self {
            observation: Observation::new(
                Timestamps {
                    event_ts: row.event_ts,
                    available_ts: row.stored_available_ts,
                    received_ts: row.recorded_received_ts,
                },
                value,
            ),
            delivery: ModeledPremiumDelivery { clock, known_at },
            schema_version: row.schema_version,
        })
    }

    pub const fn original_timestamps(&self) -> Timestamps {
        self.observation.timestamps()
    }

    pub const fn model_delivery(&self) -> ModeledPremiumDelivery {
        self.delivery
    }

    pub const fn source_schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn causal_at(&self, decision: DecisionTime) -> Option<Causal<'_, PremiumMinuteInput>> {
        if self.delivery.known_at > decision.micros() {
            return None;
        }
        self.observation.causal_at(decision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: i64 = 1_608_249_600_000_000;
    const RECORDED: i64 = 1_792_776_118_572_302;

    fn symbol() -> Symbol {
        Symbol::new("BTCUSDT").expect("symbol")
    }

    fn row(start: i64, ticks: i64) -> PremiumSourceProjection {
        PremiumSourceProjection {
            symbol: symbol(),
            interval_us: MINUTE_US,
            open_time: UtcMicros::from_micros(start),
            close_time: UtcMicros::from_micros(start + MINUTE_US - MILLIS_US),
            close_ticks: ticks,
            event_ts: EventTs::from_micros(UtcMicros::from_micros(start)),
            stored_available_ts: AvailableTs::from_micros(UtcMicros::from_micros(
                start + MINUTE_US,
            )),
            recorded_received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(RECORDED)),
            schema_version: 1,
            tick_domain: "ratio".to_owned(),
        }
    }

    fn admitted(row: PremiumSourceProjection) -> AstraPremiumObservation {
        AstraPremiumObservation::from_projection(
            &symbol(),
            row,
            PremiumModelClock::AstraProtocolV1ClosedMinuteEnd,
        )
        .expect("valid fake projection")
    }

    fn at(micros: i64) -> DecisionTime {
        DecisionTime::from_micros(UtcMicros::from_micros(micros))
    }

    #[test]
    fn source_clocks_and_model_delivery_are_distinct_and_unchanged() {
        let source = row(FIRST, 0);
        let original = Timestamps {
            event_ts: source.event_ts,
            available_ts: source.stored_available_ts,
            received_ts: source.recorded_received_ts,
        };
        let observation = admitted(source);
        assert_eq!(observation.original_timestamps(), original);
        assert_eq!(
            observation.model_delivery().clock(),
            PremiumModelClock::AstraProtocolV1ClosedMinuteEnd
        );
        assert_eq!(
            observation.model_delivery().known_at(),
            UtcMicros::from_micros(FIRST + MINUTE_US)
        );
        assert!(observation.causal_at(at(FIRST + MINUTE_US - 1)).is_none());
        assert_eq!(
            observation
                .causal_at(at(FIRST + MINUTE_US))
                .expect("model-known")
                .value()
                .close()
                .ticks(),
            0
        );
        assert_eq!(observation.original_timestamps(), original);
    }

    #[test]
    fn negative_and_zero_ratios_and_both_source_layouts_remain_values() {
        for schema_version in [1, 2] {
            for ticks in [i64::MIN, -1, 0, 1, i64::MAX] {
                let observation = admitted(PremiumSourceProjection {
                    schema_version,
                    ..row(FIRST, ticks)
                });
                assert_eq!(observation.source_schema_version(), schema_version);
                assert_eq!(
                    observation
                        .causal_at(at(FIRST + MINUTE_US))
                        .expect("model-known")
                        .value()
                        .close()
                        .ticks(),
                    ticks
                );
            }
        }
    }

    #[test]
    fn a_future_payload_mutation_cannot_change_an_earlier_view() {
        let current = admitted(row(FIRST, -7));
        for changed in [i64::MIN, -1, 0, 1, i64::MAX] {
            let future = admitted(row(FIRST + MINUTE_US, changed));
            let early: Vec<_> = [&current, &future]
                .into_iter()
                .filter_map(|value| value.causal_at(at(FIRST + MINUTE_US)))
                .map(|view| view.value().close().ticks())
                .collect();
            assert_eq!(early, [-7]);
            assert_eq!(
                future
                    .causal_at(at(FIRST + 2 * MINUTE_US))
                    .expect("future is now model-known")
                    .value()
                    .close()
                    .ticks(),
                changed
            );
        }
    }

    #[test]
    fn projection_mismatches_are_typed_refusals() {
        let cases = [
            (
                PremiumSourceProjection {
                    symbol: Symbol::new("ETHUSDT").expect("symbol"),
                    ..row(FIRST, 0)
                },
                PremiumSourceError::SymbolMismatch,
            ),
            (
                PremiumSourceProjection {
                    interval_us: 1,
                    ..row(FIRST, 0)
                },
                PremiumSourceError::IntervalMismatch,
            ),
            (
                PremiumSourceProjection {
                    schema_version: 3,
                    ..row(FIRST, 0)
                },
                PremiumSourceError::UnknownSchema { got: 3 },
            ),
            (
                PremiumSourceProjection {
                    tick_domain: "price".to_owned(),
                    ..row(FIRST, 0)
                },
                PremiumSourceError::TickDomainMismatch,
            ),
        ];
        for (source, expected) in cases {
            assert_eq!(
                AstraPremiumObservation::from_projection(
                    &symbol(),
                    source,
                    PremiumModelClock::AstraProtocolV1ClosedMinuteEnd,
                ),
                Err(expected)
            );
        }
    }

    #[test]
    fn clock_mismatches_are_not_repaired() {
        let cases = [
            (
                PremiumSourceProjection {
                    event_ts: EventTs::from_micros(UtcMicros::from_micros(FIRST + 1)),
                    ..row(FIRST, 0)
                },
                PremiumClockField::Event,
            ),
            (
                PremiumSourceProjection {
                    close_time: UtcMicros::from_micros(FIRST + MINUTE_US),
                    ..row(FIRST, 0)
                },
                PremiumClockField::Close,
            ),
            (
                PremiumSourceProjection {
                    stored_available_ts: AvailableTs::from_micros(UtcMicros::from_micros(
                        FIRST + MINUTE_US - 1,
                    )),
                    ..row(FIRST, 0)
                },
                PremiumClockField::StoredAvailable,
            ),
        ];
        for (source, field) in cases {
            assert_eq!(
                AstraPremiumObservation::from_projection(
                    &symbol(),
                    source,
                    PremiumModelClock::AstraProtocolV1ClosedMinuteEnd,
                ),
                Err(PremiumSourceError::ClockMismatch { field })
            );
        }
    }

    #[test]
    fn off_grid_and_overflow_do_not_create_an_observation() {
        let off_grid = UtcMicros::from_micros(FIRST + 1);
        assert_eq!(
            AstraPremiumObservation::from_projection(
                &symbol(),
                PremiumSourceProjection {
                    open_time: off_grid,
                    ..row(FIRST, 0)
                },
                PremiumModelClock::AstraProtocolV1ClosedMinuteEnd,
            ),
            Err(PremiumSourceError::Minute(
                PremiumInputError::OffMinuteGrid {
                    minute_start: off_grid,
                }
            ))
        );
        let last_grid = i64::MAX - i64::MAX % MINUTE_US;
        assert_eq!(
            AstraPremiumObservation::from_projection(
                &symbol(),
                PremiumSourceProjection {
                    open_time: UtcMicros::from_micros(last_grid),
                    ..row(FIRST, 0)
                },
                PremiumModelClock::AstraProtocolV1ClosedMinuteEnd,
            ),
            Err(PremiumSourceError::Overflow)
        );
    }
}
