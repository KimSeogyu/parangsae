//! Local check against the v0.3 DuckDB views. Not a CI test.
//!
//! Needs the store, `/workspace/reports/research-read-path.sql`, and `duckdb`
//! on PATH. A missing piece exits with the path that was required.
//!
//! ```text
//! cargo run -p parangsae-backtest --example readpath_parity
//! ```

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use parangsae_backtest::{Overlay, Tape, TapeConfig, UtcDate};
use parangsae_core::{resample_series, Interval, Symbol};

const STORE: &str = "/workspace/data/parangsae";
const FUNDING_ROOT: &str = "/workspace/data/parangsae/runs/funding-4db8a75";
const SQL: &str = "/workspace/reports/research-read-path.sql";
const OVERLAY: &str = "/workspace/data/parangsae/runs/resolutions/um-kline-2023-11-10.parquet";
const OVERLAY_SHA: &str = "d5f6bcd61688cac971b6d276ea4fa0826c86d998b7adc06557686da6bdcfb8ea";

const KLINE_1M: i64 = 3_581_280;
const KLINE_1H: i64 = 59_688;
const FUNDING_ROWS: i64 = 7_395;
const MARK_1M: i64 = 3_579_785;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("readpath_parity: {message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let store = PathBuf::from(STORE);
    let funding = PathBuf::from(FUNDING_ROOT);
    let sql = PathBuf::from(SQL);
    let overlay = PathBuf::from(OVERLAY);
    require_dir(&store)?;
    require_dir(&funding)?;
    require_file(&sql)?;
    require_file(&overlay)?;
    if Command::new("duckdb").arg("-version").output().is_err() {
        return Err("duckdb is not on PATH".to_owned());
    }

    let kline_1m = duck_count(&sql, "select count(*) from kline_1m")?;
    let kline_1h = duck_count(&sql, "select count(*) from kline_1h")?;
    let funding_n = duck_count(&sql, "select count(*) from funding")?;
    let mark_1m = duck_count(&sql, "select count(*) from mark_1m")?;
    check_count("kline_1m", kline_1m, KLINE_1M)?;
    check_count("kline_1h", kline_1h, KLINE_1H)?;
    check_count("funding", funding_n, FUNDING_ROWS)?;
    check_count("mark_1m", mark_1m, MARK_1M)?;

    compare_hours(&store, &funding, &sql)?;
    compare_day(&store, &funding, &sql, "2021-12-31")?;
    compare_day(&store, &funding, &sql, "2022-01-01")?;
    compare_overlay_day(&store, &funding, &sql, &overlay)?;

    let kline_after = duck_count(&sql, "select count(*) from kline_1m")?;
    let hour_after = duck_count(&sql, "select count(*) from kline_1h")?;
    check_count("kline_1m after overlay", kline_after, KLINE_1M)?;
    check_count("kline_1h after overlay", hour_after, KLINE_1H)?;
    println!("readpath_parity ok");
    println!("kline_1m={kline_1m} kline_1h={kline_1h} funding={funding_n} mark_1m={mark_1m}");
    Ok(())
}

fn compare_hours(store: &Path, funding: &Path, sql: &Path) -> Result<(), String> {
    let from = date("2024-01-01")?;
    let to = date("2024-01-31")?;
    let tape = load(store, funding, from, to, &[])?;
    let hours = resample_series(&tape.bars, Interval::ONE_HOUR).map_err(|err| err.to_string())?;
    let rows = duck_rows(
        sql,
        "select open_time_us//3600000000*3600000000 as open_time_us, \
         arg_max(close_ticks, open_time_us) as close_ticks \
         from um_1m_resolved \
         where open_time_us >= 1704067200000000 and open_time_us < 1706745600000000 \
         group by 1 having count(*) = 60 order by 1",
    )?;
    if hours.len() != 744 || rows.len() != 744 {
        return Err(format!(
            "2024-01 hours: reader {} duckdb {}",
            hours.len(),
            rows.len()
        ));
    }
    for (hour, row) in hours.iter().zip(rows.iter()) {
        let open = parse_i64(&row[0])?;
        let close = parse_i64(&row[1])?;
        if hour.open_time().micros() != open || hour.close().ticks() != close {
            return Err(format!(
                "2024-01 hour {open}: reader close {} duckdb {close}",
                hour.close().ticks()
            ));
        }
    }
    println!("2024-01 hours 744/744");
    Ok(())
}

fn compare_day(store: &Path, funding: &Path, sql: &Path, day: &str) -> Result<(), String> {
    let date = date(day)?;
    let tape = load(store, funding, date, date, &[])?;
    let start = date.start_us().micros();
    let end = date.next().start_us().micros();
    let um = duck_rows(
        sql,
        &format!(
            "select open_time_us, open_ticks, high_ticks, low_ticks, close_ticks, volume_ticks \
             from um_1m_resolved where open_time_us >= {start} and open_time_us < {end} \
             order by open_time_us"
        ),
    )?;
    let marks = duck_rows(
        sql,
        &format!(
            "select open_time_us, open_ticks, high_ticks, low_ticks, close_ticks \
             from read_parquet('{STORE}/parquet/mark-price-kline/symbol=BTCUSDT/date={day}/part.parquet') \
             order by open_time_us"
        ),
    )?;
    if tape.bars.len() != um.len() {
        return Err(format!(
            "{day} um rows: reader {} duckdb {}",
            tape.bars.len(),
            um.len()
        ));
    }
    for (bar, row) in tape.bars.iter().zip(um.iter()) {
        let values = [
            bar.open_time.micros(),
            bar.open.ticks(),
            bar.high.ticks(),
            bar.low.ticks(),
            bar.close.ticks(),
            bar.volume.ticks(),
        ];
        for (got, want) in values.iter().zip(row.iter()) {
            if *got != parse_i64(want)? {
                return Err(format!("{day} um mismatch at {}", bar.open_time.micros()));
            }
        }
    }
    if tape.marks.len() != marks.len() {
        return Err(format!(
            "{day} mark rows: reader {} duckdb {}",
            tape.marks.len(),
            marks.len()
        ));
    }
    for (bar, row) in tape.marks.iter().zip(marks.iter()) {
        let values = [
            bar.open_time.micros(),
            bar.open.ticks(),
            bar.high.ticks(),
            bar.low.ticks(),
            bar.close.ticks(),
        ];
        for (got, want) in values.iter().zip(row.iter()) {
            if *got != parse_i64(want)? {
                return Err(format!("{day} mark mismatch at {}", bar.open_time.micros()));
            }
        }
    }
    println!("{day} rows match kline_1m/mark_1m sources");
    Ok(())
}

fn compare_overlay_day(
    store: &Path,
    funding: &Path,
    sql: &Path,
    overlay: &Path,
) -> Result<(), String> {
    let day = date("2023-11-10")?;
    let sha = parse_sha(OVERLAY_SHA)?;
    let spec = Overlay {
        dataset: "um-kline".to_owned(),
        path: overlay.to_path_buf(),
        sha256: sha,
    };
    let tape = load(store, funding, day, day, &[spec])?;
    let start = day.start_us().micros();
    let end = day.next().start_us().micros();
    let rows = duck_rows(
        sql,
        &format!(
            "select open_time_us, close_ticks from um_1m_resolved \
             where open_time_us >= {start} and open_time_us < {end} order by open_time_us"
        ),
    )?;
    if tape.bars.len() != rows.len() {
        return Err(format!(
            "2023-11-10 rows: reader {} resolved {}",
            tape.bars.len(),
            rows.len()
        ));
    }
    for (bar, row) in tape.bars.iter().zip(rows.iter()) {
        if bar.open_time.micros() != parse_i64(&row[0])? || bar.close.ticks() != parse_i64(&row[1])?
        {
            return Err(format!(
                "2023-11-10 overlay mismatch at {}",
                bar.open_time.micros()
            ));
        }
    }
    println!("2023-11-10 overlay matches um_1m_resolved");
    Ok(())
}

fn load(
    store: &Path,
    funding: &Path,
    start: UtcDate,
    end: UtcDate,
    overlays: &[Overlay],
) -> Result<Tape, String> {
    let symbol = Symbol::new("BTCUSDT").expect("BTCUSDT");
    let holdout = date("2025-10-01")?.start_us();
    Tape::load(
        store,
        &symbol,
        start,
        end,
        &TapeConfig {
            holdout_start: holdout,
            overlays: overlays.to_vec(),
            funding_root: funding.to_path_buf(),
        },
    )
    .map_err(|err| err.to_string())
}

fn duck_count(sql: &Path, query: &str) -> Result<i64, String> {
    let rows = duck_rows(sql, query)?;
    let cell = rows
        .first()
        .and_then(|row| row.first())
        .ok_or("empty duckdb count")?;
    parse_i64(cell)
}

fn duck_rows(sql: &Path, query: &str) -> Result<Vec<Vec<String>>, String> {
    let output = Command::new("duckdb")
        .arg("-csv")
        .arg("-c")
        .arg(format!(".read {}", sql.display()))
        .arg("-c")
        .arg(query)
        .output()
        .map_err(|err| format!("duckdb failed to start: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "duckdb exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let cells: Vec<String> = line.split(',').map(|cell| cell.trim().to_owned()).collect();
        if cells.first().is_some_and(|cell| parse_i64(cell).is_err()) {
            continue;
        }
        rows.push(cells);
    }
    Ok(rows)
}

fn check_count(name: &str, got: i64, want: i64) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("{name} count {got} != {want}"))
    }
}

fn parse_i64(text: &str) -> Result<i64, String> {
    text.parse::<i64>()
        .map_err(|_| format!("not an integer: {text}"))
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

fn date(text: &str) -> Result<UtcDate, String> {
    UtcDate::parse(text).ok_or_else(|| format!("bad date {text}"))
}

fn require_dir(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(format!("missing directory {}", path.display()))
    }
}

fn require_file(path: &Path) -> Result<(), String> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("missing file {}", path.display()))
    }
}
