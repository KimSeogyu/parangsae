//! Bounded, single-writer ingress journal for the offline paper replay.
//!
//! A complete frame is durable before the engine observes its input. Recovery
//! drops only an incomplete final frame. A complete damaged frame is an error.
//! This file does not provide a lock against another process using the same run.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::marker::PhantomData;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 4] = b"PRJ1";
const END: &[u8; 4] = b"DONE";
const PREFIX_LEN: usize = 16;
const HEADER_LEN: usize = PREFIX_LEN + 32;
const FRAME_OVERHEAD: usize = HEADER_LEN + 32 + END.len();
const MAX_RECORD_BYTES: usize = 1_048_576;
const MAX_JOURNAL_BYTES: u64 = 67_108_864;
const MAX_RECORDS: u64 = 16_384;

#[derive(Debug)]
pub(super) enum JournalError {
    Io(std::io::Error),
    Corrupt { record: u64, reason: &'static str },
    Capacity,
    UncertainAppend,
    Codec(serde_json::Error),
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "paper journal I/O: {error}"),
            Self::Corrupt { record, reason } => {
                write!(f, "paper journal record {record}: {reason}")
            }
            Self::Capacity => write!(f, "paper journal capacity exceeded"),
            Self::UncertainAppend => write!(f, "paper journal must recover after a failed append"),
            Self::Codec(error) => write!(f, "paper journal input: {error}"),
        }
    }
}

impl std::error::Error for JournalError {}

impl From<std::io::Error> for JournalError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub(super) struct Journal<T> {
    file: File,
    next: u64,
    bytes: u64,
    poisoned: bool,
    _record: PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned> Journal<T> {
    pub(super) fn create(path: &Path) -> Result<Self, JournalError> {
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(path)?;
        file.sync_all()?;
        sync_parent(path)?;
        Ok(Self {
            file,
            next: 0,
            bytes: 0,
            poisoned: false,
            _record: PhantomData,
        })
    }

    pub(super) fn open(path: &Path) -> Result<(Self, Vec<T>), JournalError> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let length = file.metadata()?.len();
        if length > MAX_JOURNAL_BYTES {
            return Err(JournalError::Capacity);
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let (records, valid) = decode::<T>(&bytes)?;
        let valid = u64::try_from(valid).map_err(|_| JournalError::Capacity)?;
        if valid < length {
            file.set_len(valid)?;
            file.sync_all()?;
        }
        file.seek(SeekFrom::End(0))?;
        let next = u64::try_from(records.len()).map_err(|_| JournalError::Capacity)?;
        Ok((
            Self {
                file,
                next,
                bytes: valid,
                poisoned: false,
                _record: PhantomData,
            },
            records,
        ))
    }

    pub(super) fn append(&mut self, record: &T) -> Result<(), JournalError> {
        if self.poisoned {
            return Err(JournalError::UncertainAppend);
        }
        if self.next >= MAX_RECORDS {
            return Err(JournalError::Capacity);
        }
        let payload = serde_json::to_vec(record).map_err(JournalError::Codec)?;
        if payload.len() > MAX_RECORD_BYTES {
            return Err(JournalError::Capacity);
        }
        let payload_length = u32::try_from(payload.len()).map_err(|_| JournalError::Capacity)?;
        let mut frame = Vec::with_capacity(FRAME_OVERHEAD + payload.len());
        frame.extend_from_slice(MAGIC);
        frame.extend_from_slice(&self.next.to_le_bytes());
        frame.extend_from_slice(&payload_length.to_le_bytes());
        let header_digest = Sha256::digest(&frame);
        frame.extend_from_slice(&header_digest);
        frame.extend_from_slice(&Sha256::digest(&payload));
        frame.extend_from_slice(&payload);
        frame.extend_from_slice(END);
        let frame_len = u64::try_from(frame.len()).map_err(|_| JournalError::Capacity)?;
        let bytes = self
            .bytes
            .checked_add(frame_len)
            .filter(|total| *total <= MAX_JOURNAL_BYTES)
            .ok_or(JournalError::Capacity)?;
        self.poisoned = true;
        self.file.write_all(&frame)?;
        self.file.sync_all()?;
        self.poisoned = false;
        self.next += 1;
        self.bytes = bytes;
        Ok(())
    }
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<(Vec<T>, usize), JournalError> {
    let mut records = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let sequence = u64::try_from(records.len()).map_err(|_| JournalError::Capacity)?;
        if sequence >= MAX_RECORDS {
            return Err(JournalError::Capacity);
        }
        let fail = |reason| JournalError::Corrupt {
            record: sequence,
            reason,
        };
        let remaining = &bytes[at..];
        if remaining.len() < HEADER_LEN {
            // This prefix could have been written before a hard process stop.
            // Validate every available magic byte; arbitrary garbage is corrupt.
            let available_magic = remaining.len().min(MAGIC.len());
            if remaining[..available_magic] != MAGIC[..available_magic] {
                return Err(fail("invalid incomplete frame prefix"));
            }
            break;
        }
        if &remaining[..4] != MAGIC {
            return Err(fail("invalid frame prefix"));
        }
        let header_digest = Sha256::digest(&remaining[..PREFIX_LEN]);
        if header_digest[..] != remaining[PREFIX_LEN..HEADER_LEN] {
            return Err(fail("frame header changed"));
        }
        let number = u64::from_le_bytes(
            remaining[4..12]
                .try_into()
                .map_err(|_| fail("invalid sequence"))?,
        );
        if number != sequence {
            return Err(fail("nonconsecutive sequence"));
        }
        let length = usize::try_from(u32::from_le_bytes(
            remaining[12..16]
                .try_into()
                .map_err(|_| fail("invalid record length"))?,
        ))
        .map_err(|_| JournalError::Capacity)?;
        if length > MAX_RECORD_BYTES {
            return Err(JournalError::Capacity);
        }
        let complete = FRAME_OVERHEAD
            .checked_add(length)
            .ok_or(JournalError::Capacity)?;
        if remaining.len() < complete {
            break;
        }
        let start = HEADER_LEN + 32;
        let payload = &remaining[start..start + length];
        if &remaining[start + length..complete] != END {
            return Err(fail("invalid completed frame marker"));
        }
        let payload_digest = Sha256::digest(payload);
        if payload_digest[..] != remaining[HEADER_LEN..start] {
            return Err(fail("record payload changed"));
        }
        let record = serde_json::from_slice(payload)
            .map_err(|_| fail("record does not match the typed input schema"))?;
        records.push(record);
        at += complete;
    }
    Ok((records, at))
}

fn sync_parent(path: &Path) -> Result<(), JournalError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Journal, JournalError, HEADER_LEN};
    use std::io::Write;

    #[test]
    fn completed_inputs_survive_reopen_and_continue_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inputs.journal");
        let mut journal = Journal::<String>::create(&path).unwrap();
        journal.append(&"signal".into()).unwrap();
        journal.append(&"dispatch".into()).unwrap();
        drop(journal);
        let (mut journal, records) = Journal::<String>::open(&path).unwrap();
        assert_eq!(records, ["signal", "dispatch"]);
        journal.append(&"fill".into()).unwrap();
        drop(journal);
        let (_, records) = Journal::<String>::open(&path).unwrap();
        assert_eq!(records, ["signal", "dispatch", "fill"]);
    }

    #[test]
    fn incomplete_final_input_is_removed_before_the_next_append() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inputs.journal");
        let mut journal = Journal::<String>::create(&path).unwrap();
        journal.append(&"accepted".into()).unwrap();
        drop(journal);
        let complete = std::fs::read(&path).unwrap();
        let mut journal = Journal::<String>::open(&path).unwrap().0;
        journal.append(&"interrupted".into()).unwrap();
        drop(journal);
        let mut interrupted = std::fs::read(&path).unwrap();
        interrupted.truncate(interrupted.len() - 7);
        std::fs::write(&path, interrupted).unwrap();
        let (mut journal, records) = Journal::<String>::open(&path).unwrap();
        assert_eq!(records, ["accepted"]);
        assert_eq!(std::fs::read(&path).unwrap(), complete);
        journal.append(&"replacement input".into()).unwrap();
        drop(journal);
        let (_, records) = Journal::<String>::open(&path).unwrap();
        assert_eq!(records, ["accepted", "replacement input"]);
    }

    #[test]
    fn changed_completed_payload_or_header_is_refused_without_repair() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inputs.journal");
        let mut journal = Journal::<String>::create(&path).unwrap();
        journal.append(&"accepted".into()).unwrap();
        drop(journal);
        let original = std::fs::read(&path).unwrap();
        for index in [12, HEADER_LEN + 33] {
            let mut changed = original.clone();
            changed[index] ^= 1;
            std::fs::write(&path, &changed).unwrap();
            assert!(matches!(
                Journal::<String>::open(&path),
                Err(JournalError::Corrupt { .. })
            ));
            assert_eq!(std::fs::read(&path).unwrap(), changed);
        }
    }

    #[test]
    fn garbage_after_a_completed_input_is_not_a_torn_frame() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inputs.journal");
        let mut journal = Journal::<String>::create(&path).unwrap();
        journal.append(&"accepted".into()).unwrap();
        drop(journal);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"wrong").unwrap();
        assert!(matches!(
            Journal::<String>::open(&path),
            Err(JournalError::Corrupt { .. })
        ));
    }
}
