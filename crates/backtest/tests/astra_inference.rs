//! Runs the actual stdlib-only CLI on synthetic cash, never market data.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cucumber::{given, then, when, World as _};
use parangsae_backtest::UtcDate;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const C0: i64 = 10_000_000_000_000;
const SOURCE: &str = "SYNTHETIC_NO_RUN:independent-closed-form-k1";

#[derive(Debug, Default, cucumber::World)]
struct InferenceWorld {
    temporary: Option<tempfile::TempDir>,
    command_args: Vec<String>,
    first: Option<Output>,
    second: Option<Output>,
    saved: Vec<u8>,
    interpreter_version: String,
}

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../research/tools/astra_inference.py")
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn run(args: &[String]) -> Output {
    Command::new("python3")
        .args(["-I", "-B"])
        .arg(script())
        .args(args)
        .output()
        .expect("python3 is required; absence must fail, never skip")
}

fn synthetic_files(root: &Path) -> (Vec<u8>, Vec<u8>) {
    let mut day = UtcDate::parse("2020-12-18").unwrap();
    let mut equity = C0;
    let mut primary = Vec::new();
    let mut cash = Vec::new();
    for index in 0..1120 {
        equity += 1_000_000_000
            + if (index / 56) & 1 == 0 {
                100_000
            } else {
                -100_000
            };
        primary.push(json!({"day":day.to_string(),"equity_ticks":equity}));
        cash.push(json!({"day":day.to_string(),"equity_ticks":C0}));
        day = day.next();
    }
    let mut roles = Vec::new();
    for (left, right) in [
        ("shock_primary", "shock_direction_control"),
        ("range_primary", "range_direction_control"),
        ("premium_primary", "premium_price_control"),
    ] {
        roles.push(json!({"role":left,"status":"COMPLETE","exposed":true,"daily":primary}));
        roles.push(json!({"role":right,"status":"COMPLETE","exposed":false,"daily":cash}));
    }
    let evidence = json!({
        "schema":"astra-cash-evidence-v1","scope":"SYNTHETIC_NO_RUN",
        "start_day":"2020-12-18","days":1120,"c0_ticks":C0,
        "invocation_status":"COMPLETE","roles":roles
    });
    let critical = json!({
        "schema":"astra-critical-enclosures-v1","scope":"SYNTHETIC_NO_RUN",
        "start_day":"2020-12-18","days":1120,"source_id":SOURCE,
        "certificate_sha256":digest(b"SYNTHETIC_NOT_STUDENT_T"),
        "probability":{"num":239,"den":240},
        "brackets":[
            {"block_days":56,"df":19,"lower":{"num":"1","den":"1"},"upper":{"num":"1","den":"1"}},
            {"block_days":28,"df":39,"lower":{"num":"1","den":"1"},"upper":{"num":"1","den":"1"}}
        ]
    });
    let evidence = serde_json::to_vec(&evidence).unwrap();
    let critical = serde_json::to_vec(&critical).unwrap();
    fs::write(root.join("cash.json"), &evidence).unwrap();
    fs::write(root.join("critical.json"), &critical).unwrap();
    (evidence, critical)
}

#[given("a recorded Python interpreter at version 3.11 or newer")]
fn python(world: &mut InferenceWorld) {
    let result = Command::new("python3")
        .args([
            "-I",
            "-B",
            "-c",
            "import sys; assert sys.version_info >= (3,11), 'Python >=3.11 required'; print(sys.version)",
        ])
        .output()
        .expect("python3 is required, not an optional observer dependency");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    world.interpreter_version = String::from_utf8(result.stdout).unwrap();
    println!(
        "Astra CLI observer interpreter: {}",
        world.interpreter_version.trim()
    );
}

#[given("the stdlib synthetic arithmetic and input-boundary cases pass")]
fn python_cases(_world: &mut InferenceWorld) {
    let cases = script()
        .parent()
        .unwrap()
        .join("tests/test_astra_inference.py");
    let result = Command::new("python3")
        .args(["-I", "-B"])
        .arg(cases)
        .output()
        .expect("the declared Python observer dependency must be available");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stderr));
}

#[given("a complete synthetic cash fixture and fake critical value tagged NOT RUN")]
fn fixture(world: &mut InferenceWorld) {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let (cash, critical) = synthetic_files(root);
    world.command_args = vec![
        "--cash".into(),
        root.join("cash.json").to_string_lossy().into_owned(),
        "--critical".into(),
        root.join("critical.json").to_string_lossy().into_owned(),
        "--expected-cash-sha256".into(),
        digest(&cash),
        "--expected-critical-sha256".into(),
        digest(&critical),
        "--expected-critical-source".into(),
        SOURCE.into(),
        "--out".into(),
        root.join("diagnostic.json").to_string_lossy().into_owned(),
    ];
    world.temporary = Some(temporary);
}

#[when("the actual inference CLI writes the requested diagnostic")]
fn execute(world: &mut InferenceWorld) {
    world.first = Some(run(&world.command_args));
}

#[then("exact points and block variances match the independent synthetic oracle")]
fn result(world: &mut InferenceWorld) {
    let command = world.first.as_ref().unwrap();
    assert!(
        command.status.success(),
        "{}",
        String::from_utf8_lossy(&command.stderr)
    );
    let root = world.temporary.as_ref().unwrap().path();
    world.saved = fs::read(root.join("diagnostic.json")).unwrap();
    let report: Value = serde_json::from_slice(&world.saved).unwrap();
    assert_eq!(report["scope"], "SYNTHETIC_NO_RUN");
    assert_eq!(report["advance_authority"], false);
    assert_eq!(report["producer_custody_verified_here"], false);
    assert_eq!(report["cdf_certificate_verified_here"], false);
    assert_eq!(report["runtime"]["isolated"], true);
    assert_eq!(report["runtime"]["bytecode_disabled"], true);
    println!("Astra CLI runtime binding: {}", report["runtime"]);
    assert!(world
        .interpreter_version
        .starts_with(report["runtime"]["version"].as_str().unwrap()));
    for family in ["shock", "range", "premium"] {
        let result = &report["families"][family];
        assert_eq!(result["annual_simple_A"], json!({"num":"73","den":"2000"}));
        assert_eq!(
            result["economic_H"]["point"],
            json!({"num":"33","den":"2000"})
        );
        assert_eq!(
            result["direction_D"]["point"],
            json!({"num":"73","den":"2000"})
        );
        assert_eq!(result["numeric_result"], "NUMERIC_LOCAL_SUPPORT");
        for endpoint in ["economic_H", "direction_D"] {
            let rows = &result[endpoint]["by_block_days"];
            assert_eq!(
                rows["56"]["se_squared"],
                json!({"num":"5329","den":"7600000000000000"})
            );
            assert_eq!(
                rows["28"]["se_squared"],
                json!({"num":"5329","den":"15600000000000000"})
            );
            for length in ["56", "28"] {
                assert_eq!(rows[length]["status"], "SUPPORT");
                assert_eq!(rows[length]["precision"], "PASS");
                let edges = rows[length]["boundaries"].as_array().unwrap();
                assert_eq!(edges.first().and_then(Value::as_u64), Some(0));
                assert_eq!(edges.last().and_then(Value::as_u64), Some(1120));
            }
        }
    }
}

#[when("the CLI is asked to reuse that output path")]
fn repeat(world: &mut InferenceWorld) {
    world.second = Some(run(&world.command_args));
}

#[then("it refuses and preserves the existing artifact and both inputs")]
fn preserves(world: &mut InferenceWorld) {
    assert_eq!(world.second.as_ref().unwrap().status.code(), Some(2));
    let root = world.temporary.as_ref().unwrap().path();
    assert_eq!(fs::read(root.join("diagnostic.json")).unwrap(), world.saved);
    for (name, flag) in [
        ("cash.json", "--expected-cash-sha256"),
        ("critical.json", "--expected-critical-sha256"),
    ] {
        let index = world
            .command_args
            .iter()
            .position(|arg| arg == flag)
            .unwrap();
        assert_eq!(
            digest(&fs::read(root.join(name)).unwrap()),
            world.command_args[index + 1]
        );
    }
}

#[tokio::main]
async fn main() {
    InferenceWorld::cucumber()
        .with_default_cli()
        .fail_on_skipped()
        .run_and_exit(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/astra_inference.feature"),
        )
        .await;
}
