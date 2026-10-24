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

use parangsae_core::{ReceivedTs, Symbol, TimeUnit, UtcMicros};
use serde::Serialize;

use crate::civil::{day_of, parse_day, CivilDay, DayRange};
use crate::fetch::{list_prefix, BlobGet, Pace};
use crate::manifest::{ManifestLog, ObjectManifest};
use crate::store::{self, WriteCounts};
use crate::vision::basename;
use crate::vision::{
    archive_clock_day, book_day_gaps, book_row_faults, classify, collapse_by, download_url,
    funding_window, kline_day_gaps, kline_row_faults, metrics_day_gaps, metrics_row_faults,
    missing_coverage, parse_book, parse_checksum, parse_funding, parse_klines, parse_metrics,
    require_sha256, sha256_bytes, unzip_csv, ArchiveName, Cadence, Classified, FaultReport, Gap,
    KlineFact, VisionDataset,
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

/// Limits [`verify`] to datasets and inclusive UTC days.
///
/// An empty `datasets` list checks every dataset. A missing `from` or `to`
/// leaves that side of the range open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyFilter {
    pub datasets: Vec<VisionDataset>,
    pub from: Option<CivilDay>,
    pub to: Option<CivilDay>,
}

impl VerifyFilter {
    pub fn all() -> Self {
        Self {
            datasets: Vec::new(),
            from: None,
            to: None,
        }
    }

    /// `datasets` uses the same names as `vision --dataset`, including aliases.
    /// `all` selects every dataset. An unknown name, or `from` after `to`, is
    /// [`Error::Usage`].
    pub fn parse(
        datasets: &[impl AsRef<str>],
        from: Option<&str>,
        to: Option<&str>,
    ) -> Result<Self, Error> {
        let mut selected = Vec::new();
        let mut saw_all = false;
        for name in datasets {
            let name = name.as_ref();
            if name == "all" {
                saw_all = true;
                continue;
            }
            let Some(dataset) = VisionDataset::parse(name) else {
                return Err(Error::Usage(format!("unknown dataset {name}")));
            };
            if !selected.contains(&dataset) {
                selected.push(dataset);
            }
        }
        if saw_all && !selected.is_empty() {
            return Err(Error::Usage(
                "dataset all cannot be combined with another dataset".into(),
            ));
        }
        let datasets = if saw_all { Vec::new() } else { selected };
        let from = from.map(parse_day).transpose()?;
        let to = to.map(parse_day).transpose()?;
        if let (Some(from), Some(to)) = (from, to) {
            let _ = DayRange::new(from, to)?;
        }
        Ok(Self { datasets, from, to })
    }

    fn allows_dataset(&self, dataset: VisionDataset) -> bool {
        self.datasets.is_empty() || self.datasets.contains(&dataset)
    }

    fn contains_day(&self, day: CivilDay) -> bool {
        if let Some(from) = self.from {
            if day < from {
                return false;
            }
        }
        if let Some(to) = self.to {
            if day > to {
                return false;
            }
        }
        true
    }

    fn limits_days(&self) -> bool {
        self.from.is_some() || self.to.is_some()
    }
}

/// Result of one `verify` run. `fault_lines` is every fault the summary counted,
/// in the order they were found. `partition_loads` is one entry per fact-partition
/// read, `dataset/symbol/YYYY-MM-DD`, so a second read of the same partition is visible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    pub summary: RunSummary,
    pub fault_lines: Vec<String>,
    pub datasets_label: String,
    pub days_label: String,
    pub archives_in_scope: u64,
    pub archives_total: u64,
    pub partition_loads: Vec<String>,
}

impl VerifyReport {
    pub fn ok(&self) -> bool {
        self.summary.ok()
    }
}

impl std::fmt::Display for VerifyReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{summary} datasets={datasets}, days={days}, archives={in_scope}/{total} in scope",
            summary = self.summary,
            datasets = self.datasets_label,
            days = self.days_label,
            in_scope = self.archives_in_scope,
            total = self.archives_total,
        )
    }
}

fn datasets_label(datasets: &[VisionDataset]) -> String {
    if datasets.is_empty() {
        "all".to_owned()
    } else {
        datasets
            .iter()
            .map(|dataset| dataset.as_str())
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn days_label(from: Option<CivilDay>, to: Option<CivilDay>) -> String {
    match (from, to) {
        (None, None) => "all".to_owned(),
        (Some(from), Some(to)) => format!("{from}..{to}"),
        (Some(from), None) => format!("{from}.."),
        (None, Some(to)) => format!("..{to}"),
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
        materialized_days: None,
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
    object.materialized_days = Some(
        windowed
            .iter()
            .map(|row| day_of(row.open_time))
            .collect::<Result<_, _>>()?,
    );
    manifest.record_stored(object, previous)?;
    summary.add_counts(store::upsert_klines(
        &request.data_dir,
        dataset.as_str(),
        windowed,
    )?);
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

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PartitionKey {
    dataset: String,
    symbol: String,
    day: CivilDay,
}

#[derive(Default)]
struct PartitionCache {
    pending: BTreeMap<PartitionKey, u32>,
    loaded: BTreeMap<PartitionKey, Option<BTreeMap<i64, KlineFact>>>,
    loads: Vec<String>,
}

enum StoredRow {
    FileMissing,
    Equal,
    Conflict,
    Missing,
}

impl PartitionCache {
    fn compare_kline(
        &mut self,
        data_dir: &Path,
        dataset: &str,
        symbol: &str,
        day: CivilDay,
        row: &KlineFact,
    ) -> Result<StoredRow, Error> {
        let key = PartitionKey {
            dataset: dataset.to_owned(),
            symbol: symbol.to_owned(),
            day,
        };
        if !self.loaded.contains_key(&key) {
            let path = store::fact_path(data_dir, dataset, symbol, day);
            if path.exists() {
                let stored = store::load_klines(data_dir, dataset, symbol, day)?;
                self.loads.push(format!("{dataset}/{symbol}/{day}"));
                let mut index = BTreeMap::new();
                for kept in stored {
                    index.entry(kept.open_time.micros()).or_insert(kept);
                }
                self.loaded.insert(key.clone(), Some(index));
            } else {
                self.loaded.insert(key.clone(), None);
            }
        }
        let Some(index) = self.loaded.get(&key).and_then(Option::as_ref) else {
            return Ok(StoredRow::FileMissing);
        };
        Ok(match index.get(&row.open_time.micros()) {
            Some(kept) if kept == row => StoredRow::Equal,
            Some(_) => StoredRow::Conflict,
            None => StoredRow::Missing,
        })
    }

    fn release_archive(
        &mut self,
        dataset: VisionDataset,
        symbol: &str,
        name: &ArchiveName,
        filter: &VerifyFilter,
    ) {
        if !loads_kline_facts(dataset) {
            return;
        }
        for day in covered_days(name, filter) {
            let key = PartitionKey {
                dataset: dataset.as_str().to_owned(),
                symbol: symbol.to_owned(),
                day,
            };
            let finished = match self.pending.get_mut(&key) {
                Some(left) if *left > 1 => {
                    *left -= 1;
                    false
                }
                Some(left) if *left == 1 => {
                    *left = 0;
                    true
                }
                Some(_) | None => false,
            };
            if finished {
                self.pending.remove(&key);
                self.loaded.remove(&key);
            }
        }
    }
}

struct VerifyOut<'a> {
    summary: &'a mut RunSummary,
    fault_lines: &'a mut Vec<String>,
}

impl VerifyOut<'_> {
    fn fault(&mut self, fault: impl Into<String>) {
        let fault = fault.into();
        self.fault_lines.push(fault.clone());
        self.summary.note_fault(fault);
    }
}

struct VerifyCtx<'a> {
    data_dir: &'a Path,
    filter: &'a VerifyFilter,
    out: VerifyOut<'a>,
    cache: &'a mut PartitionCache,
}

struct Decision {
    in_scope: bool,
    archive: Option<(VisionDataset, ArchiveName)>,
}

pub fn verify(data_dir: &Path, filter: &VerifyFilter) -> Result<VerifyReport, Error> {
    let manifest = ManifestLog::open(data_dir)?;
    let mut summary = RunSummary::default();
    let mut fault_lines = Vec::new();
    let mut cache = PartitionCache::default();
    let mut decisions = Vec::with_capacity(manifest.active().len());
    for object in manifest.active() {
        let decision = decide(object, filter)?;
        if let Some((dataset, name)) = &decision.archive {
            if loads_kline_facts(*dataset) {
                for day in covered_days(name, filter) {
                    let key = PartitionKey {
                        dataset: dataset.as_str().to_owned(),
                        symbol: object.symbol.clone(),
                        day,
                    };
                    *cache.pending.entry(key).or_default() += 1;
                }
            }
        }
        decisions.push(decision);
    }
    let archives_total = decisions.len() as u64;
    let archives_in_scope = decisions
        .iter()
        .filter(|decision| decision.in_scope)
        .count() as u64;
    {
        let mut ctx = VerifyCtx {
            data_dir,
            filter,
            out: VerifyOut {
                summary: &mut summary,
                fault_lines: &mut fault_lines,
            },
            cache: &mut cache,
        };
        for (object, decision) in manifest.active().iter().zip(decisions.iter()) {
            if !decision.in_scope {
                continue;
            }
            let checked = verify_object(&mut ctx, object)?;
            if let Some((dataset, name)) = &decision.archive {
                ctx.cache
                    .release_archive(*dataset, &object.symbol, name, ctx.filter);
            }
            if checked {
                ctx.out.summary.archives += 1;
            }
        }
    }
    Ok(VerifyReport {
        summary,
        fault_lines,
        datasets_label: datasets_label(&filter.datasets),
        days_label: days_label(filter.from, filter.to),
        archives_in_scope,
        archives_total,
        partition_loads: cache.loads,
    })
}

fn verify_object(ctx: &mut VerifyCtx<'_>, object: &ObjectManifest) -> Result<bool, Error> {
    let path = raw_path(ctx.data_dir, &object.path);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(source) => {
            ctx.out.fault(format!("io {}: {source}", object.path));
            return Ok(false);
        }
    };
    if bytes.len() as u64 != object.size_bytes {
        ctx.out.fault(format!("size {}", object.path));
        return Ok(false);
    }
    if let Err(err) = require_sha256(&bytes, object.sha256) {
        ctx.out.fault(format!("{err}"));
        return Ok(false);
    }
    let checksum_text = match fs::read_to_string(checksum_path(&path)) {
        Ok(text) => text,
        Err(source) => {
            ctx.out
                .fault(format!("io checksum {}: {source}", object.path));
            return Ok(false);
        }
    };
    match parse_checksum(&checksum_text, basename(&object.path)) {
        Ok(digest) if digest == object.sha256 => {}
        Ok(_) => {
            ctx.out.fault(format!(
                "checksum {} does not match the manifest",
                object.path
            ));
            return Ok(false);
        }
        Err(err) => {
            ctx.out.fault(err.to_string());
            return Ok(false);
        }
    }
    let dataset = match VisionDataset::parse(&object.dataset) {
        Some(dataset) => dataset,
        None => {
            ctx.out.fault(format!("schema {}", object.dataset));
            return Ok(false);
        }
    };
    let symbol = match Symbol::new(&object.symbol) {
        Ok(symbol) => symbol,
        Err(err) => {
            ctx.out.fault(err.to_string());
            return Ok(false);
        }
    };
    let Classified::Archive(name) = classify(dataset, symbol.as_str(), basename(&object.path))?
    else {
        ctx.out.fault(format!("filename {}", object.path));
        return Ok(false);
    };
    let csv = match unzip_csv(&bytes) {
        Ok(csv) => csv,
        Err(err) => {
            ctx.out.fault(err.to_string());
            return Ok(false);
        }
    };
    verify_archive(ctx, dataset, &symbol, &name, &csv, object)?;
    Ok(true)
}

fn verify_archive(
    ctx: &mut VerifyCtx<'_>,
    dataset: VisionDataset,
    symbol: &Symbol,
    name: &ArchiveName,
    csv: &str,
    object: &ObjectManifest,
) -> Result<(), Error> {
    match dataset {
        VisionDataset::UmKline
        | VisionDataset::MarkPriceKline
        | VisionDataset::IndexPriceKline
        | VisionDataset::PremiumIndexKline
        | VisionDataset::SpotKline => verify_kline_archive(ctx, dataset, symbol, name, csv, object),
        VisionDataset::FundingRate => verify_funding_archive(ctx, symbol, name, csv, object),
        VisionDataset::Metrics => verify_metrics_archive(ctx, symbol, csv, object),
        VisionDataset::BookDepth => verify_book_archive(ctx, symbol, csv, object),
    }
}

fn verify_kline_archive(
    ctx: &mut VerifyCtx<'_>,
    dataset: VisionDataset,
    symbol: &Symbol,
    name: &ArchiveName,
    csv: &str,
    object: &ObjectManifest,
) -> Result<(), Error> {
    let clock = dataset
        .clock(archive_clock_day(name))
        .ok_or_else(|| Error::check("timestamp_unit", "clock"))?;
    let domain = dataset
        .tick_domain()
        .ok_or_else(|| Error::check("schema", "domain"))?;
    let parsed = parse_klines(csv, symbol, clock, domain, object.fetched_at)?;
    if parsed.rows.len() as u64 != object.row_count {
        ctx.out.fault(format!("manifest_row_count {}", object.path));
    }
    let faults = kline_faults_in_scope(&parsed.rows, clock.expected(), ctx.filter)?;
    if !faults.is_empty() {
        ctx.out.fault(format!("{} {}", object.path, faults[0].name));
        return Ok(());
    }
    for row in &parsed.rows {
        let day = day_of(row.open_time)?;
        if !ctx.filter.contains_day(day)
            || object
                .materialized_days
                .as_ref()
                .is_some_and(|days| !days.contains(&day))
        {
            continue;
        }
        match ctx
            .cache
            .compare_kline(ctx.data_dir, dataset.as_str(), symbol.as_str(), day, row)?
        {
            StoredRow::Equal => {}
            StoredRow::Conflict => {
                ctx.out
                    .fault(format!("conflict {}", row.open_time.micros()));
            }
            StoredRow::FileMissing | StoredRow::Missing => {
                ctx.out
                    .fault(format!("missing parquet row {}", row.open_time.micros()));
            }
        }
    }
    Ok(())
}

fn verify_funding_archive(
    ctx: &mut VerifyCtx<'_>,
    symbol: &Symbol,
    name: &ArchiveName,
    csv: &str,
    object: &ObjectManifest,
) -> Result<(), Error> {
    let parsed = parse_funding(csv, symbol, object.fetched_at)?;
    if parsed.rows.len() as u64 != object.row_count {
        ctx.out.fault(format!("manifest_row_count {}", object.path));
    }
    let gaps = match funding_window(&parsed.rows, name.month) {
        Ok(gaps) => gaps,
        Err(faults) => {
            ctx.out.fault(format!("{} {}", object.path, faults[0].name));
            return Ok(());
        }
    };
    let stored_gaps = gaps_by_day(&gaps)?;
    for (day, expected) in stored_gaps {
        if !ctx.filter.contains_day(day) {
            continue;
        }
        let actual = store::load_gaps(
            ctx.data_dir,
            VisionDataset::FundingRate.as_str(),
            symbol.as_str(),
            day,
        )?;
        if actual != expected {
            ctx.out.fault(format!("funding_slot gap mismatch {day}"));
        }
    }
    Ok(())
}

fn verify_metrics_archive(
    ctx: &mut VerifyCtx<'_>,
    symbol: &Symbol,
    csv: &str,
    object: &ObjectManifest,
) -> Result<(), Error> {
    let parsed = parse_metrics(csv, symbol, object.fetched_at)?;
    if parsed.rows.len() as u64 != object.row_count {
        ctx.out.fault(format!("manifest_row_count {}", object.path));
    }
    let faults = if ctx.filter.limits_days() {
        let mut scoped = Vec::new();
        for row in &parsed.rows {
            if ctx.filter.contains_day(day_of(row.create_time)?) {
                scoped.push(row.clone());
            }
        }
        metrics_row_faults(&scoped)
    } else {
        metrics_row_faults(&parsed.rows)
    };
    if !faults.is_empty() {
        ctx.out.fault(format!("{} {}", object.path, faults[0].name));
    }
    Ok(())
}

fn verify_book_archive(
    ctx: &mut VerifyCtx<'_>,
    symbol: &Symbol,
    csv: &str,
    object: &ObjectManifest,
) -> Result<(), Error> {
    let mut parsed = parse_book(csv, symbol, object.fetched_at)?;
    if parsed.rows.len() as u64 != object.row_count {
        ctx.out.fault(format!("manifest_row_count {}", object.path));
    }
    let faults = if ctx.filter.limits_days() {
        let mut scoped = Vec::new();
        for row in &parsed.rows {
            if ctx.filter.contains_day(day_of(row.timestamp)?) {
                scoped.push(row.clone());
            }
        }
        book_row_faults(&mut scoped)
    } else {
        book_row_faults(&mut parsed.rows)
    };
    if !faults.is_empty() {
        ctx.out.fault(format!("{} {}", object.path, faults[0].name));
    }
    Ok(())
}

fn kline_faults_in_scope(
    rows: &[KlineFact],
    unit: TimeUnit,
    filter: &VerifyFilter,
) -> Result<Vec<FaultReport>, Error> {
    if !filter.limits_days() {
        return Ok(kline_row_faults(rows, unit));
    }
    let mut scoped = Vec::new();
    for row in rows {
        if filter.contains_day(day_of(row.open_time)?) {
            scoped.push(row.clone());
        }
    }
    Ok(kline_row_faults(&scoped, unit))
}

fn decide(object: &ObjectManifest, filter: &VerifyFilter) -> Result<Decision, Error> {
    let Some(dataset) = VisionDataset::parse(&object.dataset) else {
        return Ok(Decision {
            in_scope: filter.datasets.is_empty(),
            archive: None,
        });
    };
    if !filter.allows_dataset(dataset) {
        return Ok(Decision {
            in_scope: false,
            archive: None,
        });
    }
    let Ok(symbol) = Symbol::new(&object.symbol) else {
        return Ok(Decision {
            in_scope: true,
            archive: None,
        });
    };
    match classify(dataset, symbol.as_str(), basename(&object.path))? {
        Classified::Archive(name) => {
            let in_scope = coverage_intersects(&name, filter);
            Ok(Decision {
                in_scope,
                archive: in_scope.then_some((dataset, name)),
            })
        }
        Classified::Checksum { .. } | Classified::Quarantine { .. } => Ok(Decision {
            in_scope: true,
            archive: None,
        }),
    }
}

fn coverage_intersects(name: &ArchiveName, filter: &VerifyFilter) -> bool {
    let (start, end) = match name.day {
        Some(day) => (day, day),
        None => (name.month.first(), name.month.last()),
    };
    if let Some(from) = filter.from {
        if end < from {
            return false;
        }
    }
    if let Some(to) = filter.to {
        if start > to {
            return false;
        }
    }
    true
}

fn covered_days(name: &ArchiveName, filter: &VerifyFilter) -> Vec<CivilDay> {
    match name.day {
        Some(day) => {
            if filter.contains_day(day) {
                vec![day]
            } else {
                Vec::new()
            }
        }
        None => {
            let mut days = Vec::new();
            let mut cursor = name.month.first();
            let last = name.month.last();
            loop {
                if filter.contains_day(cursor) {
                    days.push(cursor);
                }
                if cursor == last {
                    break;
                }
                cursor = cursor.next();
            }
            days
        }
    }
}

fn loads_kline_facts(dataset: VisionDataset) -> bool {
    match dataset {
        VisionDataset::UmKline
        | VisionDataset::MarkPriceKline
        | VisionDataset::IndexPriceKline
        | VisionDataset::PremiumIndexKline
        | VisionDataset::SpotKline => true,
        VisionDataset::FundingRate | VisionDataset::Metrics | VisionDataset::BookDepth => false,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_coverage_record_leaves_parquet_untouched() -> Result<(), Error> {
        let dir = tempfile::tempdir().unwrap();
        let day = CivilDay::new(2021, 6, 1).unwrap();
        let dataset = VisionDataset::UmKline;
        let file_name = "BTCUSDT-1m-2021-06.zip";
        let Classified::Archive(name) = classify(dataset, "BTCUSDT", file_name)? else {
            panic!("monthly archive");
        };
        let request = RunRequest {
            datasets: vec![dataset],
            symbol: Symbol::new("BTCUSDT").unwrap(),
            range: DayRange::new(day, day)?,
            data_dir: dir.path().to_path_buf(),
            jobs: 1,
            min_interval: Duration::ZERO,
            fetched_at: ReceivedTs::from_micros(UtcMicros::from_micros(1)),
        };
        let item = Planned {
            key: file_name.to_owned(),
            size: 1,
            name,
            checksum_key: format!("{file_name}.CHECKSUM"),
            checksum_size: 1,
        };
        let object = ObjectManifest {
            url: download_url(file_name),
            sha256: [1; 32],
            size_bytes: 1,
            schema_version: 0,
            path: file_name.to_owned(),
            dataset: dataset.as_str().to_owned(),
            symbol: "BTCUSDT".to_owned(),
            row_count: 0,
            fetched_at: request.fetched_at,
            materialized_days: None,
        };
        let mut manifest = ManifestLog::open(dir.path())?;
        fs::create_dir_all(dir.path().join("manifest/objects.jsonl")).unwrap();
        let open = day.start_us().micros() / 1_000;
        let csv = format!("{open},10,10,10,10,0,{},0,0,0,0,0\n", open + 59_999);
        let result = ingest_klines(
            &request,
            dataset,
            &item,
            &csv,
            object,
            None,
            &mut manifest,
            &mut RunSummary::default(),
        );
        assert!(matches!(result, Err(Error::Io { .. })));
        assert!(!store::fact_path(dir.path(), "um-kline", "BTCUSDT", day).exists());
        Ok(())
    }
}
