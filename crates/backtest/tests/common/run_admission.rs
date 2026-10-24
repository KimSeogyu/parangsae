//! Real-filesystem Cucumber contract for the native admission boundary.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cucumber::{given, then, when, World as _};
use serde_json::{json, Value};

use super::{claim_run_paths, run_wf_batch, WfBatchRequest};
use crate::{
    daily_return_tick, fold_schedule, run_walk_forward, selection_costs, FamilyGrid, GridPoint,
    ScheduleSpec, SweepPins, UtcDate, WalkContext, WindowAccounting, WindowResult,
};
use parangsae_sim::Cash;

#[derive(Debug, Default, cucumber::World)]
struct AdmissionWorld {
    dir: Option<tempfile::TempDir>,
    external_ledger: bool,
    pin_result: Option<Result<SweepPins, crate::BatchError>>,
    result: Option<Result<(), String>>,
    before: BTreeMap<PathBuf, Vec<u8>>,
}

impl AdmissionWorld {
    fn root(&mut self) -> &Path {
        self.dir
            .get_or_insert_with(|| tempfile::tempdir().unwrap())
            .path()
    }

    fn request(&mut self) -> WfBatchRequest {
        let external = self.external_ledger;
        let root = self.root();
        let out_dir = root.join("out");
        WfBatchRequest {
            inputs: root.join("inputs.json"),
            data_root: root.join("absent-data"),
            ledger: if external {
                root.join("external.jsonl")
            } else {
                out_dir.join("trials.jsonl")
            },
            out_dir,
            attempt: 0,
        }
    }
}

fn config() -> Value {
    json!({
        "config_id":"36dbcbd7a258b161d956b6871e323ec2c1d17878b8249fff7265f706fd45f4e3",
        "family":"H1","grid_index":0,
        "params":{"c":100000000,"deadband":50000000,"decision_hour_utc":0,"interval":240,"n":180},
        "start_us":1608249600000000_i64,"end_us":1759276800000000_i64,
        "initial_cash":100000000000000_i64
    })
}

#[given(
    regex = r"^a synthetic pin file with a (wrong|short|nonhex|null|matching|absent) expected root$"
)]
fn pin_file(world: &mut AdmissionWorld, kind: String) {
    let root = world.root();
    fs::write(root.join("fixture.txt"), b"alpha").unwrap();
    let input_path = root.join("inputs.json");
    let mut input = json!({
        "schema":"sweep-runner-inputs-v2","slippage_bps":2,"configs":[config()],
        "objects":[{"path":"fixture.txt","sha256":"8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8"}]
    });
    fs::write(&input_path, input.to_string()).unwrap();
    let matching = SweepPins::load(&input_path)
        .unwrap()
        .pinned_root()
        .to_owned();
    let expected = match kind.as_str() {
        "wrong" => Some(json!("0".repeat(64))),
        "short" => Some(json!("a".repeat(63))),
        "nonhex" => Some(json!("g".repeat(64))),
        "null" => Some(Value::Null),
        "matching" => Some(json!(matching)),
        "absent" => None,
        _ => unreachable!(),
    };
    if let Some(expected) = expected {
        input["pinned_root_expected"] = expected;
    }
    fs::write(input_path, input.to_string()).unwrap();
}

#[when("the native pin file is loaded")]
fn load_pins(world: &mut AdmissionWorld) {
    world.pin_result = Some(SweepPins::load(&world.request().inputs));
}

#[then("the expected pin root is rejected")]
fn pins_rejected(world: &mut AdmissionWorld) {
    let error = world.pin_result.as_ref().unwrap().as_ref().unwrap_err();
    assert!(
        error.to_string().contains("pinned_root_expected"),
        "{error}"
    );
}

#[then("the synthetic pin file is accepted")]
fn pins_accepted(world: &mut AdmissionWorld) {
    assert!(world.pin_result.as_ref().unwrap().is_ok());
}

#[given("an existing empty output directory")]
fn empty_output(world: &mut AdmissionWorld) {
    fs::create_dir(world.request().out_dir).unwrap();
}

#[given("an existing empty external ledger")]
fn empty_ledger(world: &mut AdmissionWorld) {
    world.external_ledger = true;
    fs::write(world.request().ledger, []).unwrap();
}

#[when("native run paths are claimed")]
#[when("native run paths are claimed again")]
fn claim(world: &mut AdmissionWorld) {
    world.result = Some(claim_run_paths(&world.request()));
}

#[then("the run path claim is rejected")]
fn claim_rejected(world: &mut AdmissionWorld) {
    assert!(
        world.result.as_ref().unwrap().is_err(),
        "claim was accepted"
    );
}

#[then("the existing output remains empty")]
#[then("the claimed output remains empty")]
fn output_empty(world: &mut AdmissionWorld) {
    assert_eq!(fs::read_dir(world.request().out_dir).unwrap().count(), 0);
}

#[then("the existing ledger remains empty")]
fn ledger_empty(world: &mut AdmissionWorld) {
    assert_eq!(fs::read(world.request().ledger).unwrap(), Vec::<u8>::new());
}

#[then("the ledger is absent")]
fn ledger_absent(world: &mut AdmissionWorld) {
    assert!(!world.request().ledger.exists());
}

#[then("output and ledger are absent")]
fn run_paths_absent(world: &mut AdmissionWorld) {
    let request = world.request();
    assert!(!request.out_dir.exists());
    assert!(!request.ledger.exists());
}

fn file_contents(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(file_contents(&path));
        } else {
            files.insert(path.clone(), fs::read(path).unwrap());
        }
    }
    files
}

#[given("a synthetic run has claimed fresh paths and written evidence")]
fn first_claim(world: &mut AdmissionWorld) {
    let request = world.request();
    claim_run_paths(&request).unwrap();
    fs::write(
        request.out_dir.join("run_log.txt"),
        b"keep failed-run evidence\n",
    )
    .unwrap();
    fs::write(request.ledger, b"keep ledger evidence\n").unwrap();
    world.before = file_contents(world.root());
}

#[then("the first run's files are unchanged")]
fn files_unchanged(world: &mut AdmissionWorld) {
    assert_eq!(
        world.before,
        file_contents(world.dir.as_ref().unwrap().path())
    );
}

#[then("an empty ledger exists under the output directory")]
fn fresh_ledger(world: &mut AdmissionWorld) {
    assert!(world.result.as_ref().unwrap().is_ok());
    let request = world.request();
    assert_eq!(request.ledger, request.out_dir.join("trials.jsonl"));
    assert_eq!(fs::read(request.ledger).unwrap(), Vec::<u8>::new());
}

#[when("a synthetic native walk-forward run uses the claimed ledger")]
fn synthetic_run(world: &mut AdmissionWorld) {
    let request = world.request();
    let pins = SweepPins::load(&request.inputs).unwrap();
    let date = |text| UtcDate::parse(text).unwrap();
    let folds = fold_schedule(&ScheduleSpec {
        sample_start: date("2021-01-01"),
        holdout_start: date("2021-01-06"),
        warmup_days: 0,
        train_days: 2,
        purge_days: 1,
        test_days: 2,
        step_days: 2,
        remainder_own_fold_days: 1,
    })
    .unwrap();
    let initial = 100_000_000_000;
    let output = run_walk_forward(
        &[FamilyGrid {
            family: "S01".to_owned(),
            slug: String::new(),
            prereg_sha256: String::new(),
            folds,
            points: vec![GridPoint {
                config: (),
                params: BTreeMap::new(),
                attempt: 0,
            }],
        }],
        &WalkContext {
            pins: &pins,
            trials_path: &request.ledger,
            initial_cash: Cash(initial),
            holdout_start: date("2021-01-06").start_us(),
        },
        &selection_costs(),
        |_, _, _, _, _| {
            Ok::<_, String>(WindowResult {
                daily_returns: vec![0, daily_return_tick(initial, initial + 1_000).unwrap()],
                equity_ticks: vec![initial, initial, initial + 1_000],
                net_result_ticks: 1_000,
                round_trips: 1,
                fill_notional_ticks: 0,
                max_dd_ticks: 0,
                accounting: WindowAccounting::ZERO,
                h4: None,
            })
        },
    )
    .unwrap();
    assert_eq!(output.n_trials, 1);
}

#[then("both scored segments have one start and one finish")]
fn ledger_appended(world: &mut AdmissionWorld) {
    let text = fs::read_to_string(world.request().ledger).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 4);
    for segment in ["train", "test"] {
        let rows: Vec<_> = lines
            .iter()
            .filter(|row| row["kind"] == "RunStarted" && row["segment"] == segment)
            .collect();
        assert_eq!(rows.len(), 1);
        let run_id = &rows[0]["run_id"];
        assert_eq!(
            lines
                .iter()
                .filter(|row| row["kind"] == "RunFinished" && &row["run_id"] == run_id)
                .count(),
            1
        );
    }
}

#[given(regex = r"^a synthetic native request with invalid (config|pins)$")]
fn invalid_request(world: &mut AdmissionWorld, kind: String) {
    pin_file(world, "absent".to_owned());
    if kind == "config" {
        let path = world.request().inputs;
        let mut input: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        input["configs"][0]["config_id"] = json!("0".repeat(64));
        fs::write(path, input.to_string()).unwrap();
    }
}

#[when("the native batch request runs")]
fn run_native(world: &mut AdmissionWorld) {
    world.result = Some(run_wf_batch(&world.request()));
}

#[then("the native batch request is rejected")]
fn native_rejected(world: &mut AdmissionWorld) {
    assert!(world.result.as_ref().unwrap().is_err());
}

#[then("the native batch error names pinned_root_expected")]
fn native_pin_rejected(world: &mut AdmissionWorld) {
    let error = world.result.as_ref().unwrap().as_ref().unwrap_err();
    assert!(error.contains("pinned_root_expected"), "{error}");
}

#[tokio::test]
async fn run_admission_features() {
    AdmissionWorld::cucumber()
        .with_default_cli()
        .run_and_exit(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/run_admission.feature"))
        .await;
}
