//! Downloaded-object identity and the idempotent write state machine.
//!
//! `merge_fact` is the Rust side of `specs/tla/ManifestWrite.tla`. Same payload
//! is a no-op. A different payload records a conflict and leaves the stored
//! value in place. `received_ts` is not part of the payload, so a second ingest
//! does not look like a new value. The model does not include checksum bytes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use parangsae_core::{ReceivedTs, UtcMicros};
use serde::{Deserialize, Serialize};

use crate::civil::{parse_day, CivilDay};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectManifest {
    pub url: String,
    pub sha256: [u8; 32],
    pub size_bytes: u64,
    pub schema_version: u32,
    pub path: String,
    pub dataset: String,
    pub symbol: String,
    pub row_count: u64,
    pub fetched_at: ReceivedTs,
    /// UTC days whose windowed archive rows must exist in Parquet, even after
    /// a failed write. Legacy records without this field check the full archive.
    pub materialized_days: Option<BTreeSet<CivilDay>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteEffect {
    Inserted,
    Unchanged,
    Conflict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored<T> {
    pub value: T,
    pub received: ReceivedTs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Slot<T> {
    Absent,
    Present(Stored<T>),
    Conflict {
        stored: Stored<T>,
        rejected: Stored<T>,
    },
}

impl<T> Slot<T> {
    pub fn stored(&self) -> Option<&Stored<T>> {
        match self {
            Self::Absent => None,
            Self::Present(stored) | Self::Conflict { stored, .. } => Some(stored),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeOutcome<T> {
    pub rows: Vec<T>,
    pub inserted: u64,
    pub unchanged: u64,
    pub conflicts: Vec<(T, T)>,
    pub committed: bool,
}

/// Merge a partition. One conflict leaves every stored payload untouched.
pub fn merge_rows<T, K>(
    existing: Vec<T>,
    incoming: Vec<T>,
    key_of: impl Fn(&T) -> K,
) -> MergeOutcome<T>
where
    T: Clone + PartialEq,
    K: Ord + Clone,
{
    let original = existing.clone();
    let mut slots: BTreeMap<K, Slot<T>> = BTreeMap::new();
    for row in existing {
        let key = key_of(&row);
        slots.insert(
            key,
            Slot::Present(Stored {
                received: placeholder_received(),
                value: row,
            }),
        );
    }
    let mut trial = slots;
    let mut inserted = 0u64;
    let mut unchanged = 0u64;
    let mut conflicts = Vec::new();
    for row in incoming {
        let key = key_of(&row);
        let slot = trial.entry(key).or_insert(Slot::Absent);
        let effect = merge_fact(slot, row, placeholder_received());
        match effect {
            WriteEffect::Inserted => inserted += 1,
            WriteEffect::Unchanged => unchanged += 1,
            WriteEffect::Conflict => {
                if let Slot::Conflict { stored, rejected } = slot {
                    conflicts.push((stored.value.clone(), rejected.value.clone()));
                }
            }
        }
    }
    if !conflicts.is_empty() {
        return MergeOutcome {
            rows: original,
            inserted: 0,
            unchanged,
            conflicts,
            committed: false,
        };
    }
    let rows = trial
        .into_values()
        .filter_map(|slot| slot.stored().map(|stored| stored.value.clone()))
        .collect();
    MergeOutcome {
        rows,
        inserted,
        unchanged,
        conflicts,
        committed: true,
    }
}

fn placeholder_received() -> ReceivedTs {
    ReceivedTs::from_micros(UtcMicros::from_micros(0))
}

/// Apply one write. The stored payload never changes once it is present.
pub fn merge_fact<T: Clone + PartialEq>(
    slot: &mut Slot<T>,
    value: T,
    received: ReceivedTs,
) -> WriteEffect {
    let incoming = Stored { value, received };
    match slot {
        Slot::Absent => {
            *slot = Slot::Present(incoming);
            WriteEffect::Inserted
        }
        Slot::Present(stored) if stored.value == incoming.value => WriteEffect::Unchanged,
        Slot::Present(stored) => {
            let stored = stored.clone();
            *slot = Slot::Conflict {
                stored,
                rejected: incoming,
            };
            WriteEffect::Conflict
        }
        Slot::Conflict { stored, .. } if stored.value == incoming.value => WriteEffect::Unchanged,
        Slot::Conflict { rejected, .. } => {
            *rejected = incoming;
            WriteEffect::Conflict
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ManifestLine {
    path: String,
    url: String,
    sha256: String,
    size_bytes: u64,
    schema_version: u32,
    dataset: String,
    symbol: String,
    row_count: u64,
    fetched_at_us: i64,
    event: String,
    previous_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    materialized_days: Option<Vec<String>>,
}

pub struct ManifestLog {
    path: PathBuf,
    active: Vec<ObjectManifest>,
}

impl ManifestLog {
    pub fn open(data_dir: &Path) -> Result<Self, super::Error> {
        let path = data_dir.join("manifest").join("objects.jsonl");
        let active = if path.exists() {
            load_active(&path)?
        } else {
            Vec::new()
        };
        Ok(Self { path, active })
    }

    pub fn active(&self) -> &[ObjectManifest] {
        &self.active
    }

    pub fn get(&self, vision_path: &str) -> Option<&ObjectManifest> {
        self.active.iter().find(|row| row.path == vision_path)
    }

    pub fn record_stored(
        &mut self,
        mut object: ObjectManifest,
        previous: Option<[u8; 32]>,
    ) -> Result<(), super::Error> {
        if let Some(current) = self.get(&object.path) {
            if current.sha256 == object.sha256
                && current.schema_version == object.schema_version
                && current.row_count == object.row_count
                && current.dataset == object.dataset
                && current.symbol == object.symbol
                && current.size_bytes == object.size_bytes
            {
                match (&current.materialized_days, &mut object.materialized_days) {
                    (Some(stored), Some(incoming)) => incoming.extend(stored),
                    _ => object.materialized_days = None,
                }
                if current.materialized_days == object.materialized_days {
                    return Ok(());
                }
            }
            if current.sha256 == object.sha256 && current.row_count != object.row_count {
                return Err(super::Error::check(
                    "manifest_row_count",
                    format!(
                        "{} row_count {} does not match the stored {}",
                        object.path, object.row_count, current.row_count
                    ),
                ));
            }
        }
        self.append(&object, "stored", previous)?;
        if let Some(pos) = self.active.iter().position(|row| row.path == object.path) {
            self.active[pos] = object;
        } else {
            self.active.push(object);
        }
        Ok(())
    }

    pub fn record_rejected(
        &mut self,
        object: ObjectManifest,
        previous: [u8; 32],
    ) -> Result<(), super::Error> {
        self.append(&object, "rejected", Some(previous))
    }

    fn append(
        &self,
        object: &ObjectManifest,
        event: &str,
        previous: Option<[u8; 32]>,
    ) -> Result<(), super::Error> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| super::Error::Io {
                path: parent.display().to_string(),
                source,
            })?;
        }
        let line = ManifestLine {
            path: object.path.clone(),
            url: object.url.clone(),
            sha256: hex::encode(object.sha256),
            size_bytes: object.size_bytes,
            schema_version: object.schema_version,
            dataset: object.dataset.clone(),
            symbol: object.symbol.clone(),
            row_count: object.row_count,
            fetched_at_us: object.fetched_at.micros().micros(),
            event: event.to_owned(),
            previous_sha256: previous.map(hex::encode),
            materialized_days: object
                .materialized_days
                .as_ref()
                .map(|days| days.iter().map(ToString::to_string).collect()),
        };
        let encoded = serde_json::to_string(&line)
            .map_err(|err| super::Error::check("manifest", err.to_string()))?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|source| super::Error::Io {
                path: self.path.display().to_string(),
                source,
            })?;
        writeln!(file, "{encoded}").map_err(|source| super::Error::Io {
            path: self.path.display().to_string(),
            source,
        })?;
        Ok(())
    }
}

fn load_active(path: &Path) -> Result<Vec<ObjectManifest>, super::Error> {
    let text = fs::read_to_string(path).map_err(|source| super::Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut active: Vec<ObjectManifest> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let parsed: ManifestLine = serde_json::from_str(line).map_err(|err| {
            super::Error::check(
                "manifest",
                format!("{} line {}: {err}", path.display(), index + 1),
            )
        })?;
        if parsed.event != "stored" {
            continue;
        }
        let bytes = hex::decode(&parsed.sha256).map_err(|err| {
            super::Error::check(
                "manifest",
                format!("bad sha256 on line {}: {err}", index + 1),
            )
        })?;
        let mut sha256 = [0u8; 32];
        if bytes.len() != 32 {
            return Err(super::Error::check("manifest", "sha256 is not 32 bytes"));
        }
        sha256.copy_from_slice(&bytes);
        let object = ObjectManifest {
            url: parsed.url,
            sha256,
            size_bytes: parsed.size_bytes,
            schema_version: parsed.schema_version,
            path: parsed.path,
            dataset: parsed.dataset,
            symbol: parsed.symbol,
            row_count: parsed.row_count,
            fetched_at: ReceivedTs::from_micros(UtcMicros::from_micros(parsed.fetched_at_us)),
            materialized_days: parsed
                .materialized_days
                .map(|days| {
                    days.into_iter()
                        .map(|day| {
                            parse_day(&day)
                                .map_err(|err| super::Error::check("manifest", err.to_string()))
                        })
                        .collect()
                })
                .transpose()?,
        };
        if let Some(pos) = active.iter().position(|row| row.path == object.path) {
            active[pos] = object;
        } else {
            active.push(object);
        }
    }
    Ok(active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn at(micros: i64) -> ReceivedTs {
        ReceivedTs::from_micros(UtcMicros::from_micros(micros))
    }

    #[test]
    fn coverage_is_idempotent_and_belongs_to_one_archive_identity() {
        let dir = tempfile::tempdir().unwrap();
        let june1 = CivilDay::new(2021, 6, 1).unwrap();
        let june2 = CivilDay::new(2021, 6, 2).unwrap();
        let june3 = CivilDay::new(2021, 6, 3).unwrap();
        let object = |sha, days| ObjectManifest {
            url: "https://example.invalid/month.zip".to_owned(),
            sha256: [sha; 32],
            size_bytes: 123,
            schema_version: 1,
            path: "month.zip".to_owned(),
            dataset: "um-kline".to_owned(),
            symbol: "BTCUSDT".to_owned(),
            row_count: 3,
            fetched_at: at(1),
            materialized_days: Some(days),
        };
        let mut log = ManifestLog::open(dir.path()).unwrap();
        log.record_stored(object(1, BTreeSet::from([june1])), None)
            .unwrap();
        log.record_stored(object(1, BTreeSet::from([june3])), None)
            .unwrap();
        let mut reopened = ManifestLog::open(dir.path()).unwrap();
        assert_eq!(
            reopened.get("month.zip").unwrap().materialized_days,
            Some(BTreeSet::from([june1, june3]))
        );
        reopened
            .record_stored(object(1, BTreeSet::from([june1, june3])), None)
            .unwrap();
        let path = dir.path().join("manifest/objects.jsonl");
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);

        reopened
            .record_stored(object(2, BTreeSet::from([june2])), Some([1; 32]))
            .unwrap();
        let replaced = ManifestLog::open(dir.path()).unwrap();
        assert_eq!(
            replaced.get("month.zip").unwrap().materialized_days,
            Some(BTreeSet::from([june2]))
        );
    }

    #[test]
    fn same_payload_keeps_the_original_received_time() {
        let mut slot = Slot::Absent;
        assert_eq!(merge_fact(&mut slot, "bar", at(1)), WriteEffect::Inserted);
        assert_eq!(merge_fact(&mut slot, "bar", at(2)), WriteEffect::Unchanged);
        match slot {
            Slot::Present(stored) => {
                assert_eq!(stored.value, "bar");
                assert_eq!(stored.received, at(1));
            }
            other => panic!("expected present, got {other:?}"),
        }
    }

    #[test]
    fn a_different_payload_does_not_replace_the_stored_value() {
        let mut slot = Slot::Absent;
        merge_fact(&mut slot, 1, at(1));
        assert_eq!(merge_fact(&mut slot, 2, at(2)), WriteEffect::Conflict);
        match &slot {
            Slot::Conflict { stored, rejected } => {
                assert_eq!(stored.value, 1);
                assert_eq!(rejected.value, 2);
            }
            other => panic!("expected conflict, got {other:?}"),
        }
        assert_eq!(merge_fact(&mut slot, 1, at(3)), WriteEffect::Unchanged);
        assert_eq!(slot.stored().unwrap().value, 1);
    }

    proptest! {
        #[test]
        fn the_first_payload_survives_later_writes(writes in prop::collection::vec(0u8..6, 1..16)) {
            let mut slot = Slot::<u8>::Absent;
            let first = writes[0];
            for (index, value) in writes.into_iter().enumerate() {
                merge_fact(&mut slot, value, at(index as i64));
            }
            let stored = slot.stored().expect("a write happened");
            prop_assert_eq!(stored.value, first);
        }
    }
}
