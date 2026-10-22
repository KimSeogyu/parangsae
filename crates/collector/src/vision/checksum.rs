//! `.CHECKSUM` is sha256. A mismatch fails. The digest is not guessed.

use sha2::{Digest, Sha256};

use crate::Error;

pub fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

pub fn parse_checksum(text: &str, zip_file_name: &str) -> Result<[u8; 32], Error> {
    let line = text
        .lines()
        .find(|line| !line.is_empty())
        .ok_or_else(|| Error::check("checksum", "checksum file is empty"))?;
    let mut parts = line.split_whitespace();
    let hash = parts
        .next()
        .ok_or_else(|| Error::check("checksum", "checksum file has no digest"))?;
    if let Some(name) = parts.next() {
        if name != zip_file_name {
            return Err(Error::check(
                "checksum",
                format!("checksum names {name}, archive is {zip_file_name}"),
            ));
        }
    }
    if parts.next().is_some() {
        return Err(Error::check("checksum", "checksum line has extra fields"));
    }
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::check("checksum", "digest is not 64 hex characters"));
    }
    let bytes = hex::decode(hash).map_err(|err| Error::check("checksum", err.to_string()))?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

pub fn require_sha256(bytes: &[u8], expected: [u8; 32]) -> Result<(), Error> {
    let got = sha256_bytes(bytes);
    if got == expected {
        Ok(())
    } else {
        Err(Error::check(
            "checksum",
            format!("sha256 {} != {}", hex::encode(got), hex::encode(expected)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flipped_digest_fails_and_names_the_check() -> Result<(), String> {
        let bytes = b"abc";
        let good = sha256_bytes(bytes);
        require_sha256(bytes, good).map_err(|err| err.to_string())?;
        let mut bad = good;
        bad[0] ^= 0xff;
        let err = match require_sha256(bytes, bad) {
            Err(err) => err,
            Ok(()) => return Err("expected a checksum error".to_string()),
        };
        assert_eq!(err.check_name(), Some("checksum"));
        Ok(())
    }
}
