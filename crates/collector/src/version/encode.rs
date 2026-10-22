//! Canonical hashing for a dataset version. Compression and receipt run ids
//! stay outside the hash. A non-increasing primary key is rejected, not sorted.

use sha2::{Digest, Sha256};

use super::id::DatasetId;
use crate::vision::{Gap, KlineFact};
use parangsae_core::Receipt;

pub const ROW_ENCODING_VERSION: &str = "rows/v2";
pub const AVAILABLE_TS_RULES_VERSION: &str = "available_ts_rules/v0.2";
pub const GAP_RULES_VERSION: &str = "gap_rules/v1";
pub const CONFLICT_RULES_VERSION: &str = "conflict_rules/v2";
pub const CONTRACT_VERSION: &str = "data-contract v0.2";
/// Fifth metrics rule string. 68 ASCII bytes. Kline, funding, and book do not hash it.
pub const METRIC_MISSING_RULES: &str =
    "metric_missing_rules/v1 merge=across_utc_days long_null_run_slots=12";
/// Rule tuple for the v2.2 metrics goldens. Not the kline tuple.
pub const METRICS_SCHEMA_RULES: [&str; 5] = [
    ROW_ENCODING_VERSION,
    "available_ts_rules/v0.3",
    "gap_rules/v2",
    CONFLICT_RULES_VERSION,
    METRIC_MISSING_RULES,
];
pub const MANIFEST_SPEC_V2_1: &str = "parangsae/dataset-manifest/v2.1";
pub const MANIFEST_SPEC_V2_2: &str = "parangsae/dataset-manifest/v2.2";
/// Kline, funding, and book manifests. Metrics uses [`MANIFEST_SPEC_V2_2`].
pub const MANIFEST_SPEC: &str = MANIFEST_SPEC_V2_1;

/// Spec string for one dataset name.
///
/// The §3 table is exactly um-kline, mark-price-kline, funding-rate, and
/// book-depth on v2.1, and metrics on v2.2. Every other name is `None`.
pub fn manifest_spec(dataset: &str) -> Option<&'static str> {
    match dataset {
        "um-kline" | "mark-price-kline" | "funding-rate" | "book-depth" => Some(MANIFEST_SPEC_V2_1),
        "metrics" => Some(MANIFEST_SPEC_V2_2),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    pub name: &'static str,
    pub ty: &'static str,
}

pub const UM_KLINE_COLUMNS: &[Column] = &[
    Column {
        name: "symbol",
        ty: "utf8",
    },
    Column {
        name: "interval_us",
        ty: "i64",
    },
    Column {
        name: "open_time_us",
        ty: "i64",
    },
    Column {
        name: "close_time_us",
        ty: "i64",
    },
    Column {
        name: "open_ticks",
        ty: "i64",
    },
    Column {
        name: "high_ticks",
        ty: "i64",
    },
    Column {
        name: "low_ticks",
        ty: "i64",
    },
    Column {
        name: "close_ticks",
        ty: "i64",
    },
    Column {
        name: "volume_ticks",
        ty: "i64",
    },
    Column {
        name: "quote_volume_ticks",
        ty: "i64",
    },
    Column {
        name: "count",
        ty: "i64",
    },
    Column {
        name: "taker_buy_volume_ticks",
        ty: "i64",
    },
    Column {
        name: "taker_buy_quote_volume_ticks",
        ty: "i64",
    },
    Column {
        name: "event_ts_us",
        ty: "i64",
    },
    Column {
        name: "available_ts_us",
        ty: "i64",
    },
    Column {
        name: "receipt",
        ty: "receipt",
    },
    Column {
        name: "schema_version",
        ty: "u32",
    },
    Column {
        name: "tick_domain",
        ty: "utf8",
    },
    Column {
        name: "zero_volume_flat",
        ty: "bool",
    },
];

/// Metrics columns for the v2.2 schema hash. Types are the spec strings, not Parquet types.
pub const METRICS_COLUMNS: &[Column] = &[
    Column {
        name: "symbol",
        ty: "utf8",
    },
    Column {
        name: "create_time_us",
        ty: "i64",
    },
    Column {
        name: "sum_open_interest_ticks",
        ty: "opt<i64>",
    },
    Column {
        name: "sum_open_interest_value_ticks",
        ty: "opt<i64>",
    },
    Column {
        name: "count_toptrader_long_short_ratio",
        ty: "opt<dec128s16>",
    },
    Column {
        name: "sum_toptrader_long_short_ratio",
        ty: "opt<dec128s16>",
    },
    Column {
        name: "count_long_short_ratio",
        ty: "opt<dec128s16>",
    },
    Column {
        name: "sum_taker_long_short_vol_ratio",
        ty: "opt<dec128s16>",
    },
    Column {
        name: "missing_tokens",
        ty: "opt<utf8>",
    },
    Column {
        name: "event_ts_us",
        ty: "i64",
    },
    Column {
        name: "available_ts_us",
        ty: "i64",
    },
    Column {
        name: "receipt",
        ty: "receipt",
    },
    Column {
        name: "schema_version",
        ty: "u32",
    },
];

/// One metrics row in the v2.2 content hash. `None` is NULL (`0x00`), not zero.
pub struct MetricsCanonicalRow<'a> {
    pub symbol: &'a str,
    pub create_time_us: i64,
    pub open_interest_ticks: Option<i64>,
    pub open_interest_value_ticks: Option<i64>,
    pub count_toptrader_long_short_ratio: Option<i128>,
    pub sum_toptrader_long_short_ratio: Option<i128>,
    pub count_long_short_ratio: Option<i128>,
    pub sum_taker_long_short_vol_ratio: Option<i128>,
    pub missing_tokens: Option<&'a str>,
    pub event_ts_us: i64,
    pub available_ts_us: i64,
    pub receipt: Receipt,
    pub schema_version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeError {
    PkOrder,
    DuplicateGap,
    DuplicateConflict,
    Length,
    Column(&'static str),
    SpecVersionMismatch,
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PkOrder => write!(f, "primary key is not strictly increasing"),
            Self::DuplicateGap => write!(f, "duplicate gap"),
            Self::DuplicateConflict => write!(f, "duplicate conflict"),
            Self::Length => write!(f, "length does not fit"),
            Self::Column(name) => write!(f, "column {name}"),
            Self::SpecVersionMismatch => write!(f, "spec version is not the one for this dataset"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BundleError {
    DuplicateDataset,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictInput {
    pub pk: Vec<i64>,
    pub class: String,
    pub stored: String,
    pub rejected: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceInput {
    pub url: String,
    pub sha256: [u8; 32],
}

pub struct LeafInput<'a> {
    pub dataset_id: &'a str,
    pub symbol: &'a str,
    pub date: &'a str,
    pub schema_hash: &'a [u8; 32],
    pub row_count: u64,
    pub pk_min_us: i64,
    pub pk_max_us: i64,
    pub content_hash: &'a [u8; 32],
    pub gaps_digest: &'a [u8; 32],
    pub conflicts_digest: &'a [u8; 32],
    pub sources_digest: &'a [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictDoc {
    pub pk: Vec<i64>,
    pub class: String,
    pub stored_sha256: [u8; 32],
    pub rejected_sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowDoc {
    pub start_us: i64,
    pub end_us: i64,
    pub reason: &'static str,
}

pub struct PartitionDoc<'a> {
    pub date: &'a str,
    pub path: &'a str,
    pub row_count: u64,
    pub pk_min_us: i64,
    pub pk_max_us: i64,
    pub available_min_us: i64,
    pub available_max_us: i64,
    pub content_hash: &'a [u8; 32],
    pub gaps: &'a [Gap],
    pub sources: &'a [SourceInput],
    pub conflicts: &'a [ConflictDoc],
    pub excluded_windows: &'a [WindowDoc],
    pub leaf_hash: &'a [u8; 32],
    pub gaps_digest: &'a [u8; 32],
    pub conflicts_digest: &'a [u8; 32],
    pub sources_digest: &'a [u8; 32],
}

pub struct ManifestDoc<'a> {
    pub dataset_id: &'a str,
    pub columns: &'a [Column],
    pub schema_hash: &'a [u8; 32],
    pub version_root: &'a [u8; 32],
    pub first_date: &'a str,
    pub last_date: &'a str,
    pub partitions: &'a [PartitionDoc<'a>],
    pub rows: u64,
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn schema_hash(dataset_id: &str, columns: &[Column]) -> Result<[u8; 32], EncodeError> {
    let mut buf = Vec::new();
    push_str(&mut buf, "parangsae/schema/v2")?;
    push_str(&mut buf, dataset_id)?;
    push_u32(&mut buf, u32_len(columns.len())?)?;
    for column in columns {
        push_str(&mut buf, column.name)?;
        push_str(&mut buf, column.ty)?;
    }
    push_str(&mut buf, "parangsae.scale=100000000")?;
    push_str(&mut buf, ROW_ENCODING_VERSION)?;
    push_str(&mut buf, AVAILABLE_TS_RULES_VERSION)?;
    push_str(&mut buf, GAP_RULES_VERSION)?;
    push_str(&mut buf, CONFLICT_RULES_VERSION)?;
    Ok(sha256(&buf))
}

/// Schema hash for `binance-vision.um.metrics.*`: per-column scale and five rule strings.
pub fn metrics_schema_hash(dataset_id: &str) -> Result<[u8; 32], EncodeError> {
    let mut buf = Vec::new();
    push_str(&mut buf, "parangsae/schema/v2")?;
    push_str(&mut buf, dataset_id)?;
    push_u32(&mut buf, u32_len(METRICS_COLUMNS.len())?)?;
    for column in METRICS_COLUMNS {
        push_str(&mut buf, column.name)?;
        push_str(&mut buf, column.ty)?;
    }
    push_str(&mut buf, "parangsae.scale=per-column")?;
    for rule in METRICS_SCHEMA_RULES {
        push_str(&mut buf, rule)?;
    }
    Ok(sha256(&buf))
}

pub fn metrics_content_hash(
    schema: &[u8; 32],
    rows: &[MetricsCanonicalRow<'_>],
) -> Result<[u8; 32], EncodeError> {
    for pair in rows.windows(2) {
        if pair[1].create_time_us <= pair[0].create_time_us {
            return Err(EncodeError::PkOrder);
        }
    }
    let mut buf = Vec::new();
    push_str(&mut buf, "parangsae/rows/v2")?;
    buf.extend_from_slice(schema);
    push_u64(&mut buf, u64_len(rows.len())?)?;
    for row in rows {
        enc_metrics(&mut buf, row)?;
    }
    Ok(sha256(&buf))
}

pub fn kline_content_hash(schema: &[u8; 32], rows: &[KlineFact]) -> Result<[u8; 32], EncodeError> {
    for pair in rows.windows(2) {
        if pair[1].open_time.micros() <= pair[0].open_time.micros() {
            return Err(EncodeError::PkOrder);
        }
    }
    let mut buf = Vec::new();
    push_str(&mut buf, "parangsae/rows/v2")?;
    buf.extend_from_slice(schema);
    push_u64(&mut buf, u64_len(rows.len())?)?;
    for row in rows {
        for column in UM_KLINE_COLUMNS {
            enc_kline(&mut buf, row, column.name)?;
        }
    }
    Ok(sha256(&buf))
}

pub fn gaps_digest(gaps: &[Gap]) -> Result<[u8; 32], EncodeError> {
    let mut items: Vec<(&str, i64, i64)> = gaps
        .iter()
        .map(|gap| (gap.kind.as_str(), gap.start.micros(), gap.end.micros()))
        .collect();
    items.sort_by(|left, right| {
        left.1
            .cmp(&right.1)
            .then(left.2.cmp(&right.2))
            .then(left.0.as_bytes().cmp(right.0.as_bytes()))
    });
    for pair in items.windows(2) {
        if pair[0] == pair[1] {
            return Err(EncodeError::DuplicateGap);
        }
    }
    let mut buf = Vec::new();
    push_str(&mut buf, "parangsae/gaps/v1")?;
    push_u32(&mut buf, u32_len(items.len())?)?;
    for (kind, start, end) in items {
        push_str(&mut buf, kind)?;
        push_i64(&mut buf, start);
        push_i64(&mut buf, end);
    }
    Ok(sha256(&buf))
}

pub fn conflicts_digest(conflicts: &[ConflictInput]) -> Result<[u8; 32], EncodeError> {
    let mut items: Vec<ConflictEnt> = conflicts
        .iter()
        .map(|conflict| ConflictEnt {
            pk: conflict.pk.clone(),
            class: conflict.class.clone(),
            stored: sha256(conflict.stored.as_bytes()),
            rejected: sha256(conflict.rejected.as_bytes()),
        })
        .collect();
    items.sort();
    items.dedup();
    let mut buf = Vec::new();
    push_str(&mut buf, "parangsae/conflicts/v1")?;
    push_u32(&mut buf, u32_len(items.len())?)?;
    for item in items {
        let ConflictEnt {
            pk,
            class,
            stored,
            rejected,
        } = item;
        push_u32(&mut buf, u32_len(pk.len())?)?;
        for element in pk {
            push_i64(&mut buf, element);
        }
        push_str(&mut buf, &class)?;
        buf.extend_from_slice(&stored);
        buf.extend_from_slice(&rejected);
    }
    Ok(sha256(&buf))
}

pub fn sources_digest(sources: &[SourceInput]) -> Result<[u8; 32], EncodeError> {
    let mut items: Vec<(&str, [u8; 32])> = sources
        .iter()
        .map(|source| (source.url.as_str(), source.sha256))
        .collect();
    items.sort_by(|left, right| {
        left.0
            .as_bytes()
            .cmp(right.0.as_bytes())
            .then(left.1.cmp(&right.1))
    });
    items.dedup();
    let mut buf = Vec::new();
    push_str(&mut buf, "parangsae/sources/v1")?;
    push_u32(&mut buf, u32_len(items.len())?)?;
    for (url, sha) in items {
        push_str(&mut buf, url)?;
        buf.extend_from_slice(&sha);
    }
    Ok(sha256(&buf))
}

pub fn leaf_hash(input: &LeafInput<'_>) -> Result<[u8; 32], EncodeError> {
    let mut buf = Vec::new();
    buf.push(0x00);
    push_str(&mut buf, "parangsae/merkle/v1")?;
    push_str(&mut buf, input.dataset_id)?;
    push_str(&mut buf, input.symbol)?;
    push_str(&mut buf, input.date)?;
    buf.extend_from_slice(input.schema_hash);
    push_u64(&mut buf, input.row_count)?;
    push_i64(&mut buf, input.pk_min_us);
    push_i64(&mut buf, input.pk_max_us);
    buf.extend_from_slice(input.content_hash);
    buf.extend_from_slice(input.gaps_digest);
    buf.extend_from_slice(input.conflicts_digest);
    buf.extend_from_slice(input.sources_digest);
    Ok(sha256(&buf))
}

pub fn node(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(65);
    buf.push(0x01);
    buf.extend_from_slice(left);
    buf.extend_from_slice(right);
    sha256(&buf)
}

pub fn merkle_tree_head(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return sha256(b"");
    }
    if leaves.len() == 1 {
        return leaves[0];
    }
    let mut width = 1usize;
    while width * 2 < leaves.len() {
        width *= 2;
    }
    node(
        &merkle_tree_head(&leaves[..width]),
        &merkle_tree_head(&leaves[width..]),
    )
}

pub fn bundle_hash(entries: &[(&str, [u8; 32], [u8; 32])]) -> Result<[u8; 32], BundleError> {
    let mut items = entries.to_vec();
    items.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    for pair in items.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(BundleError::DuplicateDataset);
        }
    }
    let mut buf = Vec::new();
    push_str(&mut buf, "parangsae/bundle/v1").map_err(|_| BundleError::DuplicateDataset)?;
    push_u32(
        &mut buf,
        u32_len(items.len()).map_err(|_| BundleError::DuplicateDataset)?,
    )
    .map_err(|_| BundleError::DuplicateDataset)?;
    for (dataset, root, digest) in items {
        push_str(&mut buf, dataset).map_err(|_| BundleError::DuplicateDataset)?;
        buf.extend_from_slice(&root);
        buf.extend_from_slice(&digest);
    }
    Ok(sha256(&buf))
}

fn manifest_spec_of(dataset_id: &str) -> Result<&'static str, EncodeError> {
    let id = DatasetId::parse(dataset_id).map_err(|_| EncodeError::SpecVersionMismatch)?;
    manifest_spec(id.dataset()).ok_or(EncodeError::SpecVersionMismatch)
}

pub fn canonical_manifest(doc: &ManifestDoc<'_>) -> Result<Vec<u8>, EncodeError> {
    let columns = doc
        .columns
        .iter()
        .map(|column| {
            serde_json::Value::Array(vec![
                serde_json::Value::String(column.name.to_owned()),
                serde_json::Value::String(column.ty.to_owned()),
            ])
        })
        .collect();
    let mut rule_versions = serde_json::Map::new();
    rule_versions.insert(
        "row_encoding".to_owned(),
        serde_json::Value::String(ROW_ENCODING_VERSION.to_owned()),
    );
    rule_versions.insert(
        "available_ts_rules".to_owned(),
        serde_json::Value::String(AVAILABLE_TS_RULES_VERSION.to_owned()),
    );
    rule_versions.insert(
        "gap_rules".to_owned(),
        serde_json::Value::String(GAP_RULES_VERSION.to_owned()),
    );
    rule_versions.insert(
        "conflict_rules".to_owned(),
        serde_json::Value::String(CONFLICT_RULES_VERSION.to_owned()),
    );
    let mut schema = serde_json::Map::new();
    schema.insert("schema_hash".to_owned(), json_hex(doc.schema_hash));
    schema.insert("columns".to_owned(), serde_json::Value::Array(columns));
    schema.insert(
        "excluded_from_hash".to_owned(),
        serde_json::Value::Array(vec![serde_json::Value::String(
            "receipt.Archive.fetched_at_run".to_owned(),
        )]),
    );
    schema.insert(
        "scale".to_owned(),
        serde_json::Value::String("100000000".to_owned()),
    );
    schema.insert(
        "contract_version".to_owned(),
        serde_json::Value::String(CONTRACT_VERSION.to_owned()),
    );
    schema.insert(
        "rule_versions".to_owned(),
        serde_json::Value::Object(rule_versions),
    );
    let mut partitions = Vec::with_capacity(doc.partitions.len());
    for part in doc.partitions {
        partitions.push(partition_json(part)?);
    }
    let mut range = serde_json::Map::new();
    range.insert(
        "first_date".to_owned(),
        serde_json::Value::String(doc.first_date.to_owned()),
    );
    range.insert(
        "last_date".to_owned(),
        serde_json::Value::String(doc.last_date.to_owned()),
    );
    let mut summary = serde_json::Map::new();
    summary.insert(
        "partitions".to_owned(),
        serde_json::Value::String(doc.partitions.len().to_string()),
    );
    summary.insert(
        "rows".to_owned(),
        serde_json::Value::String(doc.rows.to_string()),
    );
    let spec = manifest_spec_of(doc.dataset_id)?;
    let mut root = serde_json::Map::new();
    root.insert(
        "spec".to_owned(),
        serde_json::Value::String(spec.to_owned()),
    );
    root.insert(
        "dataset_id".to_owned(),
        serde_json::Value::String(doc.dataset_id.to_owned()),
    );
    root.insert("version_root".to_owned(), json_hex(doc.version_root));
    root.insert("schema".to_owned(), serde_json::Value::Object(schema));
    root.insert("range".to_owned(), serde_json::Value::Object(range));
    root.insert(
        "partitions".to_owned(),
        serde_json::Value::Array(partitions),
    );
    root.insert("summary".to_owned(), serde_json::Value::Object(summary));
    serde_json::to_vec(&serde_json::Value::Object(root)).map_err(|_| EncodeError::Length)
}

fn partition_json(part: &PartitionDoc<'_>) -> Result<serde_json::Value, EncodeError> {
    let gaps = part
        .gaps
        .iter()
        .map(|gap| {
            let mut object = serde_json::Map::new();
            object.insert(
                "kind".to_owned(),
                serde_json::Value::String(gap.kind.as_str().to_owned()),
            );
            object.insert("start_us".to_owned(), json_i64(gap.start.micros()));
            object.insert("end_us".to_owned(), json_i64(gap.end.micros()));
            serde_json::Value::Object(object)
        })
        .collect();
    let mut object = serde_json::Map::new();
    object.insert(
        "date".to_owned(),
        serde_json::Value::String(part.date.to_owned()),
    );
    object.insert(
        "path".to_owned(),
        serde_json::Value::String(part.path.to_owned()),
    );
    object.insert("row_count".to_owned(), json_u64(part.row_count));
    object.insert("pk_min_us".to_owned(), json_i64(part.pk_min_us));
    object.insert("pk_max_us".to_owned(), json_i64(part.pk_max_us));
    object.insert(
        "available_ts_min_us".to_owned(),
        json_i64(part.available_min_us),
    );
    object.insert(
        "available_ts_max_us".to_owned(),
        json_i64(part.available_max_us),
    );
    object.insert("content_hash".to_owned(), json_hex(part.content_hash));
    object.insert(
        "sources".to_owned(),
        serde_json::Value::Array(source_json(part.sources)),
    );
    object.insert("open_gaps".to_owned(), serde_json::Value::Array(gaps));
    object.insert(
        "conflicts".to_owned(),
        serde_json::Value::Array(conflict_json(part.conflicts)),
    );
    object.insert(
        "excluded_windows".to_owned(),
        serde_json::Value::Array(window_json(part.excluded_windows)),
    );
    object.insert("gaps_digest".to_owned(), json_hex(part.gaps_digest));
    object.insert(
        "conflicts_digest".to_owned(),
        json_hex(part.conflicts_digest),
    );
    object.insert("sources_digest".to_owned(), json_hex(part.sources_digest));
    object.insert("leaf_hash".to_owned(), json_hex(part.leaf_hash));
    Ok(serde_json::Value::Object(object))
}

fn source_json(sources: &[SourceInput]) -> Vec<serde_json::Value> {
    let mut items = sources.to_vec();
    items.sort_by(|left, right| {
        left.url
            .as_bytes()
            .cmp(right.url.as_bytes())
            .then(left.sha256.cmp(&right.sha256))
    });
    items.dedup();
    items
        .into_iter()
        .map(|source| {
            let mut object = serde_json::Map::new();
            object.insert(
                "url".to_owned(),
                serde_json::Value::String(source.url.clone()),
            );
            object.insert(
                "sha256".to_owned(),
                serde_json::Value::String(hex::encode(source.sha256)),
            );
            object.insert(
                "checksum_url".to_owned(),
                serde_json::Value::String(format!("{}.CHECKSUM", source.url)),
            );
            serde_json::Value::Object(object)
        })
        .collect()
}

fn conflict_json(conflicts: &[ConflictDoc]) -> Vec<serde_json::Value> {
    let mut items = conflicts.to_vec();
    items.sort_by(|left, right| {
        left.pk
            .cmp(&right.pk)
            .then(left.class.cmp(&right.class))
            .then(left.stored_sha256.cmp(&right.stored_sha256))
            .then(left.rejected_sha256.cmp(&right.rejected_sha256))
    });
    items.dedup();
    items
        .into_iter()
        .map(|conflict| {
            let mut object = serde_json::Map::new();
            object.insert(
                "pk".to_owned(),
                serde_json::Value::Array(conflict.pk.iter().copied().map(json_i64).collect()),
            );
            object.insert(
                "class".to_owned(),
                serde_json::Value::String(conflict.class),
            );
            object.insert(
                "stored_sha256".to_owned(),
                serde_json::Value::String(hex::encode(conflict.stored_sha256)),
            );
            object.insert(
                "rejected_sha256".to_owned(),
                serde_json::Value::String(hex::encode(conflict.rejected_sha256)),
            );
            serde_json::Value::Object(object)
        })
        .collect()
}

/// Maximal runs of `value_by_log` kline keys on the one-minute grid.
pub(crate) fn kline_excluded_windows(conflicts: &[ConflictDoc]) -> Vec<WindowDoc> {
    const STEP_US: i64 = 60_000_000;
    let mut pks: Vec<i64> = conflicts
        .iter()
        .filter(|conflict| conflict.class == "value_by_log" && conflict.pk.len() == 1)
        .map(|conflict| conflict.pk[0])
        .collect();
    pks.sort();
    pks.dedup();
    let mut windows = Vec::new();
    let mut index = 0;
    while index < pks.len() {
        let start = pks[index];
        let mut last = start;
        index += 1;
        while index < pks.len() && pks[index] == last.saturating_add(STEP_US) {
            last = pks[index];
            index += 1;
        }
        windows.push(WindowDoc {
            start_us: start,
            end_us: last.saturating_add(STEP_US),
            reason: "value_by_log",
        });
    }
    windows
}

fn window_json(windows: &[WindowDoc]) -> Vec<serde_json::Value> {
    windows
        .iter()
        .map(|window| {
            let mut object = serde_json::Map::new();
            object.insert("start_us".to_owned(), json_i64(window.start_us));
            object.insert("end_us".to_owned(), json_i64(window.end_us));
            object.insert(
                "reason".to_owned(),
                serde_json::Value::String(window.reason.to_owned()),
            );
            serde_json::Value::Object(object)
        })
        .collect()
}

fn enc_kline(buf: &mut Vec<u8>, row: &KlineFact, name: &str) -> Result<(), EncodeError> {
    match name {
        "symbol" => push_str(buf, row.symbol.as_str()),
        "interval_us" => {
            push_i64(buf, row.interval_us);
            Ok(())
        }
        "open_time_us" => {
            push_i64(buf, row.open_time.micros());
            Ok(())
        }
        "close_time_us" => {
            push_i64(buf, row.close_time.micros());
            Ok(())
        }
        "open_ticks" => {
            push_i64(buf, row.open_ticks);
            Ok(())
        }
        "high_ticks" => {
            push_i64(buf, row.high_ticks);
            Ok(())
        }
        "low_ticks" => {
            push_i64(buf, row.low_ticks);
            Ok(())
        }
        "close_ticks" => {
            push_i64(buf, row.close_ticks);
            Ok(())
        }
        "volume_ticks" => {
            push_i64(buf, row.volume.ticks());
            Ok(())
        }
        "quote_volume_ticks" => {
            push_i64(buf, row.quote_volume.ticks());
            Ok(())
        }
        "count" => {
            push_i64(buf, row.count);
            Ok(())
        }
        "taker_buy_volume_ticks" => {
            push_i64(buf, row.taker_buy_volume.ticks());
            Ok(())
        }
        "taker_buy_quote_volume_ticks" => {
            push_i64(buf, row.taker_buy_quote_volume.ticks());
            Ok(())
        }
        "event_ts_us" => {
            push_i64(buf, row.event_ts.micros().micros());
            Ok(())
        }
        "available_ts_us" => {
            push_i64(buf, row.available_ts.micros().micros());
            Ok(())
        }
        "receipt" => enc_receipt(buf, row.receipt),
        "schema_version" => {
            push_u32(buf, row.schema_version)?;
            Ok(())
        }
        "tick_domain" => push_str(buf, row.tick_domain.as_str()),
        "zero_volume_flat" => {
            buf.push(u8::from(row.zero_volume_flat));
            Ok(())
        }
        _ => Err(EncodeError::Column("unknown kline column")),
    }
}

fn enc_receipt(buf: &mut Vec<u8>, receipt: Receipt) -> Result<(), EncodeError> {
    match receipt {
        Receipt::Archive { .. } => buf.push(0x00),
        Receipt::Live { received_ts } => {
            buf.push(0x01);
            push_i64(buf, received_ts.micros());
        }
    }
    Ok(())
}

fn enc_metrics(buf: &mut Vec<u8>, row: &MetricsCanonicalRow<'_>) -> Result<(), EncodeError> {
    push_str(buf, row.symbol)?;
    push_i64(buf, row.create_time_us);
    push_opt_i64(buf, row.open_interest_ticks);
    push_opt_i64(buf, row.open_interest_value_ticks);
    push_opt_i128(buf, row.count_toptrader_long_short_ratio);
    push_opt_i128(buf, row.sum_toptrader_long_short_ratio);
    push_opt_i128(buf, row.count_long_short_ratio);
    push_opt_i128(buf, row.sum_taker_long_short_vol_ratio);
    match row.missing_tokens {
        None => buf.push(0x00),
        Some(text) => {
            buf.push(0x01);
            push_str(buf, text)?;
        }
    }
    push_i64(buf, row.event_ts_us);
    push_i64(buf, row.available_ts_us);
    enc_receipt(buf, row.receipt)?;
    push_u32(buf, row.schema_version)
}

fn push_opt_i64(buf: &mut Vec<u8>, value: Option<i64>) {
    match value {
        None => buf.push(0x00),
        Some(value) => {
            buf.push(0x01);
            push_i64(buf, value);
        }
    }
}

fn push_opt_i128(buf: &mut Vec<u8>, value: Option<i128>) {
    match value {
        None => buf.push(0x00),
        Some(value) => {
            buf.push(0x01);
            buf.extend_from_slice(&value.to_be_bytes());
        }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ConflictEnt {
    pk: Vec<i64>,
    class: String,
    stored: [u8; 32],
    rejected: [u8; 32],
}

fn push_str(buf: &mut Vec<u8>, text: &str) -> Result<(), EncodeError> {
    push_u32(buf, u32_len(text.len())?)?;
    buf.extend_from_slice(text.as_bytes());
    Ok(())
}

fn push_u32(buf: &mut Vec<u8>, value: u32) -> Result<(), EncodeError> {
    buf.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn push_u64(buf: &mut Vec<u8>, value: u64) -> Result<(), EncodeError> {
    buf.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn push_i64(buf: &mut Vec<u8>, value: i64) {
    buf.extend_from_slice(&value.to_be_bytes());
}

fn u32_len(len: usize) -> Result<u32, EncodeError> {
    u32::try_from(len).map_err(|_| EncodeError::Length)
}

fn u64_len(len: usize) -> Result<u64, EncodeError> {
    u64::try_from(len).map_err(|_| EncodeError::Length)
}

fn json_hex(bytes: &[u8; 32]) -> serde_json::Value {
    serde_json::Value::String(hex::encode(bytes))
}

fn json_i64(value: i64) -> serde_json::Value {
    serde_json::Value::String(value.to_string())
}

fn json_u64(value: u64) -> serde_json::Value {
    serde_json::Value::String(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::CivilDay;
    use crate::normalize::UM_KLINE_CLOCK;
    use crate::vision::{kline_day_gaps, parse_klines, GapKind, TickDomain};
    use parangsae_core::{Receipt, RunId, Symbol, UtcMicros};

    const DATASET: &str = "binance-vision.um.um-kline.BTCUSDT";

    fn expect(got: &[u8; 32], hex_text: &str) -> Result<(), String> {
        let encoded = hex::encode(got);
        if encoded == hex_text {
            Ok(())
        } else {
            Err(format!("{encoded} != {hex_text}"))
        }
    }

    const METRICS_ID: &str = "binance-vision.um.metrics.BTCUSDT";
    const NULL_OI: &str = "sum_open_interest=\"0E-16\";sum_open_interest_value=\"0E-16\"";
    const NULL_RATIOS: &str = "count_toptrader_long_short_ratio=\"\\\"\\\"\";sum_toptrader_long_short_ratio=\"\\\"\\\"\";count_long_short_ratio=\"\\\"\\\"\";sum_taker_long_short_vol_ratio=\"\\\"\\\"\"";

    fn archive() -> Receipt {
        Receipt::Archive {
            fetched_at_run: RunId::from_bytes([0; 16]),
        }
    }

    fn metrics_row<'a>(
        create_time_us: i64,
        oi: Option<i64>,
        value: Option<i64>,
        ratios: [Option<i128>; 4],
        missing: Option<&'a str>,
    ) -> MetricsCanonicalRow<'a> {
        MetricsCanonicalRow {
            symbol: "BTCUSDT",
            create_time_us,
            open_interest_ticks: oi,
            open_interest_value_ticks: value,
            count_toptrader_long_short_ratio: ratios[0],
            sum_toptrader_long_short_ratio: ratios[1],
            count_long_short_ratio: ratios[2],
            sum_taker_long_short_vol_ratio: ratios[3],
            missing_tokens: missing,
            event_ts_us: create_time_us,
            available_ts_us: create_time_us + 300_000_000,
            receipt: archive(),
            schema_version: 6,
        }
    }

    #[test]
    fn metrics_v2_2_goldens_match_section_4_7_1() -> Result<(), String> {
        let schema = metrics_schema_hash(METRICS_ID).map_err(|err| err.to_string())?;
        expect(
            &schema,
            "d4f9313a0c0a5a745412d2ae4c24e3dbc9526d6839afed37643fc501d610c2ef",
        )?;
        let three = [
            metrics_row(
                1_720_545_600_000_000,
                Some(8_107_070_500_000),
                Some(466_141_999_090_011_700),
                [
                    Some(19_127_967_400_000_000),
                    Some(16_027_240_000_000_000),
                    Some(17_918_580_100_000_000),
                    Some(10_458_250_000_000_000),
                ],
                None,
            ),
            metrics_row(
                1_720_545_900_000_000,
                None,
                None,
                [
                    Some(19_113_286_700_000_000),
                    Some(16_038_800_000_000_000),
                    Some(17_959_065_800_000_000),
                    Some(32_169_940_000_000_000),
                ],
                Some(NULL_OI),
            ),
            metrics_row(
                1_720_546_200_000_000,
                Some(8_108_182_300_000),
                Some(466_890_045_478_454_500),
                [
                    Some(19_098_314_800_000_000),
                    Some(16_041_570_000_000_000),
                    Some(17_922_812_500_000_000),
                    Some(5_946_510_000_000_000),
                ],
                None,
            ),
        ];
        let three_hash = metrics_content_hash(&schema, &three).map_err(|err| err.to_string())?;
        expect(
            &three_hash,
            "68f5b06841e63fd7e0a00ac39814e37ff3af5310a937a09c839c32cc9d9949e0",
        )?;
        let one = [metrics_row(
            1_640_995_200_000_000,
            Some(7_480_341_000_000),
            Some(345_668_379_032_415_250),
            [None, None, None, None],
            Some(NULL_RATIOS),
        )];
        let one_hash = metrics_content_hash(&schema, &one).map_err(|err| err.to_string())?;
        expect(
            &one_hash,
            "b5c3268e1fbd6552d60fa1ba892a55c5f2e4b085de7cf513d898bf2207c51784",
        )?;
        Ok(())
    }

    fn receipt() -> Receipt {
        Receipt::Archive {
            fetched_at_run: RunId::from_bytes([9; 16]),
        }
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/../../fixtures/vision/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn parsed(name: &str) -> Vec<KlineFact> {
        parse_klines(
            &fixture(name),
            &Symbol::new("BTCUSDT").unwrap(),
            UM_KLINE_CLOCK,
            TickDomain::Price,
            receipt(),
            true,
        )
        .unwrap()
        .rows
    }

    fn day(year: i32, month: u8, day_num: u8) -> CivilDay {
        CivilDay::new(year, month, day_num).unwrap()
    }

    #[test]
    fn fixture_hashes_match_the_golden_table() -> Result<(), String> {
        let schema = schema_hash(DATASET, UM_KLINE_COLUMNS).map_err(|err| format!("{err:?}"))?;
        expect(
            &schema,
            "4cb7157ad532ef2549f795cce7e7b2646fe2c88f41ab7d90d25646aa8c8f3afa",
        )?;
        let june = parsed("um_kline_2021-06-01_headerless.csv");
        let october = parsed("um_kline_2026-10-20_header.csv");
        let june_day = day(2021, 6, 1);
        let october_day = day(2026, 10, 20);
        let next = day(2021, 6, 2);
        if june_day.start_us().micros() != 1_622_505_600_000_000 {
            return Err("2021-06-01 start".into());
        }
        let june_content = kline_content_hash(&schema, &june).map_err(|err| format!("{err:?}"))?;
        let october_content =
            kline_content_hash(&schema, &october).map_err(|err| format!("{err:?}"))?;
        expect(
            &june_content,
            "e9516d86a16c59781bad9163f97157ed0e0ed627af88ede43f6cd32eccc9220c",
        )?;
        expect(
            &october_content,
            "e2f69a541ec298ad8433fbc1790e7272a47070817d5ec965beddaafa1c32a980",
        )?;
        let empty_conflicts = conflicts_digest(&[]).map_err(|err| format!("{err:?}"))?;
        let empty_sources = sources_digest(&[]).map_err(|err| format!("{err:?}"))?;
        let june_gaps = kline_day_gaps(&june, june_day);
        let october_gaps = kline_day_gaps(&october, october_day);
        let june_gap_hash = gaps_digest(&june_gaps).map_err(|err| format!("{err:?}"))?;
        let october_gap_hash = gaps_digest(&october_gaps).map_err(|err| format!("{err:?}"))?;
        let june_leaf = leaf_of(
            &schema,
            &DayLeaf {
                date: "2021-06-01",
                rows: &june,
                content: &june_content,
                gaps_hash: &june_gap_hash,
                conflicts: &empty_conflicts,
                sources: &empty_sources,
                day_start: june_day.start_us().micros(),
            },
        )?;
        let october_leaf = leaf_of(
            &schema,
            &DayLeaf {
                date: "2026-10-20",
                rows: &october,
                content: &october_content,
                gaps_hash: &october_gap_hash,
                conflicts: &empty_conflicts,
                sources: &empty_sources,
                day_start: october_day.start_us().micros(),
            },
        )?;
        expect(
            &june_leaf,
            "9408bb647c1982d997c958943baf7ba31fa5b591c4c9efee1b6fdfc518cbcfa0",
        )?;
        expect(
            &october_leaf,
            "06301e1aecc1b85e45fd69cae24a3620c46bb5880e594bde99a7470a6cec3ed2",
        )?;
        let pair = merkle_tree_head(&[june_leaf, october_leaf]);
        expect(
            &pair,
            "0db15144e2b37f37bdce8e0819d4d181e51f270a47bc85c4aeb4badbe617995c",
        )?;
        let zero_content = kline_content_hash(&schema, &[]).map_err(|err| format!("{err:?}"))?;
        let missing = Gap {
            kind: GapKind::MissingFile,
            start: next.start_us(),
            end: UtcMicros::from_micros(next.start_us().micros() + 86_400_000_000),
            detail: String::new(),
        };
        let zero_gaps =
            gaps_digest(std::slice::from_ref(&missing)).map_err(|err| format!("{err:?}"))?;
        let zero_leaf = leaf_hash(&LeafInput {
            dataset_id: DATASET,
            symbol: "BTCUSDT",
            date: "2021-06-02",
            schema_hash: &schema,
            row_count: 0,
            pk_min_us: next.start_us().micros(),
            pk_max_us: next.start_us().micros(),
            content_hash: &zero_content,
            gaps_digest: &zero_gaps,
            conflicts_digest: &empty_conflicts,
            sources_digest: &empty_sources,
        })
        .map_err(|err| format!("{err:?}"))?;
        expect(
            &zero_leaf,
            "97e9cc7715a38d2f1a082f1b663715eeace5574998317d24f2e13808fa473827",
        )?;
        let root = merkle_tree_head(&[june_leaf, zero_leaf]);
        expect(
            &root,
            "feaaa52b9c9531b0b8202363d0eafe7de484bf28116c99cd0181bb4ee91aca9c",
        )?;
        let conflict = conflicts_digest(&[ConflictInput {
            pk: vec![1_622_505_600_000_000],
            class: "schema_only_by_log".to_owned(),
            stored: "open=1 schema=1".to_owned(),
            rejected: "open=1 schema=2".to_owned(),
        }])
        .map_err(|err| format!("{err:?}"))?;
        expect(
            &conflict,
            "1c9d06cf65c34eb3025133ec5b06022da05b23dabc2091676c608c204e881944",
        )?;
        let sha = hex::decode("fc949a2cdb72753faafbed1eb93a3a1847b3ba77056b6360a3a87e93a9f37141")
            .map_err(|err| err.to_string())?;
        let mut source_sha = [0u8; 32];
        source_sha.copy_from_slice(&sha);
        let source = sources_digest(&[SourceInput {
            url: "https://data.binance.vision/data/futures/um/daily/klines/BTCUSDT/1m/BTCUSDT-1m-2021-06-01.zip".to_owned(),
            sha256: source_sha,
        }])
        .map_err(|err| format!("{err:?}"))?;
        expect(
            &source,
            "2d63d7da37aa0db89bd26da420a0c9e92a9b5453ec0a6316cb116a383790d33c",
        )?;
        let sided = leaf_hash(&LeafInput {
            dataset_id: DATASET,
            symbol: "BTCUSDT",
            date: "2021-06-01",
            schema_hash: &schema,
            row_count: u64_len(june.len()).map_err(|err| format!("{err:?}"))?,
            pk_min_us: june[0].open_time.micros(),
            pk_max_us: june[june.len() - 1].open_time.micros(),
            content_hash: &june_content,
            gaps_digest: &june_gap_hash,
            conflicts_digest: &conflict,
            sources_digest: &source,
        })
        .map_err(|err| format!("{err:?}"))?;
        expect(
            &sided,
            "cd87f926d29bcdcae3d81eb4b939f5ee74a023cd534ac29b780ce970afeccc4f",
        )?;
        let mut live = june.clone();
        for row in &mut live {
            row.receipt = Receipt::Live {
                received_ts: UtcMicros::from_micros(1_792_454_400_123_456),
            };
        }
        let live_hash = kline_content_hash(&schema, &live).map_err(|err| format!("{err:?}"))?;
        expect(
            &live_hash,
            "a0b9975b49fe9a26338e117160904f28fb0c77713c99e8cac8b280ea48c6d6b1",
        )?;
        let mut tampered = june.clone();
        tampered[0].open_ticks += 1;
        let tampered_hash =
            kline_content_hash(&schema, &tampered).map_err(|err| format!("{err:?}"))?;
        expect(
            &tampered_hash,
            "b1afc1e9bb498e0256ee27f6ae73b61d073fa4807ee8e8e601f17feccd1eac31",
        )?;
        let mut reversed = june.clone();
        reversed.reverse();
        if kline_content_hash(&schema, &reversed) != Err(EncodeError::PkOrder) {
            return Err("reversed rows were accepted".into());
        }
        let zeros = [0u8; 32];
        let bundled = bundle_hash(&[(DATASET, root, zeros)]).map_err(|err| format!("{err:?}"))?;
        expect(
            &bundled,
            "48810ed3e8c898ad67b7d94c6ad964aea74b5828ab0bd2273894ecb063648725",
        )?;
        let bytes = canonical_manifest(&ManifestDoc {
            dataset_id: DATASET,
            columns: UM_KLINE_COLUMNS,
            schema_hash: &schema,
            version_root: &root,
            first_date: "2021-06-01",
            last_date: "2021-06-02",
            rows: u64_len(june.len()).map_err(|err| format!("{err:?}"))?,
            partitions: &[
                part_doc(&PartArgs {
                    date: "2021-06-01",
                    rows: &june,
                    content: &june_content,
                    gaps: &june_gaps,
                    leaf: &june_leaf,
                    gap_hash: &june_gap_hash,
                    conflicts: &empty_conflicts,
                    sources: &empty_sources,
                    day_start: june_day.start_us().micros(),
                }),
                part_doc(&PartArgs {
                    date: "2021-06-02",
                    rows: &[],
                    content: &zero_content,
                    gaps: std::slice::from_ref(&missing),
                    leaf: &zero_leaf,
                    gap_hash: &zero_gaps,
                    conflicts: &empty_conflicts,
                    sources: &empty_sources,
                    day_start: next.start_us().micros(),
                }),
            ],
        })
        .map_err(|err| format!("{err:?}"))?;
        if bytes.len() != 2733 {
            return Err(format!("manifest bytes {}", bytes.len()));
        }
        expect(
            &sha256(&bytes),
            "f5b177adc8648d4499bc54026fa3855a70a57a8a4e16539f87dc94cef85afec8",
        )?;
        let real =
            bundle_hash(&[(DATASET, root, sha256(&bytes))]).map_err(|err| format!("{err:?}"))?;
        expect(
            &real,
            "2d21053475f514c38710a51b8a888f41665390686bcac6ed59aa2bbce7c1e4e3",
        )?;
        Ok(())
    }

    #[test]
    fn merkle_vectors_do_not_duplicate_an_odd_leaf() -> Result<(), String> {
        let mut leaves = Vec::new();
        for index in 0..5u32 {
            let mut buf = Vec::new();
            push_str(&mut buf, "leaf").map_err(|err| format!("{err:?}"))?;
            push_u32(&mut buf, index).map_err(|err| format!("{err:?}"))?;
            leaves.push(sha256(&buf));
        }
        let expected = [
            (
                0,
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                1,
                "3450fc6ac25a937aec78b45ecf4ce4f517843f6a6b0c8d1c7cea14a65233b57a",
            ),
            (
                2,
                "9ed9d0de1dfc9b990a01117b0e8e9716c0b632387c39ced449118fb24b733ed5",
            ),
            (
                3,
                "2b3c8c09ab84e0355757c00a918073b04466b8c06966b1a7b70194183f9cc074",
            ),
            (
                5,
                "310da59839bbc14ae0422b97f0dadc20d770e8962ddcc950f85139061dc7e036",
            ),
        ];
        for (count, hex_text) in expected {
            expect(&merkle_tree_head(&leaves[..count]), hex_text)?;
        }
        Ok(())
    }

    #[test]
    fn bundle_hash_is_independent_of_input_order() -> Result<(), String> {
        let root = [1u8; 32];
        let other = [2u8; 32];
        let digest = [3u8; 32];
        let um = ("binance-vision.um.um-kline.BTCUSDT", root, digest);
        let mark = ("binance-vision.um.mark-price-kline.BTCUSDT", other, digest);
        let forward = bundle_hash(&[um, mark]).map_err(|err| format!("{err:?}"))?;
        let backward = bundle_hash(&[mark, um]).map_err(|err| format!("{err:?}"))?;
        if forward != backward {
            return Err("bundle hash depends on input order".into());
        }
        if bundle_hash(&[um, um]).is_ok() {
            return Err("duplicate dataset id was accepted".into());
        }
        Ok(())
    }

    struct DayLeaf<'a> {
        date: &'a str,
        rows: &'a [KlineFact],
        content: &'a [u8; 32],
        gaps_hash: &'a [u8; 32],
        conflicts: &'a [u8; 32],
        sources: &'a [u8; 32],
        day_start: i64,
    }

    fn leaf_of(schema: &[u8; 32], day: &DayLeaf<'_>) -> Result<[u8; 32], String> {
        let rows = day.rows;
        let (pk_min, pk_max) = if rows.is_empty() {
            (day.day_start, day.day_start)
        } else {
            (
                rows[0].open_time.micros(),
                rows[rows.len() - 1].open_time.micros(),
            )
        };
        leaf_hash(&LeafInput {
            dataset_id: DATASET,
            symbol: "BTCUSDT",
            date: day.date,
            schema_hash: schema,
            row_count: u64_len(rows.len()).map_err(|err| format!("{err:?}"))?,
            pk_min_us: pk_min,
            pk_max_us: pk_max,
            content_hash: day.content,
            gaps_digest: day.gaps_hash,
            conflicts_digest: day.conflicts,
            sources_digest: day.sources,
        })
        .map_err(|err| format!("{err:?}"))
    }

    struct PartArgs<'a> {
        date: &'a str,
        rows: &'a [KlineFact],
        content: &'a [u8; 32],
        gaps: &'a [Gap],
        leaf: &'a [u8; 32],
        gap_hash: &'a [u8; 32],
        conflicts: &'a [u8; 32],
        sources: &'a [u8; 32],
        day_start: i64,
    }

    fn part_doc<'a>(args: &PartArgs<'a>) -> PartitionDoc<'a> {
        let rows = args.rows;
        let (pk_min, pk_max, available_min, available_max) = if rows.is_empty() {
            (
                args.day_start,
                args.day_start,
                args.day_start,
                args.day_start,
            )
        } else {
            let pk_min = rows.iter().map(|row| row.open_time.micros()).min().unwrap();
            let pk_max = rows.iter().map(|row| row.open_time.micros()).max().unwrap();
            let available_min = rows
                .iter()
                .map(|row| row.available_ts.micros().micros())
                .min()
                .unwrap();
            let available_max = rows
                .iter()
                .map(|row| row.available_ts.micros().micros())
                .max()
                .unwrap();
            (pk_min, pk_max, available_min, available_max)
        };
        PartitionDoc {
            date: args.date,
            path: match args.date {
                "2021-06-01" => "parquet/um-kline/symbol=BTCUSDT/date=2021-06-01/part.parquet",
                _ => "parquet/um-kline/symbol=BTCUSDT/date=2021-06-02/part.parquet",
            },
            row_count: u64::try_from(rows.len()).unwrap_or(0),
            pk_min_us: pk_min,
            pk_max_us: pk_max,
            available_min_us: available_min,
            available_max_us: available_max,
            content_hash: args.content,
            gaps: args.gaps,
            sources: &[],
            conflicts: &[],
            excluded_windows: &[],
            leaf_hash: args.leaf,
            gaps_digest: args.gap_hash,
            conflicts_digest: args.conflicts,
            sources_digest: args.sources,
        }
    }
}
