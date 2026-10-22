//! Vision backfill. Downloads are parallel. Parsing and writes are sequential.
//! Grid gaps are computed for the requested days after the merge. An archive's
//! row checks cover every row in the file, including rows outside the window.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parangsae_core::{ReceivedTs, Symbol, UtcMicros};
use serde::Serialize;

use crate::civil::{day_of, DayRange};
use crate::fetch::{list_prefix, BlobGet, Pace};
use crate::manifest::{ManifestLog, ObjectManifest};
use crate::store::{self, WriteCounts};
use crate::vision::basename;
use crate::vision::{
    archive_clock_day, book_day_gaps, book_row_faults, classify, collapse_by, download_url,
    funding_window, kline_day_gaps, kline_row_faults, metrics_day_gaps, metrics_row_faults,
    missing_coverage, parse_book, parse_checksum, parse_funding, parse_klines, parse_metrics,
    require_sha256, sha256_bytes, unzip_csv, ArchiveName, Cadence, Classified, FaultReport, Gap,
    VisionDataset,
};
use crate::Error;

#[derive(Clone, Debug)]
pub struct RunRequest {
    pub datasets: Vec<VisionDataset>,
    pub symbol: Symbol,
    pub range: DayRange,
    pub data_dir: PathBuf,
    pub jobs: usize,
    pub min_interval: Duration,
    pub fetched_at: ReceivedTs,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunSummary {
    pub rows_inserted: u64,
    pub rows_unchanged: u64,
    pub gaps: u64,
    pub allowlisted_gaps: u64,
    pub conflicts: u64,
    pub faults: u64,
    pub quarantined: u64,
    pub archives: u64,
    pub first_fault: Option<String>,
}

impl RunSummary {
    pub fn ok(&self) -> bool {
        self.faults == 0 && self.conflicts == 0 && self.gaps == 0
    }

    pub fn add_counts(&mut self, counts: WriteCounts) {
        self.rows_inserted += counts.inserted;
        self.rows_unchanged += counts.unchanged;
        self.conflicts += counts.conflicts;
        if counts.conflicts > 0 && self.first_fault.is_none() {
            self.first_fault = Some("conflict: stored payload was left unchanged".into());
        }
    }

    pub fn note_fault(&mut self, fault: impl Into<String>) {
        self.faults += 1;
        if self.first_fault.is_none() {
            self.first_fault = Some(fault.into());
        }
    }

    fn add_gap(&mut self, gap: &Gap) {
        if gap.kind.fails_command() {
            self.gaps += 1;
            if self.first_fault.is_none() {
                self.first_fault = Some(format!("{} {}", gap.kind.as_str(), gap.detail));
            }
        } else {
            self.allowlisted_gaps += 1;
        }
    }
}

impl std::fmt::Display for RunSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rows_inserted={} rows_unchanged={} gaps={} allowlisted_gaps={} conflicts={} faults={} quarantined={} archives={}",
            self.rows_inserted,
            self.rows_unchanged,
            self.gaps,
            self.allowlisted_gaps,
            self.conflicts,
            self.faults,
            self.quarantined,
            self.archives
        )?;
        if let Some(fault) = &self.first_fault {
            write!(f, " first_fault={fault}")?;
        }
        Ok(())
    }
}

pub fn received_now() -> Result<ReceivedTs, Error> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| Error::check("clock", err.to_string()))?;
    let micros = i64::try_from(elapsed.as_micros())
        .map_err(|_| Error::check("clock", "clock does not fit"))?;
    Ok(ReceivedTs::from_micros(UtcMicros::from_micros(micros)))
}

pub fn run_vision<G: BlobGet>(
    request: &RunRequest,
    getter: &G,
) -> Result<Vec<(VisionDataset, RunSummary)>, Error> {
    if request.jobs == 0 {
        return Err(Error::Usage("jobs must be at least 1".into()));
    }
    let mut out = Vec::new();
    for dataset in &request.datasets {
        out.push((*dataset, run_dataset(request, *dataset, getter)?));
    }
    Ok(out)
}

struct Planned {
    key: String,
    size: u64,
    name: ArchiveName,
    checksum_key: String,
    checksum_size: u64,
}

struct Fetched {
    sha256: [u8; 32],
    size: u64,
    previous: Option<[u8; 32]>,
}

#[derive(Clone, Copy)]
struct DayStatus {
    listed: bool,
    accepted: bool,
}

fn run_dataset<G: BlobGet>(
    request: &RunRequest,
    dataset: VisionDataset,
    getter: &G,
) -> Result<RunSummary, Error> {
    let pace = Pace::new(request.min_interval);
    let mut summary = RunSummary::default();
    let mut manifest = ManifestLog::open(&request.data_dir)?;
    let mut planned = Vec::new();
    let mut quarantined = Vec::new();
    for (cadence, prefix) in dataset.prefixes(request.symbol.as_str()) {
        let _ = cadence;
        let listed = list_prefix(
            &Paced {
                inner: getter,
                pace: &pace,
            },
            &prefix,
        )?;
        let mut checksums: BTreeMap<String, (String, u64)> = BTreeMap::new();
        let mut archives = Vec::new();
        for object in listed {
            let file_name = basename(&object.key);
            match classify(dataset, request.symbol.as_str(), file_name)? {
                Classified::Archive(name) => archives.push((object, name)),
                Classified::Checksum { zip_file_name } => {
                    checksums.insert(zip_file_name, (object.key, object.size));
                }
                Classified::Quarantine { reason } => {
                    quarantined.push((object.key, reason));
                }
            }
        }
        for (object, name) in archives {
            if !overlaps(&name, request.range) {
                continue;
            }
            let file_name = name.file_name.clone();
            match checksums.remove(&file_name) {
                Some((checksum_key, checksum_size)) => planned.push(Planned {
                    key: object.key,
                    size: object.size,
                    name,
                    checksum_key,
                    checksum_size,
                }),
                None => {
                    summary.note_fault(format!("checksum missing for {}", object.key));
                    append_failure(
                        &request.data_dir,
                        &object.key,
                        "checksum",
                        "listing has no checksum",
                    )?;
                }
            }
        }
    }
    for (key, reason) in &quarantined {
        append_quarantine(&request.data_dir, key, reason)?;
    }
    summary.quarantined = quarantined.len() as u64;
    planned.sort_by_key(|item| sort_key(&item.name));

    let jobs: Vec<FetchJob> = planned
        .iter()
        .map(|item| FetchJob {
            key: item.key.clone(),
            checksum_key: item.checksum_key.clone(),
            listed_size: item.size,
            checksum_size: item.checksum_size,
            previous_sha: manifest.get(&item.key).map(|object| object.sha256),
        })
        .collect();
    let fetched = fetch_all(getter, &pace, &request.data_dir, request.jobs, &jobs);

    let mut daily: BTreeMap<crate::civil::CivilDay, DayStatus> = BTreeMap::new();
    let mut months: BTreeSet<(i32, u8)> = BTreeSet::new();
    let mut funding_gaps: BTreeMap<crate::civil::CivilDay, Vec<Gap>> = BTreeMap::new();

    for (item, result) in planned.iter().zip(fetched) {
        let fetched = match result {
            Ok(fetched) => fetched,
            Err(err) => {
                summary.note_fault(format!("{}: {err}", item.key));
                append_failure(
                    &request.data_dir,
                    &item.key,
                    err.check_name().unwrap_or("fetch"),
                    &err.to_string(),
                )?;
                continue;
            }
        };
        summary.archives += 1;
        if let Some(day) = item.name.day {
            daily.insert(
                day,
                DayStatus {
                    listed: true,
                    accepted: false,
                },
            );
        }
        if item.name.cadence == Cadence::Monthly {
            months.insert((item.name.month.year, item.name.month.month));
        }
        match ingest_one(
            request,
            dataset,
            item,
            &fetched,
            &mut manifest,
            &mut summary,
            &mut funding_gaps,
        ) {
            Ok(accepted) => {
                if accepted {
                    if let Some(day) = item.name.day {
                        daily.insert(
                            day,
                            DayStatus {
                                listed: true,
                                accepted: true,
                            },
                        );
                    }
                }
            }
            Err(err) => {
                summary.note_fault(format!("{}: {err}", item.key));
                append_failure(
                    &request.data_dir,
                    &item.key,
                    err.check_name().unwrap_or("ingest"),
                    &err.to_string(),
                )?;
            }
        }
    }

    finish_coverage(
        request,
        dataset,
        &daily,
        &months,
        &funding_gaps,
        &mut summary,
    )?;
    Ok(summary)
}

fn finish_coverage(
    request: &RunRequest,
    dataset: VisionDataset,
    daily: &BTreeMap<crate::civil::CivilDay, DayStatus>,
    months: &BTreeSet<(i32, u8)>,
    funding_gaps: &BTreeMap<crate::civil::CivilDay, Vec<Gap>>,
    summary: &mut RunSummary,
) -> Result<(), Error> {
    let symbol = request.symbol.as_str();
    let dataset_name = dataset.as_str();
    if dataset == VisionDataset::FundingRate {
        for (year, month) in months_in_range(request.range) {
            let key = (year, month);
            if !months.contains(&key) {
                let first = crate::civil::YearMonth::new(year, month)
                    .ok_or_else(|| Error::check("date", "month"))?
                    .first();
                let gap = missing_coverage(first, false);
                store::write_gaps(
                    &request.data_dir,
                    dataset_name,
                    symbol,
                    first,
                    std::slice::from_ref(&gap),
                )?;
                summary.add_gap(&gap);
            }
        }
        for (day, gaps) in funding_gaps {
            store::write_gaps(&request.data_dir, dataset_name, symbol, *day, gaps)?;
            for gap in gaps {
                summary.add_gap(gap);
            }
        }
        return Ok(());
    }

    let book = dataset == VisionDataset::BookDepth;
    for day in request.range.days() {
        let status = daily.get(&day).copied().unwrap_or(DayStatus {
            listed: false,
            accepted: false,
        });
        if !status.listed {
            let gap = missing_coverage(day, book);
            store::write_gaps(
                &request.data_dir,
                dataset_name,
                symbol,
                day,
                std::slice::from_ref(&gap),
            )?;
            summary.add_gap(&gap);
            continue;
        }
        if !status.accepted {
            continue;
        }
        let gaps = match dataset {
            VisionDataset::UmKline
            | VisionDataset::MarkPriceKline
            | VisionDataset::IndexPriceKline
            | VisionDataset::PremiumIndexKline
            | VisionDataset::SpotKline => kline_day_gaps(
                &store::load_klines(&request.data_dir, dataset_name, symbol, day)?,
                day,
            ),
            VisionDataset::Metrics => metrics_day_gaps(
                &store::load_metrics(&request.data_dir, dataset_name, symbol, day)?,
                day,
            ),
            VisionDataset::BookDepth => book_day_gaps(
                &store::load_book(&request.data_dir, dataset_name, symbol, day)?,
                day,
            ),
            VisionDataset::FundingRate => Vec::new(),
        };
        store::write_gaps(&request.data_dir, dataset_name, symbol, day, &gaps)?;
        for gap in &gaps {
            summary.add_gap(gap);
        }
    }
    Ok(())
}

fn months_in_range(range: DayRange) -> Vec<(i32, u8)> {
    let mut out = Vec::new();
    let mut year = range.from.year();
    let mut month = range.from.month();
    loop {
        out.push((year, month));
        if year == range.to.year() && month == range.to.month() {
            break;
        }
        if month == 12 {
            year += 1;
            month = 1;
        } else {
            month += 1;
        }
    }
    out
}

fn ingest_one(
    request: &RunRequest,
    dataset: VisionDataset,
    item: &Planned,
    fetched: &Fetched,
    manifest: &mut ManifestLog,
    summary: &mut RunSummary,
    funding_gaps: &mut BTreeMap<crate::civil::CivilDay, Vec<Gap>>,
) -> Result<bool, Error> {
    let bytes = fs::read(raw_path(&request.data_dir, &item.key)).map_err(|source| Error::Io {
        path: item.key.clone(),
        source,
    })?;
    let csv = unzip_csv(&bytes)?;
    let object = ObjectManifest {
        url: download_url(&item.key),
        sha256: fetched.sha256,
        size_bytes: fetched.size,
        schema_version: 0,
        path: item.key.clone(),
        dataset: dataset.as_str().to_owned(),
        symbol: request.symbol.as_str().to_owned(),
        row_count: 0,
        fetched_at: request.fetched_at,
    };
    let accepted = match dataset {
        VisionDataset::UmKline
        | VisionDataset::MarkPriceKline
        | VisionDataset::IndexPriceKline
        | VisionDataset::PremiumIndexKline
        | VisionDataset::SpotKline => ingest_klines(
            request,
            dataset,
            item,
            &csv,
            object,
            fetched.previous,
            manifest,
            summary,
        )?,
        VisionDataset::FundingRate => ingest_funding(
            request,
            item,
            &csv,
            object,
            fetched.previous,
            manifest,
            summary,
            funding_gaps,
        )?,
        VisionDataset::Metrics => {
            ingest_metrics(request, &csv, object, fetched.previous, manifest, summary)?
        }
        VisionDataset::BookDepth => {
            ingest_book(request, &csv, object, fetched.previous, manifest, summary)?
        }
    };
    Ok(accepted)
}

#[allow(clippy::too_many_arguments)]
fn ingest_klines(
    request: &RunRequest,
    dataset: VisionDataset,
    item: &Planned,
    csv: &str,
    mut object: ObjectManifest,
    previous: Option<[u8; 32]>,
    manifest: &mut ManifestLog,
    summary: &mut RunSummary,
) -> Result<bool, Error> {
    let day = archive_clock_day(&item.name);
    let clock = dataset
        .clock(day)
        .ok_or_else(|| Error::check("timestamp_unit", "kline dataset has no clock"))?;
    let domain = dataset
        .tick_domain()
        .ok_or_else(|| Error::check("schema", "kline dataset has no tick domain"))?;
    let parsed = match parse_klines(csv, &request.symbol, clock, domain, request.fetched_at) {
        Ok(parsed) => parsed,
        Err(err) => {
            reject(
                request, &item.key, object, previous, manifest, summary, &err,
            )?;
            return Ok(false);
        }
    };
    object.schema_version = parsed.schema_version;
    object.row_count = parsed.rows.len() as u64;
    let faults = kline_row_faults(&parsed.rows, clock.expected());
    if !faults.is_empty() {
        reject_faults(
            request, &item.key, object, previous, manifest, summary, &faults,
        )?;
        return Ok(false);
    }
    let collapsed = match collapse_by(parsed.rows, |row| row.open_time.micros()) {
        Ok(rows) => rows,
        Err(fault) => {
            reject_faults(
                request,
                &item.key,
                object,
                previous,
                manifest,
                summary,
                &[fault],
            )?;
            return Ok(false);
        }
    };
    let windowed: Vec<_> = collapsed
        .into_iter()
        .filter(|row| in_range(row.open_time, request.range))
        .collect();
    summary.add_counts(store::upsert_klines(
        &request.data_dir,
        dataset.as_str(),
        windowed,
    )?);
    manifest.record_stored(object, previous)?;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn ingest_funding(
    request: &RunRequest,
    item: &Planned,
    csv: &str,
    mut object: ObjectManifest,
    previous: Option<[u8; 32]>,
    manifest: &mut ManifestLog,
    summary: &mut RunSummary,
    funding_gaps: &mut BTreeMap<crate::civil::CivilDay, Vec<Gap>>,
) -> Result<bool, Error> {
    let parsed = match parse_funding(csv, &request.symbol, request.fetched_at) {
        Ok(parsed) => parsed,
        Err(err) => {
            reject(
                request, &item.key, object, previous, manifest, summary, &err,
            )?;
            return Ok(false);
        }
    };
    object.schema_version = parsed.schema_version;
    object.row_count = parsed.rows.len() as u64;
    let collapsed = match collapse_by(parsed.rows, |row| row.funding_time.micros()) {
        Ok(rows) => rows,
        Err(fault) => {
            reject_faults(
                request,
                &item.key,
                object,
                previous,
                manifest,
                summary,
                &[fault],
            )?;
            return Ok(false);
        }
    };
    let gaps = match funding_window(&collapsed, item.name.month) {
        Ok(gaps) => gaps,
        Err(faults) => {
            reject_faults(
                request, &item.key, object, previous, manifest, summary, &faults,
            )?;
            return Ok(false);
        }
    };
    for gap in gaps {
        let day = day_of(gap.start)?;
        funding_gaps.entry(day).or_default().push(gap);
    }
    let windowed: Vec<_> = collapsed
        .into_iter()
        .filter(|row| in_range(row.funding_time, request.range))
        .collect();
    summary.add_counts(store::upsert_funding(
        &request.data_dir,
        VisionDataset::FundingRate.as_str(),
        windowed,
    )?);
    manifest.record_stored(object, previous)?;
    Ok(true)
}

fn ingest_metrics(
    request: &RunRequest,
    csv: &str,
    mut object: ObjectManifest,
    previous: Option<[u8; 32]>,
    manifest: &mut ManifestLog,
    summary: &mut RunSummary,
) -> Result<bool, Error> {
    let parsed = match parse_metrics(csv, &request.symbol, request.fetched_at) {
        Ok(parsed) => parsed,
        Err(err) => {
            let path = object.path.clone();
            reject(request, &path, object, previous, manifest, summary, &err)?;
            return Ok(false);
        }
    };
    object.schema_version = parsed.schema_version;
    object.row_count = parsed.rows.len() as u64;
    let faults = metrics_row_faults(&parsed.rows);
    if !faults.is_empty() {
        let path = object.path.clone();
        reject_faults(request, &path, object, previous, manifest, summary, &faults)?;
        return Ok(false);
    }
    let collapsed = match collapse_by(parsed.rows, |row| row.create_time.micros()) {
        Ok(rows) => rows,
        Err(fault) => {
            let path = object.path.clone();
            reject_faults(
                request,
                &path,
                object,
                previous,
                manifest,
                summary,
                &[fault],
            )?;
            return Ok(false);
        }
    };
    let windowed: Vec<_> = collapsed
        .into_iter()
        .filter(|row| in_range(row.create_time, request.range))
        .collect();
    summary.add_counts(store::upsert_metrics(
        &request.data_dir,
        VisionDataset::Metrics.as_str(),
        windowed,
    )?);
    manifest.record_stored(object, previous)?;
    Ok(true)
}

fn ingest_book(
    request: &RunRequest,
    csv: &str,
    mut object: ObjectManifest,
    previous: Option<[u8; 32]>,
    manifest: &mut ManifestLog,
    summary: &mut RunSummary,
) -> Result<bool, Error> {
    let mut parsed = match parse_book(csv, &request.symbol, request.fetched_at) {
        Ok(parsed) => parsed,
        Err(err) => {
            let path = object.path.clone();
            reject(request, &path, object, previous, manifest, summary, &err)?;
            return Ok(false);
        }
    };
    object.schema_version = parsed.schema_version;
    object.row_count = parsed.rows.len() as u64;
    let faults = book_row_faults(&mut parsed.rows);
    if !faults.is_empty() {
        let path = object.path.clone();
        reject_faults(request, &path, object, previous, manifest, summary, &faults)?;
        return Ok(false);
    }
    let collapsed = match collapse_by(parsed.rows, |row| {
        (row.timestamp.micros(), row.percentage.ticks())
    }) {
        Ok(rows) => rows,
        Err(fault) => {
            let path = object.path.clone();
            reject_faults(
                request,
                &path,
                object,
                previous,
                manifest,
                summary,
                &[fault],
            )?;
            return Ok(false);
        }
    };
    let windowed: Vec<_> = collapsed
        .into_iter()
        .filter(|row| in_range(row.timestamp, request.range))
        .collect();
    summary.add_counts(store::upsert_book(
        &request.data_dir,
        VisionDataset::BookDepth.as_str(),
        windowed,
    )?);
    manifest.record_stored(object, previous)?;
    Ok(true)
}

fn reject(
    request: &RunRequest,
    path: &str,
    object: ObjectManifest,
    previous: Option<[u8; 32]>,
    manifest: &mut ManifestLog,
    summary: &mut RunSummary,
    err: &Error,
) -> Result<(), Error> {
    summary.note_fault(format!("{path}: {err}"));
    append_failure(
        &request.data_dir,
        path,
        err.check_name().unwrap_or("parse"),
        &err.to_string(),
    )?;
    manifest.record_rejected(object, previous.unwrap_or([0u8; 32]))?;
    Ok(())
}

fn reject_faults(
    request: &RunRequest,
    path: &str,
    object: ObjectManifest,
    previous: Option<[u8; 32]>,
    manifest: &mut ManifestLog,
    summary: &mut RunSummary,
    faults: &[FaultReport],
) -> Result<(), Error> {
    let first = faults
        .first()
        .map(|fault| format!("{}: {}", fault.name, fault.detail))
        .unwrap_or_else(|| "fault".into());
    summary.faults += faults.len() as u64;
    if summary.first_fault.is_none() {
        summary.first_fault = Some(format!("{path}: {first}"));
    }
    append_failure(
        &request.data_dir,
        path,
        faults.first().map(|fault| fault.name).unwrap_or("fault"),
        &first,
    )?;
    if let Some(previous) = previous {
        manifest.record_rejected(object, previous)?;
    } else {
        manifest.record_rejected(object, [0u8; 32])?;
    }
    Ok(())
}

fn append_failure(data_dir: &Path, path: &str, check: &str, detail: &str) -> Result<(), Error> {
    #[derive(Serialize)]
    struct Line<'a> {
        path: &'a str,
        check: &'a str,
        detail: &'a str,
    }
    append_jsonl(
        &data_dir.join("failures").join("vision.jsonl"),
        &Line {
            path,
            check,
            detail,
        },
    )
}

fn append_quarantine(data_dir: &Path, key: &str, reason: &str) -> Result<(), Error> {
    #[derive(Serialize)]
    struct Line<'a> {
        key: &'a str,
        reason: &'a str,
    }
    append_jsonl(
        &data_dir.join("quarantine").join("names.jsonl"),
        &Line { key, reason },
    )
}

fn append_jsonl(path: &Path, value: &impl Serialize) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let line = serde_json::to_string(value).map_err(|err| Error::check("log", err.to_string()))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| Error::Io {
            path: path.display().to_string(),
            source,
        })?;
    writeln!(file, "{line}").map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(())
}

fn in_range(ts: UtcMicros, range: DayRange) -> bool {
    ts >= range.start_us() && ts < range.end_us()
}

fn overlaps(name: &ArchiveName, range: DayRange) -> bool {
    match name.day {
        Some(day) => range.contains(day),
        None => name.month.overlaps(range),
    }
}

fn sort_key(name: &ArchiveName) -> (u8, i32, u8, u8) {
    let cadence = match name.cadence {
        Cadence::Monthly => 0,
        Cadence::Daily => 1,
    };
    let day = name.day.map(|day| day.day()).unwrap_or(0);
    (cadence, name.month.year, name.month.month, day)
}

struct FetchJob {
    key: String,
    checksum_key: String,
    listed_size: u64,
    checksum_size: u64,
    previous_sha: Option<[u8; 32]>,
}

fn fetch_all<G: BlobGet>(
    getter: &G,
    pace: &Pace,
    data_dir: &Path,
    jobs: usize,
    work: &[FetchJob],
) -> Vec<Result<Fetched, Error>> {
    let results: Vec<Option<Result<Fetched, Error>>> = (0..work.len()).map(|_| None).collect();
    let cursor = Mutex::new(0usize);
    let results = Mutex::new(results);
    thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| loop {
                let index = {
                    let mut cursor = match cursor.lock() {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };
                    if *cursor >= work.len() {
                        return;
                    }
                    let index = *cursor;
                    *cursor += 1;
                    index
                };
                let fetched = fetch_one(getter, pace, data_dir, &work[index]);
                if let Ok(mut slot) = results.lock() {
                    slot[index] = Some(fetched);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_default()
        .into_iter()
        .map(|item| item.unwrap_or_else(|| Err(Error::check("fetch", "download worker stopped"))))
        .collect()
}

fn fetch_one<G: BlobGet>(
    getter: &G,
    pace: &Pace,
    data_dir: &Path,
    job: &FetchJob,
) -> Result<Fetched, Error> {
    pace.wait()?;
    let checksum_bytes = getter.get(&download_url(&job.checksum_key))?;
    if checksum_bytes.len() as u64 != job.checksum_size {
        return Err(Error::check(
            "size",
            format!(
                "{} checksum listed {} bytes, downloaded {}",
                job.key,
                job.checksum_size,
                checksum_bytes.len()
            ),
        ));
    }
    let checksum_text = String::from_utf8(checksum_bytes.clone())
        .map_err(|err| Error::check("checksum", err.to_string()))?;
    let expected = parse_checksum(&checksum_text, basename(&job.key))?;
    let dest = raw_path(data_dir, &job.key);
    if dest.exists() {
        let local = fs::read(&dest).map_err(|source| Error::Io {
            path: dest.display().to_string(),
            source,
        })?;
        if local.len() as u64 == job.listed_size && sha256_bytes(&local) == expected {
            write_bytes(&checksum_path(&dest), &checksum_bytes)?;
            return Ok(Fetched {
                sha256: expected,
                size: local.len() as u64,
                previous: job.previous_sha.filter(|sha| *sha != expected),
            });
        }
        preserve(data_dir, &sha256_bytes(&local), &job.key, &local)?;
    }
    pace.wait()?;
    let bytes = getter.get(&download_url(&job.key))?;
    if bytes.len() as u64 != job.listed_size {
        return Err(Error::check(
            "size",
            format!(
                "{} listed {} bytes, downloaded {}",
                job.key,
                job.listed_size,
                bytes.len()
            ),
        ));
    }
    require_sha256(&bytes, expected)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    write_bytes(&dest, &bytes)?;
    write_bytes(&checksum_path(&dest), &checksum_bytes)?;
    Ok(Fetched {
        sha256: expected,
        size: bytes.len() as u64,
        previous: job.previous_sha.filter(|sha| *sha != expected),
    })
}

fn preserve(data_dir: &Path, sha: &[u8; 32], key: &str, bytes: &[u8]) -> Result<(), Error> {
    let path = data_dir
        .join("raw-history")
        .join(hex::encode(sha))
        .join(key);
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    write_bytes(&path, bytes)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    fs::write(path, bytes).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })
}

fn raw_path(data_dir: &Path, key: &str) -> PathBuf {
    data_dir.join("raw").join(key)
}

fn checksum_path(zip_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.CHECKSUM", zip_path.display()))
}

struct Paced<'a, G> {
    inner: &'a G,
    pace: &'a Pace,
}

impl<G: BlobGet> BlobGet for Paced<'_, G> {
    fn get(&self, url: &str) -> Result<Vec<u8>, Error> {
        self.pace.wait()?;
        self.inner.get(url)
    }
}

pub fn verify(data_dir: &Path) -> Result<RunSummary, Error> {
    let manifest = ManifestLog::open(data_dir)?;
    let mut summary = RunSummary::default();
    for object in manifest.active() {
        let path = raw_path(data_dir, &object.path);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(source) => {
                summary.note_fault(format!("io {}: {source}", object.path));
                continue;
            }
        };
        if bytes.len() as u64 != object.size_bytes {
            summary.note_fault(format!("size {}", object.path));
            continue;
        }
        if let Err(err) = require_sha256(&bytes, object.sha256) {
            summary.note_fault(format!("{err}"));
            continue;
        }
        let checksum_text = match fs::read_to_string(checksum_path(&path)) {
            Ok(text) => text,
            Err(source) => {
                summary.note_fault(format!("io checksum {}: {source}", object.path));
                continue;
            }
        };
        match parse_checksum(&checksum_text, basename(&object.path)) {
            Ok(digest) if digest == object.sha256 => {}
            Ok(_) => {
                summary.note_fault(format!(
                    "checksum {} does not match the manifest",
                    object.path
                ));
                continue;
            }
            Err(err) => {
                summary.note_fault(err.to_string());
                continue;
            }
        }
        let dataset = match VisionDataset::parse(&object.dataset) {
            Some(dataset) => dataset,
            None => {
                summary.note_fault(format!("schema {}", object.dataset));
                continue;
            }
        };
        let symbol = match Symbol::new(&object.symbol) {
            Ok(symbol) => symbol,
            Err(err) => {
                summary.note_fault(err.to_string());
                continue;
            }
        };
        let Classified::Archive(name) = classify(dataset, symbol.as_str(), basename(&object.path))?
        else {
            summary.note_fault(format!("filename {}", object.path));
            continue;
        };
        let csv = match unzip_csv(&bytes) {
            Ok(csv) => csv,
            Err(err) => {
                summary.note_fault(err.to_string());
                continue;
            }
        };
        verify_archive(
            data_dir,
            dataset,
            &symbol,
            &name,
            &csv,
            object,
            &mut summary,
        )?;
        summary.archives += 1;
    }
    Ok(summary)
}

fn verify_archive(
    data_dir: &Path,
    dataset: VisionDataset,
    symbol: &Symbol,
    name: &ArchiveName,
    csv: &str,
    object: &ObjectManifest,
    summary: &mut RunSummary,
) -> Result<(), Error> {
    let fetched_at = object.fetched_at;
    match dataset {
        VisionDataset::UmKline
        | VisionDataset::MarkPriceKline
        | VisionDataset::IndexPriceKline
        | VisionDataset::PremiumIndexKline
        | VisionDataset::SpotKline => {
            let clock = dataset
                .clock(archive_clock_day(name))
                .ok_or_else(|| Error::check("timestamp_unit", "clock"))?;
            let domain = dataset
                .tick_domain()
                .ok_or_else(|| Error::check("schema", "domain"))?;
            let parsed = parse_klines(csv, symbol, clock, domain, fetched_at)?;
            if parsed.rows.len() as u64 != object.row_count {
                summary.note_fault(format!("manifest_row_count {}", object.path));
            }
            let faults = kline_row_faults(&parsed.rows, clock.expected());
            if !faults.is_empty() {
                summary.note_fault(format!("{} {}", object.path, faults[0].name));
                return Ok(());
            }
            for row in parsed.rows {
                let day = day_of(row.open_time)?;
                let path = store::fact_path(data_dir, dataset.as_str(), symbol.as_str(), day);
                if !path.exists() {
                    continue;
                }
                let stored = store::load_klines(data_dir, dataset.as_str(), symbol.as_str(), day)?;
                match stored.iter().find(|kept| kept.open_time == row.open_time) {
                    Some(kept) if kept == &row => {}
                    Some(_) => summary.note_fault(format!("conflict {}", row.open_time.micros())),
                    None => summary
                        .note_fault(format!("missing parquet row {}", row.open_time.micros())),
                }
            }
        }
        VisionDataset::FundingRate => {
            let parsed = parse_funding(csv, symbol, fetched_at)?;
            if parsed.rows.len() as u64 != object.row_count {
                summary.note_fault(format!("manifest_row_count {}", object.path));
            }
            let gaps = match funding_window(&parsed.rows, name.month) {
                Ok(gaps) => gaps,
                Err(faults) => {
                    summary.note_fault(format!("{} {}", object.path, faults[0].name));
                    return Ok(());
                }
            };
            let stored_gaps = gaps_by_day(&gaps)?;
            for (day, expected) in stored_gaps {
                let actual = store::load_gaps(data_dir, dataset.as_str(), symbol.as_str(), day)?;
                if actual != expected {
                    summary.note_fault(format!("funding_slot gap mismatch {day}"));
                }
            }
        }
        VisionDataset::Metrics => {
            let parsed = parse_metrics(csv, symbol, fetched_at)?;
            if parsed.rows.len() as u64 != object.row_count {
                summary.note_fault(format!("manifest_row_count {}", object.path));
            }
            let faults = metrics_row_faults(&parsed.rows);
            if !faults.is_empty() {
                summary.note_fault(format!("{} {}", object.path, faults[0].name));
            }
        }
        VisionDataset::BookDepth => {
            let mut parsed = parse_book(csv, symbol, fetched_at)?;
            if parsed.rows.len() as u64 != object.row_count {
                summary.note_fault(format!("manifest_row_count {}", object.path));
            }
            let faults = book_row_faults(&mut parsed.rows);
            if !faults.is_empty() {
                summary.note_fault(format!("{} {}", object.path, faults[0].name));
            }
        }
    }
    Ok(())
}

fn gaps_by_day(gaps: &[Gap]) -> Result<BTreeMap<crate::civil::CivilDay, Vec<Gap>>, Error> {
    let mut grouped = BTreeMap::new();
    for gap in gaps {
        let day = day_of(gap.start)?;
        grouped
            .entry(day)
            .or_insert_with(Vec::new)
            .push(gap.clone());
    }
    Ok(grouped)
}
