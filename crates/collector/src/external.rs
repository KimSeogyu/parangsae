//! Cross-venue public sources named by the data plan. No client in this scaffold.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalSource {
    Bybit,
    Okx,
    Deribit,
}
