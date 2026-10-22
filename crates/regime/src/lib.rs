//! Regime is a permission layer. It is not evidence of edge.
//!
//! No classifier is implemented in this scaffold.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Allow,
    StandDown,
}
