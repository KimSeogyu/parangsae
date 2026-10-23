//! First research table for the four v1 target signals. Not a CI test.
//!
//! Loads 2020-12-18..2025-09-30 and writes one report directory per signal
//! plus `table.csv`. Backtest Builder runs this on the store after the merge.
//!
//! ```text
//! cargo run -p parangsae-backtest --example first_table -- <data-root> <out-dir>
//! ```

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use parangsae_backtest::{
    run_targets, BookState, FundingObs, MarkAbsence, Overlay, Report, RunConfig, Tape, TapeConfig,
    TargetSignal, UtcDate, FEE_GRID_BPS,
};
use parangsae_core::{
    AvailableTs, BarInterval, Causal, DecisionTime, EventTs, Interval, Observation, ReceivedTs,
    ResampledBar, Symbol, Timestamps,
};
use parangsae_signals::{
    drive, drive_funding_z, CausalSignal, FundingRow, FundingZ, SignalOutput, Tsmom,
    PROD_MAX_AGE_US,
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
    let mut adapter = TsmomAdapter::new(signal);
    let dir = out.join(format!("{signal_id}-{interval}"));
    fee_row(
        tape,
        &mut adapter,
        &run_config(interval),
        &dir,
        signal_id,
        interval,
        |adapter| adapter.error.take(),
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
    let mut adapter = FundingAdapter::new(signal, funding.to_vec());
    let dir = out.join(format!("funding_z-{interval}"));
    fee_row(
        tape,
        &mut adapter,
        &run_config(interval),
        &dir,
        "funding_z",
        interval,
        |adapter| adapter.error.take(),
    )
}

fn fee_row<S, F>(
    tape: &Tape,
    signal: &mut S,
    cfg: &RunConfig,
    dir: &Path,
    signal_id: &str,
    interval: BarInterval,
    mut take_error: F,
) -> Result<String, String>
where
    S: TargetSignal,
    F: FnMut(&mut S) -> Option<String>,
{
    let mut nets = [0i64; FEE_GRID_BPS.len()];
    let mut headline: Option<Report> = None;
    for (index, bps) in FEE_GRID_BPS.iter().enumerate() {
        let costs = Costs {
            taker_fee: FeeBps(*bps),
            slippage: SlippageBps(1),
        };
        let outcome = run_targets(tape, signal, &costs, cfg);
        if let Some(err) = take_error(signal) {
            return Err(err);
        }
        let report = outcome.map_err(|err| err.to_string())?;
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
    let end = date("2025-09-30").expect("sample end").next().start_us();
    RunConfig {
        initial_cash: Cash(1_000_000 * SCALE),
        warmup_start: date("2020-12-18").expect("sample start").start_us(),
        start: date("2020-12-18").expect("sample start").start_us(),
        end,
        holdout_start: date("2025-10-01").expect("holdout").start_us(),
        forced_flat: Vec::new(),
        flatten_before: Some(end),
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
    bars: RollingBars,
    error: Option<String>,
}

impl<const N: u16> TsmomAdapter<N> {
    fn new(signal: Tsmom<N>) -> Self {
        Self {
            signal,
            bars: RollingBars::new(<Tsmom<N> as CausalSignal>::WINDOW_CAP.get()),
            error: None,
        }
    }
}

impl<const N: u16> TargetSignal for TsmomAdapter<N> {
    fn on_bar(
        &mut self,
        bars: Causal<'_, &[ResampledBar]>,
        _funding: Causal<'_, &[FundingObs]>,
        _book: BookState,
    ) -> Option<SignalOutput> {
        if self.error.is_some() {
            return None;
        }
        let decision = DecisionTime::from_micros(bars.available_ts().micros());
        let rows = match self.bars.sync(bars.value()) {
            Ok(rows) => rows.to_vec(),
            Err(err) => {
                self.error = Some(err);
                return None;
            }
        };
        match drive(&self.signal, &rows, decision) {
            Ok(output) => output,
            Err(err) => {
                self.error = Some(format!("tsmom<{N}>: {err}"));
                None
            }
        }
    }
}

struct FundingAdapter<const M: u16> {
    signal: FundingZ<M>,
    funding: Vec<Observation<FundingRow>>,
    bars: RollingBars,
    error: Option<String>,
}

impl<const M: u16> FundingAdapter<M> {
    fn new(signal: FundingZ<M>, funding: Vec<Observation<FundingRow>>) -> Self {
        Self {
            signal,
            funding,
            bars: RollingBars::new(FundingZ::<M>::BAR_CAP.get()),
            error: None,
        }
    }
}

impl<const M: u16> TargetSignal for FundingAdapter<M> {
    fn on_bar(
        &mut self,
        bars: Causal<'_, &[ResampledBar]>,
        funding: Causal<'_, &[FundingObs]>,
        _book: BookState,
    ) -> Option<SignalOutput> {
        if self.error.is_some() {
            return None;
        }
        let decision = DecisionTime::from_micros(bars.available_ts().micros());
        let bar_rows = match self.bars.sync(bars.value()) {
            Ok(rows) => rows.to_vec(),
            Err(err) => {
                self.error = Some(err);
                return None;
            }
        };
        let fund_rows = match funding_window(
            &self.funding,
            funding.value(),
            FundingZ::<M>::FUNDING_CAP.get(),
        ) {
            Ok(rows) => rows.to_vec(),
            Err(err) => {
                self.error = Some(err);
                return None;
            }
        };
        match drive_funding_z(&self.signal, &bar_rows, &fund_rows, decision) {
            Ok(output) => output,
            Err(err) => {
                self.error = Some(format!("funding_z<{M}>: {err}"));
                None
            }
        }
    }
}

fn funding_window<'a>(
    all: &'a [Observation<FundingRow>],
    view: &[FundingObs],
    cap: usize,
) -> Result<&'a [Observation<FundingRow>], String> {
    if view.len() > all.len() {
        return Err(format!(
            "funding view has {} rows; the tape has {}",
            view.len(),
            all.len()
        ));
    }
    if let Some(last) = view.last() {
        let stored = all[view.len() - 1].value();
        if stored.funding_time() != last.funding_time {
            return Err(format!(
                "funding view is not a prefix of the tape at {}",
                last.funding_time
            ));
        }
    }
    let start = view.len().saturating_sub(cap);
    Ok(&all[start..view.len()])
}

/// The last `cap` closed bars. `drive` only walks this window.
struct RollingBars {
    cap: usize,
    prefix_len: usize,
    rows: Vec<Observation<ResampledBar>>,
}

impl RollingBars {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            prefix_len: 0,
            rows: Vec::new(),
        }
    }

    fn sync(&mut self, bars: &[ResampledBar]) -> Result<&[Observation<ResampledBar>], String> {
        let Some(latest) = bars.last().copied() else {
            return Err("signal was called with no bar".to_owned());
        };
        if self.prefix_len + 1 == bars.len() {
            self.rows.push(bar_observation(latest)?);
            if self.rows.len() > self.cap {
                let extra = self.rows.len() - self.cap;
                self.rows.drain(..extra);
            }
        } else if self.prefix_len != bars.len() {
            self.rows.clear();
            let start = bars.len().saturating_sub(self.cap);
            for bar in &bars[start..] {
                self.rows.push(bar_observation(*bar)?);
            }
        }
        self.prefix_len = bars.len();
        Ok(&self.rows)
    }
}

fn bar_observation(bar: ResampledBar) -> Result<Observation<ResampledBar>, String> {
    let close = bar.close_time();
    let available = close
        .checked_add_micros(1_000)
        .ok_or("bar availability overflow")?;
    Ok(Observation::new(
        Timestamps {
            event_ts: EventTs::from_micros(close),
            available_ts: AvailableTs::from_micros(available),
            received_ts: ReceivedTs::from_micros(available),
        },
        bar,
    ))
}

fn date(text: &str) -> Result<UtcDate, String> {
    UtcDate::parse(text).ok_or_else(|| format!("bad date {text}"))
}

fn parse_args() -> Result<(PathBuf, PathBuf), String> {
    let mut args = env::args().skip(1);
    let root = args
        .next()
        .ok_or("usage: first_table <data-root> <out-dir>")?;
    let out = args
        .next()
        .ok_or("usage: first_table <data-root> <out-dir>")?;
    if args.next().is_some() {
        return Err("usage: first_table <data-root> <out-dir>".to_owned());
    }
    Ok((PathBuf::from(root), PathBuf::from(out)))
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
