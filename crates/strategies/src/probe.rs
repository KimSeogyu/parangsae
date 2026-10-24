//! Prefix check used by the walk-forward run log.
//!
//! Mutating the last bar, or a funding or premium row after the previous
//! decision, must leave every earlier output unchanged.

use std::collections::BTreeMap;

use parangsae_core::{
    AvailableTs, BarInterval, EventTs, Observation, Price, ReceivedTs, ResampledBar, Timestamps,
    UtcMicros,
};
use parangsae_signals::SignalOutput;

use crate::aux::{FundingPrint, PremiumMinute};
use crate::Strategy;

const PROBE_ORIGIN_US: i64 = 1_704_067_200_000_000;

pub fn truncation_probe(family: &str, params: &BTreeMap<String, i64>) -> Result<(), String> {
    let interval = Strategy::from_params(family, params)
        .map_err(|err| err.to_string())?
        .interval();
    let bars = probe_bars(interval, bar_count(params))?;
    let full = crate::replay(family, params, &bars).map_err(|err| err.to_string())?;
    let mutated = mutate_last_close(&bars)?;
    let changed = crate::replay(family, params, &mutated).map_err(|err| err.to_string())?;
    let prior = bars[bars.len() - 2]
        .timestamps()
        .available_ts
        .micros()
        .micros();
    if outputs_upto(&full, prior) != outputs_upto(&changed, prior) {
        return Err(format!(
            "{family} lookahead: a later bar changed an earlier output"
        ));
    }
    match family {
        "H4" => probe_funding(family, params, &bars),
        "H5" => probe_premium(family, params, &bars),
        _ => Ok(()),
    }
}

fn bar_count(params: &BTreeMap<String, i64>) -> usize {
    let n = params.get("n").copied().unwrap_or(24);
    let days = params.get("w_days").copied().unwrap_or(0);
    let raw = n.max(days.saturating_mul(6)).saturating_add(8);
    usize::try_from(raw).unwrap_or(32).clamp(16, 900)
}

fn probe_bars(
    interval: BarInterval,
    count: usize,
) -> Result<Vec<Observation<ResampledBar>>, String> {
    let mut bars = Vec::with_capacity(count);
    for index in 0..count {
        let px = 4_000_000_000_000 + i64::try_from(index).unwrap_or(0) * 100_000_000;
        bars.push(bar(index, interval, px)?);
    }
    Ok(bars)
}

fn bar(
    index: usize,
    interval: BarInterval,
    close: i64,
) -> Result<Observation<ResampledBar>, String> {
    let step = interval.micros();
    let open_us = PROBE_ORIGIN_US + i64::try_from(index).unwrap_or(0) * step;
    let price = Price::from_ticks(close).map_err(|_| "probe price".to_owned())?;
    let resampled = ResampledBar::new(
        UtcMicros::from_micros(open_us),
        interval,
        price,
        price,
        price,
        price,
    )
    .map_err(|_| "probe bar".to_owned())?;
    let available = open_us + step;
    Ok(Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(open_us)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(available)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(available)),
        },
        resampled,
    ))
}

fn mutate_last_close(
    bars: &[Observation<ResampledBar>],
) -> Result<Vec<Observation<ResampledBar>>, String> {
    let mut mutated = bars.to_vec();
    let index = bars.len() - 1;
    let current = *mutated[index].value();
    let close = current
        .close()
        .ticks()
        .checked_add(100_000_000)
        .ok_or("probe close overflow")?;
    let price = Price::from_ticks(close).map_err(|_| "probe price".to_owned())?;
    let bar = ResampledBar::new(
        current.open_time(),
        current.interval(),
        current.open(),
        price,
        current.low(),
        price,
    )
    .map_err(|_| "probe bar".to_owned())?;
    mutated[index] = Observation::new(mutated[index].timestamps(), bar);
    Ok(mutated)
}

fn outputs_upto(outputs: &[SignalOutput], prior: i64) -> Vec<SignalOutput> {
    outputs
        .iter()
        .filter(|signal| signal.decision_ts.micros().micros() <= prior)
        .cloned()
        .collect()
}

fn probe_funding(
    family: &str,
    params: &BTreeMap<String, i64>,
    bars: &[Observation<ResampledBar>],
) -> Result<(), String> {
    let prior = bars[bars.len() - 2]
        .timestamps()
        .available_ts
        .micros()
        .micros();
    let base = funding_at(prior + 3_600_000_000, 1);
    let changed = funding_at(prior + 3_600_000_000, 50_000_000);
    let left = run_funding(family, params, bars, &[base])?;
    let right = run_funding(family, params, bars, &[changed])?;
    if outputs_upto(&left, prior) != outputs_upto(&right, prior) {
        return Err(format!(
            "{family} lookahead: a later funding print changed an earlier output"
        ));
    }
    Ok(())
}

fn funding_at(funding_time_us: i64, rate_ticks: i64) -> FundingPrint {
    FundingPrint {
        funding_time_us,
        available_us: funding_time_us + 60_000_000,
        rate_ticks,
    }
}

fn run_funding(
    family: &str,
    params: &BTreeMap<String, i64>,
    bars: &[Observation<ResampledBar>],
    funding: &[FundingPrint],
) -> Result<Vec<SignalOutput>, String> {
    let mut strategy = Strategy::from_params(family, params).map_err(|err| err.to_string())?;
    let mut out = Vec::new();
    for bar in bars {
        let decision = bar.timestamps().available_ts.micros().micros();
        let visible: Vec<_> = funding
            .iter()
            .copied()
            .filter(|row| row.available_us <= decision)
            .collect();
        strategy
            .ingest_funding(&visible)
            .map_err(|err| err.to_string())?;
        if let Some(signal) = strategy.push(bar, None).map_err(|err| err.to_string())? {
            out.push(signal);
        }
    }
    Ok(out)
}

fn probe_premium(
    family: &str,
    params: &BTreeMap<String, i64>,
    bars: &[Observation<ResampledBar>],
) -> Result<(), String> {
    let prior = bars[bars.len() - 2]
        .timestamps()
        .available_ts
        .micros()
        .micros();
    let open = prior + 60_000_000;
    let base = premium_at(open, 100_000);
    let changed = premium_at(open, 900_000_000);
    let left = run_premium(family, params, bars, &[base])?;
    let right = run_premium(family, params, bars, &[changed])?;
    if outputs_upto(&left, prior) != outputs_upto(&right, prior) {
        return Err(format!(
            "{family} lookahead: a later premium minute changed an earlier output"
        ));
    }
    Ok(())
}

fn premium_at(open_us: i64, close_ticks: i64) -> PremiumMinute {
    PremiumMinute {
        open_us,
        close_ticks,
        count: 1,
        available_us: open_us + 60_000_000,
    }
}

fn run_premium(
    family: &str,
    params: &BTreeMap<String, i64>,
    bars: &[Observation<ResampledBar>],
    premium: &[PremiumMinute],
) -> Result<Vec<SignalOutput>, String> {
    let mut strategy = Strategy::from_params(family, params).map_err(|err| err.to_string())?;
    let mut out = Vec::new();
    for bar in bars {
        let decision = bar.timestamps().available_ts.micros().micros();
        let visible: Vec<_> = premium
            .iter()
            .copied()
            .filter(|row| row.available_us <= decision)
            .collect();
        strategy
            .ingest_premium(&visible)
            .map_err(|err| err.to_string())?;
        if let Some(signal) = strategy.push(bar, None).map_err(|err| err.to_string())? {
            out.push(signal);
        }
    }
    Ok(out)
}
