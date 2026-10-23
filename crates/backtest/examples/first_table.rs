//! First research table for the four v1 target signals. Not a CI test.
//!
//! Loads 2020-12-18..2025-09-30 and writes one report directory per signal
//! plus `table.csv`. Backtest Builder runs this on the store after the merge.
//!
//! ```text
//! cargo run -p parangsae-backtest --example first_table -- <data-root> [out-dir]
//! ```
//!
//! `out-dir` defaults to `research/runs/engine-v1`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use parangsae_backtest::{
    run_targets, FundingObs, MarkAbsence, Overlay, Report, RunConfig, Tape, TapeConfig,
    TargetSignal, UtcDate, FEE_GRID_BPS,
};
use parangsae_core::{
    AvailableTs, BarInterval, Causal, DecisionTime, EventTs, Interval, Observation, ReceivedTs,
    ResampledBar, Symbol, Timestamps,
};
use parangsae_signals::{
    drive, drive_funding_z, FundingRow, FundingZ, SignalOutput, Tsmom, PROD_MAX_AGE_US,
};
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};

const SCALE: i64 = 100_000_000;
const OVERLAY_SHA: &str = "d5f6bcd61688cac971b6d276ea4fa0826c86d998b7adc06557686da6bdcfb8ea";
const HEADLINE_FEE_BPS: u32 = 5;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("first_table: {message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let (root, out) = parse_args()?;
    let symbol = Symbol::new("BTCUSDT").expect("BTCUSDT");
    let holdout = date("2025-10-01")?.start_us();
    let overlay_path = root.join("runs/resolutions/um-kline-2023-11-10.parquet");
    let tape = Tape::load(
        &root,
        &symbol,
        date("2020-12-18")?,
        date("2025-09-30")?,
        &TapeConfig {
            holdout_start: holdout,
            overlays: vec![Overlay {
                dataset: "um-kline".to_owned(),
                path: overlay_path,
                sha256: parse_sha(OVERLAY_SHA)?,
            }],
            funding_root: root.join("runs/funding-4db8a75"),
        },
    )
    .map_err(|err| err.to_string())?;
    let funding = funding_observations(&tape)?;
    std::fs::create_dir_all(&out).map_err(|err| format!("create {}: {err}", out.display()))?;

    let rows = vec![
        run_tsmom::<24>(&tape, &out, BarInterval::H1, 0.05, "tsmom")?,
        run_tsmom::<42>(&tape, &out, BarInterval::H4, 0.10, "tsmom")?,
        run_funding::<90>(&tape, &funding, &out, BarInterval::H1)?,
        run_funding::<90>(&tape, &funding, &out, BarInterval::H4)?,
    ];

    let mut csv = String::from(
        "signal,interval,return_ticks,sharpe_annual_ticks,max_drawdown_ticks,turnover_ticks,\
         net_2bps_ticks,net_4bps_ticks,net_5bps_ticks,net_6bps_ticks\n",
    );
    for row in rows {
        csv.push_str(&row);
        csv.push('\n');
    }
    let table = out.join("table.csv");
    std::fs::write(&table, csv).map_err(|err| format!("write {}: {err}", table.display()))?;
    println!("table={}", table.display());
    Ok(())
}

fn run_tsmom<const N: u16>(
    tape: &Tape,
    out: &Path,
    interval: BarInterval,
    r_scale: f64,
    signal_id: &str,
) -> Result<String, String> {
    let signal = Tsmom::<N>::new(interval, r_scale).map_err(|err| err.to_string())?;
    let mut adapter = TsmomAdapter {
        signal,
        cache: BarCache::default(),
    };
    let dir = out.join(format!("{signal_id}-{interval}"));
    fee_row(
        tape,
        &mut adapter,
        &run_config(interval),
        &dir,
        signal_id,
        interval,
    )
}

fn run_funding<const M: u16>(
    tape: &Tape,
    funding: &[Observation<FundingRow>],
    out: &Path,
    interval: BarInterval,
) -> Result<String, String> {
    let signal =
        FundingZ::<M>::new(interval, 2.0, PROD_MAX_AGE_US).map_err(|err| err.to_string())?;
    let mut adapter = FundingAdapter {
        signal,
        funding: funding.to_vec(),
        cache: BarCache::default(),
    };
    let dir = out.join(format!("funding_z-{interval}"));
    fee_row(
        tape,
        &mut adapter,
        &run_config(interval),
        &dir,
        "funding_z",
        interval,
    )
}

fn fee_row<S: TargetSignal>(
    tape: &Tape,
    signal: &mut S,
    cfg: &RunConfig,
    dir: &Path,
    signal_id: &str,
    interval: BarInterval,
) -> Result<String, String> {
    let mut nets = [0i64; FEE_GRID_BPS.len()];
    let mut headline: Option<Report> = None;
    for (index, bps) in FEE_GRID_BPS.iter().enumerate() {
        let costs = Costs {
            taker_fee: FeeBps(*bps),
            slippage: SlippageBps(1),
        };
        let report = run_targets(tape, signal, &costs, cfg).map_err(|err| err.to_string())?;
        nets[index] = report.metrics.total_return.ticks();
        if *bps == HEADLINE_FEE_BPS {
            report
                .write_dir(dir)
                .map_err(|err| format!("write {}: {err}", dir.display()))?;
            headline = Some(report);
        }
    }
    let report = headline.ok_or("fee grid has no 5 bps column")?;
    let sharpe = match report.metrics.sharpe_annual {
        Some(ratio) => ratio.ticks().to_string(),
        None => "none".to_owned(),
    };
    Ok(format!(
        "{signal_id},{interval},{},{sharpe},{},{},{},{},{},{}",
        report.metrics.total_return.ticks(),
        report.metrics.max_drawdown.ticks(),
        report.metrics.turnover.ticks(),
        nets[0],
        nets[1],
        nets[2],
        nets[3],
    ))
}

fn run_config(interval: BarInterval) -> RunConfig {
    let gap_day = date("2024-08-12").expect("2024-08-12").start_us();
    let minute = 60_000_000;
    RunConfig {
        initial_cash: Cash(1_000_000 * SCALE),
        start: date("2020-12-18").expect("sample start").start_us(),
        end: date("2025-09-30").expect("sample end").next().start_us(),
        holdout_start: date("2025-10-01").expect("holdout").start_us(),
        forced_flat: Vec::new(),
        mark_gaps: vec![MarkAbsence {
            first_open: gap_day
                .checked_add_micros(10 * 3_600_000_000 + 2 * minute)
                .expect("2024-08-12 10:02"),
            last_open: gap_day
                .checked_add_micros(10 * 3_600_000_000 + 3 * minute)
                .expect("2024-08-12 10:03"),
            known_source_absence: true,
        }],
        interval: match interval {
            BarInterval::H1 => Interval::ONE_HOUR,
            BarInterval::H4 => Interval::FOUR_HOURS,
        },
        lot_ticks: 100_000,
    }
}

fn funding_observations(tape: &Tape) -> Result<Vec<Observation<FundingRow>>, String> {
    let mut rows = Vec::with_capacity(tape.funding.len());
    for stamp in &tape.funding {
        let hours = u8::try_from(stamp.interval_hours).map_err(|_| {
            format!(
                "funding interval {} at {} does not fit u8",
                stamp.interval_hours, stamp.funding_time
            )
        })?;
        rows.push(Observation::new(
            Timestamps {
                event_ts: EventTs::from_micros(stamp.funding_time),
                available_ts: AvailableTs::from_micros(stamp.available),
                received_ts: ReceivedTs::from_micros(stamp.available),
            },
            FundingRow::new(stamp.funding_time, stamp.rate, hours),
        ));
    }
    Ok(rows)
}

struct TsmomAdapter<const N: u16> {
    signal: Tsmom<N>,
    cache: BarCache,
}

impl<const N: u16> TargetSignal for TsmomAdapter<N> {
    fn on_bar(
        &mut self,
        bars: Causal<'_, &[ResampledBar]>,
        _funding: Causal<'_, &[FundingObs]>,
    ) -> Option<SignalOutput> {
        let decision = DecisionTime::from_micros(bars.available_ts().micros());
        let rows = self.cache.observations(bars.value());
        match drive(&self.signal, rows, decision) {
            Ok(output) => output,
            Err(err) => panic!("tsmom<{N}>: {err}"),
        }
    }
}

struct FundingAdapter<const M: u16> {
    signal: FundingZ<M>,
    funding: Vec<Observation<FundingRow>>,
    cache: BarCache,
}

impl<const M: u16> TargetSignal for FundingAdapter<M> {
    fn on_bar(
        &mut self,
        bars: Causal<'_, &[ResampledBar]>,
        funding: Causal<'_, &[FundingObs]>,
    ) -> Option<SignalOutput> {
        let decision = DecisionTime::from_micros(bars.available_ts().micros());
        let bar_rows = self.cache.observations(bars.value());
        let fund_rows = funding_prefix(&self.funding, funding.value());
        match drive_funding_z(&self.signal, bar_rows, fund_rows, decision) {
            Ok(output) => output,
            Err(err) => panic!("funding_z<{M}>: {err}"),
        }
    }
}

fn funding_prefix<'a>(
    all: &'a [Observation<FundingRow>],
    view: &[FundingObs],
) -> &'a [Observation<FundingRow>] {
    if view.len() > all.len() {
        panic!(
            "funding view has {} rows; the tape has {}",
            view.len(),
            all.len()
        );
    }
    if let Some(last) = view.last() {
        let stored = all[view.len() - 1].value();
        if stored.funding_time() != last.funding_time {
            panic!(
                "funding view is not a prefix of the tape at {}",
                last.funding_time
            );
        }
    }
    &all[..view.len()]
}

#[derive(Default)]
struct BarCache {
    rows: Vec<Observation<ResampledBar>>,
}

impl BarCache {
    fn observations(&mut self, bars: &[ResampledBar]) -> &[Observation<ResampledBar>] {
        if self.rows.len() + 1 == bars.len() {
            if let Some(bar) = bars.last() {
                self.rows.push(bar_observation(*bar));
            }
        } else if self.rows.len() != bars.len() {
            self.rows.clear();
            self.rows.extend(bars.iter().copied().map(bar_observation));
        }
        &self.rows
    }
}

fn bar_observation(bar: ResampledBar) -> Observation<ResampledBar> {
    let close = bar.close_time();
    let available = close
        .checked_add_micros(1_000)
        .expect("bar availability fits");
    Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(close),
            available_ts: AvailableTs::from_micros(available),
            received_ts: ReceivedTs::from_micros(available),
        },
        bar,
    )
}

fn date(text: &str) -> Result<UtcDate, String> {
    UtcDate::parse(text).ok_or_else(|| format!("bad date {text}"))
}

fn parse_args() -> Result<(PathBuf, PathBuf), String> {
    let mut args = env::args().skip(1);
    let root = args
        .next()
        .ok_or("usage: first_table <data-root> [out-dir]")?;
    let out = match args.next() {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from("research/runs/engine-v1"),
    };
    if args.next().is_some() {
        return Err("usage: first_table <data-root> [out-dir]".to_owned());
    }
    Ok((PathBuf::from(root), out))
}

fn parse_sha(hex_text: &str) -> Result<[u8; 32], String> {
    if hex_text.len() != 64 {
        return Err(format!("sha length {}", hex_text.len()));
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex_text[index * 2..index * 2 + 2], 16)
            .map_err(|_| format!("bad sha {hex_text}"))?;
    }
    Ok(out)
}
