//! Paper execution scaffold.
//!
//! There is no order type, no signed request, and no socket. A live or testnet
//! submit function is not defined, so this crate cannot place an order.

#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaperBook;
