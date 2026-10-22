//! Reviewed resolution registry. Unreviewed entries are refused.

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::source::parse_cadence;
use super::versions::{active_sha_hex, versions_on, PAYLOAD_V2};
use crate::civil::day_of;
use crate::vision::Cadence;
use parangsae_core::UtcMicros;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolutionError {
    pub check: &'static str,
    pub detail: String,
}

impl ResolutionError {
    fn new(check: &'static str, detail: impl Into<String>) -> Self {
        Self {
            check,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for ResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.check, self.detail)
    }
}

#[derive(Clone, Debug)]
pub struct VersionRef {
    pub path: String,
    pub sha256: String,
    pub cadence: Cadence,
    pub last_modified: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Resolution {
    pub id: String,
    pub dataset: String,
    pub symbol: String,
    pub pk_first_us: i64,
    pub pk_last_us: i64,
    pub key_count: u64,
    pub version_set: Vec<VersionRef>,
    pub choose_sha256: String,
    pub keys_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownAbsence {
    pub id: String,
    pub dataset: String,
    pub symbol: String,
    pub start_us: i64,
    pub end_us: i64,
    pub daily_sha256: String,
    pub monthly_sha256: String,
}

#[derive(Clone, Debug, Default)]
pub struct Registry {
    entries: Vec<Resolution>,
}

impl Registry {
    pub fn entries(&self) -> &[Resolution] {
        &self.entries
    }

    pub fn lookup(
        &self,
        dataset: &str,
        symbol: &str,
        pk_us: i64,
        active_sha: &[String],
    ) -> Option<&Resolution> {
        let mut active = active_sha.to_vec();
        active.sort();
        self.entries.iter().find(|entry| {
            entry.dataset == dataset
                && entry.symbol == symbol
                && pk_us >= entry.pk_first_us
                && pk_us <= entry.pk_last_us
                && entry.sha_set() == active
        })
    }

    /// Recompute `keys_digest` for entries whose archives are all present.
    /// An entry that is still missing an archive is pending and is not an error.
    pub fn check_complete(&self, data_dir: &Path) -> Result<(), ResolutionError> {
        for entry in &self.entries {
            if !entry_complete(data_dir, entry)? {
                continue;
            }
            let (count, digest) = recompute(data_dir, entry)?;
            if count != entry.key_count || digest != entry.keys_digest {
                return Err(ResolutionError::new(
                    "resolution_mismatch",
                    entry.id.clone(),
                ));
            }
        }
        Ok(())
    }
}

impl Resolution {
    fn sha_set(&self) -> Vec<String> {
        let mut shas: Vec<String> = self
            .version_set
            .iter()
            .map(|item| item.sha256.clone())
            .collect();
        shas.sort();
        shas
    }
}

pub fn load(dir: &Path) -> Result<Registry, ResolutionError> {
    let mut entries = Vec::new();
    for_jsonl(dir, false, |line| {
        entries.push(parse_line(line)?);
        Ok(())
    })?;
    Ok(Registry { entries })
}

/// Known-source absences. A placeholder or missing `reviewed_by` is
/// `resolution_unreviewed`. An interval longer than five minutes is
/// `absence_too_long`.
pub fn load_known_absences(dir: &Path) -> Result<Vec<KnownAbsence>, ResolutionError> {
    let mut entries = Vec::new();
    for_jsonl(dir, true, |line| {
        entries.push(parse_absence(line)?);
        Ok(())
    })?;
    Ok(entries)
}

fn for_jsonl(
    dir: &Path,
    absence: bool,
    mut each: impl FnMut(&str) -> Result<(), ResolutionError>,
) -> Result<(), ResolutionError> {
    if !dir.exists() {
        return Ok(());
    }
    let mut paths = Vec::new();
    collect_jsonl(dir, &mut paths)?;
    paths.sort();
    for path in paths {
        if is_absence_log(&path) != absence {
            continue;
        }
        let text = fs::read_to_string(&path)
            .map_err(|err| ResolutionError::new("resolution_malformed", err.to_string()))?;
        for (index, line) in text.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            each(line).map_err(|err| {
                ResolutionError::new(
                    err.check,
                    format!("{}:{} {}", path.display(), index + 1, err.detail),
                )
            })?;
        }
    }
    Ok(())
}

fn is_absence_log(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("known_source_absence.jsonl")
}

fn collect_jsonl(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<(), ResolutionError> {
    let read = fs::read_dir(dir)
        .map_err(|err| ResolutionError::new("resolution_malformed", err.to_string()))?;
    for entry in read {
        let entry =
            entry.map_err(|err| ResolutionError::new("resolution_malformed", err.to_string()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
    Ok(())
}

fn parse_line(line: &str) -> Result<Resolution, ResolutionError> {
    let value: serde_json::Value = serde_json::from_str(line)
        .map_err(|err| ResolutionError::new("resolution_malformed", err.to_string()))?;
    let obj = value
        .as_object()
        .ok_or_else(|| ResolutionError::new("resolution_malformed", "line is not an object"))?;
    let id = req_str(obj, "id")?;
    require_review(obj, &id)?;
    let key_count = obj
        .get("key_count")
        .and_then(|item| item.as_u64())
        .ok_or_else(|| ResolutionError::new("resolution_malformed", format!("{id} key_count")))?;
    let version_set = parse_version_set(obj.get("version_set"), &id)?;
    let choose = req_str(obj, "choose_sha256")?;
    if !version_set.iter().any(|item| item.sha256 == choose) {
        return Err(ResolutionError::new(
            "resolution_malformed",
            format!("{id} choose_sha256"),
        ));
    }
    Ok(Resolution {
        id,
        dataset: req_str(obj, "dataset")?,
        symbol: req_str(obj, "symbol")?,
        pk_first_us: req_i64(obj, "pk_first_us")?,
        pk_last_us: req_i64(obj, "pk_last_us")?,
        key_count,
        version_set,
        choose_sha256: choose,
        keys_digest: req_str(obj, "keys_digest")?,
    })
}

fn parse_absence(line: &str) -> Result<KnownAbsence, ResolutionError> {
    let value: serde_json::Value = serde_json::from_str(line)
        .map_err(|err| ResolutionError::new("resolution_malformed", err.to_string()))?;
    let obj = value
        .as_object()
        .ok_or_else(|| ResolutionError::new("resolution_malformed", "line is not an object"))?;
    let id = req_str(obj, "id")?;
    require_review(obj, &id)?;
    let start_us = req_i64(obj, "start_us")?;
    let end_us = req_i64(obj, "end_us")?;
    let span = end_us
        .checked_sub(start_us)
        .ok_or_else(|| ResolutionError::new("resolution_malformed", format!("{id} interval")))?;
    if span <= 0 {
        return Err(ResolutionError::new(
            "resolution_malformed",
            format!("{id} interval"),
        ));
    }
    if span > 300_000_000 {
        return Err(ResolutionError::new("absence_too_long", id));
    }
    Ok(KnownAbsence {
        id,
        dataset: req_str(obj, "dataset")?,
        symbol: req_str(obj, "symbol")?,
        start_us,
        end_us,
        daily_sha256: nested_sha(obj, "daily")?,
        monthly_sha256: nested_sha(obj, "monthly")?,
    })
}

fn require_review(
    obj: &serde_json::Map<String, serde_json::Value>,
    id: &str,
) -> Result<(), ResolutionError> {
    let reviewed_by = obj
        .get("reviewed_by")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    let reviewed_at = obj
        .get("reviewed_at")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    if reviewed_by.is_empty()
        || reviewed_at.is_empty()
        || reviewed_by.contains('<')
        || reviewed_at.contains('<')
    {
        return Err(ResolutionError::new("resolution_unreviewed", id));
    }
    if !rfc3339_utc(reviewed_at) {
        return Err(ResolutionError::new(
            "resolution_malformed",
            format!("{id} reviewed_at"),
        ));
    }
    Ok(())
}

fn nested_sha(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<String, ResolutionError> {
    obj.get(key)
        .and_then(|item| item.as_object())
        .and_then(|item| item.get("sha256"))
        .and_then(|item| item.as_str())
        .map(str::to_owned)
        .ok_or_else(|| ResolutionError::new("resolution_malformed", key))
}

fn parse_version_set(
    value: Option<&serde_json::Value>,
    id: &str,
) -> Result<Vec<VersionRef>, ResolutionError> {
    let items = value
        .and_then(|item| item.as_array())
        .ok_or_else(|| ResolutionError::new("resolution_malformed", format!("{id} version_set")))?;
    let mut out = Vec::new();
    for item in items {
        let obj = item
            .as_object()
            .ok_or_else(|| ResolutionError::new("resolution_malformed", format!("{id} version")))?;
        let cadence = parse_cadence(req_str(obj, "cadence")?.as_str())
            .ok_or_else(|| ResolutionError::new("resolution_malformed", format!("{id} cadence")))?;
        out.push(VersionRef {
            path: req_str(obj, "path")?,
            sha256: req_str(obj, "sha256")?,
            cadence,
            last_modified: obj
                .get("last_modified")
                .and_then(|value| value.as_str())
                .map(str::to_owned),
        });
    }
    Ok(out)
}

fn req_str(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<String, ResolutionError> {
    obj.get(key)
        .and_then(|item| item.as_str())
        .map(str::to_owned)
        .ok_or_else(|| ResolutionError::new("resolution_malformed", key))
}

fn req_i64(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<i64, ResolutionError> {
    let text = req_str(obj, key)?;
    text.parse::<i64>()
        .map_err(|_| ResolutionError::new("resolution_malformed", key))
}

fn rfc3339_utc(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 20 || bytes[19] != b'Z' || bytes[10] != b'T' {
        return false;
    }
    let digits =
        |range: std::ops::Range<usize>| bytes[range].iter().all(|byte| byte.is_ascii_digit());
    digits(0..4)
        && bytes[4] == b'-'
        && digits(5..7)
        && bytes[7] == b'-'
        && digits(8..10)
        && digits(11..13)
        && bytes[13] == b':'
        && digits(14..16)
        && bytes[16] == b':'
        && digits(17..19)
}

fn entry_complete(data_dir: &Path, entry: &Resolution) -> Result<bool, ResolutionError> {
    let wanted = entry.sha_set();
    let mut pk = entry.pk_first_us;
    while pk <= entry.pk_last_us {
        let active = active_sha_hex(data_dir, &entry.dataset, &entry.symbol, pk)
            .map_err(|err| ResolutionError::new("resolution_mismatch", err.to_string()))?;
        if active != wanted {
            return Ok(false);
        }
        pk += 60_000_000;
    }
    Ok(true)
}

fn recompute(data_dir: &Path, entry: &Resolution) -> Result<(u64, String), ResolutionError> {
    let mut ordered = entry.version_set.clone();
    ordered.sort_by(|left, right| {
        cadence_rank(left.cadence)
            .cmp(&cadence_rank(right.cadence))
            .then(left.sha256.cmp(&right.sha256))
    });
    let mut body = String::new();
    let mut count = 0u64;
    let mut pk = entry.pk_first_us;
    while pk <= entry.pk_last_us {
        let day = day_of(UtcMicros::from_micros(pk))
            .map_err(|err| ResolutionError::new("resolution_mismatch", err.to_string()))?;
        let versions = versions_on(data_dir, &entry.dataset, &entry.symbol, day)
            .map_err(|err| ResolutionError::new("resolution_mismatch", err.to_string()))?;
        let mut hashes = Vec::new();
        for source in &ordered {
            let hash = versions
                .iter()
                .find(|version| {
                    version.payload_version == PAYLOAD_V2
                        && version.row.open_time.micros() == pk
                        && version.source.sha256_hex() == source.sha256
                })
                .map(|version| version.data_hash.clone())
                .ok_or_else(|| ResolutionError::new("resolution_mismatch", entry.id.clone()))?;
            hashes.push(hash);
        }
        body.push_str(&pk.to_string());
        for hash in hashes {
            body.push(' ');
            body.push_str(&hash);
        }
        body.push('\n');
        count += 1;
        pk += 60_000_000;
    }
    let digest = hex::encode(Sha256::digest(body.as_bytes()));
    Ok((count, digest))
}

fn cadence_rank(cadence: Cadence) -> u8 {
    match cadence {
        Cadence::Monthly => 0,
        Cadence::Daily => 1,
    }
}
