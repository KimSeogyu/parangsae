//! Dataset version manifest.

pub mod encode;
pub mod id;
pub mod legacy;
pub mod load;
pub mod migrate;
pub mod publish;
pub mod validate;

pub use id::{
    parse_version_id, DatasetId, DatasetIdError, ManifestDigest, VersionIdError, VersionRoot,
};
pub use load::{
    ConflictEntry, ExcludedWindow, GapEntry, LoadError, Table, Verified, VerifyMode, VersionStore,
};
pub use publish::{publish, PublishError, PublishRequest, Published};

pub use migrate::{directory_digest, migrate_receipts, MigrationReport};
