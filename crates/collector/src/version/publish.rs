//! Publish a closed date range as an immutable version.
//!
//! The snapshot is a copy, not a hard link. `chmod 0444` on a hard-linked
//! snapshot would also make the live partition read-only, and the live file
//! must stay replaceable.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use parangsae_core::{RunId, UtcMicros};

use crate::civil::{CivilDay, DayRange};
use crate::manifest::ManifestLog;
use crate::run::{self, Provenance, RunCommand};
use crate::store::{self, fact_path};
use crate::version::encode::{
    self, canonical_manifest, conflicts_digest, gaps_digest, kline_content_hash, leaf_hash,
    merkle_tree_head, schema_hash, sha256, sources_digest, ConflictDoc, ConflictInput, LeafInput,
    ManifestDoc, PartitionDoc, SourceInput, WindowDoc, UM_KLINE_COLUMNS,
};
use crate::version::id::{DatasetId, ManifestDigest, VersionRoot};
use crate::vision::{
    basename, classify, kline_day_gaps, missing_coverage, Classified, Gap, KlineFact, VisionDataset,
};

pub struct PublishRequest<'a> {
    pub data_dir: &'a Path,
    pub dataset: &'a DatasetId,
    pub range: DayRange,
    pub provenance: &'a Provenance,
    pub run: RunId,
    pub built_at: UtcMicros,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published {
    pub root: VersionRoot,
    pub manifest_digest: ManifestDigest,
    pub attestation_digest: [u8; 32],
}

#[derive(Debug)]
pub enum PublishError {
    UnknownProvenance,
    DirtyProvenance,
    StaleGapCache { day: CivilDay },
    SnapshotCorrupt,
    ManifestExists,
    UnsupportedDataset,
    SpecVersionMismatch,
    Failed(String),
}

impl PublishError {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::UnknownProvenance => "UnknownProvenance",
            Self::DirtyProvenance => "DirtyProvenance",
            Self::StaleGapCache { .. } => "StaleGapCache",
            Self::SnapshotCorrupt => "SnapshotCorrupt",
            Self::ManifestExists => "ManifestExists",
            Self::UnsupportedDataset => "UnsupportedDataset",
            Self::SpecVersionMismatch => "SpecVersionMismatch",
            Self::Failed(_) => "Failed",
        }
    }
}

impl std::fmt::Display for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownProvenance => write!(f, "provenance sha is unknown"),
            Self::DirtyProvenance => write!(f, "provenance is dirty"),
            Self::StaleGapCache { day } => write!(f, "stale gap cache for {day}"),
            Self::SnapshotCorrupt => write!(f, "snapshot content hash does not match"),
            Self::ManifestExists => write!(f, "manifest already exists with different bytes"),
            Self::UnsupportedDataset => write!(f, "publish supports um-kline"),
            Self::SpecVersionMismatch => {
                write!(f, "spec version is not the one for this dataset")
            }
            Self::Failed(detail) => write!(f, "{detail}"),
        }
    }
}

pub fn publish(request: &PublishRequest<'_>) -> Result<Published, PublishError> {
    if request.provenance.dirty {
        return Err(PublishError::DirtyProvenance);
    }
    if !clean_sha(&request.provenance.git_sha) {
        return Err(PublishError::UnknownProvenance);
    }
    let dataset_name = request.dataset.dataset();
    if encode::manifest_spec(dataset_name).is_none() {
        return Err(PublishError::SpecVersionMismatch);
    }
    if dataset_name != "um-kline" {
        return Err(PublishError::UnsupportedDataset);
    }
    let schema = schema_hash(request.dataset.as_str(), UM_KLINE_COLUMNS).map_err(failed)?;
    let days = request.range.days();
    let mut built = Vec::with_capacity(days.len());
    for day in days {
        built.push(build_day(request, dataset_name, day, &schema)?);
    }
    for day in &built {
        if let Some(stored) = stored_gaps(
            request.data_dir,
            dataset_name,
            request.dataset.symbol(),
            day.day,
        )? {
            if !same_gaps(&stored, &day.gaps) {
                return Err(PublishError::StaleGapCache { day: day.day });
            }
        }
    }
    let mut file_sha = Vec::with_capacity(built.len());
    for day in &built {
        file_sha.push(retain_snapshot(request.data_dir, dataset_name, day)?);
    }
    let leaves: Vec<[u8; 32]> = built.iter().map(|day| day.leaf).collect();
    let root_bytes = merkle_tree_head(&leaves);
    let docs: Vec<PartitionDoc<'_>> = built
        .iter()
        .map(|day| PartitionDoc {
            date: day.date.as_str(),
            path: day.relative_path.as_str(),
            row_count: day.row_count,
            pk_min_us: day.pk_min_us,
            pk_max_us: day.pk_max_us,
            available_min_us: day.available_min_us,
            available_max_us: day.available_max_us,
            content_hash: &day.content,
            gaps: &day.gaps,
            sources: &day.sources,
            conflicts: &day.conflict_docs,
            excluded_windows: &day.windows,
            leaf_hash: &day.leaf,
            gaps_digest: &day.gap_hash,
            conflicts_digest: &day.conflict_hash,
            sources_digest: &day.source_hash,
        })
        .collect();
    let rows = built.iter().try_fold(0u64, |sum, day| {
        sum.checked_add(day.row_count)
            .ok_or_else(|| PublishError::Failed("row count overflow".into()))
    })?;
    let bytes = canonical_manifest(&ManifestDoc {
        dataset_id: request.dataset.as_str(),
        columns: UM_KLINE_COLUMNS,
        schema_hash: &schema,
        version_root: &root_bytes,
        first_date: built.first().map(|day| day.date.as_str()).unwrap_or(""),
        last_date: built.last().map(|day| day.date.as_str()).unwrap_or(""),
        partitions: &docs,
        rows,
    })
    .map_err(failed)?;
    let digest = encode::sha256(&bytes);
    write_manifest(request.data_dir, request.dataset, &root_bytes, &bytes)?;
    let flat_rows = built.iter().map(|day| day.flat_rows).sum::<u64>();
    let attestation = attestation_bytes(request, &built, &file_sha, flat_rows)?;
    let attestation_digest = encode::sha256(&attestation);
    write_attestation(
        request.data_dir,
        &root_bytes,
        &attestation_digest,
        &attestation,
    )?;
    run::append_run_log(
        request.data_dir,
        request.run,
        request.built_at,
        request.provenance,
        RunCommand::Publish,
    )
    .map_err(failed)?;
    let _ = fs::remove_dir_all(request.data_dir.join("staging"));
    Ok(Published {
        root: VersionRoot::from_bytes(root_bytes),
        manifest_digest: ManifestDigest::from_bytes(digest),
        attestation_digest,
    })
}

struct BuiltDay {
    day: CivilDay,
    date: String,
    relative_path: String,
    row_count: u64,
    pk_min_us: i64,
    pk_max_us: i64,
    available_min_us: i64,
    available_max_us: i64,
    content: [u8; 32],
    gaps: Vec<Gap>,
    sources: Vec<SourceInput>,
    conflict_docs: Vec<ConflictDoc>,
    windows: Vec<WindowDoc>,
    gap_hash: [u8; 32],
    conflict_hash: [u8; 32],
    source_hash: [u8; 32],
    leaf: [u8; 32],
    staged: Option<PathBuf>,
    flat_rows: u64,
    dataset_id: String,
}

fn build_day(
    request: &PublishRequest<'_>,
    dataset_name: &str,
    day: CivilDay,
    schema: &[u8; 32],
) -> Result<BuiltDay, PublishError> {
    let live = fact_path(
        request.data_dir,
        dataset_name,
        request.dataset.symbol(),
        day,
    );
    let (rows, staged) = if live.exists() {
        let staged = stage(request.data_dir, &live, &day)?;
        let decoded = store::read_klines_file(&staged, dataset_name).map_err(failed)?;
        (decoded, Some(staged))
    } else {
        (Vec::new(), None)
    };
    let content = kline_content_hash(schema, &rows).map_err(failed)?;
    let gaps = if rows.is_empty() {
        vec![missing_coverage(day, false)]
    } else {
        kline_day_gaps(&rows, day)
    };
    let gap_hash = gaps_digest(&gaps).map_err(failed)?;
    let conflicts = day_conflicts(request, dataset_name, day)?;
    let sources = day_sources(request, dataset_name, day)?;
    let conflict_docs = hash_conflicts(&conflicts);
    let windows = encode::kline_excluded_windows(&conflict_docs);
    let conflict_hash = conflicts_digest(&conflicts).map_err(failed)?;
    let source_hash = sources_digest(&sources).map_err(failed)?;
    let (pk_min, pk_max, available_min, available_max) = bounds(&rows, day);
    let row_count =
        u64::try_from(rows.len()).map_err(|_| PublishError::Failed("row count".into()))?;
    let leaf = leaf_hash(&LeafInput {
        dataset_id: request.dataset.as_str(),
        symbol: request.dataset.symbol(),
        date: &day.to_string(),
        schema_hash: schema,
        row_count,
        pk_min_us: pk_min,
        pk_max_us: pk_max,
        content_hash: &content,
        gaps_digest: &gap_hash,
        conflicts_digest: &conflict_hash,
        sources_digest: &source_hash,
    })
    .map_err(failed)?;
    let flat_rows = u64::try_from(rows.iter().filter(|row| row.zero_volume_flat).count())
        .map_err(|_| PublishError::Failed("flat row count".into()))?;
    Ok(BuiltDay {
        date: day.to_string(),
        relative_path: format!(
            "parquet/{dataset_name}/symbol={}/date={day}/part.parquet",
            request.dataset.symbol()
        ),
        day,
        row_count,
        pk_min_us: pk_min,
        pk_max_us: pk_max,
        available_min_us: available_min,
        available_max_us: available_max,
        content,
        gaps,
        sources,
        conflict_docs,
        windows,
        gap_hash,
        conflict_hash,
        source_hash,
        leaf,
        staged,
        flat_rows,
        dataset_id: request.dataset.as_str().to_owned(),
    })
}

fn bounds(rows: &[KlineFact], day: CivilDay) -> (i64, i64, i64, i64) {
    if rows.is_empty() {
        let start = day.start_us().micros();
        return (start, start, start, start);
    }
    let pk_min = rows
        .iter()
        .map(|row| row.open_time.micros())
        .min()
        .unwrap_or(0);
    let pk_max = rows
        .iter()
        .map(|row| row.open_time.micros())
        .max()
        .unwrap_or(0);
    let available_min = rows
        .iter()
        .map(|row| row.available_ts.micros().micros())
        .min()
        .unwrap_or(0);
    let available_max = rows
        .iter()
        .map(|row| row.available_ts.micros().micros())
        .max()
        .unwrap_or(0);
    (pk_min, pk_max, available_min, available_max)
}

fn stage(data_dir: &Path, live: &Path, day: &CivilDay) -> Result<PathBuf, PublishError> {
    let dest = data_dir
        .join("staging")
        .join(day.to_string())
        .join("part.parquet");
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| PublishError::Failed(err.to_string()))?;
    }
    fs::copy(live, &dest).map_err(|err| PublishError::Failed(err.to_string()))?;
    Ok(dest)
}

fn retain_snapshot(
    data_dir: &Path,
    dataset_name: &str,
    day: &BuiltDay,
) -> Result<Option<[u8; 32]>, PublishError> {
    let Some(staged) = &day.staged else {
        return Ok(None);
    };
    let dest = data_dir
        .join("snapshots")
        .join(hex::encode(day.content))
        .join("part.parquet");
    if dest.exists() {
        let existing = store::read_klines_file(&dest, dataset_name).map_err(failed)?;
        let schema = schema_hash(&day.dataset_id, UM_KLINE_COLUMNS)
            .map_err(|err| PublishError::Failed(format!("{err:?}")))?;
        let actual = kline_content_hash(&schema, &existing)
            .map_err(|err| PublishError::Failed(format!("{err:?}")))?;
        if actual != day.content {
            return Err(PublishError::SnapshotCorrupt);
        }
        let _ = fs::remove_file(staged);
    } else {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|err| PublishError::Failed(err.to_string()))?;
        }
        fs::rename(staged, &dest).map_err(|err| PublishError::Failed(err.to_string()))?;
        let mode = fs::Permissions::from_mode(0o444);
        fs::set_permissions(&dest, mode).map_err(|err| PublishError::Failed(err.to_string()))?;
    }
    let bytes = fs::read(&dest).map_err(|err| PublishError::Failed(err.to_string()))?;
    Ok(Some(encode::sha256(&bytes)))
}

fn stored_gaps(
    data_dir: &Path,
    dataset: &str,
    symbol: &str,
    day: CivilDay,
) -> Result<Option<Vec<Gap>>, PublishError> {
    let path = store::gap_path(data_dir, dataset, symbol, day);
    if !path.exists() {
        return Ok(None);
    }
    store::load_gaps(data_dir, dataset, symbol, day)
        .map(Some)
        .map_err(failed)
}

fn same_gaps(stored: &[Gap], recomputed: &[Gap]) -> bool {
    let mut left: Vec<(&str, i64, i64)> = stored
        .iter()
        .map(|gap| (gap.kind.as_str(), gap.start.micros(), gap.end.micros()))
        .collect();
    let mut right: Vec<(&str, i64, i64)> = recomputed
        .iter()
        .map(|gap| (gap.kind.as_str(), gap.start.micros(), gap.end.micros()))
        .collect();
    left.sort();
    right.sort();
    left == right
}

fn day_conflicts(
    request: &PublishRequest<'_>,
    dataset_name: &str,
    day: CivilDay,
) -> Result<Vec<ConflictInput>, PublishError> {
    let logged = super::versions::logged_conflicts(
        request.data_dir,
        dataset_name,
        request.dataset.symbol(),
        day,
    )
    .map_err(failed)?;
    Ok(logged
        .into_iter()
        .map(|line| ConflictInput {
            pk: vec![line.pk],
            class: line.class,
            stored: line.stored,
            rejected: line.rejected,
        })
        .collect())
}

fn hash_conflicts(conflicts: &[ConflictInput]) -> Vec<ConflictDoc> {
    let mut docs: Vec<ConflictDoc> = conflicts
        .iter()
        .map(|conflict| ConflictDoc {
            pk: conflict.pk.clone(),
            class: conflict.class.clone(),
            stored_sha256: sha256(conflict.stored.as_bytes()),
            rejected_sha256: sha256(conflict.rejected.as_bytes()),
        })
        .collect();
    docs.sort_by(|left, right| {
        left.pk
            .cmp(&right.pk)
            .then(left.class.cmp(&right.class))
            .then(left.stored_sha256.cmp(&right.stored_sha256))
            .then(left.rejected_sha256.cmp(&right.rejected_sha256))
    });
    docs.dedup();
    docs
}

fn day_sources(
    request: &PublishRequest<'_>,
    dataset_name: &str,
    day: CivilDay,
) -> Result<Vec<SourceInput>, PublishError> {
    let path = request.data_dir.join("manifest").join("objects.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let log = ManifestLog::open(request.data_dir).map_err(failed)?;
    let vision = VisionDataset::parse(dataset_name).ok_or(PublishError::UnsupportedDataset)?;
    let mut out = Vec::new();
    for object in log.active() {
        if object.dataset != dataset_name || object.symbol != request.dataset.symbol() {
            continue;
        }
        let Classified::Archive(name) =
            classify(vision, &object.symbol, basename(&object.path)).map_err(failed)?
        else {
            continue;
        };
        let covers = match name.day {
            Some(archive_day) => archive_day == day,
            None => name.month.year == day.year() && name.month.month == day.month(),
        };
        if !covers {
            continue;
        }
        out.push(SourceInput {
            url: object.url.clone(),
            sha256: object.sha256,
        });
    }
    Ok(out)
}

fn write_manifest(
    data_dir: &Path,
    dataset: &DatasetId,
    root: &[u8; 32],
    bytes: &[u8],
) -> Result<(), PublishError> {
    let path = data_dir
        .join("manifest")
        .join("versions")
        .join(dataset.as_str())
        .join(format!("{}.json", hex::encode(root)));
    if path.exists() {
        let existing = fs::read(&path).map_err(|err| PublishError::Failed(err.to_string()))?;
        if existing == bytes {
            return Ok(());
        }
        return Err(PublishError::ManifestExists);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| PublishError::Failed(err.to_string()))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|err| PublishError::Failed(err.to_string()))?;
    file.write_all(bytes)
        .map_err(|err| PublishError::Failed(err.to_string()))?;
    Ok(())
}

fn attestation_bytes(
    request: &PublishRequest<'_>,
    days: &[BuiltDay],
    file_sha: &[Option<[u8; 32]>],
    flat_rows: u64,
) -> Result<Vec<u8>, PublishError> {
    let mut partitions = Vec::new();
    for (day, sha) in days.iter().zip(file_sha.iter()) {
        let Some(sha) = sha else {
            continue;
        };
        let mut object = serde_json::Map::new();
        object.insert(
            "date".to_owned(),
            serde_json::Value::String(day.date.clone()),
        );
        object.insert(
            "file_sha256".to_owned(),
            serde_json::Value::String(hex::encode(sha)),
        );
        object.insert(
            "snapshot_path".to_owned(),
            serde_json::Value::String(format!(
                "snapshots/{}/part.parquet",
                hex::encode(day.content)
            )),
        );
        partitions.push(serde_json::Value::Object(object));
    }
    let mut root = serde_json::Map::new();
    root.insert(
        "built_at_us".to_owned(),
        serde_json::Value::String(request.built_at.micros().to_string()),
    );
    root.insert(
        "collector_dirty".to_owned(),
        serde_json::Value::Bool(request.provenance.dirty),
    );
    root.insert(
        "collector_git_sha".to_owned(),
        serde_json::Value::String(request.provenance.git_sha.clone()),
    );
    root.insert(
        "partitions".to_owned(),
        serde_json::Value::Array(partitions),
    );
    root.insert(
        "run_id".to_owned(),
        serde_json::Value::String(request.run.to_hex()),
    );
    root.insert(
        "zero_volume_flat_rows".to_owned(),
        serde_json::Value::String(flat_rows.to_string()),
    );
    serde_json::to_vec(&serde_json::Value::Object(root))
        .map_err(|err| PublishError::Failed(err.to_string()))
}

fn write_attestation(
    data_dir: &Path,
    root: &[u8; 32],
    digest: &[u8; 32],
    bytes: &[u8],
) -> Result<(), PublishError> {
    let path = data_dir
        .join("manifest")
        .join("attestations")
        .join(hex::encode(root))
        .join(format!("{}.json", hex::encode(digest)));
    if path.exists() {
        let existing = fs::read(&path).map_err(|err| PublishError::Failed(err.to_string()))?;
        if existing == bytes {
            return Ok(());
        }
        return Err(PublishError::Failed(
            "attestation path exists with different bytes".into(),
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| PublishError::Failed(err.to_string()))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|err| PublishError::Failed(err.to_string()))?;
    file.write_all(bytes)
        .map_err(|err| PublishError::Failed(err.to_string()))?;
    Ok(())
}

fn clean_sha(sha: &str) -> bool {
    sha.len() == 40
        && sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !matches!(byte, b'A'..=b'F'))
}

fn failed(err: impl std::fmt::Display) -> PublishError {
    PublishError::Failed(err.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    use arrow_array::{Array, Int64Array, RecordBatch};
    use arrow_schema::{Field, Schema};
    use parangsae_core::{Amount, AvailableTs, EventTs, Qty, Receipt, RunId, Symbol, UtcMicros};
    use parquet::basic::Compression;
    use std::sync::Arc;

    use super::*;
    use crate::civil::day_of;
    use crate::normalize::UM_KLINE_CLOCK;
    use crate::version::encode::{kline_content_hash, schema_hash, UM_KLINE_COLUMNS};
    use crate::version::migrate::directory_digest;
    use crate::vision::{parse_klines, TickDomain};

    const DATASET: &str = "binance-vision.um.um-kline.BTCUSDT";
    const CLEAN_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn show(err: impl std::fmt::Display) -> String {
        err.to_string()
    }

    fn clean() -> Provenance {
        Provenance {
            git_sha: CLEAN_SHA.to_owned(),
            dirty: false,
        }
    }

    fn dataset() -> Result<DatasetId, String> {
        DatasetId::parse(DATASET).map_err(show)
    }

    fn fixture_rows(receipt: Receipt) -> Result<(CivilDay, Vec<KlineFact>), String> {
        let csv = fs::read_to_string(format!(
            "{}/../../fixtures/vision/um_kline_2021-06-01_headerless.csv",
            env!("CARGO_MANIFEST_DIR")
        ))
        .map_err(show)?;
        let symbol = Symbol::new("BTCUSDT").map_err(show)?;
        let parsed = parse_klines(
            &csv,
            &symbol,
            UM_KLINE_CLOCK,
            TickDomain::Price,
            receipt,
            true,
        )
        .map_err(show)?;
        let day = day_of(parsed.rows[0].open_time).map_err(show)?;
        Ok((day, parsed.rows))
    }

    fn ingest(dir: &Path, receipt: Receipt) -> Result<CivilDay, String> {
        let (day, rows) = fixture_rows(receipt)?;
        store::upsert_klines(dir, "um-kline", rows).map_err(show)?;
        Ok(day)
    }

    fn publish_at(
        dir: &Path,
        day: CivilDay,
        run: RunId,
        built_at: i64,
        provenance: &Provenance,
    ) -> Result<Published, PublishError> {
        let dataset = dataset().expect("dataset id");
        let range = DayRange::new(day, day).expect("range");
        publish(&PublishRequest {
            data_dir: dir,
            dataset: &dataset,
            range,
            provenance,
            run,
            built_at: UtcMicros::from_micros(built_at),
        })
    }

    fn manifest_file(dir: &Path, root: VersionRoot) -> PathBuf {
        dir.join("manifest")
            .join("versions")
            .join(DATASET)
            .join(format!("{}.json", root.to_hex()))
    }

    fn snapshot_files(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let root = dir.join("snapshots");
        let Ok(entries) = fs::read_dir(root) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path().join("part.parquet");
            if path.is_file() {
                out.push(path);
            }
        }
        out.sort();
        out
    }

    fn inode(path: &Path) -> u64 {
        fs::metadata(path).expect("metadata").ino()
    }

    fn paint(rows: Vec<KlineFact>, px: i64) -> Vec<KlineFact> {
        rows.into_iter()
            .map(|mut row| {
                row.open_ticks = px;
                row.high_ticks = px;
                row.low_ticks = px;
                row.close_ticks = px;
                row
            })
            .collect()
    }

    fn temp() -> Result<tempfile::TempDir, String> {
        tempfile::tempdir().map_err(show)
    }

    #[test]
    fn same_fixtures_different_run_and_time_publish_the_same_manifest() -> Result<(), String> {
        // `--jobs` only parallelises the fetch. It is not an input to publish,
        // so two ingests below differ in RunId and built_at instead.
        let provenance = clean();
        let dir_a = temp()?;
        let dir_b = temp()?;
        let day_a = ingest(
            dir_a.path(),
            Receipt::Archive {
                fetched_at_run: RunId::from_bytes([1; 16]),
            },
        )?;
        let day_b = ingest(
            dir_b.path(),
            Receipt::Archive {
                fetched_at_run: RunId::from_bytes([2; 16]),
            },
        )?;
        assert_eq!(day_a, day_b);
        let first = publish_at(
            dir_a.path(),
            day_a,
            RunId::from_bytes([3; 16]),
            10,
            &provenance,
        )
        .map_err(show)?;
        let second = publish_at(
            dir_b.path(),
            day_b,
            RunId::from_bytes([4; 16]),
            20,
            &provenance,
        )
        .map_err(show)?;
        assert_eq!(first.root, second.root);
        let bytes_a = fs::read(manifest_file(dir_a.path(), first.root)).map_err(show)?;
        let bytes_b = fs::read(manifest_file(dir_b.path(), second.root)).map_err(show)?;
        assert_eq!(bytes_a, bytes_b);
        assert_ne!(
            first.attestation_digest, second.attestation_digest,
            "built_at is attestation-only"
        );

        let snaps = snapshot_files(dir_a.path());
        assert_eq!(snaps.len(), 1);
        let snap_ino = inode(&snaps[0]);
        let snap_bytes = fs::read(&snaps[0]).map_err(show)?;
        let foreign =
            fs::read(fact_path(dir_b.path(), "um-kline", "BTCUSDT", day_b)).map_err(show)?;
        assert_ne!(
            foreign, snap_bytes,
            "the other run id must change file bytes"
        );
        fs::write(
            fact_path(dir_a.path(), "um-kline", "BTCUSDT", day_a),
            &foreign,
        )
        .map_err(show)?;
        let reused = publish_at(
            dir_a.path(),
            day_a,
            RunId::from_bytes([5; 16]),
            30,
            &provenance,
        )
        .map_err(show)?;
        assert_eq!(reused.root, first.root);
        let again = snapshot_files(dir_a.path());
        assert_eq!(again.len(), 1);
        assert_eq!(inode(&again[0]), snap_ino);
        assert_eq!(fs::read(&again[0]).map_err(show)?, snap_bytes);
        Ok(())
    }

    #[test]
    fn content_hash_ignores_parquet_compression() -> Result<(), String> {
        let (_, rows) = fixture_rows(Receipt::Archive {
            fetched_at_run: RunId::from_bytes([1; 16]),
        })?;
        let dir = temp()?;
        let snappy = dir.path().join("snappy.parquet");
        let plain = dir.path().join("plain.parquet");
        store::write_klines_compressed(&snappy, "um-kline", &rows, Compression::SNAPPY)
            .map_err(show)?;
        store::write_klines_compressed(&plain, "um-kline", &rows, Compression::UNCOMPRESSED)
            .map_err(show)?;
        assert_ne!(
            fs::read(&snappy).map_err(show)?,
            fs::read(&plain).map_err(show)?
        );
        let schema = schema_hash(DATASET, UM_KLINE_COLUMNS).map_err(show)?;
        let left = store::read_klines_file(&snappy, "um-kline").map_err(show)?;
        let right = store::read_klines_file(&plain, "um-kline").map_err(show)?;
        let left_hash = kline_content_hash(&schema, &left).map_err(show)?;
        let right_hash = kline_content_hash(&schema, &right).map_err(show)?;
        assert_eq!(left_hash, right_hash);
        Ok(())
    }

    #[test]
    fn live_receipt_time_changes_the_root() -> Result<(), String> {
        let provenance = clean();
        let early = temp()?;
        let late = temp()?;
        let day = ingest(
            early.path(),
            Receipt::Live {
                received_ts: UtcMicros::from_micros(1),
            },
        )?;
        ingest(
            late.path(),
            Receipt::Live {
                received_ts: UtcMicros::from_micros(2),
            },
        )?;
        let left = publish_at(
            early.path(),
            day,
            RunId::from_bytes([3; 16]),
            1,
            &provenance,
        )
        .map_err(show)?;
        let right = publish_at(late.path(), day, RunId::from_bytes([3; 16]), 1, &provenance)
            .map_err(show)?;
        assert_ne!(left.root, right.root);
        Ok(())
    }

    #[test]
    fn archive_and_live_receipts_hash_differently() -> Result<(), String> {
        let provenance = clean();
        let archived = temp()?;
        let live = temp()?;
        let day = ingest(
            archived.path(),
            Receipt::Archive {
                fetched_at_run: RunId::from_bytes([1; 16]),
            },
        )?;
        ingest(
            live.path(),
            Receipt::Live {
                received_ts: UtcMicros::from_micros(1),
            },
        )?;
        let left = publish_at(
            archived.path(),
            day,
            RunId::from_bytes([3; 16]),
            1,
            &provenance,
        )
        .map_err(show)?;
        let right = publish_at(live.path(), day, RunId::from_bytes([3; 16]), 1, &provenance)
            .map_err(show)?;
        assert_ne!(left.root, right.root);
        Ok(())
    }

    /// First stored payload wins, so daily-before-monthly and monthly-before-daily
    /// publish different roots. `data-conflicts.md` is the canonical-source policy
    /// that will remove this order dependence. This test asserts the current
    /// inequality on purpose. When that policy lands and the two roots become
    /// equal, the assertion below fails: flip it to `assert_eq` and delete this
    /// expected-failure note. Do not `#[ignore]` the test.
    #[test]
    fn archive_order_changes_the_root_until_canonical_source() -> Result<(), String> {
        let (_, rows) = fixture_rows(Receipt::Archive {
            fetched_at_run: RunId::from_bytes([1; 16]),
        })?;
        let daily = paint(rows.clone(), 100);
        let monthly = paint(rows, 200);
        let daily_first = temp()?;
        let monthly_first = temp()?;
        store::upsert_klines(daily_first.path(), "um-kline", daily.clone()).map_err(show)?;
        store::upsert_klines(daily_first.path(), "um-kline", monthly.clone()).map_err(show)?;
        store::upsert_klines(monthly_first.path(), "um-kline", monthly).map_err(show)?;
        store::upsert_klines(monthly_first.path(), "um-kline", daily).map_err(show)?;
        let day = CivilDay::new(2021, 6, 1).expect("day");
        let stored_daily =
            store::load_klines(daily_first.path(), "um-kline", "BTCUSDT", day).map_err(show)?;
        let stored_monthly =
            store::load_klines(monthly_first.path(), "um-kline", "BTCUSDT", day).map_err(show)?;
        assert_eq!(stored_daily[0].open_ticks, 100);
        assert_eq!(stored_monthly[0].open_ticks, 200);
        let provenance = clean();
        let left = publish_at(
            daily_first.path(),
            day,
            RunId::from_bytes([7; 16]),
            1,
            &provenance,
        )
        .map_err(show)?;
        let right = publish_at(
            monthly_first.path(),
            day,
            RunId::from_bytes([7; 16]),
            1,
            &provenance,
        )
        .map_err(show)?;
        assert_ne!(
            left.root, right.root,
            "daily-before-monthly and monthly-before-daily published the same root. \
             A canonical-source policy may have landed; see data-conflicts.md. \
             Flip this assertion to assert_eq and delete the expected-failure note."
        );
        Ok(())
    }

    #[test]
    fn stale_gap_cache_refuses_a_complete_day() -> Result<(), String> {
        let day = CivilDay::new(2021, 6, 1).expect("day");
        let rows = complete_minutes(day)?;
        let open = temp()?;
        store::write_klines(
            &fact_path(open.path(), "um-kline", "BTCUSDT", day),
            "um-kline",
            &rows,
        )
        .map_err(show)?;
        let provenance = clean();
        publish_at(open.path(), day, RunId::from_bytes([1; 16]), 1, &provenance).map_err(show)?;

        let stale = temp()?;
        store::write_klines(
            &fact_path(stale.path(), "um-kline", "BTCUSDT", day),
            "um-kline",
            &rows,
        )
        .map_err(show)?;
        store::write_gaps(
            stale.path(),
            "um-kline",
            "BTCUSDT",
            day,
            &[missing_coverage(day, false)],
        )
        .map_err(show)?;
        let err = publish_at(
            stale.path(),
            day,
            RunId::from_bytes([1; 16]),
            1,
            &provenance,
        )
        .expect_err("stale gaps must refuse");
        match err {
            PublishError::StaleGapCache { day: got } if got == day => {}
            other => return Err(format!("expected StaleGapCache, got {other}")),
        }
        assert!(!stale.path().join("snapshots").exists());
        assert!(!stale.path().join("manifest").join("versions").exists());
        Ok(())
    }

    #[test]
    fn dirty_or_unknown_provenance_writes_nothing() -> Result<(), String> {
        let dir = temp()?;
        let day = ingest(
            dir.path(),
            Receipt::Archive {
                fetched_at_run: RunId::from_bytes([1; 16]),
            },
        )?;
        let before = directory_digest(dir.path()).map_err(show)?;
        let dirty = Provenance {
            git_sha: CLEAN_SHA.to_owned(),
            dirty: true,
        };
        let err =
            publish_at(dir.path(), day, RunId::from_bytes([2; 16]), 1, &dirty).expect_err("dirty");
        assert!(matches!(err, PublishError::DirtyProvenance));
        assert_eq!(directory_digest(dir.path()).map_err(show)?, before);
        let unknown = Provenance {
            git_sha: "unknown".to_owned(),
            dirty: false,
        };
        let err = publish_at(dir.path(), day, RunId::from_bytes([2; 16]), 1, &unknown)
            .expect_err("unknown");
        assert!(matches!(err, PublishError::UnknownProvenance));
        let upper = Provenance {
            git_sha: CLEAN_SHA.to_uppercase(),
            dirty: false,
        };
        let err = publish_at(dir.path(), day, RunId::from_bytes([2; 16]), 1, &upper)
            .expect_err("uppercase");
        assert!(matches!(err, PublishError::UnknownProvenance));
        assert_eq!(directory_digest(dir.path()).map_err(show)?, before);
        Ok(())
    }

    #[test]
    fn published_snapshot_survives_a_live_rewrite() -> Result<(), String> {
        let provenance = clean();
        let dir = temp()?;
        let day = ingest(
            dir.path(),
            Receipt::Archive {
                fetched_at_run: RunId::from_bytes([1; 16]),
            },
        )?;
        publish_at(dir.path(), day, RunId::from_bytes([2; 16]), 1, &provenance).map_err(show)?;
        let snaps = snapshot_files(dir.path());
        assert_eq!(snaps.len(), 1);
        let snap_ino = inode(&snaps[0]);
        let snap_bytes = fs::read(&snaps[0]).map_err(show)?;
        let mode = fs::metadata(&snaps[0]).map_err(show)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o444);
        let original =
            store::read_klines_file(&snaps[0], "um-kline").map_err(|err| format!("snap {err}"))?;
        let live = fact_path(dir.path(), "um-kline", "BTCUSDT", day);
        let live_ino = inode(&live);
        let batches = store::read_batches(&live).map_err(|err| format!("live {err}"))?;
        assert_eq!(batches.len(), 1);
        let rewritten = bump_i64(&batches[0], "open_ticks")?;
        store::write_batch(&live, rewritten).map_err(show)?;
        assert_ne!(inode(&live), live_ino, "write_batch must change the inode");
        assert_eq!(inode(&snaps[0]), snap_ino);
        assert_eq!(fs::read(&snaps[0]).map_err(show)?, snap_bytes);
        let loaded = store::read_klines_file(&snaps[0], "um-kline").map_err(show)?;
        assert_eq!(loaded, original);
        let live_rows = store::read_klines_file(&live, "um-kline").map_err(show)?;
        assert_ne!(live_rows[0].open_ticks, original[0].open_ticks);
        Ok(())
    }

    #[test]
    fn manifest_write_once_rejects_different_bytes() -> Result<(), String> {
        let provenance = clean();
        let dir = temp()?;
        let day = ingest(
            dir.path(),
            Receipt::Archive {
                fetched_at_run: RunId::from_bytes([1; 16]),
            },
        )?;
        let run = RunId::from_bytes([2; 16]);
        let first = publish_at(dir.path(), day, run, 1, &provenance).map_err(show)?;
        let second = publish_at(dir.path(), day, run, 1, &provenance).map_err(show)?;
        assert_eq!(first, second);
        let path = manifest_file(dir.path(), first.root);
        fs::write(&path, b"forged").map_err(show)?;
        let err = publish_at(dir.path(), day, run, 1, &provenance).expect_err("exists");
        assert!(matches!(err, PublishError::ManifestExists));
        assert_eq!(fs::read(&path).map_err(show)?, b"forged");
        Ok(())
    }

    fn complete_minutes(day: CivilDay) -> Result<Vec<KlineFact>, String> {
        let symbol = Symbol::new("BTCUSDT").map_err(show)?;
        let start = day.start_us().micros();
        let mut rows = Vec::with_capacity(1440);
        for index in 0..1440 {
            let open = start + index * 60_000_000;
            let close = open + 60_000_000 - 1_000;
            rows.push(KlineFact {
                symbol: symbol.clone(),
                interval_us: 60_000_000,
                open_time: UtcMicros::from_micros(open),
                close_time: UtcMicros::from_micros(close),
                open_ticks: 1,
                high_ticks: 1,
                low_ticks: 1,
                close_ticks: 1,
                volume: Qty::from_ticks(1).map_err(show)?,
                quote_volume: Amount::from_ticks(1).map_err(show)?,
                count: 1,
                taker_buy_volume: Qty::from_ticks(0).map_err(show)?,
                taker_buy_quote_volume: Amount::from_ticks(0).map_err(show)?,
                event_ts: EventTs::from_micros(UtcMicros::from_micros(open)),
                available_ts: AvailableTs::from_micros(UtcMicros::from_micros(close + 1_000)),
                receipt: Receipt::Archive {
                    fetched_at_run: RunId::from_bytes([1; 16]),
                },
                schema_version: 1,
                tick_domain: TickDomain::Price,
                zero_volume_flat: false,
            });
        }
        Ok(rows)
    }

    fn bump_i64(batch: &RecordBatch, name: &str) -> Result<RecordBatch, String> {
        let index = batch.schema().index_of(name).map_err(show)?;
        let column = batch
            .column(index)
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| format!("{name} is not int64"))?;
        let values: Vec<i64> = (0..column.len())
            .map(|row| column.value(row).saturating_add(1))
            .collect();
        let mut columns = batch.columns().to_vec();
        columns[index] = Arc::new(Int64Array::from(values));
        // The reader batch does not carry file metadata. Put the rows/v2 keys
        // back so write_batch still produces a file the v2 reader accepts.
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("parangsae.scale".to_owned(), "100000000".to_owned());
        metadata.insert("parangsae.row_encoding".to_owned(), "rows/v2".to_owned());
        let fields: Vec<Field> = batch
            .schema()
            .fields()
            .iter()
            .map(|field| Field::new(field.name(), field.data_type().clone(), field.is_nullable()))
            .collect();
        let schema = Arc::new(Schema::new_with_metadata(fields, metadata));
        RecordBatch::try_new(schema, columns).map_err(show)
    }
}
