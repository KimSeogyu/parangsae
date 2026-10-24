//! Order lifecycle. The only state change is [`transition`].
//!
//! Fields of [`OrderProgress`] stay private. A checkpoint validates the
//! numbers, rejects `Acked` without an ack, and keeps fill ids. It does not clamp.

use std::fmt;

use parangsae_core::{FixedError, Qty};

use crate::authority::{Elapsed, Mono, NotFoundAuthority};
use crate::ids::{Attempt, MaxAttempts};

/// Evidence buffer cap. A further distinct fill id is rejected, not evicted.
const MAX_FILL_IDS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OrderState {
    PendingNew,
    Acked,
    PartiallyFilled,
    Filled,
    PendingCancel,
    Canceled,
    Rejected,
    Expired,
    Unknown,
}

impl OrderState {
    pub const fn name(self) -> &'static str {
        match self {
            Self::PendingNew => "PendingNew",
            Self::Acked => "Acked",
            Self::PartiallyFilled => "PartiallyFilled",
            Self::Filled => "Filled",
            Self::PendingCancel => "PendingCancel",
            Self::Canceled => "Canceled",
            Self::Rejected => "Rejected",
            Self::Expired => "Expired",
            Self::Unknown => "Unknown",
        }
    }
}

impl fmt::Display for OrderState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    Ack,
    Fill,
    CancelRequested,
    CancelDenied,
    CancelConfirmed,
    Reject,
    Expire,
    Timeout,
    QueryFilled,
    QueryCanceled,
    QueryExpired,
    QueryNotFound,
    RaceFill,
}

impl Event {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ack => "Ack",
            Self::Fill => "Fill",
            Self::CancelRequested => "CancelRequested",
            Self::CancelDenied => "CancelDenied",
            Self::CancelConfirmed => "CancelConfirmed",
            Self::Reject => "Reject",
            Self::Expire => "Expire",
            Self::Timeout => "Timeout",
            Self::QueryFilled => "QueryFilled",
            Self::QueryCanceled => "QueryCanceled",
            Self::QueryExpired => "QueryExpired",
            Self::QueryNotFound => "QueryNotFound",
            Self::RaceFill => "RaceFill",
        }
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One order. Construct it with [`OrderProgress::new_pending`] or
/// [`OrderProgress::from_checkpoint`]. There is no setter and no `Default`.
///
/// External code cannot write a field:
///
/// ```compile_fail
/// use parangsae_core::Qty;
/// use parangsae_execution::{OrderProgress, OrderState};
/// let qty = Qty::from_ticks(1).unwrap();
/// let mut progress = OrderProgress::new_pending(qty).unwrap();
/// progress.state = OrderState::Filled;
/// ```
///
/// External code cannot build the struct by literal. Every field is named,
/// so this fails because the fields are private:
///
/// ```compile_fail
/// use parangsae_core::Qty;
/// use parangsae_execution::{Attempt, OrderProgress, OrderState};
/// let _progress = OrderProgress {
///     state: OrderState::PendingNew,
///     qty: Qty::from_ticks(1).unwrap(),
///     cum: Qty::from_ticks(0).unwrap(),
///     attempt: Attempt::FIRST,
///     sent: false,
///     acked: false,
///     last_sent_at: None,
///     fill_ids: Vec::<u64>::new(),
///     frozen_cum: None,
/// };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderProgress {
    state: OrderState,
    qty: Qty,
    cum: Qty,
    attempt: Attempt,
    sent: bool,
    acked: bool,
    last_sent_at: Option<Mono>,
    fill_ids: Vec<u64>,
    frozen_cum: Option<Qty>,
}

/// Numbers for a checkpoint outside this crate.
/// [`OrderProgress::from_checkpoint`] rejects an `Acked` order that was never
/// acked and keeps the fill ids on the order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub state: OrderState,
    pub qty: Qty,
    pub cum: Qty,
    pub attempt: Attempt,
    pub max_attempts: MaxAttempts,
    pub sent: bool,
    pub acked: bool,
    pub last_sent_at: Option<Mono>,
    pub fill_ids: Vec<u64>,
}

/// In-crate checkpoint. Not a public constructor: external callers use
/// [`Checkpoint`] and [`OrderProgress::from_checkpoint`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProgressRestore {
    pub(crate) state: OrderState,
    pub(crate) qty: Qty,
    pub(crate) cum: Qty,
    pub(crate) attempt: Attempt,
    pub(crate) max_attempts: MaxAttempts,
    pub(crate) sent: bool,
    pub(crate) acked: bool,
    pub(crate) last_sent_at: Option<Mono>,
    pub(crate) fill_ids: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressError {
    InvalidQuantity,
    Overfill,
    AttemptOutOfRange,
    InconsistentState,
}

impl fmt::Display for ProgressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidQuantity => write!(f, "order quantity is not positive"),
            Self::Overfill => write!(f, "cumulative fill exceeds order quantity"),
            Self::AttemptOutOfRange => write!(f, "attempt is outside the configured budget"),
            Self::InconsistentState => {
                write!(f, "acked state requires the order to have been acked")
            }
        }
    }
}

impl std::error::Error for ProgressError {}

impl OrderProgress {
    /// Initial order: `PendingNew`, cumulative fill zero, attempt zero, unsent.
    pub fn new_pending(qty: Qty) -> Result<Self, ProgressError> {
        if qty.ticks() <= 0 {
            return Err(ProgressError::InvalidQuantity);
        }
        let cum = Qty::from_ticks(0).map_err(|_| ProgressError::InvalidQuantity)?;
        Ok(Self {
            state: OrderState::PendingNew,
            qty,
            cum,
            attempt: Attempt::FIRST,
            sent: false,
            acked: false,
            last_sent_at: None,
            fill_ids: Vec::new(),
            frozen_cum: None,
        })
    }

    /// The engine binds a validated CID attempt before Risk approval.
    pub(crate) fn new_pending_attempt(
        qty: Qty,
        attempt: Attempt,
        max: MaxAttempts,
    ) -> Result<Self, ProgressError> {
        if attempt.index() >= max.get() {
            return Err(ProgressError::AttemptOutOfRange);
        }
        let mut progress = Self::new_pending(qty)?;
        progress.attempt = attempt;
        Ok(progress)
    }

    pub fn from_checkpoint(parts: Checkpoint) -> Result<Self, ProgressError> {
        Self::restore(ProgressRestore {
            state: parts.state,
            qty: parts.qty,
            cum: parts.cum,
            attempt: parts.attempt,
            max_attempts: parts.max_attempts,
            sent: parts.sent,
            acked: parts.acked,
            last_sent_at: parts.last_sent_at,
            fill_ids: parts.fill_ids,
        })
    }

    pub(crate) fn restore(parts: ProgressRestore) -> Result<Self, ProgressError> {
        if parts.qty.ticks() <= 0 {
            return Err(ProgressError::InvalidQuantity);
        }
        if parts.cum.ticks() > parts.qty.ticks() {
            return Err(ProgressError::Overfill);
        }
        if parts.attempt.index() >= parts.max_attempts.get() {
            return Err(ProgressError::AttemptOutOfRange);
        }
        if parts.state == OrderState::Acked && !parts.acked {
            return Err(ProgressError::InconsistentState);
        }
        let frozen_cum = if terminal(parts.state) {
            Some(parts.cum)
        } else {
            None
        };
        Ok(Self {
            state: parts.state,
            qty: parts.qty,
            cum: parts.cum,
            attempt: parts.attempt,
            sent: parts.sent,
            acked: parts.acked,
            last_sent_at: parts.last_sent_at,
            fill_ids: parts.fill_ids,
            frozen_cum,
        })
    }

    pub const fn state(&self) -> OrderState {
        self.state
    }

    pub const fn qty(&self) -> Qty {
        self.qty
    }

    pub const fn cum(&self) -> Qty {
        self.cum
    }

    pub const fn attempt(&self) -> Attempt {
        self.attempt
    }

    pub const fn sent(&self) -> bool {
        self.sent
    }

    pub const fn acked(&self) -> bool {
        self.acked
    }

    pub const fn last_sent_at(&self) -> Option<Mono> {
        self.last_sent_at
    }

    /// Only the owning paper engine calls this after checking real Risk dispatch.
    /// It is deliberately unavailable to external lifecycle callers.
    pub(crate) fn record_dispatch(&self, now: Mono) -> Result<Self, TransitionError> {
        if self.state != OrderState::PendingNew || self.sent {
            return Err(TransitionError::ReportConflict);
        }
        let mut next = self.clone();
        next.sent = true;
        next.last_sent_at = Some(now);
        Ok(next)
    }

    /// Cumulative fill frozen on entry to a terminal state, including a later race fill.
    pub const fn frozen_cum(&self) -> Option<Qty> {
        self.frozen_cum
    }

    /// `qty - cum` once the order is canceled or expired. Otherwise absent.
    pub fn cancelled_remaining(&self) -> Option<Qty> {
        match self.state {
            OrderState::Canceled | OrderState::Expired => {
                let ticks = self.qty.ticks().checked_sub(self.cum.ticks())?;
                Qty::from_ticks(ticks).ok()
            }
            OrderState::PendingNew
            | OrderState::Acked
            | OrderState::PartiallyFilled
            | OrderState::Filled
            | OrderState::PendingCancel
            | OrderState::Rejected
            | OrderState::Unknown => None,
        }
    }
}

/// Clock and budgets for one cell. The book injects `now`; nothing here reads the OS.
#[derive(Clone, Copy, Debug)]
pub struct CellContext<'a> {
    pub now: Mono,
    pub authority: &'a NotFoundAuthority,
    pub max_attempts: MaxAttempts,
}

/// A venue report whose shape is already parsed. Quantity checks happen in [`transition`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidatedEvent {
    Ack,
    Fill { qty: Qty, fill_id: u64 },
    CancelRequested,
    CancelDenied,
    CancelConfirmed,
    Reject,
    Expire,
    Timeout,
    QueryFilled { cum: Qty, evidence: u64 },
    QueryCanceled { cum: Qty, evidence: u64 },
    QueryExpired { cum: Qty, evidence: u64 },
    QueryNotFound { elapsed_since_last_send: Elapsed },
    RaceFill { qty: Qty, fill_id: u64 },
}

impl ValidatedEvent {
    pub const fn kind(self) -> Event {
        match self {
            Self::Ack => Event::Ack,
            Self::Fill { .. } => Event::Fill,
            Self::CancelRequested => Event::CancelRequested,
            Self::CancelDenied => Event::CancelDenied,
            Self::CancelConfirmed => Event::CancelConfirmed,
            Self::Reject => Event::Reject,
            Self::Expire => Event::Expire,
            Self::Timeout => Event::Timeout,
            Self::QueryFilled { .. } => Event::QueryFilled,
            Self::QueryCanceled { .. } => Event::QueryCanceled,
            Self::QueryExpired { .. } => Event::QueryExpired,
            Self::QueryNotFound { .. } => Event::QueryNotFound,
            Self::RaceFill { .. } => Event::RaceFill,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionError {
    IllegalTransition { from: OrderState, event: Event },
    Overfill,
    ReportConflict,
    MissingEvidence,
    InvalidQuantity,
    InconsistentQuery,
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IllegalTransition { from, event } => {
                write!(
                    f,
                    "illegal transition from {} on {}",
                    from.name(),
                    event.name()
                )
            }
            Self::Overfill => write!(f, "fill exceeds order quantity"),
            Self::ReportConflict => write!(f, "report conflicts with recorded evidence"),
            Self::MissingEvidence => write!(f, "report is missing evidence"),
            Self::InvalidQuantity => write!(f, "quantity is not valid for this report"),
            Self::InconsistentQuery => write!(f, "query does not agree with the order"),
        }
    }
}

impl std::error::Error for TransitionError {}

/// Apply one event. On `Err`, `progress` is left as it was.
pub fn transition(
    progress: &OrderProgress,
    event: &ValidatedEvent,
    cx: &CellContext<'_>,
) -> Result<OrderProgress, TransitionError> {
    let kind = event.kind();
    if !cell_legal(progress.state, kind) {
        return Err(TransitionError::IllegalTransition {
            from: progress.state,
            event: kind,
        });
    }
    apply_legal(progress, event, cx)
}

fn apply_legal(
    progress: &OrderProgress,
    event: &ValidatedEvent,
    cx: &CellContext<'_>,
) -> Result<OrderProgress, TransitionError> {
    match event {
        ValidatedEvent::Ack => apply_ack(progress),
        ValidatedEvent::Fill { qty, fill_id } => apply_fill(progress, *qty, *fill_id, false),
        ValidatedEvent::RaceFill { qty, fill_id } => apply_fill(progress, *qty, *fill_id, true),
        ValidatedEvent::CancelRequested => apply_cancel_requested(progress),
        ValidatedEvent::CancelDenied => apply_cancel_denied(progress),
        ValidatedEvent::CancelConfirmed => apply_cancel_confirmed(progress),
        ValidatedEvent::Reject => apply_reject(progress),
        ValidatedEvent::Expire => apply_expire(progress),
        ValidatedEvent::Timeout => apply_timeout(progress),
        ValidatedEvent::QueryFilled { cum, evidence } => {
            apply_query(progress, *cum, *evidence, OrderState::Filled, true)
        }
        ValidatedEvent::QueryCanceled { cum, evidence } => {
            apply_query(progress, *cum, *evidence, OrderState::Canceled, false)
        }
        ValidatedEvent::QueryExpired { cum, evidence } => {
            apply_query(progress, *cum, *evidence, OrderState::Expired, false)
        }
        ValidatedEvent::QueryNotFound {
            elapsed_since_last_send,
        } => apply_not_found(progress, *elapsed_since_last_send, cx),
    }
}

fn apply_ack(progress: &OrderProgress) -> Result<OrderProgress, TransitionError> {
    let to = match progress.state {
        OrderState::PendingNew => OrderState::Acked,
        OrderState::Acked | OrderState::PartiallyFilled | OrderState::PendingCancel => {
            progress.state
        }
        OrderState::Filled
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired
        | OrderState::Unknown => {
            return Err(illegal(progress.state, Event::Ack));
        }
    };
    let mut next = progress.with_state(to);
    next.acked = true;
    Ok(next)
}

fn apply_fill(
    progress: &OrderProgress,
    qty: Qty,
    fill_id: u64,
    race: bool,
) -> Result<OrderProgress, TransitionError> {
    let event = if race { Event::RaceFill } else { Event::Fill };
    if fill_id == 0 {
        return Err(TransitionError::MissingEvidence);
    }
    if qty.ticks() <= 0 {
        return Err(TransitionError::InvalidQuantity);
    }
    if progress.fill_ids.contains(&fill_id) {
        return Err(TransitionError::ReportConflict);
    }
    if progress.fill_ids.len() >= MAX_FILL_IDS {
        return Err(TransitionError::ReportConflict);
    }
    let new_cum_ticks = progress
        .cum
        .ticks()
        .checked_add(qty.ticks())
        .ok_or(TransitionError::Overfill)?;
    if new_cum_ticks > progress.qty.ticks() {
        return Err(TransitionError::Overfill);
    }
    let new_cum = qty_from_ticks(new_cum_ticks)?;
    let to = fill_destination(
        progress.state,
        new_cum_ticks,
        progress.qty.ticks(),
        race,
        event,
    )?;
    let mut next = progress.with_cum_and_state(new_cum, to);
    next.fill_ids.push(fill_id);
    Ok(next)
}

fn fill_destination(
    state: OrderState,
    new_cum: i64,
    qty: i64,
    race: bool,
    event: Event,
) -> Result<OrderState, TransitionError> {
    if race {
        return match state {
            OrderState::Canceled => Ok(OrderState::Canceled),
            OrderState::PendingNew
            | OrderState::Acked
            | OrderState::PartiallyFilled
            | OrderState::Filled
            | OrderState::PendingCancel
            | OrderState::Rejected
            | OrderState::Expired
            | OrderState::Unknown => Err(illegal(state, event)),
        };
    }
    let filled = new_cum == qty;
    match state {
        OrderState::PendingCancel => Ok(if filled {
            OrderState::Filled
        } else {
            OrderState::PendingCancel
        }),
        OrderState::PendingNew | OrderState::Acked | OrderState::PartiallyFilled => Ok(if filled {
            OrderState::Filled
        } else {
            OrderState::PartiallyFilled
        }),
        OrderState::Filled
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired
        | OrderState::Unknown => Err(illegal(state, event)),
    }
}

fn apply_cancel_requested(progress: &OrderProgress) -> Result<OrderProgress, TransitionError> {
    let to = match progress.state {
        OrderState::PendingNew => {
            if progress.sent {
                OrderState::PendingCancel
            } else {
                OrderState::Canceled
            }
        }
        OrderState::Acked | OrderState::PartiallyFilled | OrderState::PendingCancel => {
            OrderState::PendingCancel
        }
        OrderState::Unknown => OrderState::Unknown,
        OrderState::Filled | OrderState::Canceled | OrderState::Rejected | OrderState::Expired => {
            return Err(illegal(progress.state, Event::CancelRequested));
        }
    };
    Ok(progress.with_state(to))
}

fn apply_cancel_denied(progress: &OrderProgress) -> Result<OrderProgress, TransitionError> {
    let to = match progress.state {
        OrderState::PendingCancel => {
            if progress.cum.ticks() > 0 {
                OrderState::PartiallyFilled
            } else if progress.acked {
                OrderState::Acked
            } else {
                OrderState::Unknown
            }
        }
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired
        | OrderState::Unknown => return Err(illegal(progress.state, Event::CancelDenied)),
    };
    Ok(progress.with_state(to))
}

fn apply_cancel_confirmed(progress: &OrderProgress) -> Result<OrderProgress, TransitionError> {
    match progress.state {
        OrderState::PendingCancel => Ok(progress.with_state(OrderState::Canceled)),
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired
        | OrderState::Unknown => Err(illegal(progress.state, Event::CancelConfirmed)),
    }
}

fn apply_expire(progress: &OrderProgress) -> Result<OrderProgress, TransitionError> {
    match progress.state {
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::PendingCancel => Ok(progress.with_state(OrderState::Expired)),
        OrderState::Filled
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired
        | OrderState::Unknown => Err(illegal(progress.state, Event::Expire)),
    }
}

fn apply_reject(progress: &OrderProgress) -> Result<OrderProgress, TransitionError> {
    match progress.state {
        OrderState::PendingNew => {}
        OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::PendingCancel
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired
        | OrderState::Unknown => return Err(illegal(progress.state, Event::Reject)),
    }
    if progress.cum.ticks() != 0 {
        return Err(TransitionError::ReportConflict);
    }
    let mut next = progress.with_state(OrderState::Rejected);
    next.sent = false;
    Ok(next)
}

fn apply_timeout(progress: &OrderProgress) -> Result<OrderProgress, TransitionError> {
    match progress.state {
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::PendingCancel
        | OrderState::Unknown => Ok(progress.with_state(OrderState::Unknown)),
        OrderState::Filled | OrderState::Canceled | OrderState::Rejected | OrderState::Expired => {
            Err(illegal(progress.state, Event::Timeout))
        }
    }
}

fn apply_query(
    progress: &OrderProgress,
    reported: Qty,
    evidence: u64,
    to: OrderState,
    require_full: bool,
) -> Result<OrderProgress, TransitionError> {
    let event = match to {
        OrderState::Filled => Event::QueryFilled,
        OrderState::Canceled => Event::QueryCanceled,
        OrderState::Expired => Event::QueryExpired,
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::PendingCancel
        | OrderState::Rejected
        | OrderState::Unknown => {
            return Err(illegal(progress.state, Event::QueryFilled));
        }
    };
    match progress.state {
        OrderState::Unknown => {}
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::PendingCancel
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired => return Err(illegal(progress.state, event)),
    }
    if evidence == 0 {
        return Err(TransitionError::MissingEvidence);
    }
    if reported.ticks() > progress.qty.ticks() {
        return Err(TransitionError::Overfill);
    }
    if reported.ticks() < progress.cum.ticks() {
        return Err(TransitionError::InconsistentQuery);
    }
    if require_full && reported.ticks() != progress.qty.ticks() {
        return Err(TransitionError::InconsistentQuery);
    }
    Ok(progress.with_cum_and_state(reported, to))
}

fn apply_not_found(
    progress: &OrderProgress,
    elapsed: Elapsed,
    cx: &CellContext<'_>,
) -> Result<OrderProgress, TransitionError> {
    match progress.state {
        OrderState::Unknown => {}
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::PendingCancel
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired => {
            return Err(illegal(progress.state, Event::QueryNotFound));
        }
    }
    let Some(sent_at) = progress.last_sent_at else {
        return Err(TransitionError::MissingEvidence);
    };
    if cx.now.micros() < sent_at.micros() {
        return Err(TransitionError::InconsistentQuery);
    }
    let measured = cx.now.micros() - sent_at.micros();
    if measured != elapsed.us() {
        return Err(TransitionError::InconsistentQuery);
    }
    if !cx.authority.is_authoritative(elapsed) {
        return Ok(progress.clone());
    }
    if progress.cum.ticks() > 0 {
        return Err(TransitionError::InconsistentQuery);
    }
    match progress.attempt.next(cx.max_attempts) {
        Some(attempt) => {
            let mut next = progress.with_state(OrderState::PendingNew);
            next.attempt = attempt;
            next.sent = false;
            next.acked = false;
            next.last_sent_at = None;
            next.fill_ids.clear();
            Ok(next)
        }
        None => {
            let mut next = progress.with_state(OrderState::Rejected);
            next.sent = false;
            Ok(next)
        }
    }
}

impl OrderProgress {
    fn with_state(&self, state: OrderState) -> Self {
        self.with_cum_and_state(self.cum, state)
    }

    fn with_cum_and_state(&self, cum: Qty, state: OrderState) -> Self {
        let mut next = self.clone();
        next.cum = cum;
        next.state = state;
        next.frozen_cum = if terminal(state) { Some(cum) } else { None };
        next
    }
}

fn cell_legal(state: OrderState, event: Event) -> bool {
    match event {
        Event::Ack | Event::Fill | Event::Expire => working(state),
        Event::CancelRequested | Event::Timeout => working_or_unknown(state),
        Event::CancelDenied | Event::CancelConfirmed => is_pending_cancel(state),
        Event::Reject => is_pending_new(state),
        Event::QueryFilled | Event::QueryCanceled | Event::QueryExpired | Event::QueryNotFound => {
            is_unknown(state)
        }
        Event::RaceFill => is_canceled(state),
    }
}

fn working(state: OrderState) -> bool {
    match state {
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::PendingCancel => true,
        OrderState::Filled
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired
        | OrderState::Unknown => false,
    }
}

fn working_or_unknown(state: OrderState) -> bool {
    match state {
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::PendingCancel
        | OrderState::Unknown => true,
        OrderState::Filled | OrderState::Canceled | OrderState::Rejected | OrderState::Expired => {
            false
        }
    }
}

fn is_pending_cancel(state: OrderState) -> bool {
    match state {
        OrderState::PendingCancel => true,
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired
        | OrderState::Unknown => false,
    }
}

fn is_pending_new(state: OrderState) -> bool {
    match state {
        OrderState::PendingNew => true,
        OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::PendingCancel
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired
        | OrderState::Unknown => false,
    }
}

fn is_unknown(state: OrderState) -> bool {
    match state {
        OrderState::Unknown => true,
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::PendingCancel
        | OrderState::Canceled
        | OrderState::Rejected
        | OrderState::Expired => false,
    }
}

fn is_canceled(state: OrderState) -> bool {
    match state {
        OrderState::Canceled => true,
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::PendingCancel
        | OrderState::Rejected
        | OrderState::Expired
        | OrderState::Unknown => false,
    }
}

fn terminal(state: OrderState) -> bool {
    match state {
        OrderState::Filled | OrderState::Canceled | OrderState::Rejected | OrderState::Expired => {
            true
        }
        OrderState::PendingNew
        | OrderState::Acked
        | OrderState::PartiallyFilled
        | OrderState::PendingCancel
        | OrderState::Unknown => false,
    }
}

fn illegal(from: OrderState, event: Event) -> TransitionError {
    TransitionError::IllegalTransition { from, event }
}

fn qty_from_ticks(ticks: i64) -> Result<Qty, TransitionError> {
    match Qty::from_ticks(ticks) {
        Ok(qty) => Ok(qty),
        Err(FixedError::Negative | FixedError::Overflow) => Err(TransitionError::InvalidQuantity),
    }
}
