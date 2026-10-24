//! Strict journal boundary. Identifiers stay canonical; money stays integer ticks.

use parangsae_core::{
    AvailableTs, DecisionTime, EventTs, Observation, Price, Qty, ReceivedTs, Side, Symbol,
    Timestamps, UtcMicros,
};
use serde::{Deserialize, Serialize};

use super::{PaperAction, PaperCommand, PaperFill, PaperReport, PaperTerminal, PaperTerminalCut};
use crate::{ClientOrderId, HaltCause, Mono};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Command {
    mono_us: u64,
    decision_us: i64,
    mark: Observed<i64>,
    action: Action,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Clocks {
    event_us: i64,
    available_us: i64,
    received_us: i64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observed<T> {
    timestamps: Clocks,
    value: T,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Plan {
        client_order_id: String,
        symbol: String,
        side: Direction,
        qty_ticks: i64,
        reduce_only: bool,
    },
    Dispatch {
        client_order_id: String,
    },
    Cancel {
        client_order_id: String,
    },
    Deliver {
        report: Observed<Report>,
    },
    Timeout {
        client_order_id: String,
    },
    Reconcile {
        cut: Observed<TerminalCut>,
    },
    ReconcilePortfolio,
    Halt {
        cause: Cause,
    },
    Resume,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Direction {
    Buy,
    Sell,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Report {
    Ack { client_order_id: String },
    Fill { fill: Fill },
    CancelConfirmed { client_order_id: String },
    CancelDenied { client_order_id: String },
    Expired { client_order_id: String },
    Rejected { client_order_id: String },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fill {
    client_order_id: String,
    fill_id: u64,
    qty_ticks: i64,
    reference_price_ticks: i64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Terminal {
    Filled,
    Canceled,
    Expired,
    Rejected,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminalCut {
    client_order_id: String,
    evidence_id: u64,
    status: Terminal,
    cumulative_ticks: i64,
    receipts: Vec<Observed<Fill>>,
    complete: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Cause {
    ReconMismatch,
    UnknownOrder,
    Kill,
    Ban,
    RiskTrip,
    IllegalTransition,
    PayloadError,
}

impl From<Timestamps> for Clocks {
    fn from(value: Timestamps) -> Self {
        Self {
            event_us: value.event_ts.micros().micros(),
            available_us: value.available_ts.micros().micros(),
            received_us: value.received_ts.micros().micros(),
        }
    }
}

impl From<Clocks> for Timestamps {
    fn from(value: Clocks) -> Self {
        Self {
            event_ts: EventTs::from_micros(UtcMicros::from_micros(value.event_us)),
            available_ts: AvailableTs::from_micros(UtcMicros::from_micros(value.available_us)),
            received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(value.received_us)),
        }
    }
}

fn observe<T, U>(value: &Observation<T>, convert: impl FnOnce(&T) -> U) -> Observed<U> {
    Observed {
        timestamps: value.timestamps().into(),
        value: convert(value.value()),
    }
}

fn restore<T, U>(
    value: Observed<T>,
    convert: impl FnOnce(T) -> Result<U, String>,
) -> Result<Observation<U>, String> {
    Ok(Observation::new(
        value.timestamps.into(),
        convert(value.value)?,
    ))
}

fn cid(value: String) -> Result<ClientOrderId, String> {
    ClientOrderId::parse(&value).map_err(|error| error.to_string())
}

fn qty(value: i64) -> Result<Qty, String> {
    Qty::from_ticks(value).map_err(|error| error.to_string())
}

fn positive_qty(value: i64) -> Result<Qty, String> {
    if value <= 0 {
        return Err("paper quantity must be positive".into());
    }
    qty(value)
}

fn price(value: i64) -> Result<Price, String> {
    if value <= 0 {
        return Err("paper price must be positive".into());
    }
    Price::from_ticks(value).map_err(|error| error.to_string())
}

impl From<PaperCommand> for Command {
    fn from(value: PaperCommand) -> Self {
        let action = match value.action {
            PaperAction::Plan {
                client_order_id,
                symbol,
                side,
                qty,
                reduce_only,
            } => Action::Plan {
                client_order_id: client_order_id.to_string(),
                symbol: symbol.to_string(),
                side: match side {
                    Side::Buy => Direction::Buy,
                    Side::Sell => Direction::Sell,
                },
                qty_ticks: qty.ticks(),
                reduce_only,
            },
            PaperAction::Dispatch { client_order_id } => Action::Dispatch {
                client_order_id: client_order_id.to_string(),
            },
            PaperAction::Cancel { client_order_id } => Action::Cancel {
                client_order_id: client_order_id.to_string(),
            },
            PaperAction::Deliver(report) => Action::Deliver {
                report: observe(&report, |value| Report::from(value)),
            },
            PaperAction::Timeout { client_order_id } => Action::Timeout {
                client_order_id: client_order_id.to_string(),
            },
            PaperAction::Reconcile(cut) => Action::Reconcile {
                cut: observe(&cut, |value| TerminalCut::from(value)),
            },
            PaperAction::ReconcilePortfolio => Action::ReconcilePortfolio,
            PaperAction::Halt(cause) => Action::Halt {
                cause: cause.into(),
            },
            PaperAction::Resume => Action::Resume,
        };
        Self {
            mono_us: value.now.micros(),
            decision_us: value.decision.micros().micros(),
            mark: observe(&value.mark, |mark| mark.ticks()),
            action,
        }
    }
}

impl TryFrom<Command> for PaperCommand {
    type Error = String;
    fn try_from(value: Command) -> Result<Self, Self::Error> {
        let action = match value.action {
            Action::Plan {
                client_order_id,
                symbol,
                side,
                qty_ticks,
                reduce_only,
            } => PaperAction::Plan {
                client_order_id: cid(client_order_id)?,
                symbol: Symbol::new(&symbol).map_err(|error| error.to_string())?,
                side: match side {
                    Direction::Buy => Side::Buy,
                    Direction::Sell => Side::Sell,
                },
                qty: positive_qty(qty_ticks)?,
                reduce_only,
            },
            Action::Dispatch { client_order_id } => PaperAction::Dispatch {
                client_order_id: cid(client_order_id)?,
            },
            Action::Cancel { client_order_id } => PaperAction::Cancel {
                client_order_id: cid(client_order_id)?,
            },
            Action::Deliver { report } => {
                PaperAction::Deliver(restore(report, PaperReport::try_from)?)
            }
            Action::Timeout { client_order_id } => PaperAction::Timeout {
                client_order_id: cid(client_order_id)?,
            },
            Action::Reconcile { cut } => {
                PaperAction::Reconcile(restore(cut, PaperTerminalCut::try_from)?)
            }
            Action::ReconcilePortfolio => PaperAction::ReconcilePortfolio,
            Action::Halt { cause } => PaperAction::Halt(cause.into()),
            Action::Resume => PaperAction::Resume,
        };
        Ok(Self {
            now: Mono::from_micros(value.mono_us),
            decision: DecisionTime::from_micros(UtcMicros::from_micros(value.decision_us)),
            mark: restore(value.mark, price)?,
            action,
        })
    }
}

impl From<&PaperFill> for Fill {
    fn from(value: &PaperFill) -> Self {
        Self {
            client_order_id: value.client_order_id.to_string(),
            fill_id: value.fill_id,
            qty_ticks: value.qty.ticks(),
            reference_price_ticks: value.reference_price.ticks(),
        }
    }
}

impl TryFrom<Fill> for PaperFill {
    type Error = String;
    fn try_from(value: Fill) -> Result<Self, Self::Error> {
        if value.fill_id == 0 {
            return Err("paper fill id must be nonzero".into());
        }
        Ok(Self {
            client_order_id: cid(value.client_order_id)?,
            fill_id: value.fill_id,
            qty: positive_qty(value.qty_ticks)?,
            reference_price: price(value.reference_price_ticks)?,
        })
    }
}

impl From<&PaperReport> for Report {
    fn from(value: &PaperReport) -> Self {
        match value {
            PaperReport::Ack { client_order_id } => Self::Ack {
                client_order_id: client_order_id.to_string(),
            },
            PaperReport::Fill(fill) => Self::Fill { fill: fill.into() },
            PaperReport::CancelConfirmed { client_order_id } => Self::CancelConfirmed {
                client_order_id: client_order_id.to_string(),
            },
            PaperReport::CancelDenied { client_order_id } => Self::CancelDenied {
                client_order_id: client_order_id.to_string(),
            },
            PaperReport::Expired { client_order_id } => Self::Expired {
                client_order_id: client_order_id.to_string(),
            },
            PaperReport::Rejected { client_order_id } => Self::Rejected {
                client_order_id: client_order_id.to_string(),
            },
        }
    }
}

impl TryFrom<Report> for PaperReport {
    type Error = String;
    fn try_from(value: Report) -> Result<Self, Self::Error> {
        Ok(match value {
            Report::Ack { client_order_id } => Self::Ack {
                client_order_id: cid(client_order_id)?,
            },
            Report::Fill { fill } => Self::Fill(fill.try_into()?),
            Report::CancelConfirmed { client_order_id } => Self::CancelConfirmed {
                client_order_id: cid(client_order_id)?,
            },
            Report::CancelDenied { client_order_id } => Self::CancelDenied {
                client_order_id: cid(client_order_id)?,
            },
            Report::Expired { client_order_id } => Self::Expired {
                client_order_id: cid(client_order_id)?,
            },
            Report::Rejected { client_order_id } => Self::Rejected {
                client_order_id: cid(client_order_id)?,
            },
        })
    }
}

impl From<&PaperTerminalCut> for TerminalCut {
    fn from(value: &PaperTerminalCut) -> Self {
        Self {
            client_order_id: value.client_order_id.to_string(),
            evidence_id: value.evidence_id,
            status: match value.status {
                PaperTerminal::Filled => Terminal::Filled,
                PaperTerminal::Canceled => Terminal::Canceled,
                PaperTerminal::Expired => Terminal::Expired,
                PaperTerminal::Rejected => Terminal::Rejected,
            },
            cumulative_ticks: value.cumulative.ticks(),
            receipts: value
                .receipts
                .iter()
                .map(|receipt| observe(receipt, |value| Fill::from(value)))
                .collect(),
            complete: value.complete,
        }
    }
}

impl TryFrom<TerminalCut> for PaperTerminalCut {
    type Error = String;
    fn try_from(value: TerminalCut) -> Result<Self, Self::Error> {
        Ok(Self {
            client_order_id: cid(value.client_order_id)?,
            evidence_id: value.evidence_id,
            status: match value.status {
                Terminal::Filled => PaperTerminal::Filled,
                Terminal::Canceled => PaperTerminal::Canceled,
                Terminal::Expired => PaperTerminal::Expired,
                Terminal::Rejected => PaperTerminal::Rejected,
            },
            cumulative: qty(value.cumulative_ticks)?,
            receipts: value
                .receipts
                .into_iter()
                .map(|receipt| restore(receipt, PaperFill::try_from))
                .collect::<Result<_, _>>()?,
            complete: value.complete,
        })
    }
}

impl From<HaltCause> for Cause {
    fn from(value: HaltCause) -> Self {
        match value {
            HaltCause::ReconMismatch => Self::ReconMismatch,
            HaltCause::UnknownOrder => Self::UnknownOrder,
            HaltCause::Kill => Self::Kill,
            HaltCause::Ban => Self::Ban,
            HaltCause::RiskTrip => Self::RiskTrip,
            HaltCause::IllegalTransition => Self::IllegalTransition,
            HaltCause::PayloadError => Self::PayloadError,
        }
    }
}

impl From<Cause> for HaltCause {
    fn from(value: Cause) -> Self {
        match value {
            Cause::ReconMismatch => Self::ReconMismatch,
            Cause::UnknownOrder => Self::UnknownOrder,
            Cause::Kill => Self::Kill,
            Cause::Ban => Self::Ban,
            Cause::RiskTrip => Self::RiskTrip,
            Cause::IllegalTransition => Self::IllegalTransition,
            Cause::PayloadError => Self::PayloadError,
        }
    }
}
