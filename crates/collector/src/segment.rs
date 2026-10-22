//! Sequenced ingest segment.
//!
//! The transitions are the TLC-checked model in `specs/tla/IngestSegment.tla`.
//! A contiguous id is accepted, a duplicate id does not extend the segment, and
//! a jump moves to [`Phase::Gap`], which accepts nothing until resync. Resync
//! opens an empty segment and does not backfill the gap. Bytes on the wire,
//! checksums, and multiple symbols are not this machine.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Open,
    Gap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Update(u64),
    Resync(u64),
}

/// A broken TLA invariant. The names match `IngestSegment.cfg`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invariant {
    TypeOK,
    Contiguous,
    NoRepeatedId,
    GapHoldsSnapshot,
    NoAppendWhileGap,
}

impl fmt::Display for Invariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TypeOK => "TypeOK",
            Self::Contiguous => "Contiguous",
            Self::NoRepeatedId => "NoRepeatedId",
            Self::GapHoldsSnapshot => "GapHoldsSnapshot",
            Self::NoAppendWhileGap => "NoAppendWhileGap",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentError {
    MaxTooSmall,
    IdOutOfRange,
    SnapshotOutOfRange,
    ResyncWhileOpen,
    Fixture(FixtureError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureError {
    MissingField,
    BadInteger,
    BadPhase,
    ExpectMismatch,
}

impl fmt::Display for SegmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaxTooSmall => write!(f, "segment max is below 2"),
            Self::IdOutOfRange => write!(f, "update id is outside 1..=max"),
            Self::SnapshotOutOfRange => write!(f, "resync snapshot is outside 0..=max"),
            Self::ResyncWhileOpen => write!(f, "resync is only enabled in gap"),
            Self::Fixture(err) => write!(f, "segment fixture: {err}"),
        }
    }
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingField => "missing field",
            Self::BadInteger => "bad integer",
            Self::BadPhase => "phase is not open or gap",
            Self::ExpectMismatch => "expected invariant was not the one that broke",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    max: u64,
    phase: Phase,
    last_u: u64,
    accepted: Vec<u64>,
    gap_snapshot: Vec<u64>,
}

impl Segment {
    pub fn init(max: u64) -> Result<Self, SegmentError> {
        if max < 2 {
            return Err(SegmentError::MaxTooSmall);
        }
        Ok(Self {
            max,
            phase: Phase::Open,
            last_u: 0,
            accepted: Vec::new(),
            gap_snapshot: Vec::new(),
        })
    }

    pub const fn phase(&self) -> Phase {
        self.phase
    }

    pub const fn last_u(&self) -> u64 {
        self.last_u
    }

    pub fn accepted(&self) -> &[u64] {
        &self.accepted
    }

    pub fn gap_snapshot(&self) -> &[u64] {
        &self.gap_snapshot
    }

    pub const fn max(&self) -> u64 {
        self.max
    }

    /// Apply one enabled action from `IngestSegment.tla`.
    pub fn apply(&mut self, action: Action) -> Result<(), SegmentError> {
        match action {
            Action::Update(id) => self.update(id),
            Action::Resync(snapshot) => self.resync(snapshot),
        }
    }

    fn update(&mut self, id: u64) -> Result<(), SegmentError> {
        if id == 0 || id > self.max {
            return Err(SegmentError::IdOutOfRange);
        }
        match self.phase {
            Phase::Gap => Ok(()),
            Phase::Open if id <= self.last_u => Ok(()),
            Phase::Open if self.last_u.checked_add(1) == Some(id) => {
                self.last_u = id;
                self.accepted.push(id);
                Ok(())
            }
            Phase::Open => {
                self.phase = Phase::Gap;
                self.gap_snapshot.clone_from(&self.accepted);
                Ok(())
            }
        }
    }

    fn resync(&mut self, snapshot: u64) -> Result<(), SegmentError> {
        if snapshot > self.max {
            return Err(SegmentError::SnapshotOutOfRange);
        }
        match self.phase {
            Phase::Open => Err(SegmentError::ResyncWhileOpen),
            Phase::Gap => {
                self.phase = Phase::Open;
                self.last_u = snapshot;
                self.accepted.clear();
                self.gap_snapshot.clear();
                Ok(())
            }
        }
    }
}

/// State invariants from the model: `TypeOK`, `Contiguous`, `NoRepeatedId`,
/// and `GapHoldsSnapshot`.
pub fn state_invariant(segment: &Segment) -> Option<Invariant> {
    if segment.last_u > segment.max {
        return Some(Invariant::TypeOK);
    }
    if segment
        .accepted
        .iter()
        .chain(segment.gap_snapshot.iter())
        .any(|id| *id == 0 || *id > segment.max)
    {
        return Some(Invariant::TypeOK);
    }
    let mut previous: Option<u64> = None;
    for id in &segment.accepted {
        if let Some(prev) = previous {
            match prev.checked_add(1) {
                Some(next) if next == *id => {}
                _ => return Some(Invariant::Contiguous),
            }
        }
        if segment.accepted.iter().filter(|other| *other == id).count() > 1 {
            return Some(Invariant::NoRepeatedId);
        }
        previous = Some(*id);
    }
    if segment.phase == Phase::Gap && segment.accepted != segment.gap_snapshot {
        return Some(Invariant::GapHoldsSnapshot);
    }
    None
}

/// `NoAppendWhileGap`: a step that stays in `gap` does not append.
pub fn step_invariant(before: &Segment, after: &Segment) -> Option<Invariant> {
    if let Some(broken) = state_invariant(before) {
        return Some(broken);
    }
    if let Some(broken) = state_invariant(after) {
        return Some(broken);
    }
    if before.phase == Phase::Gap && after.phase == Phase::Gap && before.accepted != after.accepted
    {
        return Some(Invariant::NoAppendWhileGap);
    }
    None
}

/// Build a segment without running the transitions, so a fixture can describe
/// a state the machine must not reach.
pub fn from_parts(
    max: u64,
    phase: Phase,
    last_u: u64,
    accepted: Vec<u64>,
    gap_snapshot: Vec<u64>,
) -> Result<Segment, SegmentError> {
    if max < 2 {
        return Err(SegmentError::MaxTooSmall);
    }
    Ok(Segment {
        max,
        phase,
        last_u,
        accepted,
        gap_snapshot,
    })
}

/// Read a before/after fixture. `Ok(Some(name))` means that invariant broke.
/// `Ok(None)` means both states and the step satisfy the checked invariants.
pub fn pair_fault(text: &str) -> Result<Option<Invariant>, SegmentError> {
    let fields = parse_fields(text)?;
    let before = segment_from_prefix(&fields, "before")?;
    let after = segment_from_prefix(&fields, "after")?;
    let broken = step_invariant(&before, &after);
    let expect = fields
        .iter()
        .find(|(key, _)| key == "expect")
        .map(|(_, value)| value.as_str())
        .ok_or(SegmentError::Fixture(FixtureError::MissingField))?;
    match broken {
        Some(invariant) if invariant.to_string() == expect => Ok(Some(invariant)),
        Some(_) => Err(SegmentError::Fixture(FixtureError::ExpectMismatch)),
        None if expect == "none" => Ok(None),
        None => Err(SegmentError::Fixture(FixtureError::ExpectMismatch)),
    }
}

fn segment_from_prefix(fields: &[(String, String)], prefix: &str) -> Result<Segment, SegmentError> {
    let max = required_u64(fields, "max")?;
    let phase = match required(fields, &format!("{prefix}.phase"))?.as_str() {
        "open" => Phase::Open,
        "gap" => Phase::Gap,
        _ => return Err(SegmentError::Fixture(FixtureError::BadPhase)),
    };
    let last_u = required_u64(fields, &format!("{prefix}.last_u"))?;
    let accepted = required_ids(fields, &format!("{prefix}.accepted"))?;
    let gap_snapshot = required_ids(fields, &format!("{prefix}.gap"))?;
    from_parts(max, phase, last_u, accepted, gap_snapshot)
}

fn parse_fields(text: &str) -> Result<Vec<(String, String)>, SegmentError> {
    let mut fields = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or(SegmentError::Fixture(FixtureError::MissingField))?;
        fields.push((key.trim().to_owned(), value.trim().to_owned()));
    }
    Ok(fields)
}

fn required(fields: &[(String, String)], key: &str) -> Result<String, SegmentError> {
    fields
        .iter()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, value)| value.clone())
        .ok_or(SegmentError::Fixture(FixtureError::MissingField))
}

fn required_u64(fields: &[(String, String)], key: &str) -> Result<u64, SegmentError> {
    required(fields, key)?
        .parse()
        .map_err(|_| SegmentError::Fixture(FixtureError::BadInteger))
}

fn required_ids(fields: &[(String, String)], key: &str) -> Result<Vec<u64>, SegmentError> {
    let text = required(fields, key)?;
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|part| {
            part.parse()
                .map_err(|_| SegmentError::Fixture(FixtureError::BadInteger))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{merge_fact, Slot};
    use parangsae_core::{ReceivedTs, UtcMicros};
    use proptest::prelude::*;
    use proptest::test_runner::TestCaseError;

    fn apply_enabled(segment: &mut Segment, byte: u8) -> Result<(), SegmentError> {
        let mut actions = Vec::new();
        for id in 1..=segment.max() {
            actions.push(Action::Update(id));
        }
        if segment.phase() == Phase::Gap {
            for snapshot in 0..=segment.max() {
                actions.push(Action::Resync(snapshot));
            }
        }
        let index = usize::from(byte) % actions.len();
        segment.apply(actions[index])
    }

    #[test]
    fn accept_then_duplicate_then_break_then_reject_then_resync() -> Result<(), SegmentError> {
        let mut segment = Segment::init(4)?;
        segment.apply(Action::Update(1))?;
        assert_eq!(segment.accepted(), &[1]);
        let after_accept = segment.clone();
        segment.apply(Action::Update(1))?;
        assert_eq!(segment, after_accept);
        segment.apply(Action::Update(3))?;
        assert_eq!(segment.phase(), Phase::Gap);
        assert_eq!(segment.accepted(), &[1]);
        assert_eq!(segment.gap_snapshot(), &[1]);
        let frozen = segment.clone();
        segment.apply(Action::Update(2))?;
        assert_eq!(segment.accepted(), frozen.accepted());
        assert!(step_invariant(&frozen, &segment).is_none());
        segment.apply(Action::Resync(0))?;
        assert_eq!(segment.phase(), Phase::Open);
        assert_eq!(segment.last_u(), 0);
        assert!(segment.accepted().is_empty());
        Ok(())
    }

    #[test]
    fn resync_while_open_is_not_enabled() -> Result<(), SegmentError> {
        let mut segment = Segment::init(4)?;
        assert_eq!(
            segment.apply(Action::Resync(0)),
            Err(SegmentError::ResyncWhileOpen)
        );
        Ok(())
    }

    #[test]
    fn append_while_gap_fixture_breaks_no_append_while_gap() {
        let text = include_str!("../../../fixtures/guards/segment/append_while_gap.txt");
        assert_eq!(pair_fault(text), Ok(Some(Invariant::NoAppendWhileGap)));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn trace_holds_contiguous_no_append_while_gap_and_stored_stable(
            bytes in prop::collection::vec(any::<u8>(), 0..32)
        ) {
            let mut segment = Segment::init(4).map_err(|err| TestCaseError::fail(err.to_string()))?;
            let mut slot = Slot::Absent;
            let mut first_stored: Option<u8> = None;
            let received = ReceivedTs::from_micros(UtcMicros::from_micros(1));
            for byte in bytes {
                let before = segment.clone();
                apply_enabled(&mut segment, byte).map_err(|err| TestCaseError::fail(err.to_string()))?;
                prop_assert!(step_invariant(&before, &segment).is_none());
                let value = byte % 6;
                merge_fact(&mut slot, value, received);
                match first_stored {
                    None => first_stored = Some(value),
                    Some(first) => {
                        let stored = slot.stored().map(|row| row.value);
                        prop_assert_eq!(stored, Some(first));
                    }
                }
            }
        }
    }
}
