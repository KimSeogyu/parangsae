//! Deterministic policy.json. Key order and array order are part of the bytes.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::parse::{Pair, Policy};

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn canonical_json(policy: &Policy, spec_sha256: &str) -> String {
    let mut map = Map::new();
    map.insert(
        "allowed_capability_callers".to_owned(),
        pairs(&policy.capability_callers),
    );
    map.insert(
        "allowed_dependencies".to_owned(),
        pairs(&policy.dependencies),
    );
    map.insert("allowed_effects".to_owned(), pairs(&policy.allowed_effects));
    map.insert("allowed_state_writers".to_owned(), pairs(&policy.writers));
    map.insert("capabilities".to_owned(), strings(&policy.capabilities));
    map.insert("effects".to_owned(), strings(&policy.effects));
    map.insert("float_allowed".to_owned(), strings(&policy.float_allowed));
    map.insert("modules".to_owned(), strings(&policy.modules));
    map.insert("owners".to_owned(), strings(&policy.owners));
    map.insert("path_owners".to_owned(), pairs(&policy.path_owners));
    map.insert(
        "spec_sha256".to_owned(),
        Value::String(spec_sha256.to_owned()),
    );
    let mut text = serde_json::to_string_pretty(&Value::Object(map))
        .expect("policy json is finite and acyclic");
    text.push('\n');
    text
}

fn strings(map: &BTreeMap<String, u32>) -> Value {
    Value::Array(map.keys().cloned().map(Value::String).collect())
}

fn pairs(pairs: &[Pair]) -> Value {
    Value::Array(
        pairs
            .iter()
            .map(|pair| {
                Value::Array(vec![
                    Value::String(pair.left.clone()),
                    Value::String(pair.right.clone()),
                ])
            })
            .collect(),
    )
}
