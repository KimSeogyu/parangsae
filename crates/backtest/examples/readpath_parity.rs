//! Local check against the v0.3 read path, on dates before the holdout. Not a CI test.
//!
//! Needs the store and `duckdb` on PATH. A missing piece exits with the path
//! that was required.
//!
//! Every scan uses the hive file list with `2020-12-18 <= date < 2025-10-01`.
//! `um_1m_resolved` is that list plus the sha-pinned overlay. This example does
//! not `.read` the full-store SQL: those views glob holdout parquet.
//!
//! ```text
//! cargo run -p parangsae-backtest --example readpath_parity
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output};

use parangsae_backtest::{Overlay, Tape, TapeConfig, UtcDate};
use parangsae_core::{resample_series, Interval, Symbol};

const STORE: &str = "/workspace/data/parangsae";
const FUNDING_ROOT: &str = "/workspace/data/parangsae/runs/funding-4db8a75";
const OVERLAY: &str = "/workspace/data/parangsae/runs/resolutions/um-kline-2023-11-10.parquet";
const OVERLAY_SHA: &str = "d5f6bcd61688cac971b6d276ea4fa0826c86d998b7adc06557686da6bdcfb8ea";
const SAMPLE_START_DATE: &str = "2020-12-18";
const HOLDOUT_DATE: &str = "2025-10-01";

/// In-sample rows. 2,517,120 = 1,748 days × 1,440. The full-store views are larger
/// because they include the holdout (3,581,280 = 2,487 days × 1,440).
const KLINE_1M: i64 = 2_517_120;
const KLINE_1H: i64 = 41_952;
const FUNDING_ROWS: i64 = 5_244;
const MARK_1M: i64 = 2_517_118;

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
    let overlay = PathBuf::from(OVERLAY);
    require_dir(&store)?;
    require_dir(&funding)?;
    require_file(&overlay)?;
    if Command::new("duckdb").arg("-version").output().is_err() {
        return Err("duckdb is not on PATH".to_owned());
    }

    let um_files = in_sample_parts(&store, "um-kline")?;
    let mark_files = in_sample_parts(&store, "mark-price-kline")?;
    let funding_files = in_sample_parts(&funding, "funding-rate")?;
    let (kline_1m, kline_1h, funding_n, mark_1m) =
        view_counts(&um_files, &mark_files, &funding_files, &overlay)?;
    check_count("view kline_1m", kline_1m, KLINE_1M)?;
    check_count("view complete 1h", kline_1h, KLINE_1H)?;
    check_count("view funding", funding_n, FUNDING_ROWS)?;
    check_count("view mark_1m", mark_1m, MARK_1M)?;

    let spec = Overlay {
        dataset: "um-kline".to_owned(),
        path: overlay.clone(),
        sha256: parse_sha(OVERLAY_SHA)?,
    };
    let tape = load(
        &store,
        &funding,
        date("2020-12-18")?,
        date("2025-09-30")?,
        &[spec],
    )?;
    check_count("reader kline_1m", as_i64(tape.bars.len())?, KLINE_1M)?;
    let hours = resample_series(&tape.bars, Interval::ONE_HOUR).map_err(|err| err.to_string())?;
    check_count("reader complete 1h", as_i64(hours.len())?, KLINE_1H)?;
    check_count("reader funding", as_i64(tape.funding.len())?, FUNDING_ROWS)?;
    check_count("reader mark_1m", as_i64(tape.marks.len())?, MARK_1M)?;
    compare_funding_rule(&tape, &funding_files)?;

    compare_hours(&store, &funding, &um_files, &overlay)?;
    compare_day(&store, &funding, &um_files, &overlay, "2021-12-31")?;
    compare_day(&store, &funding, &um_files, &overlay, "2022-01-01")?;
    compare_overlay_day(&store, &funding, &um_files, &overlay)?;

    let (kline_after, hour_after, funding_after, mark_after) =
        view_counts(&um_files, &mark_files, &funding_files, &overlay)?;
    check_count("kline_1m after overlay", kline_after, KLINE_1M)?;
    check_count("complete 1h after overlay", hour_after, KLINE_1H)?;
    check_count("funding after overlay", funding_after, FUNDING_ROWS)?;
    check_count("mark_1m after overlay", mark_after, MARK_1M)?;
    println!("readpath_parity ok");
    println!("kline_1m={kline_1m} complete_1h={kline_1h} funding={funding_n} mark_1m={mark_1m}");
    Ok(())
}

fn view_counts(
    um_files: &[PathBuf],
    mark_files: &[PathBuf],
    funding_files: &[PathBuf],
    overlay: &Path,
) -> Result<(i64, i64, i64, i64), String> {
    let resolved = resolved_sql(um_files, overlay);
    let kline_1m = duck_count_query(&format!("select count(*) from {resolved}"))?;
    let kline_1h = duck_count_query(&format!(
        "select count(*) from (select open_time_us // 3600000000 as hour from {resolved} \
         group by 1 having count(*) = 60)"
    ))?;
    let funding_n = duck_count_query(&format!(
        "select count(*) from read_parquet({})",
        sql_paths(funding_files)
    ))?;
    let mark_1m = duck_count_query(&format!(
        "select count(*) from read_parquet({})",
        sql_paths(mark_files)
    ))?;
    Ok((kline_1m, kline_1h, funding_n, mark_1m))
}

fn resolved_sql(um_files: &[PathBuf], overlay: &Path) -> String {
    format!(
        "(select * from read_parquet({files}) s \
         where s.open_time_us not in (select open_time_us from read_parquet('{overlay}')) \
         union all by name \
         select * exclude (resolution) from read_parquet('{overlay}'))",
        files = sql_paths(um_files),
        overlay = overlay.display(),
    )
}

fn resolved_query(um_files: &[PathBuf], overlay: &Path, body: &str) -> String {
    format!(
        "with um_1m_resolved as {} {body}",
        resolved_sql(um_files, overlay)
    )
}

fn compare_funding_rule(tape: &Tape, files: &[PathBuf]) -> Result<(), String> {
    let rows = duck_exec(&format!(
        "select funding_time_us, available_ts_us, \
         greatest(available_ts_us, funding_time_us + 60000000) \
         from read_parquet({}) order by funding_time_us",
        sql_paths(files)
    ))?;
    if rows.len() != tape.funding.len() {
        return Err(format!(
            "funding rows: reader {} raw {}",
            tape.funding.len(),
            rows.len()
        ));
    }
    let mut lifted = 0i64;
    for (stamp, row) in tape.funding.iter().zip(rows.iter()) {
        let funding_time = parse_i64(&row[0])?;
        let raw = parse_i64(&row[1])?;
        let visible = parse_i64(&row[2])?;
        if stamp.funding_time.micros() != funding_time {
            return Err(format!(
                "funding time: reader {} raw {funding_time}",
                stamp.funding_time.micros()
            ));
        }
        if stamp.available.micros() != visible {
            return Err(format!(
                "funding available at {funding_time}: reader {} max(stored, ft+60s) {visible}",
                stamp.available.micros()
            ));
        }
        if raw != visible {
            lifted += 1;
        }
    }
    if lifted == 0 {
        return Err(
            "funding max(stored, ft+60s) matched raw available on every in-sample row".to_owned(),
        );
    }
    println!("funding visibility matches max(stored, ft+60s); lifted {lifted} rows above raw");
    Ok(())
}

/// `2020-12-18 <= date < 2025-10-01`. Hive directory names sort in that order.
fn in_sample_date(date: &str) -> bool {
    (SAMPLE_START_DATE..HOLDOUT_DATE).contains(&date)
}

fn in_sample_parts(root: &Path, dataset: &str) -> Result<Vec<PathBuf>, String> {
    let dir = root.join("parquet").join(dataset).join("symbol=BTCUSDT");
    if !dir.is_dir() {
        return Err(format!("missing directory {}", dir.display()));
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read {}: {err}", dir.display()))? {
        let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(date) = name.strip_prefix("date=") else {
            continue;
        };
        if !in_sample_date(date) {
            continue;
        }
        let part = entry.path().join("part.parquet");
        if part.is_file() {
            files.push(part);
        }
    }
    if files.is_empty() {
        return Err(format!("no in-sample files in {}", dir.display()));
    }
    files.sort();
    Ok(files)
}

fn sql_paths(files: &[PathBuf]) -> String {
    let mut out = String::from("[");
    for (index, path) in files.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('\'');
        out.push_str(&path.display().to_string().replace('\'', "''"));
        out.push('\'');
    }
    out.push(']');
    out
}

fn compare_hours(
    store: &Path,
    funding: &Path,
    um_files: &[PathBuf],
    overlay: &Path,
) -> Result<(), String> {
    let from = date("2024-01-01")?;
    let to = date("2024-01-31")?;
    let tape = load(store, funding, from, to, &[])?;
    let hours = resample_series(&tape.bars, Interval::ONE_HOUR).map_err(|err| err.to_string())?;
    let rows = duck_exec(&resolved_query(
        um_files,
        overlay,
        "select open_time_us//3600000000*3600000000 as open_time_us, \
         arg_max(close_ticks, open_time_us) as close_ticks \
         from um_1m_resolved \
         where open_time_us >= 1704067200000000 and open_time_us < 1706745600000000 \
         group by 1 having count(*) = 60 order by 1",
    ))?;
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

fn compare_day(
    store: &Path,
    funding: &Path,
    um_files: &[PathBuf],
    overlay: &Path,
    day: &str,
) -> Result<(), String> {
    let date = date(day)?;
    let tape = load(store, funding, date, date, &[])?;
    let start = date.start_us().micros();
    let end = date.next().start_us().micros();
    let um = duck_exec(&resolved_query(
        um_files,
        overlay,
        &format!(
            "select open_time_us, open_ticks, high_ticks, low_ticks, close_ticks, volume_ticks \
             from um_1m_resolved where open_time_us >= {start} and open_time_us < {end} \
             order by open_time_us"
        ),
    ))?;
    let mark_path = store
        .join("parquet/mark-price-kline")
        .join("symbol=BTCUSDT")
        .join(format!("date={day}"))
        .join("part.parquet");
    let marks = duck_exec(&format!(
        "select open_time_us, open_ticks, high_ticks, low_ticks, close_ticks \
         from read_parquet('{}') order by open_time_us",
        mark_path.display()
    ))?;
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
    um_files: &[PathBuf],
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
    let rows = duck_exec(&resolved_query(
        um_files,
        overlay,
        &format!(
            "select open_time_us, close_ticks from um_1m_resolved \
             where open_time_us >= {start} and open_time_us < {end} order by open_time_us"
        ),
    ))?;
    if tape.bars.len() != 1440 || rows.len() != 1440 {
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
    println!("2023-11-10 overlay matches um_1m_resolved 1440/1440");
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

fn duck_count_query(query: &str) -> Result<i64, String> {
    let rows = duck_exec(query)?;
    let cell = rows
        .first()
        .and_then(|row| row.first())
        .ok_or("empty duckdb count")?;
    parse_i64(cell)
}

fn duck_exec(query: &str) -> Result<Vec<Vec<String>>, String> {
    // The resolved view names every in-sample parquet file. That text is past
    // Linux's 128 KiB per-argument limit, so duckdb reads it from a file.
    let path = std::env::temp_dir().join(format!("readpath-parity-{}.sql", std::process::id()));
    let mut script = query.to_owned();
    if !script.trim_end().ends_with(';') {
        script.push(';');
    }
    fs::write(&path, script).map_err(|err| format!("write {}: {err}", path.display()))?;
    let output = Command::new("duckdb")
        .arg("-csv")
        .arg("-c")
        .arg(format!(".read {}", path.display()))
        .output();
    let removed = fs::remove_file(&path);
    let output = output.map_err(|err| format!("duckdb failed to start: {err}"))?;
    removed.map_err(|err| format!("remove {}: {err}", path.display()))?;
    command_rows(output)
}

fn command_rows(output: Output) -> Result<Vec<Vec<String>>, String> {
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

fn as_i64(len: usize) -> Result<i64, String> {
    i64::try_from(len).map_err(|_| format!("count {len} does not fit i64"))
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

#[cfg(test)]
mod tests {
    use super::in_sample_parts;

    #[test]
    fn in_sample_parts_drops_days_before_sample_start_and_from_the_holdout() {
        let dir = tempfile::tempdir().unwrap();
        for date in ["2020-12-17", "2020-12-18", "2025-09-30", "2025-10-01"] {
            let part = dir
                .path()
                .join("parquet/um-kline/symbol=BTCUSDT")
                .join(format!("date={date}"))
                .join("part.parquet");
            std::fs::create_dir_all(part.parent().unwrap()).unwrap();
            std::fs::write(&part, b"").unwrap();
        }
        let files = in_sample_parts(dir.path(), "um-kline").unwrap();
        let dates: Vec<String> = files
            .iter()
            .map(|path| {
                path.parent()
                    .and_then(|dir| dir.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(
            dates,
            vec!["date=2020-12-18".to_owned(), "date=2025-09-30".to_owned()]
        );
    }
}
