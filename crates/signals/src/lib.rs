//! Signal entry point.
//!
//! The argument is [`Causal`], so a caller cannot pass a row whose
//! `available_ts` is after the decision time. No signal generator is implemented.

#![forbid(unsafe_code)]

use parangsae_core::Causal;

pub fn sample_value<'a, T>(sample: &Causal<'a, T>) -> &'a T {
    sample.value()
}
