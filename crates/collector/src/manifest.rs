//! Identity of one downloaded object.
//!
//! A checksum mismatch fails the ingest. When Vision replaces a file, the
//! previous manifest is kept and the object is reprocessed. This scaffold does
//! not download.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectManifest {
    pub url: String,
    pub sha256: [u8; 32],
    pub size_bytes: u64,
    pub schema_version: u32,
}
