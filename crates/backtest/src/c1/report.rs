use parangsae_execution::carry::{
    CarryAssessmentPoint, CarryHaltEvidence, CarryPhase, CarrySnapshot, CarryStage,
};
use parangsae_execution::HaltCause;
use parangsae_sim::carry::CarryAccountState;
use serde::Serialize;

use crate::c1_source::SourceRef;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct SourceRecord {
    pub object: String,
    pub sha256: String,
    pub row: u32,
    pub original_event_us: i64,
    pub original_available_us: i64,
    pub original_received_us: i64,
    pub raw_unit: String,
}

impl From<&SourceRef> for SourceRecord {
    fn from(value: &SourceRef) -> Self {
        Self {
            object: value.object.clone(),
            sha256: value.sha256.clone(),
            row: value.row,
            original_event_us: value.original.event_ts.micros().micros(),
            original_available_us: value.original.available_ts.micros().micros(),
            original_received_us: value.original.received_ts.micros().micros(),
            raw_unit: value.raw_unit.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct AccountRecord {
    pub spot_quote: i64,
    pub spot_btc: i64,
    pub perp_wallet: i64,
    pub perp_short: i64,
    pub idle: i64,
    pub spot_quote_fees: i64,
    pub spot_btc_fees: i64,
    pub perp_fees: i64,
    pub funding_net: i64,
    pub realized_perp: i64,
    pub perp_entry_cost_scaled: String,
    pub realized_perp_scaled: String,
}

impl From<&CarryAccountState> for AccountRecord {
    fn from(value: &CarryAccountState) -> Self {
        Self {
            spot_quote: value.spot_quote.0,
            spot_btc: value.spot_btc.ticks(),
            perp_wallet: value.perp_wallet.0,
            perp_short: value.perp_short.ticks(),
            idle: value.idle.0,
            spot_quote_fees: value.spot_quote_fees.0,
            spot_btc_fees: value.spot_btc_fees.ticks(),
            perp_fees: value.perp_fees.0,
            funding_net: value.funding_net.0,
            realized_perp: value.realized_perp.0,
            perp_entry_cost_scaled: value.perp_entry_cost_scaled.to_string(),
            realized_perp_scaled: value.realized_perp_scaled.to_string(),
        }
    }
}

/// Diagnostic data only; it cannot authorize a native command or restart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct HaltEvidenceRecord {
    pub cause: &'static str,
    pub epoch: u64,
    pub command_now_us: u64,
    pub command_decision_us: i64,
    pub phase_before_refresh: &'static str,
    pub assessment: Option<RiskAssessmentRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct RiskAssessmentRecord {
    pub point: &'static str,
    pub order_pair: Option<String>,
    pub order_stage: Option<&'static str>,
    pub order_attempt: Option<u8>,
    pub funding_key: Option<u64>,
    pub account: AccountRecord,
    pub spot_mark_ticks: i64,
    pub perp_mark_ticks: i64,
    pub equity_ticks: i64,
    pub session_loss_ticks: String,
    pub max_session_loss_ticks: i64,
    pub margin_balance_ticks: Option<String>,
    pub margin_required_ticks: Option<i64>,
    pub unhedged_qty_ticks: i64,
    pub unhedged_notional_ticks: String,
    pub notional_guard_evaluated: bool,
    pub max_net_btc_ticks: i64,
    pub max_unhedged_notional_ticks: i64,
    pub loss_breached: bool,
    pub margin_breached: bool,
    pub net_quantity_breached: bool,
    pub unhedged_notional_breached: bool,
}

impl From<&CarryHaltEvidence> for HaltEvidenceRecord {
    fn from(value: &CarryHaltEvidence) -> Self {
        let cause = match value.cause {
            HaltCause::ReconMismatch => "recon_mismatch",
            HaltCause::UnknownOrder => "unknown_order",
            HaltCause::Kill => "kill",
            HaltCause::Ban => "ban",
            HaltCause::RiskTrip => "risk_trip",
            HaltCause::IllegalTransition => "illegal_transition",
            HaltCause::PayloadError => "payload_error",
        };
        Self {
            cause,
            epoch: value.epoch.get(),
            command_now_us: value.now.micros(),
            command_decision_us: value.decision.micros().micros(),
            phase_before_refresh: phase(value.phase_before_refresh),
            assessment: value.assessment.as_ref().map(|a| {
                let (point, order, funding_key) = match a.point {
                    CarryAssessmentPoint::Observation => ("observation", None, None),
                    CarryAssessmentPoint::Fill(order) => ("fill", Some(order), None),
                    CarryAssessmentPoint::Reconciliation(order) => {
                        ("reconciliation", Some(order), None)
                    }
                    CarryAssessmentPoint::Funding { key } => ("funding", None, Some(key)),
                };
                RiskAssessmentRecord {
                    point,
                    order_pair: order.map(|id| id.pair.to_string()),
                    order_stage: order.map(|id| stage(id.stage)),
                    order_attempt: order.map(|id| id.attempt),
                    funding_key,
                    account: AccountRecord::from(&a.account),
                    spot_mark_ticks: a.spot.ticks(),
                    perp_mark_ticks: a.perp.ticks(),
                    equity_ticks: a.equity.0,
                    session_loss_ticks: a.session_loss_ticks.to_string(),
                    max_session_loss_ticks: a.max_session_loss.0,
                    margin_balance_ticks: a.margin_balance_ticks.map(|n| n.to_string()),
                    margin_required_ticks: a.margin_required.map(|n| n.0),
                    unhedged_qty_ticks: a.unhedged_qty.ticks(),
                    unhedged_notional_ticks: a.unhedged_notional_ticks.to_string(),
                    notional_guard_evaluated: a.notional_guard_evaluated,
                    max_net_btc_ticks: a.max_net_btc.ticks(),
                    max_unhedged_notional_ticks: a.max_unhedged_notional.0,
                    loss_breached: a.flags.loss,
                    margin_breached: a.flags.margin,
                    net_quantity_breached: a.flags.net_quantity,
                    unhedged_notional_breached: a.flags.unhedged_notional,
                }
            }),
        }
    }
}

fn phase(value: CarryPhase) -> &'static str {
    match value {
        CarryPhase::Flat => "flat",
        CarryPhase::OpeningSpot => "opening_spot",
        CarryPhase::OpeningHedge => "opening_hedge",
        CarryPhase::Hedged => "hedged",
        CarryPhase::ClosingPerp => "closing_perp",
        CarryPhase::ClosingSpot => "closing_spot",
        CarryPhase::Unresolved => "unresolved",
        CarryPhase::Closed => "closed",
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum ActorRecord {
    Control {
        at: i64,
        event: &'static str,
        before_node: &'static str,
        after_node: &'static str,
        revision: String,
        committed_key: Option<i64>,
        entry_epoch_start: i64,
    },
    Evaluation {
        at: i64,
        key: Option<i64>,
        disposition: &'static str,
    },
    Order {
        at: i64,
        pair: String,
        stage: &'static str,
        qty: i64,
        lower: i64,
        upper: i64,
        native_upper: i64,
        deadline: i64,
        reference: SourceRecord,
    },
    Fill {
        at: i64,
        physical: i64,
        pair: String,
        stage: &'static str,
        qty: i64,
        raw_reference: i64,
        executed: i64,
        fee_asset: &'static str,
        fee_ticks: i64,
        source: SourceRecord,
        account: AccountRecord,
    },
    FundingBoundary {
        at: i64,
        key: u64,
        captured_short: i64,
    },
    FundingReceipt {
        at: i64,
        key: u64,
        effective: i64,
        rate: i64,
        charge_mark_proxy: i64,
        cashflow: i64,
        rate_source: SourceRecord,
        mark_source: SourceRecord,
        account: AccountRecord,
    },
    PairClosed {
        at: i64,
        pair: String,
        account: AccountRecord,
    },
    RiskStop {
        at: i64,
        loss: bool,
        margin: bool,
        first_halt: Option<Box<HaltEvidenceRecord>>,
    },
    ZeroExposureBoundary {
        at: i64,
    },
}

pub(super) fn stage(stage: CarryStage) -> &'static str {
    match stage {
        CarryStage::SpotEntry => "spot_entry",
        CarryStage::PerpEntry => "perp_entry",
        CarryStage::PerpExit => "perp_exit",
        CarryStage::SpotExit => "spot_exit",
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct PairRecord {
    pub pair: String,
    pub origin_key: i64,
    pub opened_decision: i64,
    pub entry_epoch_start: i64,
    pub opening_deadline: i64,
    pub physical_paired_at: Option<i64>,
    pub observed_paired_at: Option<i64>,
    pub max_exit_decision: Option<i64>,
    pub exit_reason: Option<ExitReason>,
    pub exit_decision: Option<i64>,
    pub closing_deadline: Option<i64>,
    pub closed_observed_at: Option<i64>,
    pub gross_spot: i64,
    pub net_btc: i64,
    pub forecast_numerator: String,
    pub forecast_denominator: String,
    pub headline_c4: i64,
    pub sum9_ticks: String,
    pub latest_rate_ticks: i64,
    pub decision_spot_reference: i64,
    pub decision_perp_reference: i64,
    pub spot_reference_source: SourceRecord,
    pub perp_reference_source: SourceRecord,
    pub proposal_account: AccountRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExitReason {
    Terminal,
    Risk,
    HedgeEnvelopeUnwind,
    MaxHold,
    NonpositiveFunding,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct DailyCapital {
    pub day_start: i64,
    pub economic_end_exclusive: i64,
    pub known_at: i64,
    pub equity: i64,
    pub return_ticks: i64,
    pub cash_only: bool,
    pub pending_funding: usize,
    pub account: AccountRecord,
}

#[derive(Debug)]
pub(super) struct ReplayResult {
    pub records: Vec<ActorRecord>,
    pub pairs: Vec<PairRecord>,
    pub daily: Vec<DailyCapital>,
    pub snapshot: Option<CarrySnapshot>,
    pub risk_ticks: u64,
    pub final_equity: i64,
    pub control: sgd_owned_c1opportunitycontrol_50dd0338899d::Snapshot,
}

pub(super) fn replay_diagnostic(result: &ReplayResult) -> serde_json::Value {
    use parangsae_execution::carry::CarryStage;
    use parangsae_sim::carry::FeeCharge;
    use serde_json::json;

    let snapshot = result.snapshot.as_ref().map(|snapshot| {
        let orders: Vec<_> = snapshot.orders.iter().map(|order| json!({
            "pair": order.id.pair.to_string(), "stage": stage(order.id.stage),
            "attempt": order.id.attempt, "qty_ticks": order.qty.ticks(),
            "executed_upper_ticks": order.max_price.ticks(),
            "state": order.progress.state().name(), "sent": order.progress.sent(),
            "cumulative_ticks": order.progress.cum().ticks(), "complete_cut": order.complete_cut,
        })).collect();
        let fills: Vec<_> = snapshot.fills.iter().map(|fill| {
            let source = fill.source.value();
            let (asset, fee) = match fill.fee {
                FeeCharge::Usdt(amount) => ("USDT", amount.0),
                FeeCharge::Btc(amount) => ("BTC", amount.ticks()),
            };
            json!({"pair": source.order.pair.to_string(), "stage": stage(source.order.stage),
                "fill_id":source.fill_id, "qty_ticks":source.qty.ticks(),
                "raw_reference_ticks":source.reference_price.ticks(), "executed_price_ticks":fill.price.ticks(),
                "fee_asset":asset,"fee_ticks":fee,"clocks":clocks(fill.source.timestamps())})
        }).collect();
        let funding: Vec<_> = snapshot.funding.iter().map(|funding| json!({
            "key":funding.key,"effective_us":funding.effective.micros(),
            "captured_short_ticks":funding.captured_short.ticks(),
            "cashflow_ticks":funding.cashflow.map(|amount|amount.0),
            "receipt":funding.receipt.as_ref().map(|receipt|json!({
                "effective_us":receipt.value().effective.micros(),"rate_ticks":receipt.value().rate.ticks(),
                "charge_mark_proxy_ticks":receipt.value().charge_mark.ticks(),"clocks":clocks(receipt.timestamps())
            }))
        })).collect();
        let physical_flat = snapshot.account.spot_btc.ticks()==0 && snapshot.account.perp_short.ticks()==0;
        json!({"account":AccountRecord::from(&snapshot.account),"phase":format!("{:?}",snapshot.phase),
            "observed_quantity_flat":physical_flat,"halted":snapshot.halted,"loss_latched":snapshot.loss_latched,
            "margin_latched":snapshot.margin_latched,"halt_epoch":snapshot.epoch.get(),
            "first_halt":snapshot.first_halt.as_deref().map(HaltEvidenceRecord::from),
            "spot_quote_reserved_ticks":snapshot.spot_quote_reserved.0,
            "perp_collateral_reserved_ticks":snapshot.perp_collateral_reserved.0,
            "unsupported_evidence_debug":snapshot.unsupported.as_ref().map(|value|format!("{value:?}")),
            "orders":orders,"fills":fills,"funding":funding,
            "hedged_at_us":snapshot.hedged_at.map(|at|at.micros().micros()),
            "current_pair":snapshot.pair.as_ref().map(|pair|pair.pair.to_string()),
            "normal_stage_order":[stage(CarryStage::SpotEntry),stage(CarryStage::PerpEntry),stage(CarryStage::PerpExit),stage(CarryStage::SpotExit)]})
    });
    json!({"records":result.records,"pairs":result.pairs,"daily_capital":result.daily,
        "last_observed_daily_equity_ticks":result.final_equity,"sampled_risk_ticks":result.risk_ticks,
        "engine":snapshot,"control_snapshot_data":result.control,
        "snapshot_is_authority_or_restart_checkpoint":false})
}

fn clocks(value: parangsae_core::Timestamps) -> serde_json::Value {
    serde_json::json!({"event_us":value.event_ts.micros().micros(),
        "available_us":value.available_ts.micros().micros(),
        "modeled_received_us":value.received_ts.micros().micros()})
}
