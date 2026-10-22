//! `migrate-receipts`. The default is a dry run and writes nothing.
//! `--apply` is for a temp fixture in tests, or for the repo owner on a real
//! data directory. This module never selects a data directory of its own.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use parangsae_core::{AvailableTs, Receipt, RunId, UtcMicros};
use sha2::{Digest, Sha256};

use crate::civil::YearMonth;
use crate::manifest::lookup_legacy_run;
use crate::run::{self, Provenance, RunCommand};
use crate::store::{self, fact_path};
use crate::version::legacy::{self, V1Partition};
use crate::vision::{
    book_row_faults, funding_window, kline_row_faults, metrics_row_faults, BookFact, FaultReport,
    FundingFact, KlineFact, MetricsFact, FUNDING_AVAILABLE_LAG_US,
};
use crate::Error;
use parangsae_core::TimeUnit;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssignedRun {
    pub fetched_at_us: i64,
    pub run_id: Option<RunId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FundingAvailabilityChange {
    pub path: String,
    pub funding_time_us: i64,
    pub from_us: i64,
    pub to_us: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationReport {
    pub v1_partitions: Vec<String>,
    pub assignments: Vec<AssignedRun>,
    pub funding_changes: Vec<FundingAvailabilityChange>,
}

impl std::fmt::Display for MigrationReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "partitions={}", self.v1_partitions.len())?;
        for path in &self.v1_partitions {
            writeln!(f, "v1 {path}")?;
        }
        for row in &self.assignments {
            match row.run_id {
                Some(run) => writeln!(f, "received {} -> {}", row.fetched_at_us, run.to_hex())?,
                None => writeln!(f, "received {} -> new", row.fetched_at_us)?,
            }
        }
        for change in &self.funding_changes {
            writeln!(
                f,
                "funding {} slot {} available {} -> {}",
                change.path, change.funding_time_us, change.from_us, change.to_us
            )?;
        }
        Ok(())
    }
}

pub fn migrate_receipts(
    data_dir: &Path,
    apply: bool,
    provenance: &Provenance,
) -> Result<MigrationReport, Error> {
    let paths = legacy::v1_partitions(data_dir)?;
    let mut decoded = Vec::with_capacity(paths.len());
    for path in &paths {
        let (dataset, _, _) = legacy::partition_location(data_dir, path)?;
        decoded.push(legacy::read_v1(path, &dataset)?);
    }
    let mut values = BTreeSet::new();
    for partition in &decoded {
        values.extend(partition.received_ts_us().iter().copied());
    }
    values.extend(legacy_object_times(data_dir)?);

    let mut assignments = Vec::with_capacity(values.len());
    let mut runs = BTreeMap::new();
    for value in &values {
        let existing = lookup_legacy_run(data_dir, *value)?;
        if let Some(run) = existing {
            runs.insert(*value, run);
            assignments.push(AssignedRun {
                fetched_at_us: *value,
                run_id: Some(run),
            });
        } else if apply {
            let run = run::generate_run_id()?;
            append_legacy_map(data_dir, *value, run)?;
            run::append_run_log(
                data_dir,
                run,
                UtcMicros::from_micros(*value),
                provenance,
                RunCommand::MigrateReceipts,
            )?;
            runs.insert(*value, run);
            assignments.push(AssignedRun {
                fetched_at_us: *value,
                run_id: Some(run),
            });
        } else {
            assignments.push(AssignedRun {
                fetched_at_us: *value,
                run_id: None,
            });
        }
    }

    let mut funding_changes = Vec::new();
    for (path, partition) in paths.iter().zip(decoded.iter()) {
        if let V1Partition::Funding { rows, .. } = partition {
            for row in rows {
                let to_us = funding_available_us(row.funding_time)?;
                let from_us = row.available_ts.micros().micros();
                if from_us != to_us {
                    funding_changes.push(FundingAvailabilityChange {
                        path: path.display().to_string(),
                        funding_time_us: row.funding_time.micros(),
                        from_us,
                        to_us,
                    });
                }
            }
        }
    }

    if apply {
        for (path, partition) in paths.iter().zip(decoded) {
            rewrite_partition(data_dir, path, partition, &runs)?;
        }
    }

    Ok(MigrationReport {
        v1_partitions: paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
        assignments,
        funding_changes,
    })
}

fn rewrite_partition(
    data_dir: &Path,
    path: &Path,
    partition: V1Partition,
    runs: &BTreeMap<i64, RunId>,
) -> Result<(), Error> {
    let (dataset, symbol, day) = legacy::partition_location(data_dir, path)?;
    let target = fact_path(data_dir, &dataset, &symbol, day);
    match partition {
        V1Partition::Klines {
            dataset,
            mut rows,
            received_ts_us,
        } => {
            assign_receipts(&mut rows, &received_ts_us, runs)?;
            let unit = kline_unit(&rows)?;
            refuse(&kline_row_faults(&rows, unit))?;
            store::write_klines(&target, &dataset, &rows)
        }
        V1Partition::Funding {
            mut rows,
            received_ts_us,
        } => {
            assign_receipts(&mut rows, &received_ts_us, runs)?;
            for row in &mut rows {
                let available = funding_available_us(row.funding_time)?;
                row.available_ts = AvailableTs::from_micros(UtcMicros::from_micros(available));
            }
            let month = YearMonth::new(day.year(), day.month())
                .ok_or_else(|| Error::check("funding_slot", format!("no month for {day}")))?;
            match funding_window(&rows, month) {
                Ok(_) => store::write_funding(&target, &rows),
                Err(faults) => refuse(&faults),
            }
        }
        V1Partition::Metrics {
            mut rows,
            received_ts_us,
        } => {
            assign_receipts(&mut rows, &received_ts_us, runs)?;
            refuse(&metrics_row_faults(&rows))?;
            store::write_metrics(&target, &rows)
        }
        V1Partition::Book {
            mut rows,
            received_ts_us,
        } => {
            assign_receipts(&mut rows, &received_ts_us, runs)?;
            refuse(&book_row_faults(&mut rows))?;
            store::write_book(&target, &rows)
        }
    }
}

fn assign_receipts<T: ReceiptRow>(
    rows: &mut [T],
    received_ts_us: &[i64],
    runs: &BTreeMap<i64, RunId>,
) -> Result<(), Error> {
    if rows.len() != received_ts_us.len() {
        return Err(Error::check(
            "schema",
            "v1 row count does not match received_ts_us",
        ));
    }
    for (row, received) in rows.iter_mut().zip(received_ts_us.iter()) {
        let run = runs.get(received).copied().ok_or_else(|| {
            Error::check(
                "receipt",
                format!("no mapped run for received_ts_us {received}"),
            )
        })?;
        row.set_receipt(Receipt::Archive {
            fetched_at_run: run,
        });
    }
    Ok(())
}

trait ReceiptRow {
    fn set_receipt(&mut self, receipt: Receipt);
}

impl ReceiptRow for KlineFact {
    fn set_receipt(&mut self, receipt: Receipt) {
        self.receipt = receipt;
    }
}

impl ReceiptRow for FundingFact {
    fn set_receipt(&mut self, receipt: Receipt) {
        self.receipt = receipt;
    }
}

impl ReceiptRow for MetricsFact {
    fn set_receipt(&mut self, receipt: Receipt) {
        self.receipt = receipt;
    }
}

impl ReceiptRow for BookFact {
    fn set_receipt(&mut self, receipt: Receipt) {
        self.receipt = receipt;
    }
}

fn funding_available_us(funding_time: UtcMicros) -> Result<i64, Error> {
    funding_time
        .checked_add_micros(FUNDING_AVAILABLE_LAG_US)
        .map(|ts| ts.micros())
        .ok_or_else(|| Error::check("available_ts", "funding_time + 60s overflows"))
}

fn kline_unit(rows: &[KlineFact]) -> Result<TimeUnit, Error> {
    let Some(row) = rows.first() else {
        return Ok(TimeUnit::Millis);
    };
    let delta = row.close_time.micros() - row.open_time.micros();
    if delta == row.interval_us - 1_000 {
        Ok(TimeUnit::Millis)
    } else if delta == row.interval_us - 1 {
        Ok(TimeUnit::Micros)
    } else {
        Err(Error::check(
            "close_time",
            "rewritten kline close_time matches neither source tick",
        ))
    }
}

fn refuse(faults: &[FaultReport]) -> Result<(), Error> {
    match faults.first() {
        Some(fault) => Err(Error::check(fault.name, fault.detail.clone())),
        None => Ok(()),
    }
}

fn append_legacy_map(data_dir: &Path, fetched_at_us: i64, run: RunId) -> Result<(), Error> {
    let path = data_dir.join("manifest").join("legacy-run-map.jsonl");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| Error::Io {
            path: path.display().to_string(),
            source,
        })?;
    writeln!(
        file,
        "{{\"fetched_at_us\":\"{fetched_at_us}\",\"run_id\":\"{}\"}}",
        run.to_hex()
    )
    .map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    file.sync_all().map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(())
}

fn legacy_object_times(data_dir: &Path) -> Result<Vec<i64>, Error> {
    let path = data_dir.join("manifest").join("objects.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut values = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let parsed: serde_json::Value = serde_json::from_str(line).map_err(|err| {
            Error::check(
                "manifest",
                format!("{} line {}: {err}", path.display(), index + 1),
            )
        })?;
        let missing_run = match parsed.get("run_id") {
            None | Some(serde_json::Value::Null) => true,
            Some(_) => false,
        };
        if !missing_run {
            continue;
        }
        let fetched = parsed
            .get("fetched_at_us")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| {
                Error::check(
                    "manifest",
                    format!(
                        "{} line {} has no numeric fetched_at_us",
                        path.display(),
                        index + 1
                    ),
                )
            })?;
        values.push(fetched);
    }
    Ok(values)
}

/// SHA-256 of every file under `root`, in relative-path order.
pub fn directory_digest(root: &Path) -> Result<String, Error> {
    let mut files = Vec::new();
    if root.exists() {
        collect_files(root, root, &mut files)?;
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(bytes);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn collect_files(dir: &Path, root: &Path, files: &mut Vec<(String, Vec<u8>)>) -> Result<(), Error> {
    let entries = fs::read_dir(dir).map_err(|source| Error::Io {
        path: dir.display().to_string(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::Io {
            path: dir.display().to_string(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, root, files)?;
            continue;
        }
        let relative = path.strip_prefix(root).map_err(|_| {
            Error::check(
                "schema",
                format!("{} is outside {}", path.display(), root.display()),
            )
        })?;
        let relative = relative
            .to_str()
            .ok_or_else(|| Error::check("schema", format!("{} is not utf-8", path.display())))?;
        let bytes = fs::read(&path).map_err(|source| Error::Io {
            path: path.display().to_string(),
            source,
        })?;
        files.push((relative.replace('\\', "/"), bytes));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::day_of;
    use crate::normalize::UM_KLINE_CLOCK;
    use crate::store::{fact_path, load_book, load_funding, load_klines, load_metrics};
    use crate::version::legacy::{write_v1_book, write_v1_funding, write_v1_metrics};
    use crate::vision::{
        book_row_faults, parse_book, parse_funding, parse_klines, parse_metrics, TickDomain,
        FUNDING_AVAILABLE_LAG_US,
    };
    use parangsae_core::{Receipt, Symbol};

    fn symbol() -> Symbol {
        Symbol::new("BTCUSDT").unwrap()
    }

    fn receipt() -> Receipt {
        Receipt::Archive {
            fetched_at_run: RunId::from_bytes([1; 16]),
        }
    }

    fn provenance() -> Provenance {
        Provenance {
            git_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            dirty: false,
        }
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/../../fixtures/vision/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn marker(dir: &Path, relative: &str) {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"keep").unwrap();
    }

    #[test]
    fn migration_dry_run_writes_nothing_and_apply_is_idempotent() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
        let root = dir.path();
        let klines = parse_klines(
            &fixture("um_kline_2021-06-01_headerless.csv"),
            &symbol(),
            UM_KLINE_CLOCK,
            TickDomain::Price,
            receipt(),
            true,
        )
        .map_err(|err| err.to_string())?;
        let kday = day_of(klines.rows[0].open_time).map_err(|err| err.to_string())?;
        let kpath = fact_path(root, "um-kline", "BTCUSDT", kday);
        store::write_v1_klines(&kpath, &klines.rows, &vec![42; klines.rows.len()])
            .map_err(|err| err.to_string())?;

        let mut funding = parse_funding(&fixture("funding_2026-09-01.csv"), &symbol(), receipt())
            .map_err(|err| err.to_string())?;
        for row in &mut funding.rows {
            row.available_ts = AvailableTs::from_micros(row.funding_time);
        }
        let fday = day_of(funding.rows[0].funding_time).map_err(|err| err.to_string())?;
        let fpath = fact_path(root, "funding-rate", "BTCUSDT", fday);
        write_v1_funding(&fpath, &funding.rows, &vec![42; funding.rows.len()])
            .map_err(|err| err.to_string())?;

        let metrics = parse_metrics(
            &fixture("metrics_2026-10-20_unsorted.csv"),
            &symbol(),
            receipt(),
        )
        .map_err(|err| err.to_string())?;
        let mday = day_of(metrics.rows[0].create_time).map_err(|err| err.to_string())?;
        write_v1_metrics(
            &fact_path(root, "metrics", "BTCUSDT", mday),
            &metrics.rows,
            &vec![42; metrics.rows.len()],
        )
        .map_err(|err| err.to_string())?;

        let mut book = parse_book(&fixture("book_2026-10-20_one_ts.csv"), &symbol(), receipt())
            .map_err(|err| err.to_string())?;
        if !book_row_faults(&mut book.rows).is_empty() {
            return Err("book fixture failed row checks".into());
        }
        let bday = day_of(book.rows[0].timestamp).map_err(|err| err.to_string())?;
        write_v1_book(
            &fact_path(root, "book-depth", "BTCUSDT", bday),
            &book.rows,
            &vec![42; book.rows.len()],
        )
        .map_err(|err| err.to_string())?;

        std::fs::create_dir_all(root.join("manifest")).map_err(|err| err.to_string())?;
        std::fs::write(
            root.join("manifest").join("objects.jsonl"),
            "{\"path\":\"p\",\"url\":\"https://data.binance.vision/x\",\"sha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"size_bytes\":1,\"schema_version\":1,\"dataset\":\"um-kline\",\"symbol\":\"BTCUSDT\",\"row_count\":1,\"fetched_at_us\":42,\"event\":\"stored\"}\n{\"path\":\"q\",\"url\":\"https://data.binance.vision/y\",\"sha256\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\",\"size_bytes\":1,\"schema_version\":1,\"dataset\":\"um-kline\",\"symbol\":\"BTCUSDT\",\"row_count\":1,\"fetched_at_us\":99,\"event\":\"stored\"}\n",
        )
        .map_err(|err| err.to_string())?;
        let objects_before = std::fs::read(root.join("manifest").join("objects.jsonl"))
            .map_err(|err| err.to_string())?;
        for relative in [
            "raw/marker.txt",
            "raw-history/marker.txt",
            "conflicts/marker.txt",
            "failures/marker.txt",
            "gaps/marker.txt",
        ] {
            marker(root, relative);
        }
        let before = directory_digest(root).map_err(|err| err.to_string())?;
        let dry = migrate_receipts(root, false, &provenance()).map_err(|err| err.to_string())?;
        if dry.assignments.len() != 2 {
            return Err(format!("assignments {:?}", dry.assignments));
        }
        if dry.assignments.iter().any(|row| row.run_id.is_some()) {
            return Err("dry run assigned a run id".into());
        }
        if dry.funding_changes.len() != funding.rows.len() {
            return Err(format!("funding changes {}", dry.funding_changes.len()));
        }
        let after_dry = directory_digest(root).map_err(|err| err.to_string())?;
        if before != after_dry {
            return Err("dry run changed the directory".into());
        }

        migrate_receipts(root, true, &provenance()).map_err(|err| err.to_string())?;
        if legacy::partition_kind(&kpath).map_err(|err| err.to_string())?
            != legacy::PartitionKind::V2
        {
            return Err("kline partition stayed v1".into());
        }
        let remaining = legacy::v1_partitions(root).map_err(|err| err.to_string())?;
        if let Some(path) = remaining.first() {
            return Err(format!("v1 file remains {}", path.display()));
        }
        let stored_k =
            load_klines(root, "um-kline", "BTCUSDT", kday).map_err(|err| err.to_string())?;
        if stored_k.len() != klines.rows.len() {
            return Err("kline row count".into());
        }
        for (stored, original) in stored_k.iter().zip(klines.rows.iter()) {
            if stored.symbol != original.symbol
                || stored.interval_us != original.interval_us
                || stored.open_time != original.open_time
                || stored.close_time != original.close_time
                || stored.open_ticks != original.open_ticks
                || stored.high_ticks != original.high_ticks
                || stored.low_ticks != original.low_ticks
                || stored.close_ticks != original.close_ticks
                || stored.volume != original.volume
                || stored.quote_volume != original.quote_volume
                || stored.count != original.count
                || stored.taker_buy_volume != original.taker_buy_volume
                || stored.taker_buy_quote_volume != original.taker_buy_quote_volume
                || stored.event_ts != original.event_ts
                || stored.available_ts != original.available_ts
                || stored.schema_version != original.schema_version
                || stored.tick_domain != original.tick_domain
                || stored.zero_volume_flat != original.zero_volume_flat
            {
                return Err("kline projection changed".into());
            }
            if stored.receipt.live_received_ts().is_some() {
                return Err("archive receipt exposes a time".into());
            }
        }
        let stored_f =
            load_funding(root, "funding-rate", "BTCUSDT", fday).map_err(|err| err.to_string())?;
        for (stored, original) in stored_f.iter().zip(funding.rows.iter()) {
            if stored.raw_calc_time != original.raw_calc_time
                || stored.funding_time != original.funding_time
                || stored.interval_hours != original.interval_hours
                || stored.rate != original.rate
                || stored.event_ts != original.event_ts
                || stored.schema_version != original.schema_version
            {
                return Err("funding projection changed".into());
            }
            let expected = original.funding_time.micros() + FUNDING_AVAILABLE_LAG_US;
            if stored.available_ts.micros().micros() != expected {
                return Err(format!(
                    "funding available {} != {expected}",
                    stored.available_ts.micros()
                ));
            }
        }
        let stored_m =
            load_metrics(root, "metrics", "BTCUSDT", mday).map_err(|err| err.to_string())?;
        for (stored, original) in stored_m.iter().zip(metrics.rows.iter()) {
            if stored.create_time != original.create_time
                || stored.sum_open_interest != original.sum_open_interest
                || stored.sum_open_interest_value != original.sum_open_interest_value
                || stored.count_toptrader_long_short_ratio
                    != original.count_toptrader_long_short_ratio
                || stored.sum_toptrader_long_short_ratio != original.sum_toptrader_long_short_ratio
                || stored.count_long_short_ratio != original.count_long_short_ratio
                || stored.sum_taker_long_short_vol_ratio != original.sum_taker_long_short_vol_ratio
                || stored.event_ts != original.event_ts
                || stored.available_ts != original.available_ts
                || stored.available_lag_verified != original.available_lag_verified
                || stored.schema_version != original.schema_version
            {
                return Err("metrics projection changed".into());
            }
        }
        let stored_b =
            load_book(root, "book-depth", "BTCUSDT", bday).map_err(|err| err.to_string())?;
        for (stored, original) in stored_b.iter().zip(book.rows.iter()) {
            if stored.timestamp != original.timestamp
                || stored.percentage != original.percentage
                || stored.depth != original.depth
                || stored.notional != original.notional
                || stored.level_schema != original.level_schema
                || stored.event_ts != original.event_ts
                || stored.available_ts != original.available_ts
                || stored.schema_version != original.schema_version
            {
                return Err("book projection changed".into());
            }
        }
        let k_run = stored_k[0].receipt.archive_run();
        let f_run = stored_f[0].receipt.archive_run();
        if k_run.is_none() || k_run != f_run || stored_m[0].receipt.archive_run() != k_run {
            return Err("one old received_ts did not map to one run".into());
        }
        let log = std::fs::read_to_string(root.join("manifest").join("runs.jsonl"))
            .map_err(|err| err.to_string())?;
        let lines: Vec<_> = log.lines().filter(|line| !line.is_empty()).collect();
        if lines.len() != 2 {
            return Err(format!("runs.jsonl lines: {log}"));
        }
        for value in ["42", "99"] {
            let hits = lines
                .iter()
                .filter(|line| line.contains(&format!("\"started_at_us\":\"{value}\"")))
                .count();
            if hits != 1 {
                return Err(format!("{value} appears {hits} times"));
            }
        }
        if lines
            .iter()
            .any(|line| !line.contains("\"command\":\"migrate-receipts\""))
        {
            return Err(format!("command: {log}"));
        }
        let objects_after = std::fs::read(root.join("manifest").join("objects.jsonl"))
            .map_err(|err| err.to_string())?;
        if objects_before != objects_after {
            return Err("objects.jsonl was rewritten".into());
        }
        for relative in [
            "raw/marker.txt",
            "raw-history/marker.txt",
            "conflicts/marker.txt",
            "failures/marker.txt",
            "gaps/marker.txt",
        ] {
            let bytes = std::fs::read(root.join(relative)).map_err(|err| err.to_string())?;
            if bytes != b"keep" {
                return Err(format!("{relative} was touched"));
            }
        }
        let applied = directory_digest(root).map_err(|err| err.to_string())?;
        migrate_receipts(root, true, &provenance()).map_err(|err| err.to_string())?;
        let again = directory_digest(root).map_err(|err| err.to_string())?;
        if applied != again {
            return Err("second apply changed the directory".into());
        }
        Ok(())
    }

    #[test]
    fn apply_refuses_a_funding_row_still_available_before_its_event() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
        let root = dir.path();
        let mut funding = parse_funding(&fixture("funding_2026-09-01.csv"), &symbol(), receipt())
            .map_err(|err| err.to_string())?;
        let row = &mut funding.rows[0];
        let event = row.funding_time.micros() + 120_000_000;
        row.event_ts = parangsae_core::EventTs::from_micros(UtcMicros::from_micros(event));
        row.available_ts = AvailableTs::from_micros(row.funding_time);
        let day = day_of(row.funding_time).map_err(|err| err.to_string())?;
        let path = fact_path(root, "funding-rate", "BTCUSDT", day);
        let kept = std::slice::from_ref(&funding.rows[0]);
        write_v1_funding(&path, kept, &[7]).map_err(|err| err.to_string())?;
        let before = std::fs::read(&path).map_err(|err| err.to_string())?;
        let err = migrate_receipts(root, true, &provenance()).expect_err("must fail");
        if err.check_name() != Some("available_before_event") {
            return Err(format!("check {}", err));
        }
        let after = std::fs::read(&path).map_err(|err| err.to_string())?;
        if before != after {
            return Err("rejected partition was rewritten".into());
        }
        if legacy::partition_kind(&path).map_err(|err| err.to_string())?
            != legacy::PartitionKind::V1
        {
            return Err("rejected partition is no longer v1".into());
        }
        match load_funding(root, "funding-rate", "BTCUSDT", day) {
            Err(err) if err.check_name() == Some("schema") => Ok(()),
            Ok(_) => Err("load accepted the v1 file".into()),
            Err(err) => Err(format!("load check {}", err)),
        }
    }
}
