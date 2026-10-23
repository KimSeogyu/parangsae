//! Downloaded-object identity and the idempotent write state machine.
//!
//! Generated per-key authority implements the checked FactWriteEnvelope model.
//! Native source equality determines payload identity; complete first rows and
//! latest rejected rows retain receipt/schema metadata. The partition fold
//! refuses every provisional insertion after any conflict. Archive manifests
//! below describe downloaded objects and are a separate resource.

use std::collections::BTreeMap;
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

pub(crate) use fact_authority::{BoundFact, FactOwner, RowError, WriteEffect};

#[derive(Debug)]
enum MergeDecision<T> {
    Accepted {
        rows: Vec<T>,
        inserted: u64,
        unchanged: u64,
    },
    Refused {
        original: Vec<T>,
        unchanged: u64,
        conflicts: Vec<(T, T)>,
    },
}

/// A consumed partition trial. Only `merge_rows` can create this decision.
#[derive(Debug)]
pub(crate) struct MergeOutcome<T> {
    decision: MergeDecision<T>,
}

impl<T> MergeOutcome<T> {
    /// Callbacks are storage effects after the complete per-key trial, never guard classifiers.
    pub(crate) fn commit(
        self,
        write: impl FnOnce(&[T]) -> Result<(), super::Error>,
        mut append_conflict: impl FnMut(&T, &T) -> Result<(), super::Error>,
    ) -> Result<crate::store::WriteCounts, super::Error> {
        match self.decision {
            MergeDecision::Accepted {
                rows,
                inserted,
                unchanged,
            } => {
                if inserted > 0 {
                    write(&rows)?;
                }
                Ok(crate::store::WriteCounts {
                    inserted,
                    unchanged,
                    conflicts: 0,
                })
            }
            MergeDecision::Refused {
                original,
                unchanged,
                conflicts,
            } => {
                drop(original);
                for (stored, rejected) in &conflicts {
                    append_conflict(stored, rejected)?;
                }
                Ok(crate::store::WriteCounts {
                    inserted: 0,
                    unchanged,
                    conflicts: conflicts.len() as u64,
                })
            }
        }
    }

    #[cfg(test)]
    fn rows(&self) -> &[T] {
        match &self.decision {
            MergeDecision::Accepted { rows, .. } => rows,
            MergeDecision::Refused { original, .. } => original,
        }
    }

    #[cfg(test)]
    fn conflicts(&self) -> &[(T, T)] {
        match &self.decision {
            MergeDecision::Accepted { .. } => &[],
            MergeDecision::Refused { conflicts, .. } => conflicts,
        }
    }
}

/// Replay complete persisted rows, then fold generated per-key decisions.
/// Persisted keys are expected to be unique. Duplicate baseline keys retain the
/// previous loader's last-wins behavior; a refused trial returns the original Vec.
pub(crate) fn merge_rows<T, K>(
    existing: Vec<T>,
    incoming: Vec<T>,
    key_of: impl Fn(&T) -> K,
) -> Result<MergeOutcome<T>, RowError>
where
    T: BoundFact,
    K: Ord,
{
    let original = existing.clone();
    let mut trial: BTreeMap<K, FactOwner<T>> = BTreeMap::new();
    for row in existing {
        let key = key_of(&row);
        let mut owner = FactOwner::new();
        if owner.write(row)? != WriteEffect::Insert {
            return Err(RowError::BrokenBinding);
        }
        trial.insert(key, owner);
    }
    let mut inserted = 0u64;
    let mut unchanged = 0u64;
    let mut conflicts = Vec::new();
    for row in incoming {
        let key = key_of(&row);
        let owner = trial.entry(key).or_default();
        match owner.write(row)? {
            WriteEffect::Insert => inserted += 1,
            WriteEffect::Same => unchanged += 1,
            WriteEffect::Clash => {
                let stored = owner.stored().ok_or(RowError::BrokenBinding)?;
                let rejected = owner.rejected().ok_or(RowError::BrokenBinding)?;
                conflicts.push((stored.clone(), rejected.clone()));
            }
        }
    }
    if !conflicts.is_empty() {
        return Ok(MergeOutcome {
            decision: MergeDecision::Refused {
                original,
                unchanged,
                conflicts,
            },
        });
    }
    let rows = trial
        .into_values()
        .map(|owner| owner.stored().cloned().ok_or(RowError::BrokenBinding))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(MergeOutcome {
        decision: MergeDecision::Accepted {
            rows,
            inserted,
            unchanged,
        },
    })
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
    use std::cell::{Cell, RefCell};

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
        let mut owner = FactOwner::<FundingFact>::new();
        assert_eq!(owner.write(funding(0, 1, 1)).unwrap(), WriteEffect::Insert);
        assert_eq!(owner.write(funding(0, 1, 2)).unwrap(), WriteEffect::Same);
        let stored = owner.stored().unwrap();
        assert_eq!(stored, &funding(0, 1, 1));
        assert_eq!(stored.received_ts, at(1));
        assert!(owner.rejected().is_none());
    }

    #[test]
    fn equal_and_conflicting_inputs_keep_the_first_row_and_latest_rejected_metadata() {
        let mut owner = FactOwner::<FundingFact>::new();
        assert_eq!(owner.write(funding(0, 1, 1)).unwrap(), WriteEffect::Insert);
        assert_eq!(owner.write(funding(0, 2, 2)).unwrap(), WriteEffect::Clash);
        assert_eq!(owner.write(funding(0, 2, 3)).unwrap(), WriteEffect::Clash);
        let before_same = owner.read();
        assert_eq!(owner.write(funding(0, 1, 4)).unwrap(), WriteEffect::Same);
        assert_eq!(owner.stored().unwrap().rate, Ratio::from_ticks(1));
        assert_eq!(owner.stored().unwrap().received_ts, at(1));
        assert_eq!(owner.rejected().unwrap().rate, Ratio::from_ticks(2));
        assert_eq!(owner.rejected().unwrap().received_ts, at(3));
        assert_eq!(owner.read().values, before_same.values);
        assert_eq!(owner.read().revision, "4");
    }

    #[test]
    fn any_conflict_refuses_new_keys_and_keeps_each_rejected_pair_and_equal_count() {
        let original = funding(0, 1, 1);
        let outcome = merge_rows(
            vec![original.clone()],
            vec![
                funding(1, 5, 5),
                funding(0, 1, 2),
                funding(0, 2, 3),
                funding(0, 3, 4),
                funding(0, 1, 6),
            ],
            |row| row.funding_time.micros(),
        )
        .unwrap();
        assert_eq!(outcome.rows(), &[original]);
        assert_eq!(outcome.rows()[0].received_ts, at(1));
        assert_eq!(outcome.conflicts().len(), 2);
        assert_eq!(outcome.conflicts()[0].1.received_ts, at(3));
        assert_eq!(outcome.conflicts()[1].1.received_ts, at(4));
        let writes = Cell::new(0);
        let logged = RefCell::new(Vec::new());
        let counts = outcome
            .commit(
                |_| {
                    writes.set(writes.get() + 1);
                    Ok(())
                },
                |stored, rejected| {
                    logged
                        .borrow_mut()
                        .push((stored.received_ts, rejected.received_ts));
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(writes.get(), 0);
        assert_eq!(*logged.borrow(), vec![(at(1), at(3)), (at(1), at(4))]);
        assert_eq!(
            counts,
            crate::store::WriteCounts {
                inserted: 0,
                unchanged: 2,
                conflicts: 2
            }
        );
    }

    #[test]
    fn conflict_against_a_new_key_refuses_the_whole_initially_empty_partition() {
        let outcome = merge_rows(
            Vec::new(),
            vec![funding(0, 1, 1), funding(1, 5, 2), funding(0, 2, 3)],
            |row| row.funding_time.micros(),
        )
        .unwrap();
        assert!(outcome.rows().is_empty());
        assert_eq!(outcome.conflicts()[0].0.received_ts, at(1));
        assert_eq!(outcome.conflicts()[0].1.received_ts, at(3));
        let counts = outcome
            .commit(
                |_| panic!("a refused partition cannot invoke the writer"),
                |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(counts.inserted, 0);
        assert_eq!(counts.conflicts, 1);
    }

    #[test]
    fn an_equal_only_trial_never_invokes_the_partition_writer() {
        let outcome = merge_rows(vec![funding(0, 1, 1)], vec![funding(0, 1, 2)], |row| {
            row.funding_time.micros()
        })
        .unwrap();
        assert_eq!(outcome.rows()[0].received_ts, at(1));
        let counts = outcome
            .commit(
                |_| panic!("equal-only cannot invoke the writer"),
                |_, _| panic!("equal-only has no conflicts"),
            )
            .unwrap();
        assert_eq!(
            counts,
            crate::store::WriteCounts {
                inserted: 0,
                unchanged: 1,
                conflicts: 0
            }
        );
    }

    #[test]
    fn conflict_log_failure_remains_an_error_without_partition_writes() {
        let outcome = merge_rows(vec![funding(0, 1, 1)], vec![funding(0, 2, 2)], |row| {
            row.funding_time.micros()
        })
        .unwrap();
        let failure = outcome.commit(
            |_| panic!("refusal cannot write"),
            |_, _| Err(crate::Error::check("test_log", "unwritable conflict log")),
        );
        assert!(failure.is_err());
    }

    #[test]
    fn persisted_duplicate_keys_preserve_the_existing_last_wins_loader_behavior() {
        let outcome = merge_rows(
            vec![funding(0, 1, 1), funding(0, 2, 2)],
            Vec::new(),
            |row| row.funding_time.micros(),
        )
        .unwrap();
        assert_eq!(outcome.rows().len(), 1);
        assert_eq!(outcome.rows()[0].rate, Ratio::from_ticks(2));
        assert_eq!(outcome.rows()[0].received_ts, at(2));
    }

    proptest! {
        #[test]
        fn the_first_complete_fact_survives_later_writes(writes in prop::collection::vec(0i64..6, 1..16)) {
            let mut owner = FactOwner::<FundingFact>::new();
            let first = writes[0];
            for (index, value) in writes.into_iter().enumerate() {
                owner.write(funding(0, value, index as i64)).unwrap();
            }
            let stored = owner.stored().expect("a write happened");
            prop_assert_eq!(stored.rate, Ratio::from_ticks(first));
            prop_assert_eq!(stored.received_ts, at(0));
        }
    }
}
