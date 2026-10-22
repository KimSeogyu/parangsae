//! Load a published version. Holdout is refused before any row is returned.
//!
//! Rows keep the `available_ts` stored in the snapshot. This module does not
//! recompute it.

use std::fs;
use std::path::{Path, PathBuf};

use parangsae_core::TimeUnit;

use crate::civil::{CivilDay, DayRange};
use crate::version::encode::{
    self, kline_content_hash, kline_excluded_windows, schema_hash, ConflictDoc, LeafInput,
    SourceInput, WindowDoc, UM_KLINE_COLUMNS,
};
use crate::version::id::{DatasetId, ManifestDigest, VersionRoot};
use crate::version::publish::conflict_class;
use crate::version::validate::{validate_klines, KlineCheck};
use crate::vision::{BookFact, FundingFact, Gap, GapKind, KlineFact, MetricsFact, TickDomain};
use crate::Error;

// TODO(holdout): replace the constant with the range from research/holdout.toml once its format is defined in runner-v0-spec (Backtest Builder's psg holdout-lock branch).
const HOLDOUT_START: CivilDay = match CivilDay::new(2025, 10, 1) {
    Some(day) => day,
    None => panic!("2025-10-01"),
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyMode {
    Cached,
    Full,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Table {
    Kline(Vec<KlineFact>),
    Funding(Vec<FundingFact>),
    Metrics(Vec<MetricsFact>),
    Book(Vec<BookFact>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GapEntry {
    pub kind: String,
    pub start_us: i64,
    pub end_us: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictEntry {
    pub pk: Vec<i64>,
    pub class: String,
    pub stored_sha256: [u8; 32],
    pub rejected_sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExcludedWindow {
    pub pk_start_us: i64,
    pub pk_end_us: i64,
    pub avail_start_us: i64,
    pub avail_end_us: i64,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartitionStats {
    pub row_count: u64,
    pub pk_min_us: i64,
    pub pk_max_us: i64,
    pub available_min_us: i64,
    pub available_max_us: i64,
}

#[derive(Debug)]
pub struct Verified<T> {
    value: T,
    gaps: Vec<GapEntry>,
    conflicts: Vec<ConflictEntry>,
    excluded: Vec<ExcludedWindow>,
    leaves_verified: u64,
    cache_hits: u64,
}

impl<T> Verified<T> {
    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn open_gaps(&self) -> &[GapEntry] {
        &self.gaps
    }

    pub fn conflicts(&self) -> &[ConflictEntry] {
        &self.conflicts
    }

    pub fn excluded_windows(&self) -> &[ExcludedWindow] {
        &self.excluded
    }

    pub fn leaves_verified(&self) -> u64 {
        self.leaves_verified
    }

    pub fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    pub fn is_excluded(&self, pk_us: i64) -> bool {
        self.excluded
            .iter()
            .any(|window| contains_pk(window, pk_us))
    }

    #[cfg(test)]
    pub(crate) fn from_parts(
        value: T,
        gaps: Vec<GapEntry>,
        conflicts: Vec<ConflictEntry>,
        excluded: Vec<ExcludedWindow>,
    ) -> Self {
        Self {
            value,
            gaps,
            conflicts,
            excluded,
            leaves_verified: 0,
            cache_hits: 0,
        }
    }
}

fn contains_pk(window: &ExcludedWindow, pk_us: i64) -> bool {
    if window.pk_start_us == window.pk_end_us {
        pk_us == window.pk_start_us
    } else {
        pk_us >= window.pk_start_us && pk_us < window.pk_end_us
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    UnknownVersion,
    ManifestDigestMismatch,
    ManifestRootMismatch,
    ManifestOrder,
    PartitionMissing { date: CivilDay },
    PartitionHashMismatch { date: CivilDay },
    SchemaMismatch { date: CivilDay },
    PkOrder { date: CivilDay },
    AvailableTsRule { date: CivilDay },
    RowInvariant { date: CivilDay, check: &'static str },
    PartitionStatsMismatch { date: CivilDay },
    RangeOutsideVersion,
    HoldoutRefused,
    UnlistedConflict { key: String },
    LegacyObjectLine { url: String },
    Io(String),
}

impl LoadError {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::UnknownVersion => "UnknownVersion",
            Self::ManifestDigestMismatch => "ManifestDigestMismatch",
            Self::ManifestRootMismatch => "ManifestRootMismatch",
            Self::ManifestOrder => "ManifestOrder",
            Self::PartitionMissing { .. } => "PartitionMissing",
            Self::PartitionHashMismatch { .. } => "PartitionHashMismatch",
            Self::SchemaMismatch { .. } => "SchemaMismatch",
            Self::PkOrder { .. } => "PkOrder",
            Self::AvailableTsRule { .. } => "AvailableTsRule",
            Self::RowInvariant { .. } => "RowInvariant",
            Self::PartitionStatsMismatch { .. } => "PartitionStatsMismatch",
            Self::RangeOutsideVersion => "RangeOutsideVersion",
            Self::HoldoutRefused => "HoldoutRefused",
            Self::UnlistedConflict { .. } => "UnlistedConflict",
            Self::LegacyObjectLine { .. } => "LegacyObjectLine",
            Self::Io(_) => "Io",
        }
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnlistedConflict { key } => write!(f, "unlisted conflict {key}"),
            Self::LegacyObjectLine { url } => write!(f, "legacy object line {url}"),
            Self::Io(detail) => write!(f, "{detail}"),
            Self::RowInvariant { check, .. } => write!(f, "row invariant {check}"),
            other => write!(f, "{}", other.name()),
        }
    }
}

pub struct VersionStore {
    data_dir: PathBuf,
}

impl VersionStore {
    pub fn open(data_dir: &Path) -> Result<Self, LoadError> {
        if data_dir.is_dir() {
            Ok(Self {
                data_dir: data_dir.to_path_buf(),
            })
        } else {
            Err(LoadError::Io(format!(
                "{} is not a directory",
                data_dir.display()
            )))
        }
    }

    pub fn load(
        &self,
        dataset: &DatasetId,
        root: VersionRoot,
        manifest_digest: ManifestDigest,
        range: DayRange,
    ) -> Result<Verified<Table>, LoadError> {
        self.load_with(dataset, root, manifest_digest, range, VerifyMode::Cached)
    }

    pub fn load_with(
        &self,
        dataset: &DatasetId,
        root: VersionRoot,
        manifest_digest: ManifestDigest,
        range: DayRange,
        verify: VerifyMode,
    ) -> Result<Verified<Table>, LoadError> {
        refuse_holdout(dataset, range)?;
        if dataset.dataset() != "um-kline" && dataset.dataset() != "spot-kline" {
            return Err(LoadError::SchemaMismatch { date: range.from });
        }
        let path = manifest_path(&self.data_dir, dataset, root);
        if !path.is_file() {
            return Err(LoadError::UnknownVersion);
        }
        let bytes = fs::read(&path).map_err(|err| LoadError::Io(err.to_string()))?;
        if encode::sha256(&bytes) != manifest_digest.as_bytes() {
            return Err(LoadError::ManifestDigestMismatch);
        }
        let parsed = parse_manifest(&bytes)?;
        if parsed.dataset_id != dataset.as_str() {
            return Err(LoadError::ManifestOrder);
        }
        if parsed.version_root != root {
            return Err(LoadError::ManifestRootMismatch);
        }
        check_declared_schema(&parsed)?;
        check_coverage(&parsed)?;
        let leaves = recompute_leaves(&parsed)?;
        if encode::merkle_tree_head(&leaves) != root.as_bytes() {
            return Err(LoadError::ManifestRootMismatch);
        }
        if range.from < parsed.first || range.to > parsed.last {
            return Err(LoadError::RangeOutsideVersion);
        }
        let schema = schema_hash(dataset.as_str(), UM_KLINE_COLUMNS)
            .map_err(|err| LoadError::Io(err.to_string()))?;
        let mut rows = Vec::new();
        let mut gaps = Vec::new();
        let mut conflicts = Vec::new();
        let mut hits = 0u64;
        let mut leaves_verified = 0u64;
        for day in range.days() {
            let Some(index) = parsed.parts.iter().position(|part| part.date == day) else {
                return Err(LoadError::ManifestOrder);
            };
            let part = &parsed.parts[index];
            leaves_verified += 1;
            gaps.extend(part.gaps.iter().map(gap_entry));
            conflicts.extend(part.conflicts.iter().cloned().map(conflict_entry));
            if part.stats.row_count == 0 {
                continue;
            }
            let loaded = self.read_partition(dataset, part, leaves[index], &schema, verify)?;
            if loaded.hit {
                hits += 1;
            }
            rows.extend(loaded.rows);
        }
        self.refuse_unlisted(dataset, range, &conflicts)?;
        let excluded = availability(&rows, &conflicts)?;
        Ok(Verified {
            value: Table::Kline(rows),
            gaps,
            conflicts,
            excluded,
            leaves_verified,
            cache_hits: hits,
        })
    }

    fn read_partition(
        &self,
        dataset: &DatasetId,
        part: &Part,
        leaf: [u8; 32],
        schema: &[u8; 32],
        verify: VerifyMode,
    ) -> Result<Decoded, LoadError> {
        let path = self
            .data_dir
            .join("snapshots")
            .join(hex::encode(part.content))
            .join("part.parquet");
        if !path.is_file() {
            return Err(LoadError::PartitionMissing { date: part.date });
        }
        let bytes = fs::read(&path).map_err(|err| LoadError::Io(err.to_string()))?;
        let file_sha = encode::sha256(&bytes);
        let cache = self
            .data_dir
            .join("cache")
            .join("verified")
            .join(hex::encode(leaf));
        let hit = verify == VerifyMode::Cached && read_cache(&cache) == Some(file_sha);
        let rows = crate::store::read_klines_file(&path, dataset.dataset())
            .map_err(|err| map_read(err, part.date))?;
        if hit {
            return Ok(Decoded { rows, hit: true });
        }
        let unit = match dataset.dataset() {
            "spot-kline" => crate::normalize::spot_kline_clock(part.date).expected(),
            _ => TimeUnit::Millis,
        };
        validate_klines(
            &rows,
            &KlineCheck {
                day: part.date,
                symbol: dataset.symbol(),
                stats: &part.stats,
                unit,
                domain: TickDomain::Price,
                require_archive: dataset.as_str().starts_with("binance-vision."),
            },
        )?;
        let actual =
            kline_content_hash(schema, &rows).map_err(|err| LoadError::Io(err.to_string()))?;
        if actual != part.content {
            return Err(LoadError::PartitionHashMismatch { date: part.date });
        }
        write_cache(&cache, &file_sha)?;
        Ok(Decoded { rows, hit: false })
    }

    fn refuse_unlisted(
        &self,
        dataset: &DatasetId,
        range: DayRange,
        listed: &[ConflictEntry],
    ) -> Result<(), LoadError> {
        let path = self
            .data_dir
            .join("conflicts")
            .join(format!("{}.jsonl", dataset.dataset()));
        if !path.is_file() {
            return Ok(());
        }
        let text = fs::read_to_string(&path).map_err(|err| LoadError::Io(err.to_string()))?;
        for (index, line) in text.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            let parsed: serde_json::Value = serde_json::from_str(line)
                .map_err(|err| LoadError::Io(format!("conflict line {}: {err}", index + 1)))?;
            let symbol = parsed
                .get("symbol")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if symbol != dataset.symbol() {
                continue;
            }
            let key = parsed
                .get("key")
                .and_then(|value| value.as_str())
                .ok_or_else(|| LoadError::Io(format!("conflict line {} has no key", index + 1)))?;
            let pk = key
                .parse::<i64>()
                .map_err(|_| LoadError::UnlistedConflict {
                    key: key.to_owned(),
                })?;
            let row_day = crate::civil::day_of(parangsae_core::UtcMicros::from_micros(pk))
                .map_err(|err| LoadError::Io(err.to_string()))?;
            if !range.contains(row_day) {
                continue;
            }
            let stored = json_str(&parsed, "stored")?;
            let rejected = json_str(&parsed, "rejected")?;
            let class = conflict_class(&stored, &rejected);
            if class != "value_by_log" {
                continue;
            }
            let entry = ConflictEntry {
                pk: vec![pk],
                class: class.to_owned(),
                stored_sha256: encode::sha256(stored.as_bytes()),
                rejected_sha256: encode::sha256(rejected.as_bytes()),
            };
            if !listed.contains(&entry) {
                return Err(LoadError::UnlistedConflict {
                    key: key.to_owned(),
                });
            }
        }
        Ok(())
    }
}

struct Decoded {
    rows: Vec<KlineFact>,
    hit: bool,
}

struct ParsedManifest {
    dataset_id: String,
    version_root: VersionRoot,
    schema_hash: [u8; 32],
    first: CivilDay,
    last: CivilDay,
    parts: Vec<Part>,
}

struct Part {
    date: CivilDay,
    stats: PartitionStats,
    content: [u8; 32],
    gaps: Vec<Gap>,
    sources: Vec<SourceInput>,
    conflicts: Vec<ConflictDoc>,
    windows: Vec<WindowDoc>,
}

fn refuse_holdout(dataset: &DatasetId, range: DayRange) -> Result<(), LoadError> {
    if dataset.as_str().ends_with(".holdout") || range.to >= HOLDOUT_START {
        Err(LoadError::HoldoutRefused)
    } else {
        Ok(())
    }
}

fn manifest_path(data_dir: &Path, dataset: &DatasetId, root: VersionRoot) -> PathBuf {
    data_dir
        .join("manifest")
        .join("versions")
        .join(dataset.as_str())
        .join(format!("{}.json", root.to_hex()))
}

fn check_declared_schema(parsed: &ParsedManifest) -> Result<(), LoadError> {
    let date = parsed.first;
    let expected = schema_hash(&parsed.dataset_id, UM_KLINE_COLUMNS)
        .map_err(|err| LoadError::Io(err.to_string()))?;
    if parsed.schema_hash != expected {
        return Err(LoadError::SchemaMismatch { date });
    }
    Ok(())
}

fn check_coverage(parsed: &ParsedManifest) -> Result<(), LoadError> {
    let range =
        DayRange::new(parsed.first, parsed.last).map_err(|err| LoadError::Io(err.to_string()))?;
    let days = range.days();
    if parsed.parts.len() != days.len() {
        return Err(LoadError::ManifestOrder);
    }
    for (part, day) in parsed.parts.iter().zip(days) {
        if part.date != day {
            return Err(LoadError::ManifestOrder);
        }
    }
    Ok(())
}

fn recompute_leaves(parsed: &ParsedManifest) -> Result<Vec<[u8; 32]>, LoadError> {
    let mut leaves = Vec::with_capacity(parsed.parts.len());
    for part in &parsed.parts {
        let gaps = encode::gaps_digest(&part.gaps).map_err(|_| LoadError::ManifestOrder)?;
        let conflict_hash = digest_conflict_docs(&part.conflicts)?;
        let source_hash =
            encode::sources_digest(&part.sources).map_err(|_| LoadError::ManifestOrder)?;
        let windows = kline_excluded_windows(&part.conflicts);
        if windows != part.windows {
            return Err(LoadError::ManifestOrder);
        }
        let leaf = encode::leaf_hash(&LeafInput {
            dataset_id: &parsed.dataset_id,
            symbol: symbol_of(&parsed.dataset_id),
            date: &part.date.to_string(),
            schema_hash: &parsed.schema_hash,
            row_count: part.stats.row_count,
            pk_min_us: part.stats.pk_min_us,
            pk_max_us: part.stats.pk_max_us,
            content_hash: &part.content,
            gaps_digest: &gaps,
            conflicts_digest: &conflict_hash,
            sources_digest: &source_hash,
        })
        .map_err(|err| LoadError::Io(err.to_string()))?;
        leaves.push(leaf);
    }
    Ok(leaves)
}

fn digest_conflict_docs(conflicts: &[ConflictDoc]) -> Result<[u8; 32], LoadError> {
    let mut items = conflicts.to_vec();
    items.sort_by(|left, right| {
        left.pk
            .cmp(&right.pk)
            .then(left.class.cmp(&right.class))
            .then(left.stored_sha256.cmp(&right.stored_sha256))
            .then(left.rejected_sha256.cmp(&right.rejected_sha256))
    });
    items.dedup();
    let mut buf = Vec::new();
    push_str(&mut buf, "parangsae/conflicts/v1")?;
    push_u32(&mut buf, u32_len(items.len())?)?;
    for item in items {
        push_u32(&mut buf, u32_len(item.pk.len())?)?;
        for element in item.pk {
            buf.extend_from_slice(&element.to_be_bytes());
        }
        push_str(&mut buf, &item.class)?;
        buf.extend_from_slice(&item.stored_sha256);
        buf.extend_from_slice(&item.rejected_sha256);
    }
    Ok(encode::sha256(&buf))
}

fn push_str(buf: &mut Vec<u8>, text: &str) -> Result<(), LoadError> {
    push_u32(buf, u32_len(text.len())?)?;
    buf.extend_from_slice(text.as_bytes());
    Ok(())
}

fn push_u32(buf: &mut Vec<u8>, value: u32) -> Result<(), LoadError> {
    buf.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn u32_len(len: usize) -> Result<u32, LoadError> {
    u32::try_from(len).map_err(|_| LoadError::Io("length".into()))
}

fn symbol_of(dataset_id: &str) -> &str {
    dataset_id.split('.').nth(3).unwrap_or("")
}

fn availability(
    rows: &[KlineFact],
    conflicts: &[ConflictEntry],
) -> Result<Vec<ExcludedWindow>, LoadError> {
    let docs: Vec<ConflictDoc> = conflicts
        .iter()
        .map(|conflict| ConflictDoc {
            pk: conflict.pk.clone(),
            class: conflict.class.clone(),
            stored_sha256: conflict.stored_sha256,
            rejected_sha256: conflict.rejected_sha256,
        })
        .collect();
    let windows = kline_excluded_windows(&docs);
    let mut out = Vec::with_capacity(windows.len());
    for window in windows {
        let mut matched: Vec<&KlineFact> = rows
            .iter()
            .filter(|row| pk_in(row.open_time.micros(), window.start_us, window.end_us))
            .collect();
        matched.sort_by_key(|row| row.open_time);
        let Some(first) = matched.first() else {
            let date =
                crate::civil::day_of(parangsae_core::UtcMicros::from_micros(window.start_us))
                    .map_err(|err| LoadError::Io(err.to_string()))?;
            return Err(LoadError::RowInvariant {
                date,
                check: "pk_day",
            });
        };
        let last = matched[matched.len() - 1];
        let avail_end = last
            .available_ts
            .micros()
            .checked_add_micros(1)
            .ok_or_else(|| LoadError::Io("availability end overflow".into()))?;
        out.push(ExcludedWindow {
            pk_start_us: window.start_us,
            pk_end_us: window.end_us,
            avail_start_us: first.available_ts.micros().micros(),
            avail_end_us: avail_end.micros(),
            reason: window.reason.to_owned(),
        });
    }
    Ok(out)
}

fn pk_in(pk: i64, start: i64, end: i64) -> bool {
    if start == end {
        pk == start
    } else {
        pk >= start && pk < end
    }
}

fn gap_entry(gap: &Gap) -> GapEntry {
    GapEntry {
        kind: gap.kind.as_str().to_owned(),
        start_us: gap.start.micros(),
        end_us: gap.end.micros(),
    }
}

fn conflict_entry(conflict: ConflictDoc) -> ConflictEntry {
    ConflictEntry {
        pk: conflict.pk,
        class: conflict.class,
        stored_sha256: conflict.stored_sha256,
        rejected_sha256: conflict.rejected_sha256,
    }
}

fn map_read(err: Error, date: CivilDay) -> LoadError {
    match err.check_name() {
        Some("schema" | "parquet_schema" | "scale" | "parquet") => {
            LoadError::SchemaMismatch { date }
        }
        Some("receipt") => LoadError::RowInvariant {
            date,
            check: "receipt",
        },
        _ => LoadError::Io(err.to_string()),
    }
}

fn read_cache(path: &Path) -> Option<[u8; 32]> {
    let text = fs::read_to_string(path).ok()?;
    if text.len() != 64 {
        return None;
    }
    let decoded = hex::decode(&text).ok()?;
    if decoded.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded);
    Some(out)
}

fn write_cache(path: &Path, sha: &[u8; 32]) -> Result<(), LoadError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| LoadError::Io(err.to_string()))?;
    }
    fs::write(path, hex::encode(sha)).map_err(|err| LoadError::Io(err.to_string()))
}

fn json_str(value: &serde_json::Value, key: &str) -> Result<String, LoadError> {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .map(str::to_owned)
        .ok_or_else(|| LoadError::Io(format!("conflict line missing {key}")))
}

fn parse_manifest(bytes: &[u8]) -> Result<ParsedManifest, LoadError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|err| LoadError::Io(err.to_string()))?;
    let root = value.as_object().ok_or(LoadError::ManifestOrder)?;
    if text(root, "spec")? != encode::MANIFEST_SPEC {
        return Err(LoadError::ManifestOrder);
    }
    let schema = root
        .get("schema")
        .and_then(|item| item.as_object())
        .ok_or(LoadError::ManifestOrder)?;
    let columns = schema
        .get("columns")
        .and_then(|item| item.as_array())
        .ok_or(LoadError::ManifestOrder)?;
    if columns.len() != UM_KLINE_COLUMNS.len() {
        return Err(LoadError::SchemaMismatch {
            date: parse_day(&text(
                root.get("range")
                    .and_then(|item| item.as_object())
                    .ok_or(LoadError::ManifestOrder)?,
                "first_date",
            )?)?,
        });
    }
    for (found, expected) in columns.iter().zip(UM_KLINE_COLUMNS) {
        let pair = found.as_array().ok_or(LoadError::ManifestOrder)?;
        let name = pair
            .first()
            .and_then(|item| item.as_str())
            .ok_or(LoadError::ManifestOrder)?;
        let ty = pair
            .get(1)
            .and_then(|item| item.as_str())
            .ok_or(LoadError::ManifestOrder)?;
        if name != expected.name || ty != expected.ty {
            let range = root
                .get("range")
                .and_then(|item| item.as_object())
                .ok_or(LoadError::ManifestOrder)?;
            return Err(LoadError::SchemaMismatch {
                date: parse_day(&text(range, "first_date")?)?,
            });
        }
    }
    let rules = schema
        .get("rule_versions")
        .and_then(|item| item.as_object())
        .ok_or(LoadError::ManifestOrder)?;
    if text(rules, "row_encoding")? != encode::ROW_ENCODING_VERSION
        || text(rules, "available_ts_rules")? != encode::AVAILABLE_TS_RULES_VERSION
        || text(rules, "gap_rules")? != encode::GAP_RULES_VERSION
        || text(rules, "conflict_rules")? != encode::CONFLICT_RULES_VERSION
    {
        let range = root
            .get("range")
            .and_then(|item| item.as_object())
            .ok_or(LoadError::ManifestOrder)?;
        return Err(LoadError::SchemaMismatch {
            date: parse_day(&text(range, "first_date")?)?,
        });
    }
    let range = root
        .get("range")
        .and_then(|item| item.as_object())
        .ok_or(LoadError::ManifestOrder)?;
    let first = parse_day(&text(range, "first_date")?)?;
    let last = parse_day(&text(range, "last_date")?)?;
    let parts_json = root
        .get("partitions")
        .and_then(|item| item.as_array())
        .ok_or(LoadError::ManifestOrder)?;
    let mut parts = Vec::with_capacity(parts_json.len());
    for part in parts_json {
        parts.push(parse_part(part)?);
    }
    Ok(ParsedManifest {
        dataset_id: text(root, "dataset_id")?,
        version_root: VersionRoot::from_bytes(parse_hash(&text(root, "version_root")?)?),
        schema_hash: parse_hash(&text(schema, "schema_hash")?)?,
        first,
        last,
        parts,
    })
}

fn parse_part(value: &serde_json::Value) -> Result<Part, LoadError> {
    let part = value.as_object().ok_or(LoadError::ManifestOrder)?;
    let date = parse_day(&text(part, "date")?)?;
    let relative = text(part, "path")?;
    if relative.is_empty() {
        return Err(LoadError::ManifestOrder);
    }
    let gaps = parse_gaps(part.get("open_gaps").ok_or(LoadError::ManifestOrder)?)?;
    let sources = parse_sources(part.get("sources").ok_or(LoadError::ManifestOrder)?)?;
    let conflicts = parse_conflicts(part.get("conflicts").ok_or(LoadError::ManifestOrder)?)?;
    let windows = parse_windows(
        part.get("excluded_windows")
            .ok_or(LoadError::ManifestOrder)?,
    )?;
    require_hash(part, "gaps_digest")?;
    require_hash(part, "conflicts_digest")?;
    require_hash(part, "sources_digest")?;
    require_hash(part, "leaf_hash")?;
    Ok(Part {
        date,
        stats: PartitionStats {
            row_count: parse_u64(&text(part, "row_count")?)?,
            pk_min_us: parse_i64(&text(part, "pk_min_us")?)?,
            pk_max_us: parse_i64(&text(part, "pk_max_us")?)?,
            available_min_us: parse_i64(&text(part, "available_ts_min_us")?)?,
            available_max_us: parse_i64(&text(part, "available_ts_max_us")?)?,
        },
        content: parse_hash(&text(part, "content_hash")?)?,
        gaps,
        sources,
        conflicts,
        windows,
    })
}

fn parse_gaps(value: &serde_json::Value) -> Result<Vec<Gap>, LoadError> {
    let items = value.as_array().ok_or(LoadError::ManifestOrder)?;
    let mut gaps = Vec::with_capacity(items.len());
    for item in items {
        let gap = item.as_object().ok_or(LoadError::ManifestOrder)?;
        let kind = GapKind::parse(&text(gap, "kind")?).ok_or(LoadError::ManifestOrder)?;
        gaps.push(Gap {
            kind,
            start: parangsae_core::UtcMicros::from_micros(parse_i64(&text(gap, "start_us")?)?),
            end: parangsae_core::UtcMicros::from_micros(parse_i64(&text(gap, "end_us")?)?),
            detail: String::new(),
        });
    }
    Ok(gaps)
}

fn parse_sources(value: &serde_json::Value) -> Result<Vec<SourceInput>, LoadError> {
    let items = value.as_array().ok_or(LoadError::ManifestOrder)?;
    let mut sources = Vec::with_capacity(items.len());
    for item in items {
        let source = item.as_object().ok_or(LoadError::ManifestOrder)?;
        let url = text(source, "url")?;
        let checksum = text(source, "checksum_url")?;
        if checksum != format!("{url}.CHECKSUM") {
            return Err(LoadError::ManifestOrder);
        }
        sources.push(SourceInput {
            url,
            sha256: parse_hash(&text(source, "sha256")?)?,
        });
    }
    Ok(sources)
}

fn parse_conflicts(value: &serde_json::Value) -> Result<Vec<ConflictDoc>, LoadError> {
    let items = value.as_array().ok_or(LoadError::ManifestOrder)?;
    let mut conflicts = Vec::with_capacity(items.len());
    for item in items {
        let conflict = item.as_object().ok_or(LoadError::ManifestOrder)?;
        let pk_values = conflict
            .get("pk")
            .and_then(|item| item.as_array())
            .ok_or(LoadError::ManifestOrder)?;
        let mut pk = Vec::with_capacity(pk_values.len());
        for element in pk_values {
            let text = element.as_str().ok_or(LoadError::ManifestOrder)?;
            pk.push(parse_i64(text)?);
        }
        conflicts.push(ConflictDoc {
            pk,
            class: text(conflict, "class")?,
            stored_sha256: parse_hash(&text(conflict, "stored_sha256")?)?,
            rejected_sha256: parse_hash(&text(conflict, "rejected_sha256")?)?,
        });
    }
    Ok(conflicts)
}

fn parse_windows(value: &serde_json::Value) -> Result<Vec<WindowDoc>, LoadError> {
    let items = value.as_array().ok_or(LoadError::ManifestOrder)?;
    let mut windows = Vec::with_capacity(items.len());
    for item in items {
        let window = item.as_object().ok_or(LoadError::ManifestOrder)?;
        let reason = text(window, "reason")?;
        if reason != "value_by_log" {
            return Err(LoadError::ManifestOrder);
        }
        windows.push(WindowDoc {
            start_us: parse_i64(&text(window, "start_us")?)?,
            end_us: parse_i64(&text(window, "end_us")?)?,
            reason: "value_by_log",
        });
    }
    Ok(windows)
}

fn text(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> Result<String, LoadError> {
    map.get(key)
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .ok_or(LoadError::ManifestOrder)
}

fn parse_day(text: &str) -> Result<CivilDay, LoadError> {
    let mut parts = text.split('-');
    let year = parts
        .next()
        .ok_or(LoadError::ManifestOrder)?
        .parse::<i32>()
        .map_err(|_| LoadError::ManifestOrder)?;
    let month = parts
        .next()
        .ok_or(LoadError::ManifestOrder)?
        .parse::<u8>()
        .map_err(|_| LoadError::ManifestOrder)?;
    let day = parts
        .next()
        .ok_or(LoadError::ManifestOrder)?
        .parse::<u8>()
        .map_err(|_| LoadError::ManifestOrder)?;
    if parts.next().is_some() {
        return Err(LoadError::ManifestOrder);
    }
    CivilDay::new(year, month, day).ok_or(LoadError::ManifestOrder)
}

fn parse_i64(text: &str) -> Result<i64, LoadError> {
    text.parse::<i64>().map_err(|_| LoadError::ManifestOrder)
}

fn parse_u64(text: &str) -> Result<u64, LoadError> {
    text.parse::<u64>().map_err(|_| LoadError::ManifestOrder)
}

fn require_hash(
    map: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<(), LoadError> {
    parse_hash(&text(map, key)?)?;
    Ok(())
}

fn parse_hash(text: &str) -> Result<[u8; 32], LoadError> {
    if text.len() != 64
        || text
            .bytes()
            .any(|byte| !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase())
    {
        return Err(LoadError::ManifestOrder);
    }
    let decoded = hex::decode(text).map_err(|_| LoadError::ManifestOrder)?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    use arrow_array::{RecordBatch, StringArray};
    use arrow_schema::{DataType, Field, Schema};
    use parangsae_core::{Receipt, RunId, Symbol, UtcMicros};
    use parquet::basic::Compression;
    use proptest::prelude::*;

    use super::*;
    use crate::civil::DayRange;
    use crate::normalize::UM_KLINE_CLOCK;
    use crate::run::Provenance;
    use crate::store;
    use crate::version::encode::{self, node};
    use crate::version::publish::{publish, PublishRequest};
    use crate::version::validate::available_before_event;
    use crate::vision::{parse_klines, TickDomain};

    const DATASET: &str = "binance-vision.um.um-kline.BTCUSDT";
    const CLEAN_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn show(err: impl std::fmt::Display) -> String {
        err.to_string()
    }

    fn dataset() -> DatasetId {
        DatasetId::parse(DATASET).expect("dataset id")
    }

    fn provenance() -> Provenance {
        Provenance {
            git_sha: CLEAN_SHA.to_owned(),
            dirty: false,
        }
    }

    fn fixture_rows(receipt: Receipt) -> Result<Vec<KlineFact>, String> {
        let csv = fs::read_to_string(format!(
            "{}/../../fixtures/vision/um_kline_2021-06-01_headerless.csv",
            env!("CARGO_MANIFEST_DIR")
        ))
        .map_err(show)?;
        let symbol = Symbol::new("BTCUSDT").map_err(show)?;
        Ok(parse_klines(
            &csv,
            &symbol,
            UM_KLINE_CLOCK,
            TickDomain::Price,
            receipt,
            true,
        )
        .map_err(show)?
        .rows)
    }

    fn publish_rows(
        dir: &std::path::Path,
        rows: Vec<KlineFact>,
        to: CivilDay,
    ) -> Result<PublishedDay, String> {
        let day = crate::civil::day_of(rows[0].open_time).map_err(show)?;
        store::upsert_klines(dir, "um-kline", rows).map_err(show)?;
        let dataset = dataset();
        let range = DayRange::new(day, to).map_err(show)?;
        let provenance = provenance();
        let published = publish(&PublishRequest {
            data_dir: dir,
            dataset: &dataset,
            range,
            provenance: &provenance,
            run: RunId::from_bytes([4; 16]),
            built_at: UtcMicros::from_micros(1),
        })
        .map_err(show)?;
        Ok(PublishedDay { day, published })
    }

    struct PublishedDay {
        day: CivilDay,
        published: crate::version::publish::Published,
    }

    fn publish_fixture(dir: &std::path::Path) -> Result<PublishedDay, String> {
        let rows = fixture_rows(Receipt::Archive {
            fetched_at_run: RunId::from_bytes([1; 16]),
        })?;
        let day = crate::civil::day_of(rows[0].open_time).map_err(show)?;
        publish_rows(dir, rows, day)
    }

    fn load_range(
        dir: &std::path::Path,
        published: &crate::version::publish::Published,
        range: DayRange,
        mode: VerifyMode,
    ) -> Result<Verified<Table>, LoadError> {
        VersionStore::open(dir).expect("store").load_with(
            &dataset(),
            published.root,
            published.manifest_digest,
            range,
            mode,
        )
    }

    fn manifest_bytes(dir: &std::path::Path, root: VersionRoot) -> Vec<u8> {
        fs::read(manifest_path(dir, &dataset(), root)).expect("manifest")
    }

    fn rewrite_manifest(dir: &std::path::Path, root: VersionRoot, bytes: &[u8]) -> ManifestDigest {
        let path = manifest_path(dir, &dataset(), root);
        fs::write(&path, bytes).expect("write manifest");
        ManifestDigest::from_bytes(encode::sha256(bytes))
    }

    fn snapshot(dir: &std::path::Path) -> std::path::PathBuf {
        let mut files = Vec::new();
        for entry in fs::read_dir(dir.join("snapshots"))
            .expect("snapshots")
            .flatten()
        {
            let path = entry.path().join("part.parquet");
            if path.is_file() {
                files.push(path);
            }
        }
        assert_eq!(files.len(), 1);
        files.remove(0)
    }

    fn chmod_writable(path: &std::path::Path) {
        let mut permissions = fs::metadata(path).expect("meta").permissions();
        permissions.set_mode(0o644);
        fs::set_permissions(path, permissions).expect("chmod");
    }

    #[test]
    fn a_published_fixture_loads_and_keeps_stored_availability() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(show)?;
        let published = publish_fixture(dir.path())?;
        let range = DayRange::new(published.day, published.day).map_err(show)?;
        let loaded = load_range(dir.path(), &published.published, range, VerifyMode::Cached)
            .map_err(show)?;
        let Table::Kline(rows) = loaded.value() else {
            return Err("expected klines".into());
        };
        let stored = store::read_klines_file(&snapshot(dir.path()), "um-kline").map_err(show)?;
        if rows != &stored {
            return Err("loader changed stored rows".into());
        }
        assert_eq!(loaded.cache_hits(), 0);
        let again = load_range(dir.path(), &published.published, range, VerifyMode::Cached)
            .map_err(show)?;
        assert_eq!(again.cache_hits(), 1);
        Ok(())
    }

    #[test]
    fn available_before_event_is_rejected_and_not_clamped() -> Result<(), String> {
        let day = CivilDay::new(2021, 6, 1).expect("day");
        let event = 100_i64;
        let available = 99_i64;
        let err = available_before_event(day, event, available).expect_err("early");
        assert!(matches!(err, LoadError::AvailableTsRule { .. }));
        assert_eq!((event, available), (100, 99));
        let dir = tempfile::tempdir().map_err(show)?;
        let mut rows = fixture_rows(Receipt::Archive {
            fetched_at_run: RunId::from_bytes([1; 16]),
        })?;
        let event_us = rows[0].event_ts.micros().micros();
        rows[0].available_ts =
            parangsae_core::AvailableTs::from_micros(UtcMicros::from_micros(event_us - 1));
        let published = publish_rows(dir.path(), rows, day)?;
        let path = snapshot(dir.path());
        let range = DayRange::new(day, day).map_err(show)?;
        let err = load_range(dir.path(), &published.published, range, VerifyMode::Full)
            .expect_err("load");
        assert!(matches!(err, LoadError::AvailableTsRule { .. }), "{err}");
        let stored = store::read_klines_file(&path, "um-kline").map_err(show)?;
        assert!(stored[0].available_ts.micros() < stored[0].event_ts.micros());
        Ok(())
    }

    #[test]
    fn holdout_and_aliases_refuse_before_any_row() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(show)?;
        let published = publish_fixture(dir.path())?;
        let holdout = CivilDay::new(2025, 10, 1).expect("holdout");
        let range = DayRange::new(holdout, holdout).map_err(show)?;
        let err = load_range(dir.path(), &published.published, range, VerifyMode::Full)
            .expect_err("holdout");
        assert!(matches!(err, LoadError::HoldoutRefused));
        let id = DatasetId::parse("binance-vision.um.um-kline.holdout").map_err(show)?;
        let june = DayRange::new(published.day, published.day).map_err(show)?;
        let err = VersionStore::open(dir.path())
            .map_err(show)?
            .load(
                &id,
                published.published.root,
                published.published.manifest_digest,
                june,
            )
            .expect_err("holdout id");
        assert!(matches!(err, LoadError::HoldoutRefused));
        Ok(())
    }

    #[test]
    fn a_multi_day_range_before_the_holdout_loads() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(show)?;
        let rows = fixture_rows(Receipt::Archive {
            fetched_at_run: RunId::from_bytes([1; 16]),
        })?;
        let start = crate::civil::day_of(rows[0].open_time).map_err(show)?;
        let end = start.next().next();
        let published = publish_rows(dir.path(), rows, end)?;
        let range = DayRange::new(start, end).map_err(show)?;
        let loaded =
            load_range(dir.path(), &published.published, range, VerifyMode::Full).map_err(show)?;
        let Table::Kline(rows) = loaded.value() else {
            return Err("expected klines".into());
        };
        assert_eq!(rows.len(), 3);
        assert!(loaded
            .open_gaps()
            .iter()
            .any(|gap| gap.kind == "missing_file"));
        let september = CivilDay::new(2025, 9, 30).expect("september");
        let outside = DayRange::new(september, september).map_err(show)?;
        let err = load_range(dir.path(), &published.published, outside, VerifyMode::Full)
            .expect_err("outside");
        assert!(
            matches!(err, LoadError::RangeOutsideVersion),
            "2025-09-30 is not a holdout refusal: {err}"
        );
        Ok(())
    }

    #[test]
    fn deeper_manifest_edits_fail_closed() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(show)?;
        let published = publish_fixture(dir.path())?;
        let root = published.published.root;
        let original = manifest_bytes(dir.path(), root);
        let mut forged = serde_json::from_slice::<serde_json::Value>(&original).map_err(show)?;
        forged["partitions"][0]["pk_min_us"] = serde_json::Value::String("1".into());
        let bytes = serde_json::to_vec(&forged).map_err(show)?;
        let digest = rewrite_manifest(dir.path(), root, &bytes);
        let range = DayRange::new(published.day, published.day).map_err(show)?;
        let err = VersionStore::open(dir.path())
            .map_err(show)?
            .load(&dataset(), root, digest, range)
            .expect_err("forged");
        assert!(matches!(err, LoadError::ManifestRootMismatch), "{err}");
        let original_json: serde_json::Value = serde_json::from_slice(&original).map_err(show)?;
        assert_eq!(
            forged["partitions"][0]["leaf_hash"],
            original_json["partitions"][0]["leaf_hash"]
        );

        let mut relabelled =
            serde_json::from_slice::<serde_json::Value>(&original).map_err(show)?;
        relabelled["partitions"][0]["date"] = serde_json::Value::String("2021-06-02".into());
        let bytes = serde_json::to_vec(&relabelled).map_err(show)?;
        let digest = rewrite_manifest(dir.path(), root, &bytes);
        let err = VersionStore::open(dir.path())
            .map_err(show)?
            .load(&dataset(), root, digest, range)
            .expect_err("date");
        assert!(matches!(err, LoadError::ManifestOrder), "{err}");

        let node_root = node(&root.as_bytes(), &root.as_bytes());
        let mut rooted = serde_json::from_slice::<serde_json::Value>(&original).map_err(show)?;
        rooted["version_root"] = serde_json::Value::String(hex::encode(node_root));
        let bytes = serde_json::to_vec(&rooted).map_err(show)?;
        let path = manifest_path(dir.path(), &dataset(), VersionRoot::from_bytes(node_root));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(show)?;
        }
        fs::write(&path, &bytes).map_err(show)?;
        let digest = ManifestDigest::from_bytes(encode::sha256(&bytes));
        let err = VersionStore::open(dir.path())
            .map_err(show)?
            .load(
                &dataset(),
                VersionRoot::from_bytes(node_root),
                digest,
                range,
            )
            .expect_err("node");
        assert!(matches!(err, LoadError::ManifestRootMismatch), "{err}");

        let mut rules = serde_json::from_slice::<serde_json::Value>(&original).map_err(show)?;
        rules["schema"]["rule_versions"]["available_ts_rules"] =
            serde_json::Value::String("available_ts_rules/v9".into());
        let bytes = serde_json::to_vec(&rules).map_err(show)?;
        let digest = rewrite_manifest(dir.path(), root, &bytes);
        let err = VersionStore::open(dir.path())
            .map_err(show)?
            .load(&dataset(), root, digest, range)
            .expect_err("rules");
        assert!(matches!(err, LoadError::SchemaMismatch { .. }), "{err}");

        let mut gap = serde_json::from_slice::<serde_json::Value>(&original).map_err(show)?;
        gap["partitions"][0]["open_gaps"]
            .as_array_mut()
            .expect("gaps")
            .push(serde_json::json!({
                "end_us": "2",
                "kind": "grid",
                "start_us": "1"
            }));
        let bytes = serde_json::to_vec(&gap).map_err(show)?;
        let digest = rewrite_manifest(dir.path(), root, &bytes);
        let err = VersionStore::open(dir.path())
            .map_err(show)?
            .load(&dataset(), root, digest, range)
            .expect_err("gap");
        assert!(matches!(err, LoadError::ManifestRootMismatch), "{err}");

        let mut conflict = serde_json::from_slice::<serde_json::Value>(&original).map_err(show)?;
        conflict["partitions"][0]["conflicts"]
            .as_array_mut()
            .expect("conflicts")
            .push(serde_json::json!({
                "class": "schema_only_by_log",
                "pk": ["1622505600000000"],
                "rejected_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "stored_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            }));
        let bytes = serde_json::to_vec(&conflict).map_err(show)?;
        let digest = rewrite_manifest(dir.path(), root, &bytes);
        let err = VersionStore::open(dir.path())
            .map_err(show)?
            .load(&dataset(), root, digest, range)
            .expect_err("conflict");
        assert!(matches!(err, LoadError::ManifestRootMismatch), "{err}");

        let mut source = serde_json::from_slice::<serde_json::Value>(&original).map_err(show)?;
        source["partitions"][0]["sources"]
            .as_array_mut()
            .expect("sources")
            .push(serde_json::json!({
                "checksum_url": "https://example.invalid/a.zip.CHECKSUM",
                "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "url": "https://example.invalid/a.zip"
            }));
        let bytes = serde_json::to_vec(&source).map_err(show)?;
        let digest = rewrite_manifest(dir.path(), root, &bytes);
        let err = VersionStore::open(dir.path())
            .map_err(show)?
            .load(&dataset(), root, digest, range)
            .expect_err("source");
        assert!(matches!(err, LoadError::ManifestRootMismatch), "{err}");
        Ok(())
    }

    #[test]
    fn snapshot_tampers_name_the_check() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(show)?;
        let published = publish_fixture(dir.path())?;
        let range = DayRange::new(published.day, published.day).map_err(show)?;
        let path = snapshot(dir.path());
        chmod_writable(&path);
        let mut rows = store::read_klines_file(&path, "um-kline").map_err(show)?;
        rows.pop();
        store::write_klines(&path, "um-kline", &rows).map_err(show)?;
        let err = load_range(dir.path(), &published.published, range, VerifyMode::Full)
            .expect_err("drop");
        assert!(
            matches!(err, LoadError::PartitionStatsMismatch { .. }),
            "{err}"
        );

        let rows = fixture_rows(Receipt::Archive {
            fetched_at_run: RunId::from_bytes([1; 16]),
        })?;
        let mut reversed = rows.clone();
        reversed.reverse();
        store::write_klines(&path, "um-kline", &reversed).map_err(show)?;
        let err = load_range(dir.path(), &published.published, range, VerifyMode::Full)
            .expect_err("swap");
        assert!(matches!(err, LoadError::PkOrder { .. }), "{err}");

        let mut duplicated = rows.clone();
        duplicated.push(rows[0].clone());
        store::write_klines(&path, "um-kline", &duplicated).map_err(show)?;
        let err =
            load_range(dir.path(), &published.published, range, VerifyMode::Full).expect_err("dup");
        assert!(matches!(err, LoadError::PkOrder { .. }), "{err}");

        let mut flat = rows.clone();
        flat[0].zero_volume_flat = !flat[0].zero_volume_flat;
        store::write_klines(&path, "um-kline", &flat).map_err(show)?;
        let err = load_range(dir.path(), &published.published, range, VerifyMode::Full)
            .expect_err("flat");
        assert!(
            matches!(
                err,
                LoadError::RowInvariant {
                    check: "zero_volume_flat",
                    ..
                }
            ),
            "{err}"
        );

        write_open_ticks_as_text(&path, &rows).map_err(show)?;
        let err = load_range(dir.path(), &published.published, range, VerifyMode::Full)
            .expect_err("type");
        assert!(matches!(err, LoadError::SchemaMismatch { .. }), "{err}");
        Ok(())
    }

    fn write_open_ticks_as_text(path: &std::path::Path, rows: &[KlineFact]) -> Result<(), String> {
        let symbols: Vec<String> = rows
            .iter()
            .map(|row| row.symbol.as_str().to_owned())
            .collect();
        let opens: Vec<String> = rows.iter().map(|row| row.open_ticks.to_string()).collect();
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("symbol", DataType::Utf8, false),
                Field::new("open_ticks", DataType::Utf8, false),
            ])),
            vec![
                Arc::new(StringArray::from(symbols)),
                Arc::new(StringArray::from(opens)),
            ],
        )
        .map_err(show)?;
        store::write_batch(path, batch).map_err(show)
    }

    #[test]
    fn cache_hit_rechecks_the_file_sha() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(show)?;
        let published = publish_fixture(dir.path())?;
        let range = DayRange::new(published.day, published.day).map_err(show)?;
        load_range(dir.path(), &published.published, range, VerifyMode::Cached).map_err(show)?;
        let path = snapshot(dir.path());
        chmod_writable(&path);
        let cache = cache_file(dir.path());
        let cached = fs::read(&cache).map_err(show)?;
        let mut rows = store::read_klines_file(&path, "um-kline").map_err(show)?;
        let bumped = rows[0].volume.ticks().saturating_add(1);
        rows[0].volume = parangsae_core::Qty::from_ticks(bumped).map_err(show)?;
        store::write_klines(&path, "um-kline", &rows).map_err(show)?;
        let err = load_range(dir.path(), &published.published, range, VerifyMode::Cached)
            .expect_err("hash");
        assert!(
            matches!(err, LoadError::PartitionHashMismatch { .. }),
            "{err}"
        );
        assert_eq!(fs::read(&cache).map_err(show)?, cached);

        let original = fixture_rows(Receipt::Archive {
            fetched_at_run: RunId::from_bytes([1; 16]),
        })?;
        store::write_klines_compressed(&path, "um-kline", &original, Compression::UNCOMPRESSED)
            .map_err(show)?;
        let loaded = load_range(dir.path(), &published.published, range, VerifyMode::Cached)
            .map_err(show)?;
        assert_eq!(loaded.cache_hits(), 0);
        let file_sha = hex::encode(encode::sha256(&fs::read(&path).map_err(show)?));
        assert_eq!(fs::read_to_string(&cache).map_err(show)?, file_sha);
        let hit = load_range(dir.path(), &published.published, range, VerifyMode::Cached)
            .map_err(show)?;
        assert_eq!(hit.cache_hits(), 1);
        let full =
            load_range(dir.path(), &published.published, range, VerifyMode::Full).map_err(show)?;
        assert_eq!(full.cache_hits(), 0);
        Ok(())
    }

    fn cache_file(dir: &std::path::Path) -> std::path::PathBuf {
        let mut files = Vec::new();
        let root = dir.join("cache").join("verified");
        for entry in fs::read_dir(root).expect("cache").flatten() {
            files.push(entry.path());
        }
        assert_eq!(files.len(), 1);
        files.remove(0)
    }

    #[test]
    fn an_unlisted_value_conflict_fails_and_a_listed_one_excludes_its_window() -> Result<(), String>
    {
        let dir = tempfile::tempdir().map_err(show)?;
        let rows = fixture_rows(Receipt::Archive {
            fetched_at_run: RunId::from_bytes([1; 16]),
        })?;
        let open = rows[0].open_time.micros();
        let mut changed = rows.clone();
        changed[0].open_ticks += 1;
        store::upsert_klines(dir.path(), "um-kline", rows).map_err(show)?;
        store::upsert_klines(dir.path(), "um-kline", changed).map_err(show)?;
        let day = CivilDay::new(2021, 6, 1).expect("day");
        let dataset = dataset();
        let range = DayRange::new(day, day).map_err(show)?;
        let provenance = provenance();
        let published = publish(&PublishRequest {
            data_dir: dir.path(),
            dataset: &dataset,
            range,
            provenance: &provenance,
            run: RunId::from_bytes([8; 16]),
            built_at: UtcMicros::from_micros(1),
        })
        .map_err(show)?;
        let loaded = load_range(dir.path(), &published, range, VerifyMode::Full).map_err(show)?;
        assert!(loaded.is_excluded(open));
        assert!(!loaded.is_excluded(open + 60_000_000));
        let window = &loaded.excluded_windows()[0];
        assert_eq!(window.pk_start_us, open);
        assert_eq!(window.pk_end_us, open + 60_000_000);
        assert_eq!(window.reason, "value_by_log");
        let Table::Kline(table) = loaded.value() else {
            return Err("klines".into());
        };
        assert_eq!(
            window.avail_start_us,
            table[0].available_ts.micros().micros()
        );
        assert_eq!(
            window.avail_end_us,
            table[0].available_ts.micros().micros() + 1
        );

        let log = dir.path().join("conflicts").join("um-kline.jsonl");
        let mut text = fs::read_to_string(&log).map_err(show)?;
        text.push_str(&format!(
            "{{\"dataset\":\"um-kline\",\"key\":\"{open}\",\"rejected\":\"open=9 schema=9\",\"stored\":\"open=9 schema=1\",\"symbol\":\"BTCUSDT\"}}\n"
        ));
        fs::write(&log, &text).map_err(show)?;
        load_range(dir.path(), &published, range, VerifyMode::Full).map_err(show)?;
        text.push_str(&format!(
            "{{\"dataset\":\"um-kline\",\"key\":\"{open}\",\"rejected\":\"open=2\",\"stored\":\"open=1\",\"symbol\":\"BTCUSDT\"}}\n"
        ));
        fs::write(&log, text).map_err(show)?;
        let err =
            load_range(dir.path(), &published, range, VerifyMode::Full).expect_err("unlisted");
        assert!(matches!(err, LoadError::UnlistedConflict { .. }), "{err}");
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(8))]
        #[test]
        fn flipping_a_manifest_byte_mismatches_the_pinned_digest(index in 0usize..4096) {
            let dir = tempfile::tempdir().unwrap();
            let published = publish_fixture(dir.path()).unwrap();
            let path = manifest_path(dir.path(), &dataset(), published.published.root);
            let mut bytes = fs::read(&path).unwrap();
            let at = index % bytes.len();
            bytes[at] ^= 0x01;
            fs::write(&path, &bytes).unwrap();
            let range = DayRange::new(published.day, published.day).unwrap();
            let err = load_range(dir.path(), &published.published, range, VerifyMode::Full).expect_err("digest");
            prop_assert!(matches!(err, LoadError::ManifestDigestMismatch));
        }

        #[test]
        fn a_bumped_volume_is_a_partition_hash_mismatch(add in 1i64..32) {
            let dir = tempfile::tempdir().unwrap();
            let published = publish_fixture(dir.path()).unwrap();
            let path = snapshot(dir.path());
            chmod_writable(&path);
            let mut rows = store::read_klines_file(&path, "um-kline").unwrap();
            let bumped = rows[0].volume.ticks().saturating_add(add);
            rows[0].volume = parangsae_core::Qty::from_ticks(bumped).unwrap();
            store::write_klines(&path, "um-kline", &rows).unwrap();
            let range = DayRange::new(published.day, published.day).unwrap();
            let err = load_range(dir.path(), &published.published, range, VerifyMode::Full).expect_err("volume");
            let mismatch = matches!(err, LoadError::PartitionHashMismatch { .. });
            prop_assert!(mismatch);
        }
    }
}
