//! Archive identity carried with a parsed version.

use parangsae_core::RunId;

use crate::vision::Cadence;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: String,
    pub sha256: [u8; 32],
    pub cadence: Cadence,
    pub schema_version: u32,
    pub last_modified: Option<String>,
    pub fetched_at_run: RunId,
}

impl Source {
    pub fn sha256_hex(&self) -> String {
        hex::encode(self.sha256)
    }

    pub fn cadence_str(&self) -> &'static str {
        match self.cadence {
            Cadence::Monthly => "monthly",
            Cadence::Daily => "daily",
        }
    }

    /// Monthly is 0, daily is 1, then path bytes, then sha256 bytes.
    pub fn rank_parts(&self) -> (u8, &str, &[u8; 32]) {
        let cadence = match self.cadence {
            Cadence::Monthly => 0,
            Cadence::Daily => 1,
        };
        (cadence, self.path.as_str(), &self.sha256)
    }
}

pub fn parse_cadence(text: &str) -> Option<Cadence> {
    match text {
        "monthly" => Some(Cadence::Monthly),
        "daily" => Some(Cadence::Daily),
        _ => None,
    }
}
