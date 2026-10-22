//! Phase names for a sequenced feed such as depth diff.
//!
//! The transition rules are the TLC-checked model in `specs/tla/IngestSegment.tla`:
//! a contiguous update id is accepted, a duplicate id does not extend the
//! segment, and a sequence break moves to [`SegmentPhase::Gap`] which accepts
//! nothing until resync. This module does not implement those transitions.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentPhase {
    Open { last_u: u64 },
    Gap { last_u: u64 },
}
