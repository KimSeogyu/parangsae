use parangsae_execution::carry::{CarrySnapshot, CarryStage};
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
