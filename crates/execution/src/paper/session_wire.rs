use super::PaperSession;
use crate::{
    ApprovalTiming, ExecutionConfig, MaxAttempts, Mono, NotFoundAuthority, PositiveMicros,
};
use parangsae_core::{
    Amount, AvailableTs, DecisionTime, EventTs, Observation, Price, Qty, ReceivedTs, Symbol,
    Timestamps, UtcMicros,
};
use parangsae_risk::Limits;
use parangsae_sim::{Cash, Costs, FeeBps, SlippageBps};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Session {
    symbol: String,
    initial_cash_ticks: i64,
    initial_equity_ticks: i64,
    initial_mark: Mark,
    initial_decision_us: i64,
    initial_mono_us: u64,
    costs: Cost,
    limits: Limit,
    execution: Execution,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mark {
    event_us: i64,
    available_us: i64,
    received_us: i64,
    price_ticks: i64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cost {
    taker_fee_bps: u32,
    slippage_bps: u32,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Limit {
    max_order_ticks: i64,
    max_position_ticks: i64,
    max_notional_ticks: i64,
    max_session_loss_ticks: i64,
    max_approval_age_us: u64,
    max_mark_age_us: u64,
    capacity: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Execution {
    max_dispatch_latency_us: u64,
    recv_window_ms: u32,
    margin_ms: u32,
    max_attempts: u8,
}

impl From<PaperSession> for Session {
    fn from(value: PaperSession) -> Self {
        let stamps = value.initial_mark.timestamps();
        Self {
            symbol: value.symbol.to_string(),
            initial_cash_ticks: value.initial_cash.0,
            initial_equity_ticks: value.initial_equity.0,
            initial_mark: Mark {
                event_us: stamps.event_ts.micros().micros(),
                available_us: stamps.available_ts.micros().micros(),
                received_us: stamps.received_ts.micros().micros(),
                price_ticks: value.initial_mark.value().ticks(),
            },
            initial_decision_us: value.initial_decision.micros().micros(),
            initial_mono_us: value.initial_now.micros(),
            costs: Cost {
                taker_fee_bps: value.costs.taker_fee.0,
                slippage_bps: value.costs.slippage.0,
            },
            limits: Limit {
                max_order_ticks: value.limits.max_order().ticks(),
                max_position_ticks: value.limits.max_position().ticks(),
                max_notional_ticks: value.limits.max_notional().ticks(),
                max_session_loss_ticks: value.limits.max_session_loss().ticks(),
                max_approval_age_us: value.limits.max_approval_age_us(),
                max_mark_age_us: value.limits.max_mark_age_us(),
                capacity: value.limits.capacity(),
            },
            execution: Execution {
                max_dispatch_latency_us: value.execution.timing().max_dispatch_latency().get(),
                recv_window_ms: value.execution.authority().recv_window_ms(),
                margin_ms: value.execution.authority().margin_ms(),
                max_attempts: value.execution.max_attempts().get(),
            },
        }
    }
}

impl TryFrom<Session> for PaperSession {
    type Error = String;
    fn try_from(value: Session) -> Result<Self, Self::Error> {
        let limits = Limits::new(
            Qty::from_ticks(value.limits.max_order_ticks).map_err(|error| error.to_string())?,
            Qty::from_ticks(value.limits.max_position_ticks).map_err(|error| error.to_string())?,
            Amount::from_ticks(value.limits.max_notional_ticks)
                .map_err(|error| error.to_string())?,
            Amount::from_ticks(value.limits.max_session_loss_ticks)
                .map_err(|error| error.to_string())?,
            value.limits.max_approval_age_us,
            value.limits.max_mark_age_us,
            value.limits.capacity,
        )
        .map_err(|error| error.to_string())?;
        let age =
            PositiveMicros::new(limits.max_approval_age_us()).map_err(|error| error.to_string())?;
        let timing = ApprovalTiming::new(
            age,
            PositiveMicros::new(value.execution.max_dispatch_latency_us)
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let authority =
            NotFoundAuthority::new(value.execution.recv_window_ms, value.execution.margin_ms)
                .map_err(|error| error.to_string())?;
        let execution = ExecutionConfig::new(
            timing,
            authority,
            MaxAttempts::new(value.execution.max_attempts).map_err(|error| error.to_string())?,
            age,
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            symbol: Symbol::new(&value.symbol).map_err(|error| error.to_string())?,
            initial_cash: Cash(value.initial_cash_ticks),
            initial_equity: Cash(value.initial_equity_ticks),
            initial_mark: Observation::new(
                Timestamps {
                    event_ts: EventTs::from_micros(UtcMicros::from_micros(
                        value.initial_mark.event_us,
                    )),
                    available_ts: AvailableTs::from_micros(UtcMicros::from_micros(
                        value.initial_mark.available_us,
                    )),
                    received_ts: ReceivedTs::from_micros(UtcMicros::from_micros(
                        value.initial_mark.received_us,
                    )),
                },
                Price::from_ticks(value.initial_mark.price_ticks)
                    .map_err(|error| error.to_string())?,
            ),
            initial_decision: DecisionTime::from_micros(UtcMicros::from_micros(
                value.initial_decision_us,
            )),
            initial_now: Mono::from_micros(value.initial_mono_us),
            costs: Costs {
                taker_fee: FeeBps(value.costs.taker_fee_bps),
                slippage: SlippageBps(value.costs.slippage_bps),
            },
            limits,
            execution,
        })
    }
}
