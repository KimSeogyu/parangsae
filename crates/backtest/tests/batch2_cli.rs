//! Synthetic CLI preflight. No market partitions or research ledger exist.

use std::fs;
use std::process::Command;

use serde_json::{json, Value};

fn prereg_config(family: &str) -> Value {
    let (params, id) = match family {
        "H1" => (
            json!({"c":100000000,"deadband":50000000,"decision_hour_utc":0,"interval":240,"n":180}),
            "36dbcbd7a258b161d956b6871e323ec2c1d17878b8249fff7265f706fd45f4e3",
        ),
        "H2" => (
            json!({"deadband":25000000,"n_members":12}),
            "48b79103113204ab9810b93d837ca3716de60c6583250764e7a8e7b1f0731238",
        ),
        _ => unreachable!(),
    };
    json!({
        "config_id":id,"family":family,"grid_index":0,"params":params,
        "start_us":1608249600000000_i64,"end_us":1759276800000000_i64,
        "initial_cash":100000000000000_i64
    })
}

#[test]
fn cli_accepts_h1_h2_prereg_configs_and_refuses_missing_data_pins_before_writes() {
    for family in ["H1", "H2"] {
        let dir = tempfile::tempdir().unwrap();
        let inputs = dir.path().join("synthetic-inputs.json");
        let out = dir.path().join("out");
        let ledger = dir.path().join("trials.jsonl");
        let mut input = json!({
            "schema":"sweep-runner-inputs-v2","slippage_bps":2,
            "configs":[prereg_config(family)],"objects":[]
        });
        for valid in [true, false] {
            if !valid {
                input["configs"][0]["config_id"] = json!("0".repeat(64));
            }
            fs::write(&inputs, input.to_string()).unwrap();
            let output = Command::new(env!("CARGO_BIN_EXE_wf_batch"))
                .arg("--inputs")
                .arg(&inputs)
                .arg("--data-root")
                .arg(dir.path().join("absent-data"))
                .arg("--out")
                .arg(&out)
                .arg("--ledger")
                .arg(&ledger)
                .args(["--attempt", "0"])
                .output()
                .unwrap();
            assert!(!output.status.success());
            let error = String::from_utf8(output.stderr).unwrap();
            if valid {
                assert!(error.contains("is not in the pinned objects"), "{error}");
            } else {
                assert!(error.contains("is not the fold-window hash"), "{error}");
            }
            assert!(!out.exists());
            assert!(!ledger.exists());
        }
    }
}
