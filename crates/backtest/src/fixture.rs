//! Test fixtures. These are not strategies and they are not evidence of edge.

use parangsae_core::{Qty, Side};
use parangsae_sim::MarketOrder;

use crate::{BarView, Strategy};

#[derive(Clone, Copy, Debug, Default)]
pub struct AlwaysFlat;

impl Strategy for AlwaysFlat {
    fn on_bar(&mut self, _view: BarView<'_>) -> Option<MarketOrder> {
        None
    }
}

/// Buys `qty` on the first bar and then holds.
#[derive(Clone, Copy, Debug)]
pub struct BuyAndHold {
    qty: Qty,
    placed: bool,
}

impl BuyAndHold {
    pub fn new(qty: Qty) -> Self {
        Self { qty, placed: false }
    }
}

impl Strategy for BuyAndHold {
    fn on_bar(&mut self, _view: BarView<'_>) -> Option<MarketOrder> {
        if self.placed {
            return None;
        }
        self.placed = true;
        Some(MarketOrder {
            side: Side::Buy,
            qty: self.qty,
        })
    }
}

/// Alternates buy and sell of `qty` every `n` bars, starting with a buy.
#[derive(Clone, Copy, Debug)]
pub struct FlipEveryN {
    n: u64,
    qty: Qty,
    seen: u64,
}

impl FlipEveryN {
    pub fn new(n: u64, qty: Qty) -> Self {
        Self { n, qty, seen: 0 }
    }
}

impl Strategy for FlipEveryN {
    fn on_bar(&mut self, _view: BarView<'_>) -> Option<MarketOrder> {
        let index = self.seen;
        self.seen = self.seen.saturating_add(1);
        if self.n == 0 || !index.is_multiple_of(self.n) {
            return None;
        }
        let side = if (index / self.n).is_multiple_of(2) {
            Side::Buy
        } else {
            Side::Sell
        };
        Some(MarketOrder {
            side,
            qty: self.qty,
        })
    }
}

/// Plays a script of orders, one per bar. A missing entry is no order.
#[derive(Clone, Debug)]
pub struct Scripted {
    orders: Vec<Option<MarketOrder>>,
    next: usize,
}

impl Scripted {
    pub fn new(orders: Vec<Option<MarketOrder>>) -> Self {
        Self { orders, next: 0 }
    }
}

impl Strategy for Scripted {
    fn on_bar(&mut self, _view: BarView<'_>) -> Option<MarketOrder> {
        let order = self.orders.get(self.next).copied().flatten();
        self.next = self.next.saturating_add(1);
        order
    }
}

/// What one `on_bar` call was allowed to see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub decision_us: i64,
    pub open_us: Vec<i64>,
    pub available_us: Vec<i64>,
    pub close_ticks: Vec<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct Recorder {
    pub calls: Vec<Seen>,
}

impl Strategy for Recorder {
    fn on_bar(&mut self, view: BarView<'_>) -> Option<MarketOrder> {
        self.calls.push(Seen {
            decision_us: view.decision().micros(),
            open_us: view.iter().map(|bar| bar.open_time.micros()).collect(),
            available_us: view.iter().map(|bar| bar.available.micros()).collect(),
            close_ticks: view.iter().map(|bar| bar.close.ticks()).collect(),
        });
        None
    }
}
