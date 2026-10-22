//! Dataset and version identifiers. The inner string of [`DatasetId`] is private.

use std::fmt;

/// `<source>.<market>.<dataset>.<symbol>`, for example
/// `binance-vision.um.um-kline.BTCUSDT`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DatasetId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatasetIdError {
    Malformed,
}

impl fmt::Display for DatasetIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => write!(f, "dataset id must be four dot-separated segments"),
        }
    }
}

impl DatasetId {
    pub fn parse(text: &str) -> Result<Self, DatasetIdError> {
        let parts: Vec<&str> = text.split('.').collect();
        if parts.len() != 4 || parts.iter().any(|part| part.is_empty()) {
            return Err(DatasetIdError::Malformed);
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn dataset(&self) -> &str {
        self.segment(2)
    }

    pub fn symbol(&self) -> &str {
        self.segment(3)
    }

    fn segment(&self, index: usize) -> &str {
        self.0.split('.').nth(index).unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VersionRoot([u8; 32]);

impl VersionRoot {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(self) -> [u8; 32] {
        self.0
    }

    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ManifestDigest([u8; 32]);

impl ManifestDigest {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(self) -> [u8; 32] {
        self.0
    }

    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }
}
