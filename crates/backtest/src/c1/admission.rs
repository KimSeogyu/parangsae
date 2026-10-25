//! C1's single metadata boundary. Parsing does not confer study authority.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::canon::{json_string, sha256_hex};
use crate::date::UtcDate;

const START: i64 = 1_608_249_600_000_000;
const END: i64 = 1_759_276_800_000_000;
const LIMIT: u64 = 16 * 1024 * 1024;
const CATALOG: &str = "c1-source-manifest.json";
const PREREG: &str = "research/prereg/2026-10-24-c1-positive-funding-carry.md";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct C1BatchRequest {
    pub inputs: PathBuf,
    pub data_root: PathBuf,
    pub out: PathBuf,
    pub ledger: PathBuf,
    pub attempt: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct C1AdmissionError {
    pub code: &'static str,
    pub detail: String,
}

impl fmt::Display for C1AdmissionError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for C1AdmissionError {}

type Result<T> = std::result::Result<T, C1AdmissionError>;

fn refuse(code: &'static str, detail: impl Into<String>) -> C1AdmissionError {
    C1AdmissionError {
        code,
        detail: detail.into(),
    }
}

fn require(ok: bool, code: &'static str, detail: impl Into<String>) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(refuse(code, detail))
    }
}

fn io(error: std::io::Error) -> C1AdmissionError {
    refuse("IO", error.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(super) struct Hash64(pub String);

impl<'de> Deserialize<'de> for Hash64 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let value = String::deserialize(d)?;
        if value.len() != 64
            || !value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(de::Error::custom("64 lowercase hex characters required"));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(super) struct CanonJson(pub Value);

impl<'de> Deserialize<'de> for CanonJson {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct CanonVisitor;
        impl<'de> Visitor<'de> for CanonVisitor {
            type Value = CanonJson;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("duplicate-free ASCII JSON with exact safe integers")
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(CanonJson(Value::Null))
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Self::Value, E> {
                Ok(CanonJson(Value::Bool(value)))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> std::result::Result<Self::Value, E> {
                if value.unsigned_abs() > 9_007_199_254_740_991 {
                    return Err(E::custom("unsafe canonical integer"));
                }
                Ok(CanonJson(json!(value)))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Self::Value, E> {
                if value > 9_007_199_254_740_991 {
                    return Err(E::custom("unsafe canonical integer"));
                }
                Ok(CanonJson(json!(value)))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                if !value.is_ascii() {
                    return Err(E::custom("non-ASCII canonical string"));
                }
                Ok(CanonJson(Value::String(value.to_owned())))
            }
            fn visit_string<E: de::Error>(
                self,
                value: String,
            ) -> std::result::Result<Self::Value, E> {
                self.visit_str(&value)
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut result = Vec::new();
                while let Some(value) = seq.next_element::<CanonJson>()? {
                    result.push(value.0);
                }
                Ok(CanonJson(Value::Array(result)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut result = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, CanonJson>()? {
                    if !key.is_ascii() || result.contains_key(&key) {
                        return Err(de::Error::custom("non-ASCII or duplicate canonical key"));
                    }
                    result.insert(key, value.0);
                }
                Ok(CanonJson(Value::Object(result)))
            }
        }
        d.deserialize_any(CanonVisitor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArtifactRef {
    pub path: String,
    pub sha256: Hash64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub(super) enum ChargedRole {
    #[serde(rename = "TRAIN_MAX168")]
    TrainMax168,
    #[serde(rename = "TRAIN_MAX336")]
    TrainMax336,
    #[serde(rename = "OOS_HEADLINE")]
    OosHeadline,
    #[serde(rename = "OOS_HIGHER_PERP_FEE")]
    OosHigherPerpFee,
    #[serde(rename = "OOS_SLIPPAGE_STRESS")]
    OosSlippageStress,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct C1RoleConfig {
    pub role: ChargedRole,
    pub config_id: Hash64,
    pub config_preimage: CanonJson,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum C1InputKind {
    SpotTrade,
    UmTrade,
    PerpMark,
    Funding,
    SpotStatus,
    UmResolution,
    Prereg,
    SourceManifest,
    Contract,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct C1PinnedObject {
    pub path: String,
    pub sha256: Hash64,
    pub kind: C1InputKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct C1SourceSelection {
    pub source_policy_id: String,
    pub daily_rebuilt_days: Vec<String>,
    pub spot_status_days: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct C1Inputs {
    pub schema: String,
    pub protocol_version: String,
    pub study_id: Hash64,
    pub settings: CanonJson,
    pub prereg: ArtifactRef,
    pub roles: Vec<C1RoleConfig>,
    pub source_selection: C1SourceSelection,
    pub source_manifest: ArtifactRef,
    pub contracts: Vec<ArtifactRef>,
    pub objects: Vec<C1PinnedObject>,
    pub pinned_root_expected: Hash64,
}

#[derive(Debug)]
pub(super) struct AdmittedC1 {
    pub inputs: C1Inputs,
    pub pin: String,
    pub prereg_identity: ArtifactRef,
    pub source_manifest: PathBuf,
    pub out: PathBuf,
    pub ledger: PathBuf,
    pub _ledger_file: File,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceObject {
    path: String,
    sha256: Hash64,
    role: C1InputKind,
    date: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceManifest {
    schema: String,
    source_start_us: i64,
    source_end_us_exclusive: i64,
    symbol: String,
    source_policy_id: String,
    daily_rebuilt_days: Vec<String>,
    spot_status_days: Vec<String>,
    data_objects: Vec<SourceObject>,
}

fn canonical_id(value: &Value) -> String {
    fn encode(value: &Value) -> String {
        match value {
            Value::Null => "null".to_owned(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => value.to_string(),
            Value::String(value) => json_string(value),
            Value::Array(values) => format!(
                "[{}]",
                values.iter().map(encode).collect::<Vec<_>>().join(",")
            ),
            Value::Object(values) => {
                let sorted: BTreeMap<_, _> = values.iter().collect();
                let fields = sorted
                    .into_iter()
                    .map(|(key, value)| format!("{}:{}", json_string(key), encode(value)))
                    .collect::<Vec<_>>();
                format!("{{{}}}", fields.join(","))
            }
        }
    }
    // CanonJson admits only ASCII keys and exact safe integers.
    sha256_hex(encode(value).as_bytes())
}

fn logical(path: &str) -> Result<()> {
    require(
        !path.is_empty()
            && path.is_ascii()
            && !path.starts_with('/')
            && !path.contains('\\')
            && !path.contains('\0'),
        "PATH",
        path,
    )?;
    require(
        path.split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".."),
        "PATH",
        path,
    )
}

fn absolute(path: &Path) -> Result<()> {
    let text = path
        .to_str()
        .ok_or_else(|| refuse("PATH", "non-UTF8 path"))?;
    require(
        text.starts_with('/') && text != "/" && !text.contains('\\') && !text.contains('\0'),
        "PATH",
        text,
    )?;
    require(
        text[1..]
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".."),
        "PATH",
        text,
    )
}

fn walk(path: &Path, missing_ok: bool) -> Result<Option<Metadata>> {
    absolute(path)?;
    let mut cursor = PathBuf::from("/");
    let mut last = None;
    for part in path.components() {
        if let Component::Normal(part) = part {
            cursor.push(part);
            match fs::symlink_metadata(&cursor) {
                Ok(meta) => {
                    require(
                        !meta.file_type().is_symlink(),
                        "SYMLINK",
                        cursor.display().to_string(),
                    )?;
                    if cursor != path {
                        require(meta.is_dir(), "PATH", cursor.display().to_string())?;
                    }
                    last = Some(meta);
                }
                Err(error) if missing_ok && error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(None)
                }
                Err(error) => return Err(io(error)),
            }
        }
    }
    Ok(last)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileIdentity {
    dev: u64,
    ino: u64,
    len: u64,
    mode: u32,
    nlink: u64,
    mtime: i64,
    mtime_ns: i64,
    ctime: i64,
    ctime_ns: i64,
}

fn identity(meta: &Metadata) -> FileIdentity {
    FileIdentity {
        dev: meta.dev(),
        ino: meta.ino(),
        len: meta.len(),
        mode: meta.mode(),
        nlink: meta.nlink(),
        mtime: meta.mtime(),
        mtime_ns: meta.mtime_nsec(),
        ctime: meta.ctime(),
        ctime_ns: meta.ctime_nsec(),
    }
}

fn readonly_file(path: &Path, meta: &Metadata) -> Result<()> {
    require(
        meta.is_file() && meta.nlink() == 1 && meta.mode() & 0o222 == 0,
        "SNAPSHOT_FILE",
        path.display().to_string(),
    )
}

fn checked_read(path: &Path, expected: &Metadata, limit: u64) -> Result<Vec<u8>> {
    readonly_file(path, expected)?;
    require(
        expected.len() <= limit,
        "METADATA_SIZE",
        path.display().to_string(),
    )?;
    let mut file = File::open(path).map_err(io)?;
    require(
        identity(&file.metadata().map_err(io)?) == identity(expected),
        "FILE_CHANGED",
        path.display().to_string(),
    )?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    require(
        u64::try_from(bytes.len()).map_err(|_| refuse("METADATA_SIZE", "length"))? <= limit,
        "METADATA_SIZE",
        path.display().to_string(),
    )?;
    require(
        identity(&file.metadata().map_err(io)?) == identity(expected),
        "FILE_CHANGED",
        path.display().to_string(),
    )?;
    let after = walk(path, false)?.ok_or_else(|| refuse("FILE_CHANGED", "missing"))?;
    require(
        identity(&after) == identity(expected),
        "FILE_CHANGED",
        path.display().to_string(),
    )?;
    Ok(bytes)
}

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let canonical: CanonJson =
        serde_json::from_slice(bytes).map_err(|e| refuse("JSON", e.to_string()))?;
    serde_json::from_value(canonical.0).map_err(|e| refuse("SCHEMA", e.to_string()))
}

fn validate_roles(inputs: &C1Inputs) -> Result<()> {
    use ChargedRole::*;
    let roles = [
        TrainMax168,
        TrainMax336,
        OosHeadline,
        OosHigherPerpFee,
        OosSlippageStress,
    ];
    require(
        inputs.roles.len() == roles.len(),
        "ROLES",
        "exactly five charged roles required",
    )?;
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    for (index, (row, role)) in inputs.roles.iter().zip(roles).enumerate() {
        require(
            row.role == role && seen.insert(row.config_id.0.clone()),
            "ROLES",
            "role order or duplicate identity",
        )?;
        let mut expected = json!({"identity_schema":"parangsae.c1.config.v1", "protocol_version":inputs.protocol_version, "study_settings_id":inputs.study_id.0, "family":"C1", "role":role});
        let object = expected
            .as_object_mut()
            .ok_or_else(|| refuse("SCHEMA", "config object"))?;
        if index < 2 {
            object.insert("kind".to_owned(), json!("train_config"));
            object.insert("grid_index".to_owned(), json!(index));
            object.insert(
                "params".to_owned(),
                json!({"max_hold_h":if index == 0 {168} else {336}}),
            );
            object.insert("actual_cost_scenario".to_owned(), json!("HEADLINE"));
            candidates.push(row.config_id.0.clone());
        } else {
            object.insert("kind".to_owned(), json!("selected_policy"));
            object.insert("ordered_candidate_config_ids".to_owned(), json!(candidates));
            object.insert(
                "selection_contract".to_owned(),
                json!("TRAIN_HEADLINE_POSITIVE_NET_SR_THEN_SHORTER_ELSE_FLAT_V1"),
            );
            object.insert(
                "actual_cost_scenario".to_owned(),
                json!(["HEADLINE", "HIGHER_PERP_FEE", "SLIPPAGE_STRESS"][index - 2]),
            );
            object.insert("opportunity_cost_estimator".to_owned(), json!("HEADLINE"));
            object.insert("continuous_account".to_owned(), json!(true));
        }
        require(
            row.config_preimage.0 == expected && canonical_id(&expected) == row.config_id.0,
            "CONFIG_ID",
            "canonical role preimage mismatch",
        )?;
    }
    Ok(())
}

type DataNames = BTreeMap<String, (C1InputKind, String)>;

fn data_names(selection: &C1SourceSelection) -> Result<DataNames> {
    crate::c1_source::validate_source_profile(
        &selection.source_policy_id,
        &selection.daily_rebuilt_days,
        &selection.spot_status_days,
    )
    .map_err(|error| refuse("SOURCE_PROFILE", format!("{error:?}")))?;
    let mut result = BTreeMap::new();
    let mut day =
        UtcDate::parse("2020-12-18").ok_or_else(|| refuse("SOURCE_PROFILE", "start date"))?;
    while day.start_us().micros() < END {
        let date = day.to_string();
        for (prefix, kind) in [
            ("parquet/um-kline", C1InputKind::UmTrade),
            ("parquet/mark-price-kline", C1InputKind::PerpMark),
            (
                "runs/funding-4db8a75/parquet/funding-rate",
                C1InputKind::Funding,
            ),
        ] {
            result.insert(
                format!("{prefix}/symbol=BTCUSDT/date={date}/part.parquet"),
                (kind, date.clone()),
            );
        }
        let prefix = if selection.daily_rebuilt_days.contains(&date) {
            "research/c1/spot-valid"
        } else {
            "parquet/spot-kline"
        };
        result.insert(
            format!("{prefix}/symbol=BTCUSDT/date={date}/part.parquet"),
            (C1InputKind::SpotTrade, date),
        );
        day = day.next();
    }
    for date in &selection.spot_status_days {
        result.insert(
            format!("research/c1/spot-status/symbol=BTCUSDT/date={date}/status.jsonl"),
            (C1InputKind::SpotStatus, date.clone()),
        );
    }
    result.insert(
        "runs/resolutions/um-kline-2023-11-10.parquet".to_owned(),
        (C1InputKind::UmResolution, "2023-11-10".to_owned()),
    );
    Ok(result)
}

fn validate_metadata(inputs: &C1Inputs) -> Result<DataNames> {
    require(
        inputs.schema == "parangsae-c1-inputs-v1" && inputs.protocol_version == "C1-2026-10-24-v1",
        "SCHEMA",
        "unsupported C1 protocol",
    )?;
    let expected = json!({"identity_schema":"parangsae.c1.study.v1", "protocol_version":inputs.protocol_version, "settings":inputs.settings.0});
    require(
        canonical_id(&expected) == inputs.study_id.0,
        "STUDY_ID",
        "canonical settings differ",
    )?;
    super::study::validate_runtime_settings(&inputs.settings.0)?;
    require(
        inputs.settings.0.pointer("/scope/family") == Some(&json!("C1"))
            && inputs.settings.0.pointer("/scope/development_start_us") == Some(&json!(START))
            && inputs
                .settings
                .0
                .pointer("/scope/development_end_exclusive_us")
                == Some(&json!(END))
            && inputs.settings.0.pointer("/scope/holdout_start_us") == Some(&json!(END)),
        "HOLDOUT",
        "full development scope required",
    )?;
    validate_roles(inputs)?;
    let selected = &inputs.source_selection;
    require(
        !selected.source_policy_id.trim().is_empty() && selected.source_policy_id.len() <= 512,
        "SOURCE_PROFILE",
        "source policy identity unbound",
    )?;
    for (key, value) in [
        ("source_policy_id", json!(selected.source_policy_id)),
        ("daily_rebuilt_days", json!(selected.daily_rebuilt_days)),
        ("spot_status_days", json!(selected.spot_status_days)),
    ] {
        require(
            inputs.settings.0.get("sources").and_then(|s| s.get(key)) == Some(&value),
            "SOURCE_PROFILE",
            format!("settings source profile {key} differs or is unbound"),
        )?;
    }
    require(
        inputs.source_manifest.path == CATALOG,
        "SOURCE_MANIFEST",
        "fixed root catalog required",
    )?;
    require(
        inputs.prereg.path == PREREG,
        "PREREG",
        "the C1 preregistration is required",
    )?;
    let data = data_names(selected)?;
    let mut expected: BTreeMap<String, (C1InputKind, Option<&Hash64>)> = data
        .iter()
        .map(|(path, (kind, _))| (path.clone(), (*kind, None)))
        .collect();
    for (artifact, kind) in std::iter::once((&inputs.prereg, C1InputKind::Prereg))
        .chain(std::iter::once((
            &inputs.source_manifest,
            C1InputKind::SourceManifest,
        )))
        .chain(inputs.contracts.iter().map(|a| (a, C1InputKind::Contract)))
    {
        logical(&artifact.path)?;
        require(
            artifact.path.ends_with(".md")
                || artifact.path.ends_with(".json")
                || artifact.path.ends_with(".sql"),
            "CONTRACT",
            &artifact.path,
        )?;
        require(
            !artifact.path.starts_with("parquet/")
                && !artifact.path.starts_with("runs/")
                && !artifact.path.contains("/spot-status/"),
            "CONTRACT",
            &artifact.path,
        )?;
        require(
            artifact.path != "sweep-inputs-v2.json",
            "CONTRACT",
            "H4 control is not a C1 contract",
        )?;
        require(
            expected
                .insert(artifact.path.clone(), (kind, Some(&artifact.sha256)))
                .is_none(),
            "DUPLICATE_PATH",
            &artifact.path,
        )?;
    }
    require(
        !inputs.contracts.is_empty(),
        "CONTRACT",
        "explicit nondata contracts required",
    )?;
    require(
        inputs.objects.len() == expected.len(),
        "UNIVERSE",
        "closed object count differs",
    )?;
    let mut paths = BTreeSet::new();
    for object in &inputs.objects {
        logical(&object.path)?;
        require(paths.insert(&object.path), "DUPLICATE_PATH", &object.path)?;
        let Some((kind, digest)) = expected.get(&object.path) else {
            return Err(refuse("UNIVERSE", &object.path));
        };
        require(
            object.kind == *kind && digest.is_none_or(|hash| *hash == object.sha256),
            "UNIVERSE",
            &object.path,
        )?;
    }
    require(
        pin_from_declared(inputs) == inputs.pinned_root_expected.0,
        "PIN_MISMATCH",
        "declared portable pin differs",
    )?;
    Ok(data)
}

fn pin_from_declared(inputs: &C1Inputs) -> String {
    let sorted: BTreeMap<_, _> = inputs
        .objects
        .iter()
        .map(|o| (&o.path, &o.sha256.0))
        .collect();
    canonical_id(&json!(sorted
        .into_iter()
        .map(|(p, h)| [p, h])
        .collect::<Vec<_>>()))
}

fn inventory(root: &Path, allowed: &BTreeSet<String>) -> Result<BTreeMap<String, Metadata>> {
    let directories: BTreeSet<_> = allowed
        .iter()
        .flat_map(|name| {
            Path::new(name)
                .ancestors()
                .skip(1)
                .filter_map(|p| p.to_str())
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect();
    let mut stack = vec![root.to_path_buf()];
    let mut files = BTreeMap::new();
    let mut physical = BTreeSet::new();
    while let Some(directory) = stack.pop() {
        let meta =
            walk(&directory, false)?.ok_or_else(|| refuse("PATH", "missing snapshot directory"))?;
        require(
            meta.is_dir() && meta.mode() & 0o222 == 0,
            "SNAPSHOT_DIRECTORY",
            directory.display().to_string(),
        )?;
        for entry in fs::read_dir(&directory).map_err(io)? {
            let path = entry.map_err(io)?.path();
            let meta = fs::symlink_metadata(&path).map_err(io)?;
            require(
                !meta.file_type().is_symlink(),
                "SYMLINK",
                path.display().to_string(),
            )?;
            if meta.is_dir() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| refuse("PATH", "outside snapshot"))?
                    .to_str()
                    .ok_or_else(|| refuse("PATH", "non-UTF8 directory"))?;
                require(
                    directories.contains(relative),
                    "UNEXPECTED_MEMBER",
                    relative,
                )?;
                stack.push(path);
                continue;
            }
            readonly_file(&path, &meta)?;
            require(
                physical.insert((meta.dev(), meta.ino())),
                "DUPLICATE_PHYSICAL",
                path.display().to_string(),
            )?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| refuse("PATH", "outside snapshot"))?
                .to_str()
                .ok_or_else(|| refuse("PATH", "non-UTF8 member"))?
                .to_owned();
            require(allowed.contains(&relative), "UNEXPECTED_MEMBER", &relative)?;
            files.insert(relative, meta);
        }
    }
    require(
        files.len() == allowed.len(),
        "MISSING_MEMBER",
        "restricted snapshot is incomplete",
    )?;
    Ok(files)
}

fn validate_catalog(catalog: &SourceManifest, inputs: &C1Inputs, data: &DataNames) -> Result<()> {
    let selected = &inputs.source_selection;
    require(
        catalog.schema == "parangsae-c1-source-manifest-v1"
            && catalog.source_start_us == START
            && catalog.source_end_us_exclusive == END
            && catalog.symbol == "BTCUSDT"
            && catalog.source_policy_id == selected.source_policy_id
            && catalog.daily_rebuilt_days == selected.daily_rebuilt_days
            && catalog.spot_status_days == selected.spot_status_days,
        "SOURCE_MANIFEST",
        "catalog source profile differs",
    )?;
    require(
        catalog.data_objects.len() == data.len(),
        "SOURCE_MANIFEST",
        "DATA subset count differs",
    )?;
    let objects: BTreeMap<_, _> = inputs.objects.iter().map(|o| (&o.path, o)).collect();
    let mut seen = BTreeSet::new();
    for row in &catalog.data_objects {
        logical(&row.path)?;
        require(seen.insert(&row.path), "DUPLICATE_PATH", &row.path)?;
        let expected = data
            .get(&row.path)
            .ok_or_else(|| refuse("SOURCE_MANIFEST", &row.path))?;
        let full = objects
            .get(&row.path)
            .ok_or_else(|| refuse("SOURCE_MANIFEST", &row.path))?;
        require(
            expected == &(row.role, row.date.clone())
                && row.sha256 == full.sha256
                && row.role == full.kind,
            "SOURCE_MANIFEST",
            &row.path,
        )?;
    }
    Ok(())
}

fn hash_checked(path: &Path, expected: &Metadata) -> Result<String> {
    let mut file = File::open(path).map_err(io)?;
    require(
        identity(&file.metadata().map_err(io)?) == identity(expected),
        "FILE_CHANGED",
        path.display().to_string(),
    )?;
    let mut sha = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = file.read(&mut buffer).map_err(io)?;
        if count == 0 {
            break;
        }
        sha.update(&buffer[..count]);
    }
    require(
        identity(&file.metadata().map_err(io)?) == identity(expected),
        "FILE_CHANGED",
        path.display().to_string(),
    )?;
    let after =
        walk(path, false)?.ok_or_else(|| refuse("FILE_CHANGED", "missing snapshot member"))?;
    require(
        identity(&after) == identity(expected),
        "FILE_CHANGED",
        path.display().to_string(),
    )?;
    Ok(sha
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(super) fn admit(request: &C1BatchRequest) -> Result<AdmittedC1> {
    require(
        request.attempt == 4,
        "ATTEMPT",
        "C1 local attempt4 only; no automatic retry",
    )?;
    for path in [
        &request.inputs,
        &request.data_root,
        &request.out,
        &request.ledger,
    ] {
        absolute(path)?;
    }
    require(
        request.inputs.parent() == Some(request.data_root.as_path()),
        "PATH",
        "control and root catalog share the snapshot parent",
    )?;
    require(
        request.ledger == request.out.join("trials.jsonl")
            && !request.out.starts_with(&request.data_root)
            && !request.data_root.starts_with(&request.out),
        "PATH",
        "disjoint fresh output and exact ledger required",
    )?;
    require(
        walk(&request.out, true)?.is_none() && walk(&request.ledger, true)?.is_none(),
        "OUTPUT_EXISTS",
        "output or ledger already exists",
    )?;
    let control_meta =
        walk(&request.inputs, false)?.ok_or_else(|| refuse("PATH", "missing input control"))?;
    let control_bytes = checked_read(&request.inputs, &control_meta, LIMIT)?;
    let inputs: C1Inputs = parse(&control_bytes)?;
    let data = validate_metadata(&inputs)?;
    let control_name = request
        .inputs
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| refuse("PATH", "control basename"))?;
    logical(control_name)?;
    require(
        control_name.ends_with(".json") && !inputs.objects.iter().any(|o| o.path == control_name),
        "CONTROL",
        "control is a separate unpinned JSON member",
    )?;
    let allowed: BTreeSet<_> = inputs
        .objects
        .iter()
        .map(|o| o.path.clone())
        .chain(std::iter::once(control_name.to_owned()))
        .collect();
    let before = inventory(&request.data_root, &allowed)?;
    require(
        identity(
            before
                .get(control_name)
                .ok_or_else(|| refuse("CONTROL", "missing control"))?,
        ) == identity(&control_meta),
        "FILE_CHANGED",
        "control changed during metadata admission",
    )?;
    let source_manifest = request.data_root.join(CATALOG);
    let catalog: SourceManifest = parse(&checked_read(
        &source_manifest,
        before
            .get(CATALOG)
            .ok_or_else(|| refuse("SOURCE_MANIFEST", "missing catalog"))?,
        LIMIT,
    )?)?;
    validate_catalog(&catalog, &inputs, &data)?;
    for object in &inputs.objects {
        let meta = before
            .get(&object.path)
            .ok_or_else(|| refuse("MISSING_MEMBER", &object.path))?;
        require(
            hash_checked(&request.data_root.join(&object.path), meta)? == object.sha256.0,
            "OBJECT_HASH",
            &object.path,
        )?;
    }
    let after = inventory(&request.data_root, &allowed)?;
    require(
        before
            .iter()
            .all(|(p, m)| after.get(p).is_some_and(|a| identity(a) == identity(m))),
        "FILE_CHANGED",
        "snapshot changed while hashing",
    )?;
    fs::create_dir(&request.out).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            refuse("OUTPUT_EXISTS", "output was claimed concurrently")
        } else {
            io(e)
        }
    })?;
    if let Some(parent) = request.out.parent() {
        File::open(parent).and_then(|f| f.sync_all()).map_err(io)?;
    }
    let ledger_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&request.ledger)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                refuse(
                    "LEDGER_EXISTS",
                    "ledger was claimed concurrently; preserve output",
                )
            } else {
                io(e)
            }
        })?;
    ledger_file.sync_all().map_err(io)?;
    File::open(&request.out)
        .and_then(|f| f.sync_all())
        .map_err(io)?;
    if let Some(parent) = request.out.parent() {
        File::open(parent).and_then(|f| f.sync_all()).map_err(io)?;
    }
    Ok(AdmittedC1 {
        pin: inputs.pinned_root_expected.0.clone(),
        prereg_identity: inputs.prereg.clone(),
        inputs,
        source_manifest,
        out: request.out.clone(),
        ledger: request.ledger.clone(),
        _ledger_file: ledger_file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cucumber::{given, then, when, World as _};
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn hash(bytes: &[u8]) -> Hash64 {
        Hash64(sha256_hex(bytes))
    }

    fn fixture_inputs() -> C1Inputs {
        let settings: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/c1/runtime-settings.json"
        ))
        .unwrap();
        let sources = &settings["sources"];
        let selection = C1SourceSelection {
            source_policy_id: sources["source_policy_id"].as_str().unwrap().to_owned(),
            daily_rebuilt_days: serde_json::from_value(sources["daily_rebuilt_days"].clone())
                .unwrap(),
            spot_status_days: serde_json::from_value(sources["spot_status_days"].clone()).unwrap(),
        };
        let study = canonical_id(
            &json!({"identity_schema":"parangsae.c1.study.v1", "protocol_version":"C1-2026-10-24-v1", "settings":settings}),
        );
        let mut roles = Vec::new();
        let mut candidates = Vec::new();
        for (index, role) in [
            ChargedRole::TrainMax168,
            ChargedRole::TrainMax336,
            ChargedRole::OosHeadline,
            ChargedRole::OosHigherPerpFee,
            ChargedRole::OosSlippageStress,
        ]
        .into_iter()
        .enumerate()
        {
            let mut value = json!({"identity_schema":"parangsae.c1.config.v1", "protocol_version":"C1-2026-10-24-v1", "study_settings_id":study, "family":"C1", "role":role});
            let fields = value.as_object_mut().unwrap();
            if index < 2 {
                fields.insert("kind".to_owned(), json!("train_config"));
                fields.insert("grid_index".to_owned(), json!(index));
                fields.insert(
                    "params".to_owned(),
                    json!({"max_hold_h":if index == 0 {168} else {336}}),
                );
                fields.insert("actual_cost_scenario".to_owned(), json!("HEADLINE"));
            } else {
                fields.insert("kind".to_owned(), json!("selected_policy"));
                fields.insert("ordered_candidate_config_ids".to_owned(), json!(candidates));
                fields.insert(
                    "selection_contract".to_owned(),
                    json!("TRAIN_HEADLINE_POSITIVE_NET_SR_THEN_SHORTER_ELSE_FLAT_V1"),
                );
                fields.insert(
                    "actual_cost_scenario".to_owned(),
                    json!(["HEADLINE", "HIGHER_PERP_FEE", "SLIPPAGE_STRESS"][index - 2]),
                );
                fields.insert("opportunity_cost_estimator".to_owned(), json!("HEADLINE"));
                fields.insert("continuous_account".to_owned(), json!(true));
            }
            let id = canonical_id(&value);
            if index < 2 {
                candidates.push(id.clone());
            }
            roles.push(C1RoleConfig {
                role,
                config_id: Hash64(id),
                config_preimage: CanonJson(value),
            });
        }
        let objects = data_names(&selection)
            .unwrap()
            .into_iter()
            .map(|(path, (kind, _))| C1PinnedObject {
                path,
                sha256: hash(b"x"),
                kind,
            })
            .collect();
        C1Inputs {
            schema: "parangsae-c1-inputs-v1".to_owned(),
            protocol_version: "C1-2026-10-24-v1".to_owned(),
            study_id: Hash64(study),
            settings: CanonJson(settings),
            prereg: ArtifactRef {
                path: PREREG.to_owned(),
                sha256: hash(b"x"),
            },
            roles,
            source_selection: selection,
            source_manifest: ArtifactRef {
                path: CATALOG.to_owned(),
                sha256: hash(b"x"),
            },
            contracts: vec![ArtifactRef {
                path: "research/c1/source-policy.json".to_owned(),
                sha256: hash(b"x"),
            }],
            objects,
            pinned_root_expected: hash(b"unbound"),
        }
    }

    fn rebind_settings(inputs: &mut C1Inputs) {
        inputs.study_id = Hash64(canonical_id(&json!({
            "identity_schema":"parangsae.c1.study.v1",
            "protocol_version":inputs.protocol_version,
            "settings":inputs.settings.0
        })));
        let mut candidates = Vec::new();
        for (index, row) in inputs.roles.iter_mut().enumerate() {
            row.config_preimage.0["study_settings_id"] = json!(inputs.study_id.0);
            if index >= 2 {
                row.config_preimage.0["ordered_candidate_config_ids"] = json!(candidates);
            }
            row.config_id = Hash64(canonical_id(&row.config_preimage.0));
            if index < 2 {
                candidates.push(row.config_id.0.clone());
            }
        }
    }

    #[derive(Debug)]
    struct Fixture {
        directory: tempfile::TempDir,
        request: C1BatchRequest,
        inputs: C1Inputs,
    }

    fn writable(path: &Path) {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return;
        };
        if meta.file_type().is_symlink() {
            return;
        }
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if meta.is_dir() { 0o755 } else { 0o644 }),
        )
        .unwrap();
        if meta.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                writable(&entry.unwrap().path());
            }
        }
    }

    fn freeze(path: &Path) {
        let meta = fs::symlink_metadata(path).unwrap();
        if meta.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                freeze(&entry.unwrap().path());
            }
        }
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if meta.is_dir() { 0o555 } else { 0o444 }),
        )
        .unwrap();
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            writable(self.directory.path());
            fn bytes(path: &Path, seen: &mut BTreeSet<(u64, u64)>) -> u64 {
                let meta = fs::symlink_metadata(path).unwrap();
                if meta.is_dir() {
                    fs::read_dir(path)
                        .unwrap()
                        .map(|entry| bytes(&entry.unwrap().path(), seen))
                        .sum()
                } else if meta.is_file() && seen.insert((meta.dev(), meta.ino())) {
                    meta.len()
                } else {
                    0
                }
            }
            let removed = bytes(self.directory.path(), &mut BTreeSet::new());
            fs::remove_dir_all(self.directory.path()).unwrap();
            assert!(!self.directory.path().exists());
            println!("C1_SYNTHETIC_FIXTURE_REMOVED_BYTES={removed}");
        }
    }

    impl Fixture {
        fn new(materialize: bool) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().canonicalize().unwrap();
            let data_root = root.join("snapshot");
            fs::create_dir(&data_root).unwrap();
            let request = C1BatchRequest {
                inputs: data_root.join("c1-inputs-v1.json"),
                data_root,
                out: root.join("output"),
                ledger: root.join("output/trials.jsonl"),
                attempt: 4,
            };
            let mut inputs = fixture_inputs();
            let data = data_names(&inputs.source_selection).unwrap();
            let rows: Vec<_> = data.iter().map(|(path, (role, date))| json!({"path":path,"sha256":hash(b"x"),"role":role,"date":date})).collect();
            let catalog = serde_json::to_vec(&json!({"schema":"parangsae-c1-source-manifest-v1", "source_start_us":START,"source_end_us_exclusive":END,"symbol":"BTCUSDT","source_policy_id":inputs.source_selection.source_policy_id,"daily_rebuilt_days":inputs.source_selection.daily_rebuilt_days,"spot_status_days":inputs.source_selection.spot_status_days,"data_objects":rows})).unwrap();
            inputs.source_manifest.sha256 = hash(&catalog);
            for (artifact, kind) in [
                (&inputs.prereg, C1InputKind::Prereg),
                (&inputs.source_manifest, C1InputKind::SourceManifest),
                (&inputs.contracts[0], C1InputKind::Contract),
            ] {
                inputs.objects.push(C1PinnedObject {
                    path: artifact.path.clone(),
                    sha256: artifact.sha256.clone(),
                    kind,
                });
            }
            inputs.pinned_root_expected = Hash64(pin_from_declared(&inputs));
            if materialize {
                for object in &inputs.objects {
                    let path = request.data_root.join(&object.path);
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(
                        path,
                        if object.path == CATALOG {
                            catalog.as_slice()
                        } else {
                            b"x"
                        },
                    )
                    .unwrap();
                }
            }
            fs::write(&request.inputs, serde_json::to_vec(&inputs).unwrap()).unwrap();
            freeze(&request.data_root);
            Self {
                directory,
                request,
                inputs,
            }
        }

        fn write_control(&self) {
            fs::set_permissions(&self.request.data_root, fs::Permissions::from_mode(0o755))
                .unwrap();
            fs::set_permissions(&self.request.inputs, fs::Permissions::from_mode(0o644)).unwrap();
            fs::write(
                &self.request.inputs,
                serde_json::to_vec(&self.inputs).unwrap(),
            )
            .unwrap();
            fs::set_permissions(&self.request.inputs, fs::Permissions::from_mode(0o444)).unwrap();
            fs::set_permissions(&self.request.data_root, fs::Permissions::from_mode(0o555))
                .unwrap();
        }

        fn first_data(&self) -> PathBuf {
            self.request.data_root.join(&self.inputs.objects[0].path)
        }
    }

    #[test]
    fn duplicate_float_and_unsafe_canonical_numbers_are_refused() {
        for (text, reason) in [
            (r#"{"a":1,"a":2}"#, "duplicate"),
            (r#"{"a":1.0}"#, "floating point"),
            (r#"{"a":9007199254740992}"#, "unsafe"),
        ] {
            let error = serde_json::from_str::<CanonJson>(text).unwrap_err();
            assert!(error.to_string().contains(reason), "{error}");
        }
        let value: CanonJson = serde_json::from_str(r#"{"a":[true,-1,"x"],"z":null}"#).unwrap();
        assert_eq!(value.0, json!({"a":[true,-1,"x"],"z":null}));
        assert_eq!(
            canonical_id(&value.0),
            "e2b5e797925034b21549abd91667931b0f7adb96734f2a8aa74333fbb185282b"
        );
    }

    #[test]
    fn invalid_metadata_never_needs_a_data_payload() {
        let mut fixture = Fixture::new(false);
        fixture.inputs.roles[0].config_id = hash(b"wrong role");
        fixture.write_control();
        assert_eq!(admit(&fixture.request).unwrap_err().code, "CONFIG_ID");
        assert!(!fixture.request.out.exists());
        assert!(!fixture.request.data_root.join(CATALOG).exists());
    }

    #[test]
    fn self_consistent_unsupported_source_profile_is_refused_before_payloads() {
        let mut fixture = Fixture::new(false);
        fixture.inputs.source_selection.source_policy_id = "UNSUPPORTED_CALLER_POLICY".to_owned();
        fixture.inputs.settings.0["sources"]["source_policy_id"] =
            json!("UNSUPPORTED_CALLER_POLICY");
        rebind_settings(&mut fixture.inputs);
        fixture.write_control();
        assert_eq!(admit(&fixture.request).unwrap_err().code, "SOURCE_PROFILE");
        assert!(!fixture.request.data_root.join(CATALOG).exists());
        assert!(!fixture.request.out.exists());
    }

    #[test]
    fn recomputed_ids_cannot_override_the_actual_runtime_contract() {
        let mut fixture = Fixture::new(false);
        let original = fixture.inputs.settings.0.clone();
        for path in [
            "/capital/initial_total_cash_ticks",
            "/cost_scenarios/HEADLINE/perp_fee_bps",
            "/horizons_h/0",
            "/clock/entry_decision_offset_us",
            "/folds/schedule/0/train_end_exclusive_us",
        ] {
            fixture.inputs.settings.0 = original.clone();
            *fixture.inputs.settings.0.pointer_mut(path).unwrap() = json!(1);
            rebind_settings(&mut fixture.inputs);
            fixture.write_control();
            assert_eq!(
                admit(&fixture.request).unwrap_err().code,
                "RUNTIME_SETTINGS"
            );
            assert!(!fixture.request.data_root.join(CATALOG).exists());
            assert!(!fixture.request.out.exists());
            assert!(!fixture.request.ledger.exists());
        }
    }

    #[test]
    fn recomputed_ids_cannot_change_funding_floor_or_enable_mark_carry() {
        let mut fixture = Fixture::new(false);
        let original = fixture.inputs.settings.0.clone();
        let mut observed = Vec::new();
        for (path, value) in [
            ("/sources/funding_effective_available_floor_us", json!(0)),
            ("/sources/perp_mark_gap_carry", json!(true)),
        ] {
            fixture.inputs.settings.0 = original.clone();
            *fixture.inputs.settings.0.pointer_mut(path).unwrap() = value;
            rebind_settings(&mut fixture.inputs);
            fixture.write_control();
            observed.push(admit(&fixture.request).unwrap_err().code);
            assert!(!fixture.request.data_root.join(CATALOG).exists());
            assert!(!fixture.request.out.exists());
            assert!(!fixture.request.ledger.exists());
        }
        assert!(
            observed
                .iter()
                .all(|code| matches!(*code, "RUNTIME_SETTINGS" | "SOURCE_PROFILE")),
            "actual semantic refusal codes were {observed:?}"
        );
    }

    #[test]
    fn escaped_and_holdout_objects_are_refused_before_payloads() {
        let mut fixture = Fixture::new(false);
        let original = fixture.inputs.objects[0].path.clone();
        for (path, expected) in [
            ("../outside", "PATH"),
            (
                "parquet/um-kline/symbol=BTCUSDT/date=2025-10-01/part.parquet",
                "UNIVERSE",
            ),
        ] {
            fixture.inputs.objects[0].path = path.to_owned();
            fixture.write_control();
            assert_eq!(admit(&fixture.request).unwrap_err().code, expected);
            assert!(!fixture.request.out.exists());
        }
        fixture.inputs.objects[0].path = original;
        fixture.inputs.pinned_root_expected = hash(b"wrong root");
        fixture.write_control();
        assert_eq!(admit(&fixture.request).unwrap_err().code, "PIN_MISMATCH");
        assert!(!fixture.request.out.exists());
    }

    #[test]
    fn null_expected_pin_and_unknown_wire_fields_are_refused() {
        let fixture = Fixture::new(false);
        let mut value = serde_json::to_value(&fixture.inputs).unwrap();
        value["pinned_root_expected"] = Value::Null;
        assert_eq!(
            parse::<C1Inputs>(&serde_json::to_vec(&value).unwrap())
                .unwrap_err()
                .code,
            "SCHEMA"
        );
        value = serde_json::to_value(&fixture.inputs).unwrap();
        value["source_sha"] = json!("not authority");
        assert_eq!(
            parse::<C1Inputs>(&serde_json::to_vec(&value).unwrap())
                .unwrap_err()
                .code,
            "SCHEMA"
        );
    }

    #[test]
    fn independent_readonly_members_and_closed_directories_are_required() {
        let fixture = Fixture::new(true);
        let path = fixture.first_data();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(admit(&fixture.request).unwrap_err().code, "SNAPSHOT_FILE");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let alias = fixture.directory.path().join("outside-hardlink");
        fs::hard_link(&path, &alias).unwrap();
        assert_eq!(admit(&fixture.request).unwrap_err().code, "SNAPSHOT_FILE");
        fs::remove_file(alias).unwrap();
        fs::set_permissions(
            &fixture.request.data_root,
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let extra = fixture.request.data_root.join("unexpected-empty-directory");
        fs::create_dir(&extra).unwrap();
        fs::set_permissions(&extra, fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(
            &fixture.request.data_root,
            fs::Permissions::from_mode(0o555),
        )
        .unwrap();
        assert_eq!(
            admit(&fixture.request).unwrap_err().code,
            "UNEXPECTED_MEMBER"
        );
        fs::set_permissions(
            &fixture.request.data_root,
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        fs::remove_dir(extra).unwrap();
        fs::set_permissions(
            &fixture.request.data_root,
            fs::Permissions::from_mode(0o555),
        )
        .unwrap();
        let parent = path.parent().unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
        fs::remove_file(&path).unwrap();
        symlink(fixture.directory.path().join("missing-outside"), &path).unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o555)).unwrap();
        assert_eq!(admit(&fixture.request).unwrap_err().code, "SYMLINK");
        assert!(!fixture.request.out.exists());
    }

    #[test]
    fn wrong_actual_bytes_do_not_claim_output() {
        let fixture = Fixture::new(true);
        let path = fixture.first_data();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        fs::write(&path, b"different").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        assert_eq!(admit(&fixture.request).unwrap_err().code, "OBJECT_HASH");
        assert!(!fixture.request.out.exists());
        assert!(!fixture.request.ledger.exists());
    }

    #[test]
    fn full_metadata_fixture_claims_once_without_a_started_trial() {
        let fixture = Fixture::new(true);
        let admitted = admit(&fixture.request).unwrap();
        assert_eq!(admitted.pin, fixture.inputs.pinned_root_expected.0);
        assert_eq!(admitted.inputs.roles.len(), 5);
        assert_eq!(admitted.prereg_identity, fixture.inputs.prereg);
        assert_eq!(
            admitted.source_manifest,
            fixture.request.data_root.join(CATALOG)
        );
        assert_eq!(
            crate::canon::sha256_file(&admitted.source_manifest).unwrap(),
            admitted.inputs.source_manifest.sha256.0
        );
        assert_eq!(admitted.out, fixture.request.out);
        assert_eq!(admitted.ledger, fixture.request.ledger);
        assert_eq!(admitted._ledger_file.metadata().unwrap().len(), 0);
        assert_eq!(fs::read(&fixture.request.ledger).unwrap(), b"");
        assert_eq!(admit(&fixture.request).unwrap_err().code, "OUTPUT_EXISTS");
        assert!(fixture.request.out.is_dir());
        assert!(fixture.request.ledger.is_file());
    }

    #[test]
    fn concurrent_admissions_cannot_claim_two_writers() {
        let fixture = Fixture::new(true);
        std::thread::scope(|scope| {
            let first = scope.spawn(|| admit(&fixture.request));
            let second = scope.spawn(|| admit(&fixture.request));
            let outcomes = [first.join().unwrap(), second.join().unwrap()];
            assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
            let refused = outcomes
                .iter()
                .find_map(|result| result.as_ref().err())
                .unwrap();
            assert_eq!(refused.code, "OUTPUT_EXISTS");
        });
        assert!(fs::read(&fixture.request.ledger).unwrap().is_empty());
    }

    #[derive(Debug, Default, cucumber::World)]
    struct AdmissionWorld {
        fixture: Option<Fixture>,
        error: Option<C1AdmissionError>,
        admitted: Option<AdmittedC1>,
    }

    #[given("a C1 metadata fixture without market payloads")]
    fn without_payloads(world: &mut AdmissionWorld) {
        world.fixture = Some(Fixture::new(false));
    }

    #[given("an independent readonly synthetic C1 object tree")]
    fn complete_tree(world: &mut AdmissionWorld) {
        world.fixture = Some(Fixture::new(true));
    }

    #[when("the proposed candidate identity is changed")]
    fn wrong_candidate(world: &mut AdmissionWorld) {
        let fixture = world.fixture.as_mut().unwrap();
        fixture.inputs.roles[0].config_id = hash(b"wrong");
        fixture.write_control();
        world.error = Some(admit(&fixture.request).unwrap_err());
    }

    #[then("C1 refuses the identity before requiring a data file or output")]
    fn identity_refusal(world: &mut AdmissionWorld) {
        assert_eq!(world.error.as_ref().unwrap().code, "CONFIG_ID");
        let request = &world.fixture.as_ref().unwrap().request;
        assert!(!request.data_root.join(CATALOG).exists());
        assert!(!request.out.exists());
    }

    #[when("the expected portable pin is changed")]
    fn wrong_pin(world: &mut AdmissionWorld) {
        let fixture = world.fixture.as_mut().unwrap();
        fixture.inputs.pinned_root_expected = hash(b"wrong pin");
        fixture.write_control();
        world.error = Some(admit(&fixture.request).unwrap_err());
    }

    #[then("C1 refuses the pin without creating output or ledger")]
    fn pin_refusal(world: &mut AdmissionWorld) {
        assert_eq!(world.error.as_ref().unwrap().code, "PIN_MISMATCH");
        let request = &world.fixture.as_ref().unwrap().request;
        assert!(!request.out.exists());
        assert!(!request.ledger.exists());
    }

    #[when("the caller changes the source policy and recomputes every identity")]
    fn unsupported_profile(world: &mut AdmissionWorld) {
        let fixture = world.fixture.as_mut().unwrap();
        fixture.inputs.source_selection.source_policy_id = "UNSUPPORTED_CALLER_POLICY".to_owned();
        fixture.inputs.settings.0["sources"]["source_policy_id"] =
            json!("UNSUPPORTED_CALLER_POLICY");
        rebind_settings(&mut fixture.inputs);
        fixture.write_control();
        world.error = Some(admit(&fixture.request).unwrap_err());
    }

    #[then("the actual Data profile rejects it before payloads or output")]
    fn unsupported_profile_refused(world: &mut AdmissionWorld) {
        assert_eq!(world.error.as_ref().unwrap().code, "SOURCE_PROFILE");
        let request = &world.fixture.as_ref().unwrap().request;
        assert!(!request.data_root.join(CATALOG).exists());
        assert!(!request.out.exists());
    }

    #[when("the caller changes capital and recomputes every identity")]
    fn unsupported_runtime(world: &mut AdmissionWorld) {
        let fixture = world.fixture.as_mut().unwrap();
        fixture.inputs.settings.0["capital"]["initial_total_cash_ticks"] = json!(1);
        rebind_settings(&mut fixture.inputs);
        fixture.write_control();
        world.error = Some(admit(&fixture.request).unwrap_err());
    }

    #[then("the actual runtime contract rejects it before payloads or output")]
    fn unsupported_runtime_refused(world: &mut AdmissionWorld) {
        assert_eq!(world.error.as_ref().unwrap().code, "RUNTIME_SETTINGS");
        let request = &world.fixture.as_ref().unwrap().request;
        assert!(!request.data_root.join(CATALOG).exists());
        assert!(!request.out.exists());
    }

    #[when("C1 admits the synthetic object tree once")]
    fn admit_once(world: &mut AdmissionWorld) {
        world.admitted = Some(admit(&world.fixture.as_ref().unwrap().request).unwrap());
    }

    #[then("the new ledger is empty and a second admission is refused")]
    fn one_claim(world: &mut AdmissionWorld) {
        let fixture = world.fixture.as_ref().unwrap();
        assert!(fs::read(&fixture.request.ledger).unwrap().is_empty());
        assert_eq!(admit(&fixture.request).unwrap_err().code, "OUTPUT_EXISTS");
        assert_eq!(
            world.admitted.as_ref().unwrap().pin,
            fixture.inputs.pinned_root_expected.0
        );
    }

    #[tokio::test]
    async fn admission_behavior_feature() {
        AdmissionWorld::cucumber()
            .with_default_cli()
            .fail_on_skipped()
            .run_and_exit(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/features/c1_admission.feature"
            ))
            .await;
    }
}
