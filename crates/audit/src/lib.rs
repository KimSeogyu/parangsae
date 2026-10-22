//! Read a dataset version that [`parangsae_collector::version::VersionStore::load`]
//! refuses under §5.4. The result is an [`AuditView`], which is not a
//! `Verified<Table>`.

#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use parangsae_collector::version::{
    prove_manifest, DatasetId, ManifestDigest, ManifestProofError, VersionRoot,
};

/// A verified audit snapshot. It has no conversion into a research table.
pub struct AuditView {
    dataset_id: String,
    root: VersionRoot,
    partitions: usize,
}

impl AuditView {
    pub fn dataset_id(&self) -> &str {
        &self.dataset_id
    }

    pub fn root(&self) -> VersionRoot {
        self.root
    }

    pub fn partitions(&self) -> usize {
        self.partitions
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AuditError {
    DigestMismatch,
    RootMismatch,
    Malformed,
    Io(String),
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DigestMismatch => write!(f, "manifest digest mismatch"),
            Self::RootMismatch => write!(f, "manifest root mismatch"),
            Self::Malformed => write!(f, "manifest is malformed"),
            Self::Io(detail) => write!(f, "{detail}"),
        }
    }
}

/// Open one manifest for audit. Digest and root are checked. A §5.4 refusal is not.
pub fn open_version(
    data_dir: &Path,
    dataset_id: &str,
    root: VersionRoot,
    manifest_digest: ManifestDigest,
) -> Result<AuditView, AuditError> {
    let dataset = DatasetId::parse(dataset_id).map_err(|_| AuditError::Malformed)?;
    let path = data_dir
        .join("manifest")
        .join("versions")
        .join(dataset.as_str())
        .join(format!("{}.json", root.to_hex()));
    let bytes = fs::read(&path).map_err(|err| AuditError::Io(err.to_string()))?;
    let proof = match prove_manifest(&bytes, manifest_digest.as_bytes()) {
        Ok(proof) => proof,
        Err(ManifestProofError::DigestMismatch) => return Err(AuditError::DigestMismatch),
        Err(ManifestProofError::RootMismatch) => return Err(AuditError::RootMismatch),
        Err(ManifestProofError::Malformed) => return Err(AuditError::Malformed),
    };
    if proof.root != root || proof.dataset_id != dataset_id {
        return Err(AuditError::RootMismatch);
    }
    Ok(AuditView {
        dataset_id: proof.dataset_id,
        root: proof.root,
        partitions: proof.partitions,
    })
}

/// `Ok` when no runner, psg, or sim package reaches `parangsae-audit`.
///
/// The walk follows every dependency kind: normal, build, and dev.
pub fn runner_depends_on_audit(metadata: &serde_json::Value) -> Result<(), String> {
    let packages = metadata
        .get("packages")
        .and_then(|value| value.as_array())
        .ok_or("packages")?;
    let mut by_name: HashMap<&str, &serde_json::Value> = HashMap::new();
    for package in packages {
        let name = package
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or("name")?;
        by_name.insert(name, package);
    }
    for name in by_name.keys().copied() {
        if !is_research_consumer(name) {
            continue;
        }
        let mut stack = vec![name];
        let mut seen = HashSet::new();
        while let Some(current) = stack.pop() {
            if !seen.insert(current) {
                continue;
            }
            if current == "parangsae-audit" {
                return Err(format!("{name} depends on parangsae-audit"));
            }
            let Some(package) = by_name.get(current) else {
                continue;
            };
            let Some(deps) = package
                .get("dependencies")
                .and_then(|value| value.as_array())
            else {
                continue;
            };
            for dep in deps {
                if let Some(dep_name) = dep.get("name").and_then(|value| value.as_str()) {
                    stack.push(dep_name);
                }
            }
        }
    }
    Ok(())
}

fn is_research_consumer(name: &str) -> bool {
    matches!(name, "runner" | "psg" | "sim")
        || name.ends_with("-runner")
        || name.ends_with("-psg")
        || name.ends_with("-sim")
}
