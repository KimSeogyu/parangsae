//! The C1 data boundary. Its manifest is a subset of the separately admitted run pin.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{self, File, Metadata},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

use arrow_array::{Array, Date32Array, Int64Array, RecordBatch, StringArray, UInt32Array};
use arrow_schema::DataType;
use parangsae_core::{EventTs, Qty};
use parquet::arrow::{arrow_reader::ParquetRecordBatchReaderBuilder, ProjectionMask};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::*;

const REBUILT: [&str; 6] = [
    "2020-12-21",
    "2021-02-11",
    "2021-04-25",
    "2021-08-13",
    "2021-12-24",
    "2023-03-24",
];
const STATUS_DAYS: [&str; 10] = [
    "2020-12-21",
    "2020-12-25",
    "2021-02-11",
    "2021-03-06",
    "2021-04-20",
    "2021-04-25",
    "2021-08-13",
    "2021-09-29",
    "2021-12-24",
    "2023-03-24",
];
// Exact machine keys from the separately authorized original-source key census.
const CERTIFIED_NORMAL_GAPS: [(&str, i64, i64); 4] = [
    ("2020-12-25", 1_608_861_600_000_000, 60),
    ("2021-03-06", 1_614_996_000_000_000, 90),
    ("2021-04-20", 1_618_884_000_000_000, 150),
    ("2021-09-29", 1_632_898_800_000_000, 120),
];
const MARK_GAP_DAY: &str = "2024-08-12";
const OVERLAY_DAY: &str = "2023-11-10";
const MAX_OBJECT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    #[serde(rename = "spot-trade")]
    Spot,
    #[serde(rename = "um-trade")]
    UmTrade,
    #[serde(rename = "perp-mark")]
    Mark,
    #[serde(rename = "funding")]
    Funding,
    #[serde(rename = "spot-status")]
    SpotStatus,
    #[serde(rename = "um-resolution")]
    Resolution,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Object {
    path: String,
    sha256: String,
    role: Role,
    date: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    source_start_us: i64,
    source_end_us_exclusive: i64,
    symbol: String,
    source_policy_id: String,
    daily_rebuilt_days: Vec<String>,
    spot_status_days: Vec<String>,
    data_objects: Vec<Object>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
    bytes: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
    mode: u32,
}

impl Identity {
    fn from(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            bytes: metadata.len(),
            mtime: (metadata.mtime(), metadata.mtime_nsec()),
            ctime: (metadata.ctime(), metadata.ctime_nsec()),
            mode: metadata.mode(),
        }
    }
}

#[derive(Clone, Debug)]
struct BoundObject {
    object: Object,
    path: PathBuf,
    before: Identity,
}

#[derive(Debug)]
struct DayRows {
    bars: BTreeMap<i64, SourceBar>,
    status: BTreeMap<i64, SourceHealth>,
}

type PriceCache = VecDeque<((PriceSource, i64), DayRows)>;
type FundingCache = VecDeque<(i64, BTreeMap<i64, SourceFunding>)>;

#[derive(Debug)]
pub(super) struct FileSource {
    objects: BTreeMap<(Role, i64), BoundObject>,
    price_cache: RefCell<PriceCache>,
    funding_cache: RefCell<FundingCache>,
}

fn fault(path: impl AsRef<Path>, reason: impl ToString) -> SourceError {
    SourceError::Input {
        path: path.as_ref().display().to_string(),
        reason: reason.to_string(),
    }
}

fn regular(path: &Path) -> Result<Metadata, SourceError> {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|e| fault(ancestor, e))?;
        if metadata.file_type().is_symlink() || (ancestor != path && !metadata.is_dir()) {
            return Err(fault(path, "symlink or non-directory ancestor"));
        }
    }
    let metadata = fs::symlink_metadata(path).map_err(|e| fault(path, e))?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.mode() & 0o222 != 0 {
        return Err(fault(
            path,
            "input must be an independent read-only regular file",
        ));
    }
    Ok(metadata)
}

fn logical(object: &Object) -> Result<i64, SourceError> {
    let path = Path::new(&object.path);
    if path.is_absolute()
        || path
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
        || object.path.contains('\\')
        || object.sha256.len() != 64
        || !object
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(fault(path, "noncanonical logical path or digest"));
    }
    let date = UtcDate::parse(&object.date).ok_or_else(|| fault(path, "date syntax"))?;
    let day = date.start_us().micros();
    // Filename/calendar admission precedes opening or hashing any DATA object.
    if !(SAMPLE..END).contains(&day) {
        return Err(fault(path, "forbidden source date"));
    }
    let expected = match object.role {
        Role::Spot if REBUILT.contains(&object.date.as_str()) => {
            format!("research/c1/spot-valid/symbol=BTCUSDT/date={date}/part.parquet")
        }
        Role::Spot => format!("parquet/spot-kline/symbol=BTCUSDT/date={date}/part.parquet"),
        Role::UmTrade => format!("parquet/um-kline/symbol=BTCUSDT/date={date}/part.parquet"),
        Role::Mark => format!("parquet/mark-price-kline/symbol=BTCUSDT/date={date}/part.parquet"),
        Role::Funding => format!(
            "runs/funding-4db8a75/parquet/funding-rate/symbol=BTCUSDT/date={date}/part.parquet"
        ),
        Role::SpotStatus => {
            format!("research/c1/spot-status/symbol=BTCUSDT/date={date}/status.jsonl")
        }
        Role::Resolution if object.date == OVERLAY_DAY => {
            "runs/resolutions/um-kline-2023-11-10.parquet".into()
        }
        Role::Resolution => return Err(fault(path, "unexpected resolution date")),
    };
    if object.path != expected {
        return Err(fault(path, "role/date/path mapping"));
    }
    Ok(day)
}

impl BoundObject {
    fn open(&self) -> Result<File, SourceError> {
        if Identity::from(&regular(&self.path)?) != self.before {
            return Err(fault(&self.path, "input metadata changed"));
        }
        let mut file = File::open(&self.path).map_err(|e| fault(&self.path, e))?;
        if Identity::from(&file.metadata().map_err(|e| fault(&self.path, e))?) != self.before {
            return Err(fault(&self.path, "opened input identity changed"));
        }
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let n = file.read(&mut buffer).map_err(|e| fault(&self.path, e))?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        if hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            != self.object.sha256
        {
            return Err(fault(&self.path, "input digest mismatch"));
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|e| fault(&self.path, e))?;
        self.unchanged()?;
        Ok(file)
    }

    fn unchanged(&self) -> Result<(), SourceError> {
        if Identity::from(&regular(&self.path)?) != self.before {
            return Err(fault(&self.path, "input changed during read"));
        }
        Ok(())
    }
}

impl FileSource {
    pub(super) fn load(path: &Path) -> Result<Self, SourceError> {
        if !path.is_absolute()
            || path.file_name().and_then(|s| s.to_str()) != Some("c1-source-manifest.json")
        {
            return Err(fault(path, "fixed source manifest filename required"));
        }
        let metadata = regular(path)?;
        if metadata.len() > 4 * 1024 * 1024 {
            return Err(fault(path, "manifest capacity"));
        }
        let before = Identity::from(&metadata);
        let bytes = fs::read(path).map_err(|e| fault(path, e))?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| fault(path, e))?;
        if Identity::from(&regular(path)?) != before {
            return Err(fault(path, "manifest changed"));
        }
        Self::validate_manifest(&manifest, path)?;
        let root = path
            .parent()
            .ok_or_else(|| fault(path, "manifest root missing"))?;
        let mut objects = BTreeMap::new();
        let mut aliases = BTreeSet::new();
        // The entire filename universe is closed before even one DATA hash/read.
        for object in &manifest.data_objects {
            let day = logical(object)?;
            if objects.contains_key(&(object.role, day)) {
                return Err(fault(path, "duplicate role/day"));
            }
            let full = root.join(&object.path);
            let metadata = regular(&full)?;
            if metadata.len() > MAX_OBJECT_BYTES
                || !aliases.insert((metadata.dev(), metadata.ino()))
            {
                return Err(fault(&full, "input capacity or alias"));
            }
            objects.insert(
                (object.role, day),
                BoundObject {
                    object: object.clone(),
                    path: full,
                    before: Identity::from(&metadata),
                },
            );
        }
        let source = Self {
            objects,
            price_cache: RefCell::new(VecDeque::new()),
            funding_cache: RefCell::new(VecDeque::new()),
        };
        let mut day = SAMPLE;
        while day < END {
            for kind in [PriceSource::Spot, PriceSource::UmTrade, PriceSource::UmMark] {
                source.day(kind, day)?;
            }
            let rates = source.funding_day(day)?;
            // First C1 profile: every slot, including flat periods, must support
            // its mandatory receipt at S+60s. No position-dependent admission.
            for row in rates.values() {
                let proxy = source.bar(PriceSource::UmMark, row.settlement.micros())?;
                cash_profile(row, &proxy)?;
            }
            day += DAY;
        }
        Ok(source)
    }

    fn validate_manifest(manifest: &Manifest, path: &Path) -> Result<(), SourceError> {
        if manifest.schema != "parangsae-c1-source-manifest-v1"
            || manifest.symbol != "BTCUSDT"
            || manifest.source_start_us != SAMPLE
            || manifest.source_end_us_exclusive != END
            || manifest.source_policy_id != "C1_ARCHIVED_SPARSE_KEY_CERTIFIED_V2"
            || manifest.daily_rebuilt_days != REBUILT
            || manifest.spot_status_days != STATUS_DAYS
        {
            return Err(fault(path, "unsupported C1 source contract"));
        }
        let mut expected = BTreeSet::new();
        let mut day = SAMPLE;
        while day < END {
            for role in [Role::Spot, Role::UmTrade, Role::Mark, Role::Funding] {
                expected.insert((role, day));
            }
            let date = UtcDate::from_micros(UtcMicros::from_micros(day)).to_string();
            if STATUS_DAYS.contains(&date.as_str()) {
                expected.insert((Role::SpotStatus, day));
            }
            day += DAY;
        }
        expected.insert((
            Role::Resolution,
            UtcDate::parse(OVERLAY_DAY)
                .expect("literal date")
                .start_us()
                .micros(),
        ));
        let mut actual = BTreeSet::new();
        for object in &manifest.data_objects {
            if !actual.insert((object.role, logical(object)?)) {
                return Err(fault(path, "duplicate DATA object"));
            }
        }
        if actual != expected {
            return Err(fault(
                path,
                "DATA universe differs from full preholdout contract",
            ));
        }
        Ok(())
    }

    pub(super) fn start(&self) -> i64 {
        SAMPLE
    }

    fn object(&self, role: Role, day: i64) -> Result<&BoundObject, SourceError> {
        self.objects
            .get(&(role, day))
            .ok_or(SourceError::Invariant("admitted DATA object missing"))
    }

    pub(super) fn bar(&self, source: PriceSource, key: i64) -> Result<SourceBar, SourceError> {
        in_sample(key)?;
        let day = key.div_euclid(DAY) * DAY;
        self.day(source, day)?;
        let cache = self.price_cache.borrow();
        let rows = &cache
            .iter()
            .find(|(k, _)| *k == (source, day))
            .ok_or(SourceError::Invariant("price cache missing"))?
            .1;
        if let Some(health) = rows.status.get(&key) {
            return Err(SourceError::Unavailable {
                source,
                key: UtcMicros::from_micros(key),
                health: *health,
            });
        }
        rows.bars.get(&key).cloned().ok_or(SourceError::Invariant(
            "admitted minute absent without status",
        ))
    }

    fn day(&self, source: PriceSource, day: i64) -> Result<(), SourceError> {
        if self
            .price_cache
            .borrow()
            .iter()
            .any(|(key, _)| *key == (source, day))
        {
            return Ok(());
        }
        let role = match source {
            PriceSource::Spot => Role::Spot,
            PriceSource::UmTrade => Role::UmTrade,
            PriceSource::UmMark => Role::Mark,
        };
        let object = self.object(role, day)?;
        let mut status = BTreeMap::new();
        if source == PriceSource::Spot {
            if let Some(object) = self.objects.get(&(Role::SpotStatus, day)) {
                status = status_rows(object, day)?;
            }
        }
        if source == PriceSource::UmMark
            && UtcDate::from_micros(UtcMicros::from_micros(day)).to_string() == MARK_GAP_DAY
        {
            for minute in [10 * 60 + 2, 10 * 60 + 3] {
                status.insert(day + minute * MINUTE, SourceHealth::Gap);
            }
        }
        let expected = (0..1440)
            .map(|i| day + i * MINUTE)
            .filter(|key| !status.contains_key(key))
            .collect();
        let mut bars = price_rows(object, source, day, false, &expected)?;
        if source == PriceSource::UmTrade {
            if let Some(overlay) = self.objects.get(&(Role::Resolution, day)) {
                let start = day + (15 * 60 + 6) * MINUTE;
                let expected = (0..100).map(|i| start + i * MINUTE).collect();
                let replacements = price_rows(overlay, source, day, true, &expected)?;
                for (key, row) in replacements {
                    let base = bars
                        .get(&key)
                        .ok_or_else(|| fault(&overlay.path, "resolution key absent in base"))?;
                    if base.bar.close_time != row.bar.close_time
                        || base.bar.available != row.bar.available
                    {
                        return Err(fault(&overlay.path, "resolution changes base clock"));
                    }
                    bars.insert(key, row);
                }
            }
        }
        if bars.keys().any(|key| status.contains_key(key))
            || bars.len() + status.len() != 1440
            || (0..1440).any(|i| {
                !bars.contains_key(&(day + i * MINUTE)) && !status.contains_key(&(day + i * MINUTE))
            })
        {
            return Err(fault(
                &object.path,
                "price/status key partition is not exact",
            ));
        }
        let mut cache = self.price_cache.borrow_mut();
        if cache.iter().filter(|(k, _)| k.0 == source).count() == 2 {
            let index = cache
                .iter()
                .position(|(k, _)| k.0 == source)
                .expect("counted cache entry");
            cache.remove(index);
        }
        cache.push_back(((source, day), DayRows { bars, status }));
        Ok(())
    }

    pub(super) fn funding(&self, slot: i64) -> Result<SourceFunding, SourceError> {
        in_sample(slot)?;
        if slot.rem_euclid(SLOT) != 0 {
            return Err(SourceError::Range);
        }
        let day = slot.div_euclid(DAY) * DAY;
        let rows = self.funding_day(day)?;
        rows.get(&slot)
            .cloned()
            .ok_or(SourceError::Invariant("funding semantic key missing"))
    }

    fn funding_day(&self, day: i64) -> Result<BTreeMap<i64, SourceFunding>, SourceError> {
        if let Some((_, rows)) = self
            .funding_cache
            .borrow()
            .iter()
            .find(|(key, _)| *key == day)
        {
            return Ok(rows.clone());
        }
        let rows = funding_rows(self.object(Role::Funding, day)?, day)?;
        let mut cache = self.funding_cache.borrow_mut();
        if cache.len() == 4 {
            cache.pop_front();
        }
        cache.push_back((day, rows.clone()));
        Ok(rows)
    }
}

fn cash_profile(row: &SourceFunding, proxy: &SourceBar) -> Result<(), SourceError> {
    let cut = row
        .settlement
        .micros()
        .checked_add(MINUTE)
        .ok_or(SourceError::Overflow)?;
    let decision = DecisionTime::from_micros(UtcMicros::from_micros(cut));
    if admitted_funding(row, decision)?.is_none() {
        return Err(fault(
            &row.source.object,
            "unsupported funding availability at S+60s",
        ));
    }
    if proxy.bar.open_time != row.settlement
        || ceil_to_ms(proxy.source.original.available_ts.micros())?.micros() > cut
        || proxy.bar.open.ticks() <= 0
    {
        return Err(fault(
            &proxy.source.object,
            "funding mark-open proxy unavailable at S+60s",
        ));
    }
    Ok(())
}

const KLINE_NAMES: [&str; 18] = [
    "symbol",
    "interval_us",
    "open_time_us",
    "close_time_us",
    "open_ticks",
    "high_ticks",
    "low_ticks",
    "close_ticks",
    "volume_ticks",
    "quote_volume_ticks",
    "count",
    "taker_buy_volume_ticks",
    "taker_buy_quote_volume_ticks",
    "event_ts_us",
    "available_ts_us",
    "received_ts_us",
    "schema_version",
    "tick_domain",
];
const FUNDING_NAMES: [&str; 9] = [
    "symbol",
    "raw_calc_time_us",
    "funding_time_us",
    "interval_hours",
    "rate_ticks",
    "event_ts_us",
    "available_ts_us",
    "received_ts_us",
    "schema_version",
];

fn batches(
    object: &BoundObject,
    funding: bool,
    overlay: bool,
    key_only: bool,
) -> Result<Vec<RecordBatch>, SourceError> {
    let file = object.open()?;
    let builder =
        ParquetRecordBatchReaderBuilder::try_new(file).map_err(|e| fault(&object.path, e))?;
    if !(0..=1440).contains(&builder.metadata().file_metadata().num_rows()) {
        return Err(fault(&object.path, "daily row capacity"));
    }
    let schema = builder.schema();
    let names = if funding {
        FUNDING_NAMES.as_slice()
    } else {
        KLINE_NAMES.as_slice()
    };
    let mut expected: BTreeSet<&str> = names.iter().copied().collect();
    if overlay {
        expected.extend(["date", "resolution"]);
    }
    let actual: BTreeSet<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
    if expected != actual
        || schema.fields().len() != expected.len()
        || schema.metadata().get("parangsae.scale").map(String::as_str) != Some("100000000")
    {
        return Err(fault(&object.path, "canonical column set or scale"));
    }
    for field in schema.fields() {
        let dtype = match field.name().as_str() {
            "symbol" | "tick_domain" | "resolution" => DataType::Utf8,
            "schema_version" => DataType::UInt32,
            "date" => DataType::Date32,
            _ => DataType::Int64,
        };
        if *field.data_type() != dtype {
            return Err(fault(&object.path, "canonical physical type"));
        }
    }
    let key = if funding {
        "funding_time_us"
    } else {
        "open_time_us"
    };
    let builder = if key_only {
        let indices = [
            schema
                .index_of("symbol")
                .map_err(|e| fault(&object.path, e))?,
            schema.index_of(key).map_err(|e| fault(&object.path, e))?,
        ];
        let mask = ProjectionMask::roots(builder.parquet_schema(), indices);
        builder.with_projection(mask)
    } else {
        builder
    };
    let reader = builder
        .with_batch_size(512)
        .build()
        .map_err(|e| fault(&object.path, e))?;
    let mut rows = Vec::new();
    let mut count = 0_usize;
    for batch in reader {
        let batch = batch.map_err(|e| fault(&object.path, e))?;
        count = count
            .checked_add(batch.num_rows())
            .ok_or(SourceError::Overflow)?;
        if count > 1440 {
            return Err(fault(&object.path, "daily row capacity"));
        }
        if batch.columns().iter().any(|c| c.null_count() != 0) {
            return Err(fault(&object.path, "null source field"));
        }
        rows.push(batch);
    }
    object.unchanged()?;
    Ok(rows)
}

fn int(batch: &RecordBatch, name: &str, row: usize) -> Result<i64, SourceError> {
    batch
        .column_by_name(name)
        .and_then(|a| a.as_any().downcast_ref::<Int64Array>())
        .map(|a| a.value(row))
        .ok_or(SourceError::Invariant("validated integer column"))
}
fn string<'a>(batch: &'a RecordBatch, name: &str, row: usize) -> Result<&'a str, SourceError> {
    batch
        .column_by_name(name)
        .and_then(|a| a.as_any().downcast_ref::<StringArray>())
        .map(|a| a.value(row))
        .ok_or(SourceError::Invariant("validated string column"))
}
fn version(batch: &RecordBatch, row: usize) -> Result<u32, SourceError> {
    batch
        .column_by_name("schema_version")
        .and_then(|a| a.as_any().downcast_ref::<UInt32Array>())
        .map(|a| a.value(row))
        .ok_or(SourceError::Invariant("validated schema column"))
}

fn keys(
    object: &BoundObject,
    day: i64,
    funding: bool,
    overlay: bool,
) -> Result<BTreeSet<i64>, SourceError> {
    let mut seen = BTreeSet::new();
    for batch in batches(object, funding, overlay, true)? {
        for i in 0..batch.num_rows() {
            let key = int(
                &batch,
                if funding {
                    "funding_time_us"
                } else {
                    "open_time_us"
                },
                i,
            )?;
            if string(&batch, "symbol", i)? != "BTCUSDT"
                || !(day..day + DAY).contains(&key)
                || key.rem_euclid(if funding { SLOT } else { MINUTE }) != 0
                || !seen.insert(key)
            {
                return Err(fault(
                    &object.path,
                    "symbol/calendar/grid/duplicate key before payload",
                ));
            }
        }
    }
    Ok(seen)
}

fn clocks(batch: &RecordBatch, row: usize) -> Result<Timestamps, SourceError> {
    Ok(Timestamps {
        event_ts: EventTs::from_micros(UtcMicros::from_micros(int(batch, "event_ts_us", row)?)),
        available_ts: AvailableTs::from_micros(UtcMicros::from_micros(int(
            batch,
            "available_ts_us",
            row,
        )?)),
        received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(int(
            batch,
            "received_ts_us",
            row,
        )?)),
    })
}

fn source_ref(
    object: &BoundObject,
    row: u32,
    original: Timestamps,
    source: PriceSource,
    key: i64,
) -> SourceRef {
    SourceRef {
        object: object.object.path.clone(),
        sha256: object.object.sha256.clone(),
        row,
        original,
        raw_unit: if source == PriceSource::Spot && key >= SPOT_MICROS {
            TimeUnit::Micros
        } else {
            TimeUnit::Millis
        },
    }
}

fn price_rows(
    object: &BoundObject,
    source: PriceSource,
    day: i64,
    overlay: bool,
    expected: &BTreeSet<i64>,
) -> Result<BTreeMap<i64, SourceBar>, SourceError> {
    if keys(object, day, false, overlay)? != *expected {
        return Err(fault(&object.path, "source key set before payload"));
    }
    let mut rows = BTreeMap::new();
    let mut ordinal = 0;
    for batch in batches(object, false, overlay, false)? {
        for i in 0..batch.num_rows() {
            let key = int(&batch, "open_time_us", i)?;
            if !matches!(version(&batch, i)?, 1 | 2)
                || string(&batch, "tick_domain", i)? != "price"
                || int(&batch, "interval_us", i)? != MINUTE
            {
                return Err(fault(&object.path, "kline schema/domain/interval"));
            }
            for name in [
                "open_ticks",
                "high_ticks",
                "low_ticks",
                "close_ticks",
                "volume_ticks",
                "quote_volume_ticks",
                "count",
                "taker_buy_volume_ticks",
                "taker_buy_quote_volume_ticks",
            ] {
                if int(&batch, name, i)? < 0 {
                    return Err(fault(&object.path, "negative source price/quantity/count"));
                }
            }
            if int(&batch, "taker_buy_volume_ticks", i)? > int(&batch, "volume_ticks", i)?
                || int(&batch, "taker_buy_quote_volume_ticks", i)?
                    > int(&batch, "quote_volume_ticks", i)?
            {
                return Err(fault(&object.path, "taker quantity exceeds total"));
            }
            if overlay {
                let date = batch
                    .column_by_name("date")
                    .and_then(|a| a.as_any().downcast_ref::<Date32Array>())
                    .ok_or(SourceError::Invariant("validated overlay date"))?
                    .value(i);
                if i64::from(date) * DAY != day
                    || !string(&batch, "resolution", i)?
                        .ends_with("pending_independent_data_reviewer_signoff")
                {
                    return Err(fault(&object.path, "overlay provenance shape"));
                }
            }
            let price =
                |name| Price::from_ticks(int(&batch, name, i)?).map_err(|e| fault(&object.path, e));
            let bar = Bar {
                open_time: UtcMicros::from_micros(key),
                close_time: UtcMicros::from_micros(int(&batch, "close_time_us", i)?),
                available: UtcMicros::from_micros(int(&batch, "available_ts_us", i)?),
                open: price("open_ticks")?,
                high: price("high_ticks")?,
                low: price("low_ticks")?,
                close: price("close_ticks")?,
                volume: Qty::from_ticks(int(&batch, "volume_ticks", i)?)
                    .map_err(|e| fault(&object.path, e))?,
                count: int(&batch, "count", i)?,
            };
            rows.insert(
                key,
                SourceBar::admit(
                    source,
                    bar,
                    source_ref(object, ordinal, clocks(&batch, i)?, source, key),
                )?,
            );
            ordinal += 1;
        }
    }
    Ok(rows)
}

fn funding_rows(
    object: &BoundObject,
    day: i64,
) -> Result<BTreeMap<i64, SourceFunding>, SourceError> {
    if keys(object, day, true, false)? != (0..3).map(|i| day + i * SLOT).collect() {
        return Err(fault(
            &object.path,
            "funding semantic key set before payload",
        ));
    }
    let mut rows = BTreeMap::new();
    let mut ordinal = 0;
    for batch in batches(object, true, false, false)? {
        for i in 0..batch.num_rows() {
            let key = int(&batch, "funding_time_us", i)?;
            let raw = int(&batch, "raw_calc_time_us", i)?;
            if version(&batch, i)? != 3
                || int(&batch, "interval_hours", i)? != 8
                || raw.abs_diff(key) > 1_000_000
            {
                return Err(fault(&object.path, "funding schema/grid/raw snap/cadence"));
            }
            let original = clocks(&batch, i)?;
            if original.available_ts.micros().micros().max(key + MINUTE)
                < original.event_ts.micros().micros()
            {
                return Err(fault(&object.path, "funding effective before event"));
            }
            rows.insert(
                key,
                SourceFunding {
                    settlement: UtcMicros::from_micros(key),
                    rate: Ratio::from_ticks(int(&batch, "rate_ticks", i)?),
                    interval_hours: 8,
                    source: source_ref(object, ordinal, original, PriceSource::UmTrade, key),
                },
            );
            ordinal += 1;
        }
    }
    if rows.keys().copied().ne((0..3).map(|i| day + i * SLOT)) {
        return Err(fault(&object.path, "funding expected three semantic keys"));
    }
    Ok(rows)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusRow {
    kind: String,
    open_time_us: i64,
    modeled_notice_at_us: i64,
    reason: String,
    source: serde_json::Value,
    #[serde(default)]
    raw_close: Option<String>,
}

fn status_rows(object: &BoundObject, day: i64) -> Result<BTreeMap<i64, SourceHealth>, SourceError> {
    let mut text = String::new();
    object
        .open()?
        .read_to_string(&mut text)
        .map_err(|e| fault(&object.path, e))?;
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        let status: StatusRow = serde_json::from_str(line).map_err(|e| fault(&object.path, e))?;
        let health = match status.kind.as_str() {
            "Gap" => SourceHealth::Gap,
            "Quarantined" => SourceHealth::Quarantined,
            _ => return Err(fault(&object.path, "unsupported price-free status")),
        };
        if !(day..day + DAY).contains(&status.open_time_us)
            || status.open_time_us.rem_euclid(MINUTE) != 0
            || status.modeled_notice_at_us
                != ceil_to_ms(UtcMicros::from_micros(source_available(
                    PriceSource::Spot,
                    status.open_time_us,
                )?))?
                .micros()
            || status.reason.is_empty()
            || !status.source.is_object()
            || (health == SourceHealth::Quarantined && status.raw_close.is_none())
            || rows.insert(status.open_time_us, health).is_some()
        {
            return Err(fault(&object.path, "invalid/duplicate price-free status"));
        }
    }
    if rows.len() > 1440 {
        return Err(fault(&object.path, "status capacity"));
    }
    if let Some((_, start, count)) = CERTIFIED_NORMAL_GAPS
        .iter()
        .find(|(date, _, _)| *date == object.object.date)
    {
        if rows.values().any(|health| *health != SourceHealth::Gap)
            || rows
                .keys()
                .copied()
                .ne((0..*count).map(|i| start + i * MINUTE))
        {
            return Err(fault(
                &object.path,
                "normal status differs from certified original-source key set",
            ));
        }
    }
    object.unchanged()?;
    Ok(rows)
}

#[cfg(test)]
mod tests;
