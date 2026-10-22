//! Mid-rank in integer ticks. There is no conversion to [`crate::Target`].

use std::cmp::Ordering;

/// 1.0 rank, in ticks. The top of the reference window.
pub const RANK_SCALE: i64 = 100_000_000;

const RANK_SCALE_U: u64 = 100_000_000;

/// `p` in `[0, 1]`, stored as ticks. No `Default`, and no path to [`crate::Target`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rank(i64);

impl Rank {
    /// Mid-rank of `cur` against `refs`.
    ///
    /// `ticks = ((2·lt + eq) · 1e8) / (2·k)` in integer arithmetic. `lt` counts
    /// strict `<`, `eq` counts `==`, both on finite f64 via `partial_cmp`.
    pub fn of(cur: f64, refs: &[f64]) -> Result<Self, RankError> {
        if refs.is_empty() {
            return Err(RankError::EmptyReference);
        }
        if !cur.is_finite() {
            return Err(RankError::NonFinite);
        }
        let mut less: u64 = 0;
        let mut equal: u64 = 0;
        for reference in refs {
            if !reference.is_finite() {
                return Err(RankError::NonFinite);
            }
            match reference.partial_cmp(&cur) {
                Some(Ordering::Less) => less += 1,
                Some(Ordering::Equal) => equal += 1,
                Some(Ordering::Greater) => {}
                None => return Err(RankError::NonFinite),
            }
        }
        let k = u64::try_from(refs.len()).map_err(|_| RankError::Overflow)?;
        let numer = less
            .checked_mul(2)
            .and_then(|value| value.checked_add(equal))
            .and_then(|value| value.checked_mul(RANK_SCALE_U))
            .ok_or(RankError::Overflow)?;
        let denom = k.checked_mul(2).ok_or(RankError::Overflow)?;
        let ticks = numer / denom;
        let ticks = i64::try_from(ticks).map_err(|_| RankError::Overflow)?;
        if ticks > RANK_SCALE {
            return Err(RankError::Overflow);
        }
        Ok(Self(ticks))
    }

    pub const fn ticks(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankError {
    NonFinite,
    EmptyReference,
    Overflow,
}

impl std::fmt::Display for RankError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite => write!(f, "rank input is not finite"),
            Self::EmptyReference => write!(f, "rank reference window is empty"),
            Self::Overflow => write!(f, "rank count overflowed"),
        }
    }
}
