// Generated whole-row adapter. Include this as a private application module.
// The separate primitive crate must be imported as sgd_fact_authority.
// Source profile: closed audited Rust Fact types; no arbitrary Eq/Clone callbacks.
use sgd_fact_authority as authority;

mod sealed {
    pub trait SupportedFact: Clone {
        fn identities(&self) -> (String, String);
    }
}

// External generic callers may name this bound but cannot implement its private superclass.
pub(crate) trait BoundFact: sealed::SupportedFact {}

fn part(output: &mut String, bytes: &[u8]) {
    output.push_str(&bytes.len().to_string());
    output.push(':');
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output.push(';');
}
fn header(row: &str, flavor: &str) -> String {
    let mut output = String::new();
    part(&mut output, b"sgd-row-v1");
    part(&mut output, row.as_bytes());
    part(&mut output, flavor.as_bytes());
    output
}
fn field(output: &mut String, name: &str, kind: &str, bytes: &[u8]) {
    part(output, name.as_bytes());
    part(output, kind.as_bytes());
    part(output, bytes);
}

impl BoundFact for crate::vision::KlineFact {}
impl sealed::SupportedFact for crate::vision::KlineFact {
    fn identities(&self) -> (String, String) {
        // No rest pattern: an added field invalidates this generated binding.
        let crate::vision::KlineFact {
            symbol: row_field_0,
            interval_us: row_field_1,
            open_time: row_field_2,
            close_time: row_field_3,
            open_ticks: row_field_4,
            high_ticks: row_field_5,
            low_ticks: row_field_6,
            close_ticks: row_field_7,
            volume: row_field_8,
            quote_volume: row_field_9,
            count: row_field_10,
            taker_buy_volume: row_field_11,
            taker_buy_quote_volume: row_field_12,
            event_ts: row_field_13,
            available_ts: row_field_14,
            received_ts: row_field_15,
            schema_version: row_field_16,
            tick_domain: row_field_17,
        } = self;
        let mut payload = header("KlineFact", "payload");
        let mut provenance = header("KlineFact", "provenance");
        field(
            &mut payload,
            "symbol",
            "string",
            (row_field_0.as_str()).as_bytes(),
        );
        field(
            &mut payload,
            "interval_us",
            "i64",
            &(*row_field_1).to_be_bytes(),
        );
        field(
            &mut payload,
            "open_time",
            "i64",
            &(row_field_2.micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "close_time",
            "i64",
            &(row_field_3.micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "open_ticks",
            "i64",
            &(*row_field_4).to_be_bytes(),
        );
        field(
            &mut payload,
            "high_ticks",
            "i64",
            &(*row_field_5).to_be_bytes(),
        );
        field(
            &mut payload,
            "low_ticks",
            "i64",
            &(*row_field_6).to_be_bytes(),
        );
        field(
            &mut payload,
            "close_ticks",
            "i64",
            &(*row_field_7).to_be_bytes(),
        );
        field(
            &mut payload,
            "volume",
            "i64",
            &(row_field_8.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "quote_volume",
            "i64",
            &(row_field_9.ticks()).to_be_bytes(),
        );
        field(&mut payload, "count", "i64", &(*row_field_10).to_be_bytes());
        field(
            &mut payload,
            "taker_buy_volume",
            "i64",
            &(row_field_11.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "taker_buy_quote_volume",
            "i64",
            &(row_field_12.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "event_ts",
            "i64",
            &(row_field_13.micros().micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "available_ts",
            "i64",
            &(row_field_14.micros().micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "schema_version",
            "u32",
            &(match *row_field_16 {
                1 | 2 => 1,
                value => value,
            })
            .to_be_bytes(),
        );
        field(
            &mut payload,
            "tick_domain",
            "enum",
            (match row_field_17 {
                crate::vision::TickDomain::Price => "Price",
                crate::vision::TickDomain::Ratio => "Ratio",
            })
            .as_bytes(),
        );
        field(
            &mut provenance,
            "received_ts",
            "i64",
            &(row_field_15.micros().micros()).to_be_bytes(),
        );
        field(
            &mut provenance,
            "schema_version",
            "u32",
            &(*row_field_16).to_be_bytes(),
        );
        (payload, provenance)
    }
}

impl BoundFact for crate::vision::FundingFact {}
impl sealed::SupportedFact for crate::vision::FundingFact {
    fn identities(&self) -> (String, String) {
        // No rest pattern: an added field invalidates this generated binding.
        let crate::vision::FundingFact {
            symbol: row_field_0,
            raw_calc_time: row_field_1,
            funding_time: row_field_2,
            interval_hours: row_field_3,
            rate: row_field_4,
            event_ts: row_field_5,
            available_ts: row_field_6,
            received_ts: row_field_7,
            schema_version: row_field_8,
        } = self;
        let mut payload = header("FundingFact", "payload");
        let mut provenance = header("FundingFact", "provenance");
        field(
            &mut payload,
            "symbol",
            "string",
            (row_field_0.as_str()).as_bytes(),
        );
        field(
            &mut payload,
            "raw_calc_time",
            "i64",
            &(row_field_1.micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "funding_time",
            "i64",
            &(row_field_2.micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "interval_hours",
            "i64",
            &(*row_field_3).to_be_bytes(),
        );
        field(
            &mut payload,
            "rate",
            "i64",
            &(row_field_4.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "event_ts",
            "i64",
            &(row_field_5.micros().micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "available_ts",
            "i64",
            &(row_field_6.micros().micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "schema_version",
            "u32",
            &(*row_field_8).to_be_bytes(),
        );
        field(
            &mut provenance,
            "received_ts",
            "i64",
            &(row_field_7.micros().micros()).to_be_bytes(),
        );
        (payload, provenance)
    }
}

impl BoundFact for crate::vision::MetricsFact {}
impl sealed::SupportedFact for crate::vision::MetricsFact {
    fn identities(&self) -> (String, String) {
        // No rest pattern: an added field invalidates this generated binding.
        let crate::vision::MetricsFact {
            symbol: row_field_0,
            create_time: row_field_1,
            sum_open_interest: row_field_2,
            sum_open_interest_value: row_field_3,
            count_toptrader_long_short_ratio: row_field_4,
            sum_toptrader_long_short_ratio: row_field_5,
            count_long_short_ratio: row_field_6,
            sum_taker_long_short_vol_ratio: row_field_7,
            event_ts: row_field_8,
            available_ts: row_field_9,
            received_ts: row_field_10,
            available_lag_verified: row_field_11,
            schema_version: row_field_12,
        } = self;
        let mut payload = header("MetricsFact", "payload");
        let mut provenance = header("MetricsFact", "provenance");
        field(
            &mut payload,
            "symbol",
            "string",
            (row_field_0.as_str()).as_bytes(),
        );
        field(
            &mut payload,
            "create_time",
            "i64",
            &(row_field_1.micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "sum_open_interest",
            "i64",
            &(row_field_2.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "sum_open_interest_value",
            "i64",
            &(row_field_3.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "count_toptrader_long_short_ratio",
            "i64",
            &(row_field_4.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "sum_toptrader_long_short_ratio",
            "i64",
            &(row_field_5.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "count_long_short_ratio",
            "i64",
            &(row_field_6.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "sum_taker_long_short_vol_ratio",
            "i64",
            &(row_field_7.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "event_ts",
            "i64",
            &(row_field_8.micros().micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "available_ts",
            "i64",
            &(row_field_9.micros().micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "available_lag_verified",
            "bool",
            &[u8::from(*row_field_11)],
        );
        field(
            &mut payload,
            "schema_version",
            "u32",
            &(*row_field_12).to_be_bytes(),
        );
        field(
            &mut provenance,
            "received_ts",
            "i64",
            &(row_field_10.micros().micros()).to_be_bytes(),
        );
        (payload, provenance)
    }
}

impl BoundFact for crate::vision::BookFact {}
impl sealed::SupportedFact for crate::vision::BookFact {
    fn identities(&self) -> (String, String) {
        // No rest pattern: an added field invalidates this generated binding.
        let crate::vision::BookFact {
            symbol: row_field_0,
            timestamp: row_field_1,
            percentage: row_field_2,
            depth: row_field_3,
            notional: row_field_4,
            level_schema: row_field_5,
            event_ts: row_field_6,
            available_ts: row_field_7,
            received_ts: row_field_8,
            schema_version: row_field_9,
        } = self;
        let mut payload = header("BookFact", "payload");
        let mut provenance = header("BookFact", "provenance");
        field(
            &mut payload,
            "symbol",
            "string",
            (row_field_0.as_str()).as_bytes(),
        );
        field(
            &mut payload,
            "timestamp",
            "i64",
            &(row_field_1.micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "percentage",
            "i64",
            &(row_field_2.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "depth",
            "i64",
            &(row_field_3.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "notional",
            "i64",
            &(row_field_4.ticks()).to_be_bytes(),
        );
        field(
            &mut payload,
            "level_schema",
            "u32",
            &(*row_field_5).to_be_bytes(),
        );
        field(
            &mut payload,
            "event_ts",
            "i64",
            &(row_field_6.micros().micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "available_ts",
            "i64",
            &(row_field_7.micros().micros()).to_be_bytes(),
        );
        field(
            &mut payload,
            "schema_version",
            "u32",
            &(*row_field_9).to_be_bytes(),
        );
        field(
            &mut provenance,
            "received_ts",
            "i64",
            &(row_field_8.micros().micros()).to_be_bytes(),
        );
        (payload, provenance)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WriteEffect {
    Clash,
    Insert,
    Same,
}

#[derive(Debug)]
pub(crate) enum RowError {
    Transition(authority::TransitionError),
    BrokenBinding,
}
impl std::fmt::Display for RowError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transition(error) => write!(formatter, "{error}"),
            Self::BrokenBinding => formatter.write_str("ROW302: complete rows disagree with the generated payload/provenance binding; regenerate and verify the source binding"),
        }
    }
}
impl std::error::Error for RowError {}

struct RowPair {
    payload: &'static str,
    provenance: &'static str,
    empty_payload: authority::SymbolicValue,
    empty_provenance: authority::SymbolicValue,
}
fn matches<T: sealed::SupportedFact>(
    row: &Option<T>,
    snapshot: &authority::Snapshot,
    pair: &RowPair,
) -> bool {
    let (payload, provenance) = match row {
        None => (pair.empty_payload.clone(), pair.empty_provenance.clone()),
        Some(row) => {
            let (payload, provenance) = row.identities();
            (
                authority::SymbolicValue::Data {
                    domain: "Values".to_owned(),
                    value: payload,
                },
                authority::SymbolicValue::Data {
                    domain: "Receipts".to_owned(),
                    value: provenance,
                },
            )
        }
    };
    snapshot.values.get(pair.payload) == Some(&payload)
        && snapshot.values.get(pair.provenance) == Some(&provenance)
}
fn lift<T: sealed::SupportedFact>(
    receipt: &authority::Receipt,
    pair: &RowPair,
    incoming: &T,
    old_0: &Option<T>,
    old_1: &Option<T>,
) -> Result<Option<T>, RowError> {
    match (
        receipt.updates.get(pair.payload),
        receipt.updates.get(pair.provenance),
    ) {
        (
            Some(authority::ValueExpression::Literal { value: a }),
            Some(authority::ValueExpression::Literal { value: b }),
        ) if a == &pair.empty_payload && b == &pair.empty_provenance => Ok(None),
        (
            Some(authority::ValueExpression::Input { parameter: a }),
            Some(authority::ValueExpression::Input { parameter: b }),
        ) if a == "v" && b == "provenance" => Ok(Some(incoming.clone())),
        (
            Some(authority::ValueExpression::State { variable: a }),
            Some(authority::ValueExpression::State { variable: b }),
        ) if a == "stored" && b == "storedProvenance" => Ok(old_0.clone()),
        (
            Some(authority::ValueExpression::State { variable: a }),
            Some(authority::ValueExpression::State { variable: b }),
        ) if a == "rejected" && b == "rejectedProvenance" => Ok(old_1.clone()),
        _ => Err(RowError::BrokenBinding),
    }
}

// Private state and a sealed concrete Fact bound prohibit arbitrary source equality.
pub(crate) struct FactOwner<T: BoundFact> {
    authority: authority::FactEnvelopeMachine,
    stored: Option<T>,
    rejected: Option<T>,
}
impl<T: BoundFact> Default for FactOwner<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: BoundFact> FactOwner<T> {
    pub(crate) fn new() -> Self {
        Self {
            authority: authority::FactEnvelopeMachine::new(),
            stored: None,
            rejected: None,
        }
    }
    pub(crate) fn stored(&self) -> Option<&T> {
        self.stored.as_ref()
    }
    pub(crate) fn rejected(&self) -> Option<&T> {
        self.rejected.as_ref()
    }
    pub(crate) fn read(&self) -> authority::Snapshot {
        self.authority.read()
    }

    pub(crate) fn write(&mut self, incoming: T) -> Result<WriteEffect, RowError> {
        let stored_pair = RowPair {
            payload: "stored",
            provenance: "storedProvenance",
            empty_payload: authority::SymbolicValue::String {
                value: "none".to_owned(),
            },
            empty_provenance: authority::SymbolicValue::String {
                value: "none".to_owned(),
            },
        };
        let rejected_pair = RowPair {
            payload: "rejected",
            provenance: "rejectedProvenance",
            empty_payload: authority::SymbolicValue::String {
                value: "none".to_owned(),
            },
            empty_provenance: authority::SymbolicValue::String {
                value: "none".to_owned(),
            },
        };
        let before = self.read();
        if !matches(&self.stored, &before, &stored_pair)
            || !matches(&self.rejected, &before, &rejected_pair)
        {
            return Err(RowError::BrokenBinding);
        }
        let (incoming_payload, incoming_provenance) = incoming.identities();
        let old_stored = self.stored.clone();
        let old_rejected = self.rejected.clone();
        let pending = self
            .authority
            .prepare(&incoming_payload, &incoming_provenance)
            .map_err(RowError::Transition)?;
        let receipt = pending.receipt();
        let next_stored = lift(receipt, &stored_pair, &incoming, &old_stored, &old_rejected)?;
        let next_rejected = lift(
            receipt,
            &rejected_pair,
            &incoming,
            &old_stored,
            &old_rejected,
        )?;
        if !matches(&next_stored, &receipt.after, &stored_pair)
            || !matches(&next_rejected, &receipt.after, &rejected_pair)
        {
            return Err(RowError::BrokenBinding);
        }
        let effect = match receipt.action.as_str() {
            "Clash" => WriteEffect::Clash,
            "Insert" => WriteEffect::Insert,
            "Same" => WriteEffect::Same,
            _ => return Err(RowError::BrokenBinding),
        };
        // All fallible checks, user-type clones, and encoding finish before this commit.
        pending.commit();
        self.stored = next_stored;
        self.rejected = next_rejected;
        Ok(effect)
    }
}
