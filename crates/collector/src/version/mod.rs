//! Dataset version manifest. Receipt migration is the first piece; hashing and
//! publish land in later units.

pub mod encode;
pub mod id;
pub mod legacy;
pub mod migrate;
pub mod publish;

pub use id::{DatasetId, DatasetIdError, ManifestDigest, VersionRoot};
pub use publish::{publish, PublishError, PublishRequest, Published};

pub use migrate::{directory_digest, migrate_receipts, MigrationReport};
