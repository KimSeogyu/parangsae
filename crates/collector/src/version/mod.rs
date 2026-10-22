//! Dataset version manifest. Receipt migration is the first piece; hashing and
//! publish land in later units.

pub mod encode;
pub mod legacy;
pub mod migrate;

pub use migrate::{directory_digest, migrate_receipts, MigrationReport};
