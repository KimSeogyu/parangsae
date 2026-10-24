//! C1 source clocks and read-only observations. No order or strategy authority.

use std::collections::BTreeMap;

use parangsae_core::{
    AvailableTs, Bar, Causal, DecisionTime, Observation, Price, Ratio, ReceivedTs, TimeUnit,
    Timestamps, UtcMicros,
};
use parangsae_signals::FundingRow;

use crate::date::UtcDate;

mod file_source;
mod profile;
pub(crate) use profile::{validate_source_profile, validate_source_settings};

const MINUTE: i64 = profile::BAR_INTERVAL_US;
const HOUR: i64 = 3_600_000_000;
const DAY: i64 = 24 * HOUR;
const SLOT: i64 = profile::FUNDING_CADENCE_US;
const SAMPLE: i64 = 1_608_249_600_000_000;
const END: i64 = 1_759_276_800_000_000;
const SPOT_MICROS: i64 = 1_735_689_600_000_000;
const NORMAL_AGE: i64 = 120_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum C1Instrument {
    Spot,
    Perp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PriceSource {
    Spot,
    UmTrade,
    UmMark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceHealth {
    Pending,
    Gap,
    Quarantined,
    TooOld,
    NonPositive,
    ZeroCount,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SourceError {
    Range,
    Overflow,
    Clock,
    Invariant(&'static str),
    Input {
        path: String,
        reason: String,
    },
    Unavailable {
        source: PriceSource,
        key: UtcMicros,
        health: SourceHealth,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceRef {
    pub(crate) object: String,
    pub(crate) sha256: String,
    pub(crate) row: u32,
    pub(crate) original: Timestamps,
    pub(crate) raw_unit: TimeUnit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActorPrice {
    pub(crate) observation: Observation<Price>,
    pub(crate) source: SourceRef,
    pub(crate) key: UtcMicros,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct C1Marks {
    pub(crate) spot: ActorPrice,
    pub(crate) perp_risk: ActorPrice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct C1PairRefs {
    pub(crate) spot: ActorPrice,
    pub(crate) um_trade: ActorPrice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct C1DailyMarks {
    pub(crate) economic_end_exclusive: UtcMicros,
    pub(crate) knowledge_at: DecisionTime,
    pub(crate) spot: ActorPrice,
    pub(crate) perp_risk: ActorPrice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExecutionState {
    Pending {
        until: UtcMicros,
    },
    Valid {
        reference_open: Price,
        count: i64,
        timestamps: Timestamps,
        source: SourceRef,
    },
    ZeroCount {
        source: SourceRef,
    },
    Gap {
        key: UtcMicros,
    },
    Quarantined {
        key: UtcMicros,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceBar {
    bar: Bar,
    source: SourceRef,
}

impl SourceBar {
    fn admit(kind: PriceSource, bar: Bar, source: SourceRef) -> Result<Self, SourceError> {
        let key = bar.open_time.micros();
        in_sample(key)?;
        let tick = if kind == PriceSource::Spot && key >= SPOT_MICROS {
            1
        } else {
            1_000
        };
        let unit = if tick == 1 {
            TimeUnit::Micros
        } else {
            TimeUnit::Millis
        };
        let close = key
            .checked_add(MINUTE)
            .and_then(|end| end.checked_sub(tick))
            .ok_or(SourceError::Overflow)?;
        if bar.close_time.micros() != close
            || bar.available.micros() != source_available(kind, key)?
            || source.original.available_ts.micros() != bar.available
            || source.original.event_ts.micros() != bar.open_time
            || source.raw_unit != unit
        {
            return Err(SourceError::Invariant("unsupported source close/clock"));
        }
        if bar.count < 0
            || bar.low.ticks() > bar.open.ticks().min(bar.close.ticks())
            || bar.high.ticks() < bar.open.ticks().max(bar.close.ticks())
        {
            return Err(SourceError::Invariant("source count/OHLC order"));
        }
        Ok(Self { bar, source })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceFunding {
    settlement: UtcMicros,
    rate: Ratio,
    interval_hours: u8,
    source: SourceRef,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AdmittedFunding {
    pub(crate) observation: Observation<FundingRow>,
    pub(crate) source: SourceRef,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct C1RateFrame {
    pub(crate) expected_latest: Option<UtcMicros>,
    pub(crate) rows: Vec<AdmittedFunding>,
    decision: DecisionTime,
}

impl C1RateFrame {
    pub(crate) fn views(&self) -> Vec<Causal<'_, FundingRow>> {
        self.rows
            .iter()
            .filter_map(|row| row.observation.causal_at(self.decision))
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct C1FundingEvidence {
    pub(crate) rate: Observation<FundingRow>,
    pub(crate) rate_source: SourceRef,
    pub(crate) mark_open_proxy: ActorPrice,
    pub(crate) modeled_delivery: UtcMicros,
}

/// The actor cannot construct the source facts or inspect future slot maps.
#[derive(Debug)]
pub(crate) struct C1SourceView {
    bars: BTreeMap<(PriceSource, i64), SourceBar>,
    statuses: BTreeMap<(PriceSource, i64), SourceHealth>,
    funding: BTreeMap<i64, SourceFunding>,
    files: Option<file_source::FileSource>,
}

pub(crate) fn ceil_to_ms(value: UtcMicros) -> Result<UtcMicros, SourceError> {
    let value = value
        .micros()
        .checked_add(999)
        .ok_or(SourceError::Overflow)?;
    value
        .div_euclid(1_000)
        .checked_mul(1_000)
        .map(UtcMicros::from_micros)
        .ok_or(SourceError::Overflow)
}

fn in_sample(key: i64) -> Result<(), SourceError> {
    if !(SAMPLE..END).contains(&key) || key.rem_euclid(MINUTE) != 0 {
        return Err(SourceError::Range);
    }
    Ok(())
}

fn source_available(source: PriceSource, key: i64) -> Result<i64, SourceError> {
    key.checked_add(MINUTE)
        .and_then(|end| {
            if source == PriceSource::Spot && key >= SPOT_MICROS {
                end.checked_add(999)
            } else {
                Some(end)
            }
        })
        .ok_or(SourceError::Overflow)
}

fn decision_us(decision: DecisionTime) -> Result<i64, SourceError> {
    let at = decision.micros().micros();
    if at.rem_euclid(1_000) != 0 || !(SAMPLE..=END + 1_000).contains(&at) {
        return Err(SourceError::Clock);
    }
    Ok(at)
}

fn latest_key(source: PriceSource, decision: DecisionTime) -> Result<i64, SourceError> {
    let at = decision_us(decision)?;
    let mut key = at
        .div_euclid(MINUTE)
        .checked_mul(MINUTE)
        .and_then(|at| at.checked_sub(MINUTE))
        .ok_or(SourceError::Overflow)?;
    if ceil_to_ms(UtcMicros::from_micros(source_available(source, key)?))?.micros() > at {
        key = key.checked_sub(MINUTE).ok_or(SourceError::Overflow)?;
    }
    in_sample(key)?;
    Ok(key)
}

fn leg_source(leg: C1Instrument) -> PriceSource {
    match leg {
        C1Instrument::Spot => PriceSource::Spot,
        C1Instrument::Perp => PriceSource::UmTrade,
    }
}

fn admitted_funding(
    row: &SourceFunding,
    decision: DecisionTime,
) -> Result<Option<AdmittedFunding>, SourceError> {
    decision_us(decision)?;
    let effective = row.source.original.available_ts.micros().micros().max(
        row.settlement
            .micros()
            .checked_add(profile::FUNDING_FLOOR_US)
            .ok_or(SourceError::Overflow)?,
    );
    if effective < row.source.original.event_ts.micros().micros() {
        return Err(SourceError::Clock);
    }
    let delivery = ceil_to_ms(UtcMicros::from_micros(effective))?;
    if delivery > decision.micros() || row.source.original.event_ts.micros() > decision.micros() {
        return Ok(None);
    }
    Ok(Some(AdmittedFunding {
        observation: Observation::new(
            Timestamps {
                event_ts: row.source.original.event_ts,
                available_ts: AvailableTs::from_micros(UtcMicros::from_micros(effective)),
                received_ts: ReceivedTs::from_micros(delivery),
            },
            FundingRow::new(row.settlement, row.rate, row.interval_hours),
        ),
        source: row.source.clone(),
    }))
}

#[derive(Clone, Copy)]
enum PriceUse {
    DecisionClose,
    ExecutionOpen,
    FundingOpen,
    DailyClose,
}

impl C1SourceView {
    pub(crate) fn load_exact(manifest: &std::path::Path) -> Result<Self, SourceError> {
        Ok(Self {
            bars: BTreeMap::new(),
            statuses: BTreeMap::new(),
            funding: BTreeMap::new(),
            files: Some(file_source::FileSource::load(manifest)?),
        })
    }

    #[cfg(test)]
    pub(crate) fn synthetic(
        bars: Vec<(PriceSource, Bar, SourceRef)>,
        statuses: Vec<(PriceSource, UtcMicros, SourceHealth)>,
        funding: Vec<(UtcMicros, Ratio, u8, SourceRef)>,
    ) -> Result<Self, SourceError> {
        if bars.len() > 18 * 1440 * 3 || statuses.len() > 18 * 1440 * 3 || funding.len() > 64 {
            return Err(SourceError::Invariant("synthetic source capacity"));
        }
        let mut view = Self {
            bars: BTreeMap::new(),
            statuses: BTreeMap::new(),
            funding: BTreeMap::new(),
            files: None,
        };
        for (kind, bar, source) in bars {
            let key = bar.open_time.micros();
            let row = SourceBar::admit(kind, bar, source)?;
            if view.bars.insert((kind, key), row).is_some() {
                return Err(SourceError::Invariant("duplicate synthetic source key"));
            }
        }
        for (kind, key, health) in statuses {
            in_sample(key.micros())?;
            if !matches!(health, SourceHealth::Gap | SourceHealth::Quarantined)
                || view.bars.contains_key(&(kind, key.micros()))
                || view.statuses.insert((kind, key.micros()), health).is_some()
            {
                return Err(SourceError::Invariant("synthetic source status"));
            }
        }
        for (settlement, rate, interval_hours, source) in funding {
            in_sample(settlement.micros())?;
            if settlement.micros().rem_euclid(SLOT) != 0 || interval_hours != 8 {
                return Err(SourceError::Invariant("synthetic funding cadence"));
            }
            let row = SourceFunding {
                settlement,
                rate,
                interval_hours,
                source,
            };
            if view.funding.insert(settlement.micros(), row).is_some() {
                return Err(SourceError::Invariant("duplicate synthetic funding key"));
            }
        }
        Ok(view)
    }

    fn row(
        &self,
        source: PriceSource,
        key: i64,
        decision: DecisionTime,
    ) -> Result<SourceBar, SourceError> {
        decision_us(decision)?;
        in_sample(key)?;
        let until = ceil_to_ms(UtcMicros::from_micros(source_available(source, key)?))?;
        if until > decision.micros() {
            return Err(SourceError::Unavailable {
                source,
                key: UtcMicros::from_micros(key),
                health: SourceHealth::Pending,
            });
        }
        if let Some(files) = &self.files {
            return files.bar(source, key);
        }
        if let Some(health) = self.statuses.get(&(source, key)) {
            return Err(SourceError::Unavailable {
                source,
                key: UtcMicros::from_micros(key),
                health: *health,
            });
        }
        let row = self
            .bars
            .get(&(source, key))
            .cloned()
            .ok_or(SourceError::Unavailable {
                source,
                key: UtcMicros::from_micros(key),
                health: SourceHealth::Gap,
            })?;
        if row.source.original.event_ts.micros() > decision.micros()
            || row.source.original.available_ts.micros() > decision.micros()
        {
            return Err(SourceError::Clock);
        }
        Ok(row)
    }

    fn price_at(
        &self,
        source: PriceSource,
        key: i64,
        decision: DecisionTime,
        purpose: PriceUse,
    ) -> Result<ActorPrice, SourceError> {
        let row = self.row(source, key, decision)?;
        let age = decision
            .micros()
            .micros()
            .checked_sub(row.source.original.event_ts.micros().micros())
            .ok_or(SourceError::Overflow)?;
        if matches!(purpose, PriceUse::DecisionClose) && age > NORMAL_AGE {
            return Err(SourceError::Unavailable {
                source,
                key: UtcMicros::from_micros(key),
                health: SourceHealth::TooOld,
            });
        }
        let price = match purpose {
            PriceUse::ExecutionOpen | PriceUse::FundingOpen => row.bar.open,
            PriceUse::DecisionClose | PriceUse::DailyClose => row.bar.close,
        };
        if price.ticks() <= 0 {
            return Err(SourceError::Unavailable {
                source,
                key: UtcMicros::from_micros(key),
                health: SourceHealth::NonPositive,
            });
        }
        let clocks = row.source.original;
        Ok(ActorPrice {
            observation: Observation::new(
                Timestamps {
                    event_ts: clocks.event_ts,
                    available_ts: clocks.available_ts,
                    received_ts: ReceivedTs::from_micros(ceil_to_ms(clocks.available_ts.micros())?),
                },
                price,
            ),
            source: row.source.clone(),
            key: UtcMicros::from_micros(key),
        })
    }

    pub(crate) fn marks_at(&self, decision: DecisionTime) -> Result<C1Marks, SourceError> {
        Ok(C1Marks {
            spot: self.price_at(
                PriceSource::Spot,
                latest_key(PriceSource::Spot, decision)?,
                decision,
                PriceUse::DecisionClose,
            )?,
            perp_risk: self.price_at(
                PriceSource::UmMark,
                latest_key(PriceSource::UmMark, decision)?,
                decision,
                PriceUse::DecisionClose,
            )?,
        })
    }

    pub(crate) fn decision_ref_at(
        &self,
        leg: C1Instrument,
        decision: DecisionTime,
    ) -> Result<ActorPrice, SourceError> {
        let source = leg_source(leg);
        self.price_at(
            source,
            latest_key(source, decision)?,
            decision,
            PriceUse::DecisionClose,
        )
    }

    pub(crate) fn pair_refs_at(
        &self,
        open: UtcMicros,
        decision: DecisionTime,
    ) -> Result<C1PairRefs, SourceError> {
        let key = open.micros();
        if latest_key(PriceSource::UmTrade, decision)? != key {
            return Err(SourceError::Clock);
        }
        for source in [PriceSource::Spot, PriceSource::UmTrade] {
            if self.row(source, key, decision)?.bar.count <= 0 {
                return Err(SourceError::Unavailable {
                    source,
                    key: open,
                    health: SourceHealth::ZeroCount,
                });
            }
        }
        Ok(C1PairRefs {
            spot: self.price_at(PriceSource::Spot, key, decision, PriceUse::DecisionClose)?,
            um_trade: self.price_at(
                PriceSource::UmTrade,
                key,
                decision,
                PriceUse::DecisionClose,
            )?,
        })
    }

    pub(crate) fn execution_at(
        &self,
        leg: C1Instrument,
        open: UtcMicros,
        decision: DecisionTime,
    ) -> Result<ExecutionState, SourceError> {
        decision_us(decision)?;
        let source = leg_source(leg);
        in_sample(open.micros())?;
        let until = ceil_to_ms(UtcMicros::from_micros(source_available(
            source,
            open.micros(),
        )?))?;
        if decision.micros() < until {
            return Ok(ExecutionState::Pending { until });
        }
        let row = match self.row(source, open.micros(), decision) {
            Ok(row) => row,
            Err(SourceError::Unavailable {
                health: SourceHealth::Gap,
                ..
            }) => return Ok(ExecutionState::Gap { key: open }),
            Err(SourceError::Unavailable {
                health: SourceHealth::Quarantined,
                ..
            }) => return Ok(ExecutionState::Quarantined { key: open }),
            Err(error) => return Err(error),
        };
        if row.bar.count == 0 {
            return Ok(ExecutionState::ZeroCount {
                source: row.source.clone(),
            });
        }
        let price = self.price_at(source, open.micros(), decision, PriceUse::ExecutionOpen)?;
        Ok(ExecutionState::Valid {
            reference_open: *price.observation.value(),
            count: row.bar.count,
            timestamps: price.observation.timestamps(),
            source: price.source,
        })
    }

    pub(crate) fn rate_frame_at(&self, decision: DecisionTime) -> Result<C1RateFrame, SourceError> {
        let at = decision_us(decision)?;
        let expected = at
            .checked_sub(MINUTE)
            .ok_or(SourceError::Overflow)?
            .div_euclid(SLOT)
            * SLOT;
        let mut frame = C1RateFrame {
            expected_latest: (expected >= SAMPLE).then_some(UtcMicros::from_micros(expected)),
            rows: Vec::new(),
            decision,
        };
        if frame.expected_latest.is_none() {
            return Ok(frame);
        }
        let oldest = expected
            .checked_sub(8 * SLOT)
            .ok_or(SourceError::Overflow)?
            .max(self.files.as_ref().map_or(SAMPLE, |files| files.start()));
        if oldest > expected {
            return Ok(frame);
        }
        let mut slot = oldest;
        while slot <= expected {
            let row = if let Some(files) = &self.files {
                Some(files.funding(slot)?)
            } else {
                self.funding.get(&slot).cloned()
            };
            let Some(row) = row else {
                slot = slot.checked_add(SLOT).ok_or(SourceError::Overflow)?;
                continue;
            };
            let Some(admitted) = admitted_funding(&row, decision)? else {
                break;
            };
            frame.rows.push(admitted);
            slot = slot.checked_add(SLOT).ok_or(SourceError::Overflow)?;
        }
        Ok(frame)
    }

    pub(crate) fn funding_at(
        &self,
        settlement: UtcMicros,
        decision: DecisionTime,
    ) -> Result<C1FundingEvidence, SourceError> {
        let source = if let Some(files) = &self.files {
            files.funding(settlement.micros())?
        } else {
            self.funding
                .get(&settlement.micros())
                .cloned()
                .ok_or(SourceError::Invariant("funding source missing"))?
        };
        let admitted = admitted_funding(&source, decision)?
            .ok_or(SourceError::Invariant("funding rate unavailable"))?;
        let rate = admitted.observation;
        let mark_open_proxy = self.price_at(
            PriceSource::UmMark,
            settlement.micros(),
            decision,
            PriceUse::FundingOpen,
        )?;
        let rate_delivery = rate.timestamps().received_ts.micros();
        let mark_delivery = mark_open_proxy
            .observation
            .timestamps()
            .received_ts
            .micros();
        Ok(C1FundingEvidence {
            rate,
            rate_source: admitted.source,
            mark_open_proxy,
            modeled_delivery: rate_delivery.max(mark_delivery),
        })
    }

    pub(crate) fn daily_marks_at(
        &self,
        day: UtcDate,
        decision: DecisionTime,
    ) -> Result<C1DailyMarks, SourceError> {
        decision_us(decision)?;
        let end = day
            .start_us()
            .micros()
            .checked_add(DAY)
            .ok_or(SourceError::Overflow)?;
        let knowledge = end.checked_add(1_000).ok_or(SourceError::Overflow)?;
        if decision.micros().micros() != knowledge {
            return Err(SourceError::Clock);
        }
        let key = end.checked_sub(MINUTE).ok_or(SourceError::Overflow)?;
        Ok(C1DailyMarks {
            economic_end_exclusive: UtcMicros::from_micros(end),
            knowledge_at: decision,
            spot: self.price_at(PriceSource::Spot, key, decision, PriceUse::DailyClose)?,
            perp_risk: self.price_at(PriceSource::UmMark, key, decision, PriceUse::DailyClose)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cucumber::{given, then, when, World as _};
    use parangsae_core::{EventTs, Qty};

    fn at(value: i64) -> DecisionTime {
        DecisionTime::from_micros(UtcMicros::from_micros(value))
    }
    fn fixture(source: PriceSource, key: i64) -> SourceBar {
        let available = source_available(source, key).unwrap();
        let tick = if source == PriceSource::Spot && key >= SPOT_MICROS {
            1
        } else {
            1_000
        };
        let price = Price::from_ticks(10_000_000_000).unwrap();
        SourceBar::admit(
            source,
            Bar {
                open_time: UtcMicros::from_micros(key),
                close_time: UtcMicros::from_micros(key + MINUTE - tick),
                available: UtcMicros::from_micros(available),
                open: price,
                high: price,
                low: price,
                close: price,
                volume: Qty::from_ticks(100_000_000).unwrap(),
                count: 1,
            },
            SourceRef {
                object: "synthetic".into(),
                sha256: "0".repeat(64),
                row: 0,
                raw_unit: if tick == 1 {
                    TimeUnit::Micros
                } else {
                    TimeUnit::Millis
                },
                original: Timestamps {
                    event_ts: EventTs::from_micros(UtcMicros::from_micros(key)),
                    available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available)),
                    received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(
                        1_792_800_000_000_000,
                    )),
                },
            },
        )
        .unwrap()
    }
    fn empty() -> C1SourceView {
        C1SourceView {
            bars: BTreeMap::new(),
            statuses: BTreeMap::new(),
            funding: BTreeMap::new(),
            files: None,
        }
    }

    #[test]
    fn post2025_spot_waits_for_ceil_ms_without_rewriting_original_clocks() {
        let mut view = empty();
        let key = SPOT_MICROS;
        let row = fixture(PriceSource::Spot, key);
        let original = row.source.original;
        view.bars.insert((PriceSource::Spot, key), row);
        assert_eq!(
            view.execution_at(
                C1Instrument::Spot,
                UtcMicros::from_micros(key),
                at(key + MINUTE)
            )
            .unwrap(),
            ExecutionState::Pending {
                until: UtcMicros::from_micros(key + MINUTE + 1_000)
            }
        );
        let ExecutionState::Valid {
            timestamps, source, ..
        } = view
            .execution_at(
                C1Instrument::Spot,
                UtcMicros::from_micros(key),
                at(key + MINUTE + 1_000),
            )
            .unwrap()
        else {
            panic!("valid evidence expected")
        };
        assert_eq!(source.original, original);
        assert_eq!(
            timestamps.available_ts.micros().micros(),
            key + MINUTE + 999
        );
        assert_eq!(
            timestamps.received_ts.micros().micros(),
            key + MINUTE + 1_000
        );
        assert_eq!(timestamps.event_ts, original.event_ts);
    }

    #[test]
    fn exact_boundary_context_uses_prior_observable_spot_not_future_close() {
        let mut view = empty();
        let boundary = SPOT_MICROS + HOUR;
        for (source, key) in [
            (PriceSource::Spot, boundary - 2 * MINUTE),
            (PriceSource::Spot, boundary - MINUTE),
            (PriceSource::UmMark, boundary - MINUTE),
            (PriceSource::UmTrade, boundary - MINUTE),
        ] {
            view.bars.insert((source, key), fixture(source, key));
        }
        assert_eq!(
            view.marks_at(at(boundary)).unwrap().spot.key.micros(),
            boundary - 2 * MINUTE
        );
        assert!(view
            .pair_refs_at(UtcMicros::from_micros(boundary - MINUTE), at(boundary))
            .is_err());
        let pair = view
            .pair_refs_at(
                UtcMicros::from_micros(boundary - MINUTE),
                at(boundary + 1_000),
            )
            .unwrap();
        assert_eq!(pair.spot.key, pair.um_trade.key);
    }

    #[test]
    fn missing_latest_mark_never_falls_back_to_an_old_valid_row() {
        let mut view = empty();
        let boundary = SPOT_MICROS + HOUR;
        for (source, key) in [
            (PriceSource::Spot, boundary - MINUTE),
            (PriceSource::UmMark, boundary - 2 * MINUTE),
        ] {
            view.bars.insert((source, key), fixture(source, key));
        }
        assert!(matches!(
            view.marks_at(at(boundary + 1_000)),
            Err(SourceError::Unavailable {
                source: PriceSource::UmMark,
                health: SourceHealth::Gap,
                ..
            })
        ));
    }

    #[test]
    fn future_gap_and_count_are_not_exposed_before_notice() {
        let mut view = empty();
        let key = SPOT_MICROS;
        view.statuses
            .insert((PriceSource::Spot, key), SourceHealth::Quarantined);
        assert!(matches!(
            view.execution_at(
                C1Instrument::Spot,
                UtcMicros::from_micros(key),
                at(key + MINUTE)
            ),
            Ok(ExecutionState::Pending { .. })
        ));
        assert!(matches!(
            view.execution_at(
                C1Instrument::Spot,
                UtcMicros::from_micros(key),
                at(key + MINUTE + 1_000)
            ),
            Ok(ExecutionState::Quarantined { .. })
        ));
        view.statuses.clear();
        let mut row = fixture(PriceSource::Spot, key);
        row.bar.count = 0;
        view.bars.insert((PriceSource::Spot, key), row);
        assert!(matches!(
            view.execution_at(
                C1Instrument::Spot,
                UtcMicros::from_micros(key),
                at(key + MINUTE + 1_000)
            ),
            Ok(ExecutionState::ZeroCount { .. })
        ));
    }

    #[test]
    fn before_first_funding_knowledge_returns_an_empty_frame() {
        let mut view = empty();
        view.funding.insert(
            SAMPLE,
            SourceFunding {
                settlement: UtcMicros::from_micros(SAMPLE),
                rate: Ratio::from_ticks(1_000),
                interval_hours: 8,
                source: fixture(PriceSource::UmTrade, SAMPLE).source,
            },
        );
        let frame = view.rate_frame_at(at(SAMPLE + 1_000)).unwrap();
        assert_eq!(frame.expected_latest, None);
        assert!(frame.views().is_empty());
    }

    #[test]
    fn rate_floor_is_applied_once_and_original_clocks_are_retained() {
        let mut view = empty();
        let mut source = fixture(PriceSource::UmTrade, SAMPLE).source;
        source.original.available_ts = AvailableTs::from_micros(UtcMicros::from_micros(SAMPLE));
        source.original.event_ts = EventTs::from_micros(UtcMicros::from_micros(SAMPLE + 1_000));
        let original = source.original;
        view.funding.insert(
            SAMPLE,
            SourceFunding {
                settlement: UtcMicros::from_micros(SAMPLE),
                rate: Ratio::from_ticks(-2_000),
                interval_hours: 8,
                source,
            },
        );
        assert!(view
            .rate_frame_at(at(SAMPLE + MINUTE - 1_000))
            .unwrap()
            .views()
            .is_empty());
        let frame = view.rate_frame_at(at(SAMPLE + MINUTE)).unwrap();
        let row = &frame.rows[0];
        assert_eq!(row.source.original, original);
        assert_eq!(row.observation.timestamps().event_ts, original.event_ts);
        assert_eq!(
            row.observation.timestamps().available_ts.micros().micros(),
            SAMPLE + MINUTE
        );
        assert_eq!(
            row.observation.timestamps().received_ts.micros().micros(),
            SAMPLE + MINUTE
        );
        assert_eq!(frame.views()[0].value().rate().ticks(), -2_000);
    }

    #[test]
    fn later_rate_cannot_skip_an_unavailable_older_prefix() {
        let mut view = empty();
        for slot in [SAMPLE, SAMPLE + SLOT, SAMPLE + 2 * SLOT] {
            let mut source = fixture(PriceSource::UmTrade, slot).source;
            if slot == SAMPLE {
                source.original.available_ts =
                    AvailableTs::from_micros(UtcMicros::from_micros(SAMPLE + 3 * SLOT));
            }
            view.funding.insert(
                slot,
                SourceFunding {
                    settlement: UtcMicros::from_micros(slot),
                    rate: Ratio::from_ticks(1),
                    interval_hours: 8,
                    source,
                },
            );
        }
        let frame = view.rate_frame_at(at(SAMPLE + 2 * SLOT + MINUTE)).unwrap();
        assert_eq!(
            frame.expected_latest,
            Some(UtcMicros::from_micros(SAMPLE + 2 * SLOT))
        );
        assert!(frame.views().is_empty());
    }

    #[test]
    fn decision_trade_close_is_not_the_perp_risk_mark() {
        let mut view = empty();
        let boundary = SPOT_MICROS + HOUR;
        let key = boundary - MINUTE;
        for source in [PriceSource::Spot, PriceSource::UmTrade, PriceSource::UmMark] {
            let mut row = fixture(source, key);
            if source == PriceSource::UmTrade {
                row.bar.close = Price::from_ticks(20_000_000_000).unwrap();
                row.bar.high = row.bar.close;
            }
            view.bars.insert((source, key), row);
        }
        let decision = at(boundary + 1_000);
        assert_eq!(
            view.decision_ref_at(C1Instrument::Perp, decision)
                .unwrap()
                .observation
                .value()
                .ticks(),
            20_000_000_000
        );
        assert_eq!(
            view.marks_at(decision)
                .unwrap()
                .perp_risk
                .observation
                .value()
                .ticks(),
            10_000_000_000
        );
    }

    #[test]
    fn historical_funding_proxy_uses_open_and_not_current_mark_close() {
        let mut view = empty();
        let mut mark = fixture(PriceSource::UmMark, SAMPLE);
        mark.bar.close = Price::from_ticks(11_000_000_000).unwrap();
        mark.bar.high = mark.bar.close;
        let source = mark.source.clone();
        view.bars.insert((PriceSource::UmMark, SAMPLE), mark);
        view.funding.insert(
            SAMPLE,
            SourceFunding {
                settlement: UtcMicros::from_micros(SAMPLE),
                rate: Ratio::from_ticks(10),
                interval_hours: 8,
                source,
            },
        );
        let evidence = view
            .funding_at(UtcMicros::from_micros(SAMPLE), at(SAMPLE + 3 * MINUTE))
            .unwrap();
        assert_eq!(
            evidence.mark_open_proxy.observation.value().ticks(),
            10_000_000_000
        );
        assert_eq!(evidence.modeled_delivery.micros(), SAMPLE + MINUTE);
    }

    #[test]
    fn daily_marks_require_exact_final_keys_at_the_knowledge_cut() {
        let mut view = empty();
        let day = UtcDate::parse("2025-09-30").unwrap();
        let key = END - MINUTE;
        view.bars.insert(
            (PriceSource::UmMark, key),
            fixture(PriceSource::UmMark, key),
        );
        view.bars.insert(
            (PriceSource::Spot, key - MINUTE),
            fixture(PriceSource::Spot, key - MINUTE),
        );
        assert!(matches!(
            view.daily_marks_at(day, at(END + 1_000)),
            Err(SourceError::Unavailable {
                source: PriceSource::Spot,
                health: SourceHealth::Gap,
                ..
            })
        ));
        view.bars
            .insert((PriceSource::Spot, key), fixture(PriceSource::Spot, key));
        assert!(view.daily_marks_at(day, at(END)).is_err());
        let marks = view.daily_marks_at(day, at(END + 1_000)).unwrap();
        assert_eq!(marks.economic_end_exclusive.micros(), END);
        assert_eq!(
            marks.spot.source.original.available_ts.micros().micros(),
            END + 999
        );
        assert_eq!(
            marks
                .spot
                .observation
                .timestamps()
                .received_ts
                .micros()
                .micros(),
            END + 1_000
        );
    }

    #[test]
    fn forbidden_source_keys_and_non_grid_decisions_are_refused() {
        let view = empty();
        assert_eq!(
            view.execution_at(C1Instrument::Spot, UtcMicros::from_micros(END), at(END)),
            Err(SourceError::Range)
        );
        assert_eq!(
            view.rate_frame_at(at(SAMPLE - 1_000)),
            Err(SourceError::Clock)
        );
        assert_eq!(view.rate_frame_at(at(SAMPLE + 1)), Err(SourceError::Clock));
    }

    #[test]
    fn an_early_source_close_cannot_construct_a_valid_bar() {
        let row = fixture(PriceSource::Spot, SAMPLE);
        let mut early = row.bar;
        early.close_time = UtcMicros::from_micros(SAMPLE + 20_000_000);
        early.available = UtcMicros::from_micros(SAMPLE + 20_001_000);
        let mut source = row.source;
        source.original.available_ts = AvailableTs::from_micros(early.available);
        assert!(matches!(
            SourceBar::admit(PriceSource::Spot, early, source),
            Err(SourceError::Invariant("unsupported source close/clock"))
        ));
    }

    #[test]
    fn synthetic_factory_validates_rows_and_does_not_expose_a_production_constructor() {
        let row = fixture(PriceSource::Spot, SPOT_MICROS);
        let view = C1SourceView::synthetic(
            vec![(PriceSource::Spot, row.bar, row.source.clone())],
            vec![],
            vec![],
        )
        .unwrap();
        assert!(matches!(
            view.execution_at(
                C1Instrument::Spot,
                UtcMicros::from_micros(SPOT_MICROS),
                at(SPOT_MICROS + MINUTE + 1_000)
            ),
            Ok(ExecutionState::Valid { .. })
        ));
        let mut invalid = row.bar;
        invalid.available = UtcMicros::from_micros(SPOT_MICROS + MINUTE);
        assert!(C1SourceView::synthetic(
            vec![(PriceSource::Spot, invalid, row.source)],
            vec![],
            vec![]
        )
        .is_err());
    }

    #[test]
    fn synthetic_history_can_cover_two_cycles_with_a_finite_rate_cap() {
        let funding = (0..14)
            .map(|n| {
                let slot = SAMPLE + n * SLOT;
                (
                    UtcMicros::from_micros(slot),
                    Ratio::from_ticks(1),
                    8,
                    fixture(PriceSource::UmTrade, slot).source,
                )
            })
            .collect();
        assert!(C1SourceView::synthetic(vec![], vec![], funding).is_ok());
    }

    #[test]
    fn maximum_hold_synthetic_history_has_a_finite_sixty_four_row_boundary() {
        let funding: Vec<_> = (0..65)
            .map(|n| {
                let slot = SAMPLE + n * SLOT;
                (
                    UtcMicros::from_micros(slot),
                    Ratio::from_ticks(0),
                    8,
                    fixture(PriceSource::UmTrade, slot).source,
                )
            })
            .collect();
        assert!(matches!(
            C1SourceView::synthetic(vec![], vec![], funding.clone()),
            Err(SourceError::Invariant("synthetic source capacity"))
        ));
        assert!(C1SourceView::synthetic(vec![], vec![], funding[..64].to_vec()).is_ok());
    }

    #[derive(Debug, Default, cucumber::World)]
    struct SourceWorld {
        view: Option<C1SourceView>,
        before: Option<ExecutionState>,
        after: Option<ExecutionState>,
        marks: Option<Result<C1Marks, SourceError>>,
    }

    #[given("a closed 2025 spot source bar with its original archive receipt")]
    fn spot_source(world: &mut SourceWorld) {
        let mut view = empty();
        view.bars.insert(
            (PriceSource::Spot, SPOT_MICROS),
            fixture(PriceSource::Spot, SPOT_MICROS),
        );
        world.view = Some(view);
    }

    #[when("the C1 reader checks before and at its modeled delivery")]
    fn source_notice(world: &mut SourceWorld) {
        let view = world.view.as_ref().unwrap();
        world.before = Some(
            view.execution_at(
                C1Instrument::Spot,
                UtcMicros::from_micros(SPOT_MICROS),
                at(SPOT_MICROS + MINUTE),
            )
            .unwrap(),
        );
        world.after = Some(
            view.execution_at(
                C1Instrument::Spot,
                UtcMicros::from_micros(SPOT_MICROS),
                at(SPOT_MICROS + MINUTE + 1_000),
            )
            .unwrap(),
        );
    }

    #[then("the source clocks stay unchanged and only modeled evidence is admitted")]
    fn source_clock_result(world: &mut SourceWorld) {
        assert!(matches!(
            &world.before,
            Some(ExecutionState::Pending { .. })
        ));
        let Some(ExecutionState::Valid {
            timestamps, source, ..
        }) = &world.after
        else {
            panic!("valid evidence expected")
        };
        assert_eq!(
            source.original.received_ts.micros().micros(),
            1_792_800_000_000_000
        );
        assert_eq!(
            source.original.available_ts.micros().micros(),
            SPOT_MICROS + MINUTE + 999
        );
        assert_eq!(
            timestamps.received_ts.micros().micros(),
            SPOT_MICROS + MINUTE + 1_000
        );
    }

    #[given("an older risk mark but no latest expected risk mark")]
    fn risk_gap_source(world: &mut SourceWorld) {
        let mut view = empty();
        let boundary = SPOT_MICROS + HOUR;
        view.bars.insert(
            (PriceSource::Spot, boundary - MINUTE),
            fixture(PriceSource::Spot, boundary - MINUTE),
        );
        view.bars.insert(
            (PriceSource::UmMark, boundary - 2 * MINUTE),
            fixture(PriceSource::UmMark, boundary - 2 * MINUTE),
        );
        world.view = Some(view);
    }

    #[when("a required C1 mark context is requested")]
    fn require_marks(world: &mut SourceWorld) {
        world.marks = Some(
            world
                .view
                .as_ref()
                .unwrap()
                .marks_at(at(SPOT_MICROS + HOUR + 1_000)),
        );
    }

    #[then("the latest key gap is returned without mark carry")]
    fn risk_gap_result(world: &mut SourceWorld) {
        assert!(matches!(
            &world.marks,
            Some(Err(SourceError::Unavailable {
                source: PriceSource::UmMark,
                health: SourceHealth::Gap,
                ..
            }))
        ));
    }

    #[tokio::test]
    async fn source_contract_feature() {
        SourceWorld::cucumber()
            .with_default_cli()
            .fail_on_skipped()
            .run_and_exit(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/features/c1_source.feature"
            ))
            .await;
    }
}
