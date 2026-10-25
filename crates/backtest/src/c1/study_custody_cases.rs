use super::*;
use c1_report_custody::FinalizationStatus;
use cucumber::{then, when, World as _};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    Healthy,
    Open,
    Write,
    FileSync,
    DirectorySync,
}

impl Fault {
    fn parse(value: &str) -> Self {
        match value {
            "healthy" => Self::Healthy,
            "open" => Self::Open,
            "write" => Self::Write,
            "file_sync" => Self::FileSync,
            "directory_sync" => Self::DirectorySync,
            _ => panic!("unknown synthetic report fault"),
        }
    }
}

#[derive(Debug)]
struct Observation {
    _directory: tempfile::TempDir,
    retained_ledger: PathBuf,
    ledger_bytes: Vec<u8>,
    expected: Value,
    error: C1BatchError,
    fault: Fault,
}

struct RealFileFault {
    file: File,
    remaining: Option<usize>,
    fail_sync: bool,
}

impl Write for RealFileFault {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let count = self
            .remaining
            .map_or(bytes.len(), |left| left.min(bytes.len()));
        if count == 0 {
            return Err(std::io::Error::other(
                "synthetic partial artifact write failure",
            ));
        }
        let count = self.file.write(&bytes[..count])?;
        if let Some(left) = &mut self.remaining {
            *left -= count;
        }
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl RealFileFault {
    fn sync_all(&mut self) -> std::io::Result<()> {
        if self.fail_sync {
            Err(std::io::Error::other("synthetic artifact sync failure"))
        } else {
            self.file.sync_all()
        }
    }
}

fn statistics_sentinel(metrics: &SeriesMetrics) -> SelectionStatistics {
    use super::super::metrics::{Gates, StatisticGate, Verdict};
    // Typed custody data only; this is never offered as a full-study statistic.
    SelectionStatistics {
        headline: metrics.clone(),
        higher_fee: metrics.clone(),
        candidate_config_count: 2,
        declared_charged_role_cap: 5,
        implemented_history_floor: 202,
        exact_global_n: None,
        history_complete: false,
        is_dates_us: Vec::new(),
        is_source_folds: Vec::new(),
        is_return_columns: vec![Vec::new(), Vec::new()],
        cscv_gap_rows: 16,
        cscv_splits: 0,
        cross_config_variance_ticks: 25_000_000,
        dsr_probability_ticks: None,
        pbo_probability_ticks: None,
        defined_full_is_sharpe_columns: 0,
        active_test_folds: 0,
        gates: Gates {
            headline_positive_return: StatisticGate::Undefined,
            headline_positive_sharpe: StatisticGate::Undefined,
            higher_fee_positive_sharpe: StatisticGate::Undefined,
            dsr_at_least_95: StatisticGate::Undefined,
            pbo_at_most_20: StatisticGate::Undefined,
            coverage_50_pairs_10_folds: false,
        },
        verdict: Verdict::InconclusiveSparseNoPromotion,
    }
}

fn uncertain_batch() -> (tempfile::TempDir, BatchEvidence, PathBuf, Vec<u8>) {
    let (directory, admitted, request, first) = super::tests::synthetic_report_batch();
    let log = TrialLog::new(&admitted.ledger);
    let mut next = first.identity.clone();
    next.run_id.push_str("-terminal-uncertain");
    log.begin(StartRecord {
        run_id: &next.run_id,
        trial_id: &next.trial_id,
        config_id: &next.config_id,
        pinned_root: &next.pinned_root,
        prereg_sha256: &next.prereg_sha256,
        attempt: next.attempt,
        family: "C1",
        ts: first.range.start.micros(),
        fold: next.fold,
        segment: Some(next.unit_kind),
        role: Some(&next.role),
    })
    .unwrap();
    let source = C1SourceView::synthetic(Vec::new(), Vec::new(), Vec::new()).unwrap();
    let execution = execute_unit(
        &source,
        Scenario::Headline,
        vec![PolicyWindow {
            start: first.range.start.micros(),
            end: first.range.end.micros(),
            hold: C1MaxHold::Hours168,
            enabled: false,
        }],
        first.range,
    );
    let bytes = std::fs::read(&admitted.ledger).unwrap();
    let retained = directory.path().join("retained-original-ledger.jsonl");
    std::fs::rename(&admitted.ledger, &retained).unwrap();
    std::fs::create_dir(&admitted.ledger).unwrap();
    let second = persist_execution(&log, next, first.range, execution);
    let original = second.complete().unwrap_err().to_string();
    assert!(matches!(
        second.persistence,
        Persistence::TerminalUncertain(_)
    ));
    assert!(matches!(second.outcome, Outcome::Complete { .. }));
    let statistics = statistics_sentinel(first.complete().unwrap().1);
    let mut batch = BatchEvidence::new(admitted, request);
    batch.units = vec![first, second];
    batch.selected = vec![Some(C1MaxHold::Hours168), None];
    batch.original_failure = Some(original);
    batch.statistics = Some(statistics);
    (directory, batch, retained, bytes)
}

#[test]
fn owned_report_custody_retains_a_real_metrics_refusal_prefix() {
    let (_directory, admitted, request, first) = super::tests::synthetic_report_batch();
    let source = C1SourceView::synthetic(Vec::new(), Vec::new(), Vec::new()).unwrap();
    let mut range = first.range;
    range.end = UtcMicros::from_micros(range.end.micros() + 86_400_000_000_i64);
    let outcome = execute_unit(
        &source,
        Scenario::Headline,
        vec![PolicyWindow {
            start: first.range.start.micros(),
            end: first.range.end.micros(),
            hold: C1MaxHold::Hours168,
            enabled: false,
        }],
        range,
    );
    let Outcome::Technical {
        prefix: Some(prefix),
        ..
    } = &outcome
    else {
        panic!("the real metrics boundary must refuse an incomplete calendar");
    };
    assert_eq!(prefix.daily.len(), 1);
    let mut id = first.identity.clone();
    id.run_id.push_str("-calendar-refusal");
    let log = TrialLog::new(&admitted.ledger);
    log.begin(StartRecord {
        run_id: &id.run_id,
        trial_id: &id.trial_id,
        config_id: &id.config_id,
        pinned_root: &id.pinned_root,
        prereg_sha256: &id.prereg_sha256,
        attempt: id.attempt,
        family: "C1",
        ts: range.start.micros(),
        fold: id.fold,
        segment: Some(id.unit_kind),
        role: Some(&id.role),
    })
    .unwrap();
    let technical = persist_execution(&log, id, range, outcome);
    assert!(matches!(technical.persistence, Persistence::Aborted));
    let original = technical.complete().unwrap_err().to_string();
    let mut batch = BatchEvidence::new(admitted, request);
    batch.units = vec![first, technical];
    batch.original_failure = Some(original);
    let expected = batch.diagnostic();
    std::fs::create_dir(batch.admitted.out.join("C1-diagnostics.json")).unwrap();
    let error = finalize_batch(batch, write_outputs).unwrap_err();
    let BatchFailure::Finalized(owner) = &error.failure else {
        panic!("missing real batch owner");
    };
    assert_eq!(owner.payload().diagnostic(), expected);
    let Outcome::Technical {
        prefix: Some(prefix),
        ..
    } = &owner.payload().units[1].outcome
    else {
        panic!("the original technical prefix was lost");
    };
    assert_eq!(prefix.daily.len(), 1);
    assert!(matches!(
        owner.status(),
        FinalizationStatus::Completed(Err(_))
    ));
}

fn observe(fault: Fault) -> Observation {
    let (directory, batch, retained_ledger, ledger_bytes) = uncertain_batch();
    let expected = batch.diagnostic();
    if fault == Fault::Open {
        std::fs::create_dir(batch.admitted.out.join("C1-diagnostics.json")).unwrap();
    }
    let error = finalize_batch(batch, |batch| {
        write_outputs_with(
            batch,
            |path, bytes, ack| {
                let name = path.file_name().unwrap();
                let injected = if fault == Fault::Write && name == "table_C1.csv" {
                    Some((Some(17), false))
                } else if fault == Fault::FileSync && name == "fold_daily_returns.csv" {
                    Some((None, true))
                } else {
                    None
                };
                if let Some((remaining, fail_sync)) = injected {
                    let file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)
                        .map_err(|error| ReportFailure::io(ReportStage::Open, path, error, ack))?;
                    let mut file = RealFileFault {
                        file,
                        remaining,
                        fail_sync,
                    };
                    return write_artifact(path, bytes, ack, &mut file, RealFileFault::sync_all);
                }
                create_output(path, bytes, ack)
            },
            |path| {
                let file = File::open(path)?;
                if fault == Fault::DirectorySync {
                    Err(std::io::Error::other("synthetic directory sync failure"))
                } else {
                    file.sync_all()
                }
            },
        )
    })
    .unwrap_err();
    Observation {
        _directory: directory,
        retained_ledger,
        ledger_bytes,
        expected,
        error,
        fault,
    }
}

fn assert_retained(value: &Observation) {
    let BatchFailure::Finalized(owner) = &value.error.failure else {
        panic!("the owning error must retain the real finalized batch");
    };
    let batch = owner.payload();
    assert_eq!(batch.diagnostic(), value.expected);
    let (first, metrics) = batch.units[0].complete().unwrap();
    assert_eq!(first.daily.len(), 1);
    assert_eq!(metrics.final_equity_ticks, 100_000_000_000_000);
    let Outcome::Complete { run, metrics } = &batch.units[1].outcome else {
        panic!("ledger failure must retain the actual complete execution");
    };
    assert_eq!(run.daily.len(), 1);
    assert_eq!(metrics.final_equity_ticks, 100_000_000_000_000);
    assert!(matches!(
        batch.units[1].persistence,
        Persistence::TerminalUncertain(_)
    ));
    assert!(batch.units[1].complete().is_err());
    assert_eq!(
        std::fs::read(&value.retained_ledger).unwrap(),
        value.ledger_bytes
    );
    let FinalizationStatus::Completed(outcome) = owner.status() else {
        panic!("the actual writer result must drive the generated completion");
    };
    let expected_count = match value.fault {
        Fault::Healthy | Fault::DirectorySync => 6,
        Fault::Open => 0,
        Fault::Write => 1,
        Fault::FileSync => 2,
    };
    let ack = match outcome {
        Ok(ack) => {
            assert_eq!(value.fault, Fault::Healthy);
            assert!(ack.directory_synced);
            ack
        }
        Err(error) => {
            let expected_stage = match value.fault {
                Fault::Open => ReportStage::Open,
                Fault::Write => ReportStage::Write,
                Fault::FileSync => ReportStage::FileSync,
                Fault::DirectorySync => ReportStage::DirectorySync,
                Fault::Healthy => panic!("the healthy writer must complete"),
            };
            assert_eq!(error.stage, expected_stage);
            assert!(!error.acknowledged.directory_synced);
            assert!(matches!(error.cause, ReportCause::Io(_)));
            &error.acknowledged
        }
    };
    assert_eq!(ack.synced_artifacts.len(), expected_count);
    assert!(ack
        .synced_artifacts
        .iter()
        .all(|path| path.file_name().unwrap() != "trials.jsonl"));
    if value.fault == Fault::Write {
        assert_eq!(
            std::fs::read(batch.admitted.out.join("table_C1.csv")).unwrap(),
            b"study_id,run_id,c"
        );
    }
    if value.fault == Fault::FileSync {
        assert!(
            std::fs::read(batch.admitted.out.join("fold_daily_returns.csv"))
                .unwrap()
                .len()
                > 17
        );
    }
}

#[test]
fn owned_report_custody_retains_native_results_and_both_failure_channels() {
    for fault in [
        Fault::Open,
        Fault::Healthy,
        Fault::Write,
        Fault::FileSync,
        Fault::DirectorySync,
    ] {
        assert_retained(&observe(fault));
    }
}

struct HandoffSink {
    bytes: Vec<u8>,
    remaining: Option<usize>,
    fail_flush: bool,
}

impl Write for HandoffSink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let count = self
            .remaining
            .map_or(bytes.len(), |left| left.min(bytes.len()));
        if count == 0 {
            return Err(std::io::Error::other("synthetic handoff write failure"));
        }
        self.bytes.extend_from_slice(&bytes[..count]);
        if let Some(left) = &mut self.remaining {
            *left -= count;
        }
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if self.fail_flush {
            Err(std::io::Error::other("synthetic handoff flush failure"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn owned_report_custody_handoff_returns_the_same_owner_on_write_and_flush_failures() {
    for (remaining, fail_flush) in [(None, false), (Some(17), false), (None, true)] {
        let value = observe(Fault::Open);
        assert_retained(&value);
        let expected = value.error.evidence();
        let mut sink = HandoffSink {
            bytes: Vec::new(),
            remaining,
            fail_flush,
        };
        let retained = match value.error.handoff(&mut sink) {
            Ok(owner) => {
                assert!(remaining.is_none() && !fail_flush);
                assert_eq!(
                    serde_json::from_slice::<Value>(&sink.bytes).unwrap(),
                    expected
                );
                owner
            }
            Err((owner, error)) => {
                assert!(remaining.is_some() || fail_flush);
                assert_eq!(error.kind(), std::io::ErrorKind::Other);
                if remaining.is_some() {
                    assert_eq!(sink.bytes.len(), 17);
                }
                owner
            }
        };
        assert_eq!(retained.evidence(), expected);
        let BatchFailure::Finalized(owner) = retained.failure else {
            panic!("the CLI handoff must not erase the batch owner");
        };
        assert_eq!(owner.payload().units.len(), 2);
        assert_eq!(owner.payload().diagnostic(), value.expected);
    }
}

#[test]
fn owned_report_custody_handoff_preserves_non_utf8_native_paths_without_panicking() {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let bytes = b"/synthetic/\xff-input.json".to_vec();
    let request = C1BatchRequest {
        inputs: PathBuf::from(OsString::from_vec(bytes.clone())),
        data_root: PathBuf::from("/synthetic/snapshot"),
        out: PathBuf::from("/synthetic/out"),
        ledger: PathBuf::from("/synthetic/out/trials.jsonl"),
        attempt: 4,
    };
    let error = C1BatchError {
        failure: BatchFailure::Entry {
            request: Box::new(request),
            error: StudyFailure("synthetic native path admission refusal".to_owned()),
        },
    };
    let mut sink = Vec::new();
    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| error.handoff(&mut sink)));
    let retained = outcome
        .expect("an ordinary native path must not panic away its error owner")
        .unwrap();
    let BatchFailure::Entry { request, .. } = &retained.failure else {
        panic!("the original entry refusal must remain owned");
    };
    assert_eq!(request.inputs.as_os_str().as_bytes(), bytes);
    let rendered: Value = serde_json::from_slice(&sink).unwrap();
    assert_eq!(rendered["request"]["inputs"]["encoding"], "unix_bytes");
    assert_eq!(rendered["request"]["inputs"]["bytes"], json!(bytes));
    assert_eq!(
        rendered["entry_error"],
        "synthetic native path admission refusal"
    );
}

#[derive(Debug, Default, cucumber::World)]
struct CustodyWorld {
    observation: Option<Observation>,
    handoff_error: Option<std::io::ErrorKind>,
}

#[when(
    regex = r"^owning C1 finalization encounters a (healthy|open|write|file_sync|directory_sync) report sink$"
)]
fn finish_report(world: &mut CustodyWorld, fault: String) {
    world.observation = Some(observe(Fault::parse(&fault)));
}

#[then("all native unit evidence and its original ledger cause remain owned with truthful report acknowledgements")]
fn retained_report(world: &mut CustodyWorld) {
    assert_retained(world.observation.as_ref().unwrap());
}

#[when(regex = r"^the actual owned C1 failure handoff encounters a (write|flush) sink failure$")]
fn failed_handoff(world: &mut CustodyWorld, fault: String) {
    let mut value = observe(Fault::Open);
    let mut sink = HandoffSink {
        bytes: Vec::new(),
        remaining: (fault == "write").then_some(17),
        fail_flush: fault == "flush",
    };
    let (owner, error) = value.error.handoff(&mut sink).unwrap_err();
    value.error = owner;
    world.handoff_error = Some(error.kind());
    world.observation = Some(value);
}

#[then("the handoff is unacknowledged and both original and report errors still own every native result")]
fn unacknowledged_handoff(world: &mut CustodyWorld) {
    assert_eq!(world.handoff_error, Some(std::io::ErrorKind::Other));
    assert_retained(world.observation.as_ref().unwrap());
}

#[tokio::test]
async fn c1_report_custody_feature() {
    CustodyWorld::cucumber()
        .with_default_cli()
        .fail_on_skipped()
        .run_and_exit(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/features/c1_report_custody.feature"
        ))
        .await;
}
