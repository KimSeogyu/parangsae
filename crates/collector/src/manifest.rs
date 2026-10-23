//! Downloaded-object identity and the idempotent write state machine.
//!
//! Generated closed partition data folds combine the checked FactWriteEnvelope
//! row roles with PartitionMergeControl and literal key/group metadata. Reports
//! are unbound data: physical storage and conflict logging remain ordinary
//! application code. Archive manifests below describe a separate resource.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use parangsae_core::{ReceivedTs, UtcMicros};
use serde::{Deserialize, Serialize};

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
}

#[path = "../../../spec/sgd/generated/fact-write/rust/adapter.rs"]
mod fact_authority;

pub(crate) use fact_authority::{fold_partition, PartitionFact, UnboundPartitionReport};

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
        object: ObjectManifest,
        previous: Option<[u8; 32]>,
    ) -> Result<(), super::Error> {
        if let Some(current) = self.get(&object.path) {
            if current.sha256 == object.sha256
                && current.schema_version == object.schema_version
                && current.row_count == object.row_count
                && current.dataset == object.dataset
            {
                return Ok(());
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
    use crate::vision::FundingFact;
    use parangsae_core::{AvailableTs, EventTs, Ratio, Symbol};
    use proptest::prelude::*;

    fn at(micros: i64) -> ReceivedTs {
        ReceivedTs::from_micros(UtcMicros::from_micros(micros))
    }

    fn funding(key: i64, rate: i64, received: i64) -> FundingFact {
        let time = UtcMicros::from_micros(key);
        FundingFact {
            symbol: Symbol::new("BTCUSDT").unwrap(),
            raw_calc_time: time,
            funding_time: time,
            interval_hours: 8,
            rate: Ratio::from_ticks(rate),
            event_ts: EventTs::from_micros(time),
            available_ts: AvailableTs::from_micros(time),
            received_ts: at(received),
            schema_version: crate::vision::SCHEMA_FUNDING,
        }
    }

    #[test]
    fn same_payload_keeps_the_complete_original_received_time() {
        let report = fold_partition(Vec::new(), vec![funding(0, 1, 1), funding(0, 1, 2)])
            .unwrap()
            .into_report()
            .unwrap();
        match report {
            UnboundPartitionReport::Write { rows, counts, .. } => {
                assert_eq!(rows, vec![funding(0, 1, 1)]);
                assert_eq!(rows[0].received_ts, at(1));
                assert_eq!(counts.inserted(), 1);
                assert_eq!(counts.unchanged(), 1);
            }
            _ => panic!("one insert and one Same must close as Write"),
        }
    }

    #[test]
    fn equal_and_conflicting_inputs_keep_the_complete_first_row_and_every_rejected_metadata() {
        let original = funding(0, 1, 1);
        let report = fold_partition(
            vec![original.clone()],
            vec![funding(0, 2, 2), funding(0, 2, 3), funding(0, 1, 4)],
        )
        .unwrap()
        .into_report()
        .unwrap();
        match report {
            UnboundPartitionReport::Refused {
                original: rows,
                conflicts,
                counts,
                ..
            } => {
                assert_eq!(rows, vec![original]);
                assert_eq!(rows[0].received_ts, at(1));
                assert_eq!(counts.inserted(), 0);
                assert_eq!(counts.unchanged(), 1);
                assert_eq!(conflicts.len(), 2);
                assert!(conflicts
                    .iter()
                    .all(|pair| pair.stored().received_ts == at(1)));
                assert_eq!(conflicts[0].rejected().received_ts, at(2));
                assert_eq!(conflicts[1].rejected().received_ts, at(3));
            }
            _ => panic!("a Clash must refuse the complete trial"),
        }
    }

    #[test]
    fn any_conflict_refuses_new_keys_and_keeps_each_rejected_pair_and_equal_count() {
        let original = funding(0, 1, 1);
        let closed = fold_partition(
            vec![original.clone()],
            vec![
                funding(1, 5, 5),
                funding(0, 1, 2),
                funding(0, 2, 3),
                funding(0, 3, 4),
                funding(0, 1, 6),
            ],
        )
        .unwrap();
        assert_eq!(closed.counts().inserted(), 0);
        assert_eq!(closed.counts().unchanged(), 2);
        match closed.into_report().unwrap() {
            UnboundPartitionReport::Refused {
                original: rows,
                conflicts,
                counts,
                ..
            } => {
                assert_eq!(rows, vec![original]);
                assert_eq!(rows[0].received_ts, at(1));
                assert_eq!(conflicts.len(), 2);
                assert_eq!(conflicts[0].rejected().received_ts, at(3));
                assert_eq!(conflicts[1].rejected().received_ts, at(4));
                assert_eq!(counts.inserted(), 0);
                assert_eq!(counts.unchanged(), 2);
            }
            _ => panic!("provisional insertions cannot escape a refused trial"),
        }
    }

    #[test]
    fn conflict_against_a_new_key_refuses_the_whole_initially_empty_partition() {
        let report = fold_partition(
            Vec::new(),
            vec![funding(0, 1, 1), funding(1, 5, 2), funding(0, 2, 3)],
        )
        .unwrap()
        .into_report()
        .unwrap();
        match report {
            UnboundPartitionReport::Refused {
                original,
                conflicts,
                counts,
                ..
            } => {
                assert!(original.is_empty());
                assert_eq!(conflicts.len(), 1);
                assert_eq!(conflicts[0].stored().received_ts, at(1));
                assert_eq!(conflicts[0].rejected().received_ts, at(3));
                assert_eq!(counts.inserted(), 0);
            }
            _ => panic!("a new conflicting key must refuse the initial trial"),
        }
    }

    #[test]
    fn an_equal_only_trial_reports_noop_with_the_original_rows() {
        let report = fold_partition(vec![funding(0, 1, 1)], vec![funding(0, 1, 2)])
            .unwrap()
            .into_report()
            .unwrap();
        match report {
            UnboundPartitionReport::Noop { rows, counts, .. } => {
                assert_eq!(rows[0].received_ts, at(1));
                assert_eq!(counts.inserted(), 0);
                assert_eq!(counts.unchanged(), 1);
            }
            _ => panic!("equal-only must close as Noop"),
        }
    }

    #[test]
    fn noop_keeps_duplicate_baselines_but_write_materializes_the_last_complete_row() {
        let baseline = vec![funding(0, 1, 1), funding(0, 2, 2)];
        match fold_partition(baseline.clone(), Vec::new())
            .unwrap()
            .into_report()
            .unwrap()
        {
            UnboundPartitionReport::Noop { rows, .. } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].received_ts, at(1));
                assert_eq!(rows[1].received_ts, at(2));
            }
            _ => panic!("an empty incoming batch is Noop"),
        }
        match fold_partition(baseline, vec![funding(1, 3, 3)])
            .unwrap()
            .into_report()
            .unwrap()
        {
            UnboundPartitionReport::Write { rows, counts, .. } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].rate, Ratio::from_ticks(2));
                assert_eq!(rows[0].received_ts, at(2));
                assert_eq!(rows[1].funding_time.micros(), 1);
                assert_eq!(counts.inserted(), 1);
            }
            _ => panic!("an inserted key must materialize the last baseline row"),
        }
    }

    proptest! {
        #[test]
        fn the_first_complete_fact_survives_later_writes(writes in prop::collection::vec(0i64..6, 1..16)) {
            let first = writes[0];
            let equal = writes.iter().skip(1).filter(|value| **value == first).count();
            let different = writes.iter().skip(1).filter(|value| **value != first).count();
            let incoming = writes.into_iter().enumerate()
                .map(|(index, value)| funding(0, value, i64::try_from(index).unwrap())).collect();
            let report = fold_partition(Vec::new(), incoming).unwrap().into_report().unwrap();
            prop_assert_eq!(report.counts().unchanged(), u64::try_from(equal).unwrap());
            match report {
                UnboundPartitionReport::Write { rows, counts, .. } => {
                    prop_assert_eq!(different, 0);
                    prop_assert_eq!(counts.inserted(), 1);
                    prop_assert_eq!(rows.len(), 1);
                    prop_assert_eq!(rows[0].rate, Ratio::from_ticks(first));
                    prop_assert_eq!(rows[0].received_ts, at(0));
                }
                UnboundPartitionReport::Refused { original, conflicts, counts, .. } => {
                    prop_assert!(original.is_empty());
                    prop_assert_eq!(counts.inserted(), 0);
                    prop_assert_eq!(conflicts.len(), different);
                    for conflict in conflicts {
                        prop_assert_eq!(conflict.stored().rate, Ratio::from_ticks(first));
                        prop_assert_eq!(conflict.stored().received_ts, at(0));
                        prop_assert_ne!(conflict.rejected().rate, Ratio::from_ticks(first));
                    }
                }
                UnboundPartitionReport::Noop { .. } => prop_assert!(false, "nonempty initial batch cannot be Noop"),
            }
        }
    }
}
