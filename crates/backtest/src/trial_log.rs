//! Append-only `trials.jsonl`. The log is single-writer only: there is no
//! lock, and concurrent writers are not supported.
//!
//! `RunStarted` is fsynced before the caller computes. It carries
//! `prereg_sha256`, the sha256 of the prereg file hashed at run start.
//! The next line for that `run_id` is `RunFinished` or `RunAborted`. A second
//! start with the same `run_id` is refused. Lines are not rewritten or deleted.
//!
//! A hard crash that leaves `RunStarted` with no terminal line counts as aborted.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::canon::{json_string, sha256_hex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrialError {
    Io(String),
    DuplicateRunId(String),
    Corrupt { line: usize },
    NotStarted(String),
    AlreadyTerminal(String),
}

impl std::fmt::Display for TrialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "trial log append failed: {msg}"),
            Self::DuplicateRunId(id) => write!(f, "duplicate run_id {id}"),
            Self::Corrupt { line } => write!(f, "trial log line {line} is not a record"),
            Self::NotStarted(id) => write!(f, "run_id {id} has no RunStarted"),
            Self::AlreadyTerminal(id) => write!(f, "run_id {id} already finished or aborted"),
        }
    }
}

impl std::error::Error for TrialError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Line {
    #[serde(rename = "RunStarted")]
    Started {
        run_id: String,
        trial_id: String,
        config_id: String,
        pinned_root: String,
        /// Sha256 of the prereg file. Empty on rows written before the field existed.
        #[serde(default)]
        prereg_sha256: String,
        attempt: u64,
        family: String,
        ts: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fold: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        segment: Option<String>,
    },
    #[serde(rename = "RunFinished")]
    Finished {
        run_id: String,
        metrics_digest: String,
    },
    #[serde(rename = "RunAborted")]
    Aborted { run_id: String, reason: String },
}

pub(crate) struct StartRecord<'a> {
    pub run_id: &'a str,
    pub trial_id: &'a str,
    pub config_id: &'a str,
    pub pinned_root: &'a str,
    pub prereg_sha256: &'a str,
    pub attempt: u64,
    pub family: &'a str,
    pub ts: i64,
    pub fold: Option<u32>,
    pub segment: Option<&'a str>,
}

/// `sha256` of the JCS object `{attempt, config_id, pinned_root}`.
pub fn trial_preimage(config_id: &str, pinned_root: &str, attempt: u64) -> String {
    format!(
        "{{\"attempt\":{attempt},\"config_id\":{},\"pinned_root\":{}}}",
        json_string(config_id),
        json_string(pinned_root)
    )
}

pub fn trial_id(config_id: &str, pinned_root: &str, attempt: u64) -> String {
    sha256_hex(trial_preimage(config_id, pinned_root, attempt).as_bytes())
}

/// N is the number of distinct `trial_id` values on `RunStarted` lines, plus
/// one for each legacy `exploratory` row that has no `trial_id`.
///
/// A second fold of the same config is the same trial. `RunFinished` and
/// `RunAborted` have no `trial_id` and do not count. A `RunStarted` with no
/// terminal line still counts: aborted and invalid runs count. Two legacy
/// rows with the same body still count as two trials. The 13 legacy rows in
/// `research/trials.jsonl` are this shape; this function does not rewrite them.
pub fn distinct_trial_count(path: &Path) -> Result<u64, TrialError> {
    let text = read_text(path)?;
    let mut ids = BTreeSet::new();
    let mut legacy = 0u64;
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let line_no = index + 1;
        let value: serde_json::Value =
            serde_json::from_str(line).map_err(|_| TrialError::Corrupt { line: line_no })?;
        match value.get("kind").and_then(|kind| kind.as_str()) {
            Some("exploratory") => {
                if trial_id_present(&value) {
                    return Err(TrialError::Corrupt { line: line_no });
                }
                legacy = legacy
                    .checked_add(1)
                    .ok_or_else(|| TrialError::Io("trial count".to_owned()))?;
            }
            Some("RunStarted" | "RunFinished" | "RunAborted") => {
                let parsed: Line = serde_json::from_value(value)
                    .map_err(|_| TrialError::Corrupt { line: line_no })?;
                if let Line::Started { trial_id, .. } = parsed {
                    ids.insert(trial_id);
                }
            }
            _ => return Err(TrialError::Corrupt { line: line_no }),
        }
    }
    let distinct =
        u64::try_from(ids.len()).map_err(|_| TrialError::Io("trial count".to_owned()))?;
    distinct
        .checked_add(legacy)
        .ok_or_else(|| TrialError::Io("trial count".to_owned()))
}

fn trial_id_present(value: &serde_json::Value) -> bool {
    match value.get("trial_id") {
        None | Some(serde_json::Value::Null) => false,
        Some(_) => true,
    }
}

/// Append `RunFinished`. A second terminal line for `run_id` is refused.
pub fn finish_trial(path: &Path, run_id: &str, metrics_digest: &str) -> Result<(), TrialError> {
    TrialLog::new(path).finish(run_id, metrics_digest)
}

pub(crate) struct TrialLog {
    path: PathBuf,
}

impl TrialLog {
    pub(crate) fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    pub(crate) fn begin(&self, start: StartRecord<'_>) -> Result<(), TrialError> {
        let lines = read_lines(&self.path)?;
        let (started, _) = presence(&lines, start.run_id);
        if started {
            return Err(TrialError::DuplicateRunId(start.run_id.to_owned()));
        }
        append_value(
            &self.path,
            &Line::Started {
                run_id: start.run_id.to_owned(),
                trial_id: start.trial_id.to_owned(),
                config_id: start.config_id.to_owned(),
                pinned_root: start.pinned_root.to_owned(),
                prereg_sha256: start.prereg_sha256.to_owned(),
                attempt: start.attempt,
                family: start.family.to_owned(),
                ts: start.ts,
                fold: start.fold,
                segment: start.segment.map(str::to_owned),
            },
        )
    }

    pub(crate) fn finish(&self, run_id: &str, metrics_digest: &str) -> Result<(), TrialError> {
        self.terminal(Line::Finished {
            run_id: run_id.to_owned(),
            metrics_digest: metrics_digest.to_owned(),
        })
    }

    pub(crate) fn abort(&self, run_id: &str, reason: &str) -> Result<(), TrialError> {
        self.terminal(Line::Aborted {
            run_id: run_id.to_owned(),
            reason: reason.to_owned(),
        })
    }

    fn terminal(&self, line: Line) -> Result<(), TrialError> {
        let run_id = match &line {
            Line::Finished { run_id, .. } | Line::Aborted { run_id, .. } => run_id.clone(),
            Line::Started { .. } => return Err(TrialError::Io("terminal record".to_owned())),
        };
        let lines = read_lines(&self.path)?;
        let (started, terminal) = presence(&lines, &run_id);
        if !started {
            return Err(TrialError::NotStarted(run_id));
        }
        if terminal {
            return Err(TrialError::AlreadyTerminal(run_id));
        }
        append_value(&self.path, &line)
    }
}

fn presence(lines: &[Line], run_id: &str) -> (bool, bool) {
    let mut started = false;
    let mut terminal = false;
    for line in lines {
        match line {
            Line::Started { run_id: id, .. } if id == run_id => started = true,
            Line::Finished { run_id: id, .. } | Line::Aborted { run_id: id, .. }
                if id == run_id =>
            {
                terminal = true;
            }
            Line::Started { .. } | Line::Finished { .. } | Line::Aborted { .. } => {}
        }
    }
    (started, terminal)
}

fn read_text(path: &Path) -> Result<String, TrialError> {
    if !path.exists() {
        return Ok(String::new());
    }
    let text = fs::read_to_string(path).map_err(|err| TrialError::Io(err.to_string()))?;
    if !text.is_empty() && !text.ends_with('\n') {
        return Err(TrialError::Corrupt {
            line: text.lines().count(),
        });
    }
    Ok(text)
}

fn read_lines(path: &Path) -> Result<Vec<Line>, TrialError> {
    let text = read_text(path)?;
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let parsed =
            serde_json::from_str(line).map_err(|_| TrialError::Corrupt { line: index + 1 })?;
        out.push(parsed);
    }
    Ok(out)
}

pub(crate) fn append_fsync(path: &Path, line: &str) -> Result<(), TrialError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| TrialError::Io(err.to_string()))?;
    let mut bytes = line.as_bytes().to_vec();
    bytes.push(b'\n');
    file.write_all(&bytes)
        .map_err(|err| TrialError::Io(err.to_string()))?;
    file.flush()
        .map_err(|err| TrialError::Io(err.to_string()))?;
    file.sync_all()
        .map_err(|err| TrialError::Io(err.to_string()))?;
    sync_parent(path)?;
    Ok(())
}

fn append_value(path: &Path, value: &impl Serialize) -> Result<(), TrialError> {
    let line = serde_json::to_string(value).map_err(|err| TrialError::Io(err.to_string()))?;
    append_fsync(path, &line)
}

fn sync_parent(path: &Path) -> Result<(), TrialError> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let dir = File::open(parent).map_err(|err| TrialError::Io(err.to_string()))?;
    dir.sync_all()
        .map_err(|err| TrialError::Io(err.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preimage_is_sorted_jcs_and_escapes() {
        assert_eq!(
            trial_preimage("S01", "pin-v2", 1),
            "{\"attempt\":1,\"config_id\":\"S01\",\"pinned_root\":\"pin-v2\"}"
        );
        assert_eq!(
            trial_preimage("a\"b", "p", 0),
            "{\"attempt\":0,\"config_id\":\"a\\\"b\",\"pinned_root\":\"p\"}"
        );
        assert_eq!(
            trial_id(&"a".repeat(64), &"b".repeat(64), 0),
            "f1d676a241133f91c32cfbd9f578c43d475a103194a281c63cee13d3d254b1db"
        );
        assert_eq!(
            trial_id("S01", "pin-v2", 1),
            "348a2ff92849201353936a41b07acb0a187315168a133dc2d8ba4053522143ba"
        );
    }

    #[test]
    fn a_torn_line_refuses_the_next_append() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trials.jsonl");
        fs::write(&path, "{\"kind\":\"RunStarted\"}").unwrap();
        let err = TrialLog::new(&path)
            .begin(StartRecord {
                run_id: "r",
                trial_id: "t",
                config_id: "c",
                pinned_root: "p",
                prereg_sha256: "",
                attempt: 1,
                family: "S01",
                ts: 0,
                fold: None,
                segment: None,
            })
            .unwrap_err();
        assert_eq!(err, TrialError::Corrupt { line: 1 });
    }

    #[test]
    fn n_counts_distinct_trial_ids_including_an_aborted_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trials.jsonl");
        let log = TrialLog::new(&path);
        for (run_id, trial_id) in [("a", "same"), ("b", "same"), ("c", "other")] {
            log.begin(StartRecord {
                run_id,
                trial_id,
                config_id: "c",
                pinned_root: "p",
                prereg_sha256: "ab",
                attempt: 0,
                family: "S01",
                ts: 0,
                fold: Some(1),
                segment: Some("train"),
            })
            .unwrap();
        }
        log.finish("a", "digest").unwrap();
        log.abort("c", "stopped").unwrap();
        assert_eq!(distinct_trial_count(&path).unwrap(), 2);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("\"prereg_sha256\":\"ab\"").count(), 3);
    }

    #[test]
    fn attempt0_ledger_is_unchanged_and_n_is_distinct() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../research/ledger/wf-batch/attempt0-06ea184-trials.jsonl");
        assert_eq!(
            crate::canon::sha256_file(&path).unwrap(),
            "9e4e782b27695b02e88df91e7cdd001051a59d41d97ed798c4ddad6da7619116"
        );
        assert_eq!(distinct_trial_count(&path).unwrap(), 37);
    }

    #[test]
    fn legacy_exploratory_rows_without_a_trial_id_each_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trials.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"kind\":\"exploratory\",\"note\":\"realized-vol\",\"row\":2}\n",
                "{\"kind\":\"exploratory\",\"note\":\"realized-vol\",\"row\":2}\n",
                "{\"kind\":\"exploratory\",\"question\":\"session vol\"}\n",
                "{\"kind\":\"RunStarted\",\"run_id\":\"r\",\"trial_id\":\"t\",\"config_id\":\"c\",\"pinned_root\":\"p\",\"attempt\":0,\"family\":\"S01\",\"ts\":0}\n",
                "{\"kind\":\"RunStarted\",\"run_id\":\"r2\",\"trial_id\":\"t\",\"config_id\":\"c\",\"pinned_root\":\"p\",\"attempt\":0,\"family\":\"S01\",\"ts\":1}\n",
                "{\"kind\":\"RunFinished\",\"run_id\":\"r\",\"metrics_digest\":\"d\"}\n",
                "{\"kind\":\"RunAborted\",\"run_id\":\"r2\",\"reason\":\"stopped\"}\n",
            ),
        )
        .unwrap();
        assert_eq!(distinct_trial_count(&path).unwrap(), 4);
    }
}
