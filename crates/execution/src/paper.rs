//! Paper venue stub.
//!
//! `submit` refuses every order. The refusal is definitive: the order is
//! rejected and its reservation is released. This is not a working paper adapter.
//!
//! The intended fill, once the paper-driver names are pinned, is
//! `sim::Ledger::apply_fill` with a `MarketOrder`, a `Price`, `&Costs`, and a
//! `UtcMicros`. That call needs an explicit conversion to `sim::Side`. `core`
//! does not export a side type, so the conversion is not written here. The
//! bijection from `ClientOrderId` to a sim ledger key is not pinned either.
//!
//! Outside this crate the venue trait cannot be implemented:
//!
//! ```compile_fail
//! struct Other;
//! impl parangsae_execution::Venue for Other {
//!     fn submit(
//!         &self,
//!         _order: parangsae_execution::PaperOrder,
//!     ) -> Result<parangsae_execution::PaperAck, parangsae_execution::PaperRefusal> {
//!         Err(parangsae_execution::PaperRefusal {
//!             error: parangsae_execution::VenueError::PaperNotWired,
//!             state: parangsae_execution::OrderState::Rejected,
//!             reservation: parangsae_execution::Reservation::Released,
//!         })
//!     }
//! }
//! ```

use parangsae_core::Qty;

use crate::ids::ClientOrderId;
use crate::lifecycle::{OrderState, ProgressError};

mod seal {
    pub trait Sealed {}
}

/// In-process venue. The only implementation is [`PaperVenue`].
pub trait Venue: seal::Sealed {
    fn submit(&self, order: PaperOrder) -> Result<PaperAck, PaperRefusal>;
}

/// Offline paper boundary. It never contacts a venue.
#[derive(Debug, Default)]
pub struct PaperVenue {
    _private: (),
}

impl PaperVenue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn submit(&self, order: PaperOrder) -> Result<PaperAck, PaperRefusal> {
        Venue::submit(self, order)
    }

    /// Sim-key lookup. No bijection is pinned, so this always refuses.
    pub fn sim_key(&self, id: &ClientOrderId) -> Result<SimKey, VenueError> {
        let _ = id;
        Err(VenueError::PaperNotWired)
    }
}

impl seal::Sealed for PaperVenue {}

impl Venue for PaperVenue {
    fn submit(&self, order: PaperOrder) -> Result<PaperAck, PaperRefusal> {
        let _ = (self._private, order);
        Err(PaperRefusal {
            error: VenueError::PaperNotWired,
            state: OrderState::Rejected,
            reservation: Reservation::Released,
        })
    }
}

/// Success is impossible while the adapter is unwired.
#[derive(Debug)]
pub enum PaperAck {}

/// Sim ledger key. No value exists until the bijection is pinned.
#[derive(Debug)]
pub enum SimKey {}

/// An order handed to the paper stub. The reservation starts held.
#[derive(Debug)]
pub struct PaperOrder {
    client_order_id: ClientOrderId,
    qty: Qty,
    reservation: Reservation,
}

impl PaperOrder {
    pub fn new(client_order_id: ClientOrderId, qty: Qty) -> Result<Self, ProgressError> {
        if qty.ticks() <= 0 {
            return Err(ProgressError::InvalidQuantity);
        }
        Ok(Self {
            client_order_id,
            qty,
            reservation: Reservation::Held,
        })
    }

    pub fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    pub const fn qty(&self) -> Qty {
        self.qty
    }

    pub const fn reservation(&self) -> Reservation {
        self.reservation
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reservation {
    Held,
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VenueError {
    PaperNotWired,
}

impl std::fmt::Display for VenueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PaperNotWired => write!(f, "paper venue is not wired"),
        }
    }
}

impl std::error::Error for VenueError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaperRefusal {
    pub error: VenueError,
    pub state: OrderState,
    pub reservation: Reservation,
}
