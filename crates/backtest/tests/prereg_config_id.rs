//! Each frozen prereg prints one preimage. The hash of those bytes is the config_id.

use std::collections::BTreeMap;

use parangsae_backtest::{canonical_config_id, RunConfig, TrialSpec, HOLDOUT_START_US};
use parangsae_core::{Interval, UtcMicros};
use parangsae_sim::Cash;
use sha2::{Digest, Sha256};

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn id_of(preimage: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(preimage).unwrap();
    let mut params = BTreeMap::new();
    for (key, item) in value["params"].as_object().unwrap() {
        params.insert(key.clone(), item.as_i64().unwrap());
    }
    let start = value["start_us"].as_i64().unwrap();
    let end = value["end_us"].as_i64().unwrap();
    let spec = TrialSpec {
        run_id: String::new(),
        config_id: String::new(),
        family: value["family"].as_str().unwrap().to_owned(),
        params,
        attempt: 1,
        ts: UtcMicros::from_micros(start),
        run: RunConfig {
            initial_cash: Cash(value["initial_cash"].as_i64().unwrap()),
            warmup_start: UtcMicros::from_micros(start),
            start: UtcMicros::from_micros(start),
            end: UtcMicros::from_micros(end),
            holdout_start: UtcMicros::from_micros(HOLDOUT_START_US),
            forced_flat: Vec::new(),
            flatten_before: None,
            mark_gaps: Vec::new(),
            interval: Interval::ONE_HOUR,
            lot_ticks: 100_000,
        },
    };
    let hashed = canonical_config_id(&spec);
    assert_eq!(
        hashed,
        sha256_hex(preimage.as_bytes()),
        "canonical preimage diverged from the printed bytes"
    );
    hashed
}

#[test]
fn h1_preimage_matches_the_printed_config_id() {
    let preimage = r#"{"end_us":1759276800000000,"family":"H1","initial_cash":100000000000000,"params":{"c":100000000,"deadband":50000000,"decision_hour_utc":0,"interval":240,"n":180},"start_us":1608249600000000}"#;
    assert_eq!(
        id_of(preimage),
        "36dbcbd7a258b161d956b6871e323ec2c1d17878b8249fff7265f706fd45f4e3"
    );
}

#[test]
fn h2_preimage_matches_the_printed_config_id() {
    let preimage = r#"{"end_us":1759276800000000,"family":"H2","initial_cash":100000000000000,"params":{"deadband":25000000,"n_members":12},"start_us":1608249600000000}"#;
    assert_eq!(
        id_of(preimage),
        "48b79103113204ab9810b93d837ca3716de60c6583250764e7a8e7b1f0731238"
    );
}

#[test]
fn h3_preimage_matches_the_printed_config_id() {
    let preimage = r#"{"end_us":1759276800000000,"family":"H3","initial_cash":100000000000000,"params":{"er_l":168,"er_q":50000000,"hold_h":48,"n":168},"start_us":1608249600000000}"#;
    assert_eq!(
        id_of(preimage),
        "99e81cbc994b92b246a3fcdb863b250c731cff1b4e9ed068e95df750407bb0f8"
    );
}

#[test]
fn h4_preimage_matches_the_printed_config_id() {
    let preimage = r#"{"end_us":1759276800000000,"family":"H4","initial_cash":100000000000000,"params":{"confirm_h":24,"hold_h":72,"q":10000000,"sum_settlements":3},"start_us":1608249600000000}"#;
    assert_eq!(
        id_of(preimage),
        "f225df71988599ef4623c16edbd2c7ea4a2fcff37f0ccf852ddbc9ba146caffe"
    );
}

#[test]
fn h5_preimage_matches_the_printed_config_id() {
    let preimage = r#"{"end_us":1759276800000000,"family":"H5","initial_cash":100000000000000,"params":{"hold_h":72,"k":200000000,"lookback_d":90},"start_us":1608249600000000}"#;
    assert_eq!(
        id_of(preimage),
        "03789be2fab62f87bbc7cb915a1a5f03e4dccdeca2a73ade9e2fc0ad5240934a"
    );
}
