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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionIdError {
    Alias,
    Malformed,
}

impl fmt::Display for VersionIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Alias => write!(f, "version id alias is refused"),
            Self::Malformed => write!(f, "version id is malformed"),
        }
    }
}

/// Parse `<dataset_id>@<64 lowercase hex>`.
///
/// `@latest` in any case, and a `YYYY-MM-DD` root, are [`VersionIdError::Alias`].
/// A root that is not 64 lowercase hex characters is [`VersionIdError::Malformed`].
pub fn parse_version_id(text: &str) -> Result<(DatasetId, VersionRoot), VersionIdError> {
    let Some((dataset, root)) = text.split_once('@') else {
        return Err(VersionIdError::Malformed);
    };
    if dataset.is_empty() || text.matches('@').count() != 1 {
        return Err(VersionIdError::Malformed);
    }
    if is_alias(root) {
        return Err(VersionIdError::Alias);
    }
    let dataset = DatasetId::parse(dataset).map_err(|_| VersionIdError::Malformed)?;
    let root = VersionRoot::parse_hex(root)?;
    Ok((dataset, root))
}

fn is_alias(root: &str) -> bool {
    if root.eq_ignore_ascii_case("latest") {
        return true;
    }
    let bytes = root.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
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

    pub fn parse_hex(text: &str) -> Result<Self, VersionIdError> {
        Ok(Self(decode_hex(text)?))
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

    pub fn parse_hex(text: &str) -> Result<Self, VersionIdError> {
        Ok(Self(decode_hex(text)?))
    }
}

fn decode_hex(text: &str) -> Result<[u8; 32], VersionIdError> {
    if text.len() != 64
        || text
            .bytes()
            .any(|byte| !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase())
    {
        return Err(VersionIdError::Malformed);
    }
    let decoded = hex::decode(text).map_err(|_| VersionIdError::Malformed)?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "binance-vision.um.um-kline.BTCUSDT";

    #[test]
    fn aliases_and_bad_hex_are_refused() -> Result<(), String> {
        let latest = format!("{ID}@latest");
        let shout = format!("{ID}@LATEST");
        let date = format!("{ID}@2026-10-22");
        let short = format!("{ID}@{}", "a".repeat(63));
        let upper = format!("{ID}@{}", "A".repeat(64));
        assert!(matches!(
            parse_version_id(&latest),
            Err(VersionIdError::Alias)
        ));
        assert!(matches!(
            parse_version_id(&shout),
            Err(VersionIdError::Alias)
        ));
        assert!(matches!(
            parse_version_id(&date),
            Err(VersionIdError::Alias)
        ));
        assert!(matches!(
            parse_version_id(&short),
            Err(VersionIdError::Malformed)
        ));
        assert!(matches!(
            parse_version_id(&upper),
            Err(VersionIdError::Malformed)
        ));
        let hex_text = "ab".repeat(32);
        let (dataset, root) =
            parse_version_id(&format!("{ID}@{hex_text}")).map_err(|err| err.to_string())?;
        if dataset.as_str() != ID || root.to_hex() != hex_text {
            return Err("parsed id did not round-trip".into());
        }
        Ok(())
    }
}
