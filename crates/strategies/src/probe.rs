//! Prefix check used by the walk-forward run log.
//!
//! An active comparison prefix is required. Later bar, funding, and premium
//! mutations must leave that prefix unchanged. This is a synthetic regression
//! check, not a proof for every input or a strategy-performance observation.

use std::collections::BTreeMap;

use parangsae_core::{
    AvailableTs, BarInterval, EventTs, Observation, Price, ReceivedTs, ResampledBar, Timestamps,
    UtcMicros,
};
use parangsae_signals::SignalOutput;

use crate::aux::{FundingPrint, PremiumMinute};
use crate::Strategy;

const PROBE_ORIGIN_US: i64 = 1_704_067_200_000_000;
const HOUR_US: i64 = 3_600_000_000;
const DAY_US: i64 = 24 * HOUR_US;

pub fn truncation_probe(family: &str, params: &BTreeMap<String, i64>) -> Result<(), String> {
    match family {
        "H4" => return probe_funding(family, params),
        "H5" => return probe_premium(family, params),
        _ => {}
    }
    let interval = Strategy::from_params(family, params)
        .map_err(|err| err.to_string())?
        .interval();
    let bars = probe_bars(interval, bar_count(family, params))?;
    let full = crate::replay(family, params, &bars).map_err(|err| err.to_string())?;
    let mutated = mutate_last_close(&bars)?;
    let changed = crate::replay(family, params, &mutated).map_err(|err| err.to_string())?;
    let prior = bars[bars.len() - 2]
        .timestamps()
        .available_ts
        .micros()
        .micros();
    compare_prefix(family, &full, &changed, prior)
}

fn bar_count(family: &str, params: &BTreeMap<String, i64>) -> usize {
    if family == "H2" {
        // The longest member needs 181 UTC H4 bars. Complete the last H4
        // bucket and retain active decisions before the final mutation.
        return 736;
    }
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
        let px = 4_000_000_000_000 + i64::try_from(index).unwrap_or(0) * 10_000_000_000;
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
    bar_at(open_us, interval, close)
}

fn bar_at(
    open_us: i64,
    interval: BarInterval,
    close: i64,
) -> Result<Observation<ResampledBar>, String> {
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
    let available = open_us + interval.micros();
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
        .checked_div(2)
        .ok_or("probe close overflow")?;
    let price = Price::from_ticks(close).map_err(|_| "probe price".to_owned())?;
    let bar = ResampledBar::new(
        current.open_time(),
        current.interval(),
        current.open(),
        Price::from_ticks(current.high().ticks().max(close))
            .map_err(|_| "probe high".to_owned())?,
        Price::from_ticks(current.low().ticks().min(close)).map_err(|_| "probe low".to_owned())?,
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

fn compare_prefix(
    family: &str,
    full: &[SignalOutput],
    changed: &[SignalOutput],
    prior: i64,
) -> Result<(), String> {
    let prefix = outputs_upto(full, prior);
    if !prefix
        .iter()
        .any(|signal| signal.decision.target_ticks() != 0)
    {
        return Err(format!(
            "{family} causality probe has no active comparison decision"
        ));
    }
    if prefix != outputs_upto(changed, prior) {
        return Err(format!(
            "{family} lookahead: future input changed an earlier output"
        ));
    }
    Ok(())
}

struct AuxiliaryProbe<T> {
    bars: Vec<Observation<ResampledBar>>,
    rows: Vec<T>,
    prior: i64,
    mutation_at: i64,
}

fn probe_funding(family: &str, params: &BTreeMap<String, i64>) -> Result<(), String> {
    let probe = funding_inputs(params)?;
    let left = run_funding(family, params, &probe.bars, &probe.rows)?;
    let bars = mutate_last_close(&probe.bars)?;
    let bar_changed = run_funding(family, params, &bars, &probe.rows)?;
    compare_prefix(family, &left, &bar_changed, probe.prior)?;
    let mut changed = probe.rows;
    let future = changed
        .iter_mut()
        .find(|row| row.funding_time_us == probe.mutation_at)
        .ok_or("probe future funding is absent")?;
    future.rate_ticks = -100_000_000;
    let right = run_funding(family, params, &probe.bars, &changed)?;
    compare_prefix(family, &left, &right, probe.prior)
}

fn funding_inputs(params: &BTreeMap<String, i64>) -> Result<AuxiliaryProbe<FundingPrint>, String> {
    let confirm = *params.get("confirm_h").ok_or("probe needs confirm_h")?;
    if !(1..=168).contains(&confirm) {
        return Err("unsupported confirmation window in funding probe".to_owned());
    }
    let event = PROBE_ORIGIN_US;
    let future = event + 256 * HOUR_US;
    let mut rows = Vec::new();
    let mut time = event - 365 * DAY_US - 16 * HOUR_US;
    while time <= future {
        rows.push(funding_at(
            time,
            if time == event || time == future {
                100_000_000
            } else {
                0
            },
        ));
        time += 8 * HOUR_US;
    }
    let mut bars = Vec::new();
    for index in 0..(256 + confirm + 3) {
        let falls = (confirm..confirm + 2).contains(&index) || index >= 256 + confirm;
        bars.push(bar_at(
            event + (index - 1) * HOUR_US,
            BarInterval::H1,
            if falls { 9_000_000_000 } else { 10_000_000_000 },
        )?);
    }
    Ok(AuxiliaryProbe {
        bars,
        rows,
        prior: event + (confirm + 1) * HOUR_US,
        mutation_at: future,
    })
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

fn probe_premium(family: &str, params: &BTreeMap<String, i64>) -> Result<(), String> {
    let probe = premium_inputs(params)?;
    let left = run_premium(family, params, &probe.bars, &probe.rows)?;
    let bars = mutate_last_close(&probe.bars)?;
    let bar_changed = run_premium(family, params, &bars, &probe.rows)?;
    compare_prefix(family, &left, &bar_changed, probe.prior)?;
    let mut changed = probe.rows;
    for row in &mut changed {
        if row.open_us >= probe.mutation_at {
            row.close_ticks = -100_000_000;
        }
    }
    let right = run_premium(family, params, &probe.bars, &changed)?;
    compare_prefix(family, &left, &right, probe.prior)
}

fn premium_inputs(params: &BTreeMap<String, i64>) -> Result<AuxiliaryProbe<PremiumMinute>, String> {
    let lookback = *params.get("lookback_d").ok_or("probe needs lookback_d")?;
    if !(80..=366).contains(&lookback) {
        return Err("unsupported lookback in premium probe".to_owned());
    }
    let future_day = lookback + 10;
    let mut rows = Vec::new();
    for day in 0..=future_day {
        let close = if day == lookback || day == future_day {
            100_000_000
        } else if day % 2 == 0 {
            1_000
        } else {
            2_000
        };
        for minute in 0..1_380 {
            rows.push(premium_at(
                PROBE_ORIGIN_US + day * DAY_US + minute * 60_000_000,
                close,
            ));
        }
    }
    let first_hour = (lookback + 1) * 24;
    let last_hour = (future_day + 1) * 24;
    let bars = ((first_hour - 1)..(last_hour + 2))
        .map(|index| {
            bar(
                usize::try_from(index).map_err(|_| "probe index".to_owned())?,
                BarInterval::H1,
                4_000_000_000_000,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AuxiliaryProbe {
        bars,
        rows,
        prior: PROBE_ORIGIN_US + (first_hour + 1) * HOUR_US,
        mutation_at: PROBE_ORIGIN_US + future_day * DAY_US,
    })
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

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn h2_params() -> BTreeMap<String, i64> {
        BTreeMap::from([
            ("deadband".to_owned(), 25_000_000),
            ("n_members".to_owned(), 12),
        ])
    }

    #[test]
    fn h2_probe_compares_an_active_prior_decision() {
        let params = h2_params();
        let bars = probe_bars(BarInterval::H1, bar_count("H2", &params)).unwrap();
        let outputs = crate::replay("H2", &params, &bars).unwrap();
        let prior = decision_us(&bars[bars.len() - 2]);
        assert!(
            outputs_upto(&outputs, prior)
                .iter()
                .any(|signal| signal.decision.target_ticks() != 0),
            "the probe must exercise a live target before the future mutation"
        );
        let mut leaky = outputs.clone();
        leaky[0].accept += 1;
        assert!(compare_prefix("H2", &outputs, &leaky, prior).is_err());
    }

    #[test]
    fn an_inactive_probe_cannot_report_a_causality_pass() {
        let mut params = h2_params();
        params.insert("deadband".to_owned(), 200_000_001);
        let result = truncation_probe("H2", &params);
        assert!(result.is_err(), "an all-flat probe was accepted");
    }

    #[test]
    fn h1_frozen_grid_probes_have_active_comparison_prefixes() {
        for n in [180, 360] {
            let params = BTreeMap::from([
                ("c".to_owned(), 100_000_000),
                ("interval".to_owned(), 240),
                ("n".to_owned(), n),
                ("deadband".to_owned(), 50_000_000),
                ("decision_hour_utc".to_owned(), 0),
            ]);
            truncation_probe("H1", &params).unwrap();
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(4))]

        #[test]
        fn h2_future_price_mutations_preserve_active_prior_decisions(divisor in 2_i64..=5) {
            let params = h2_params();
            let bars = probe_bars(BarInterval::H1, bar_count("H2", &params)).unwrap();
            let full = crate::replay("H2", &params, &bars).unwrap();
            let last = bars.len() - 1;
            let prior = decision_us(&bars[last - 1]);
            let mut changed = bars.clone();
            changed[last] = bar(last, BarInterval::H1, bars[last].value().close().ticks() / divisor).unwrap();
            let mutated = crate::replay("H2", &params, &changed).unwrap();
            prop_assert!(compare_prefix("H2", &full, &mutated, prior).is_ok());
            prop_assert_ne!(full.last(), mutated.last());
        }
    }

    #[test]
    fn funding_probe_has_active_prior_and_distinct_future_decisions() {
        let params = BTreeMap::from([
            ("q".to_owned(), 10_000_000),
            ("sum_settlements".to_owned(), 3),
            ("confirm_h".to_owned(), 24),
            ("hold_h".to_owned(), 72),
        ]);
        let probe = funding_inputs(&params).unwrap();
        assert!(probe
            .rows
            .iter()
            .all(|row| row.funding_time_us % (8 * HOUR_US) == 0));
        let full = run_funding("H4", &params, &probe.bars, &probe.rows).unwrap();
        let mut rows = probe.rows.clone();
        let future = rows
            .iter_mut()
            .find(|row| row.funding_time_us == probe.mutation_at)
            .unwrap();
        assert!(future.available_us > probe.prior);
        assert!(future.available_us < decision_us(probe.bars.last().unwrap()));
        future.rate_ticks = -100_000_000;
        let changed = run_funding("H4", &params, &probe.bars, &rows).unwrap();
        compare_prefix("H4", &full, &changed, probe.prior).unwrap();
        assert_ne!(
            full, changed,
            "the future funding mutation must reach a decision"
        );
        truncation_probe("H4", &params).unwrap();
    }

    #[test]
    fn premium_probe_has_active_prior_and_distinct_future_decisions() {
        let params = BTreeMap::from([
            ("lookback_d".to_owned(), 90),
            ("k".to_owned(), 200_000_000),
            ("hold_h".to_owned(), 72),
        ]);
        let probe = premium_inputs(&params).unwrap();
        let full = run_premium("H5", &params, &probe.bars, &probe.rows).unwrap();
        let mut rows = probe.rows.clone();
        for row in &mut rows {
            if row.open_us >= probe.mutation_at {
                assert!(row.available_us > probe.prior);
                assert!(row.available_us < decision_us(probe.bars.last().unwrap()));
                row.close_ticks = -100_000_000;
            }
        }
        let changed = run_premium("H5", &params, &probe.bars, &rows).unwrap();
        compare_prefix("H5", &full, &changed, probe.prior).unwrap();
        assert_ne!(
            full, changed,
            "the future premium mutation must reach a decision"
        );
        truncation_probe("H5", &params).unwrap();
    }

    fn decision_us(bar: &Observation<ResampledBar>) -> i64 {
        bar.timestamps().available_ts.micros().micros()
    }
}
