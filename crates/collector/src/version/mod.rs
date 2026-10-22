//! Dataset version manifest.

pub mod encode;
pub mod id;
pub mod legacy;
pub mod load;
pub mod migrate;
pub mod preflight;
pub mod publish;
pub mod resolve;
pub mod source;
pub mod validate;
pub mod versions;

#[cfg(test)]
mod c3_test;

pub use preflight::{
    preflight, requirements, Degradation, FoldSpec, FoldStamp, PreflightError, Requirement,
    RequirementError,
};

pub use id::{
    parse_version_id, DatasetId, DatasetIdError, ManifestDigest, VersionIdError, VersionRoot,
};
pub use load::{
    prove_manifest, ConflictEntry, ExcludedWindow, GapEntry, LoadError, ManifestProof,
    ManifestProofError, Table, Verified, VerifyMode, VersionStore,
};
pub use publish::{publish, PublishError, PublishRequest, Published};

pub use migrate::{directory_digest, migrate_receipts, MigrationReport};
