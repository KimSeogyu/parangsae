//! Typed inputs and observations for one bounded offline paper session.
//!
//! Inputs carry data, never an approval credential. The engine checks each
//! request through its own RiskGate before paper dispatch.

use parangsae_core::{DecisionTime, Observation, Price, Qty, Side, Symbol};

use crate::{ClientOrderId, HaltCause, Mono, OrderProgress};

mod wire;

/// One deterministic delivery. The monotonic and UTC clocks remain distinct.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "wire::Command", into = "wire::Command")]
pub struct PaperCommand {
    pub now: Mono,
    pub decision: DecisionTime,
    pub mark: Observation<Price>,
    pub action: PaperAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaperAction {
    Plan {
        client_order_id: ClientOrderId,
        symbol: Symbol,
        side: Side,
        qty: Qty,
        reduce_only: bool,
    },
    Dispatch {
        client_order_id: ClientOrderId,
    },
    Cancel {
        client_order_id: ClientOrderId,
    },
    Deliver(Observation<PaperReport>),
    Timeout {
        client_order_id: ClientOrderId,
    },
    Reconcile(Observation<PaperTerminalCut>),
    ReconcilePortfolio,
    Halt(HaltCause),
    Resume {
        expected_epoch: u64,
    },
}

/// A paper fill references the explicit source price before configured costs.
/// The engine applies slippage and the taker fee exactly once through Ledger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaperFill {
    pub client_order_id: ClientOrderId,
    pub fill_id: u64,
    pub qty: Qty,
    pub reference_price: Price,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaperReport {
    Ack { client_order_id: ClientOrderId },
    Fill(PaperFill),
    CancelConfirmed { client_order_id: ClientOrderId },
    CancelDenied { client_order_id: ClientOrderId },
    Expired { client_order_id: ClientOrderId },
    Rejected { client_order_id: ClientOrderId },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperTerminal {
    Filled,
    Canceled,
    Expired,
    Rejected,
}

/// Complete priced evidence supplied by the explicitly bounded paper source.
/// A quantity-only snapshot cannot settle an unaccounted fill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaperTerminalCut {
    pub client_order_id: ClientOrderId,
    pub evidence_id: u64,
    pub status: PaperTerminal,
    pub cumulative: Qty,
    pub receipts: Vec<Observation<PaperFill>>,
    pub complete: bool,
}

/// A copied observation of an order, without mutable engine authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaperOrderSnapshot {
    pub client_order_id: ClientOrderId,
    pub progress: OrderProgress,
}
