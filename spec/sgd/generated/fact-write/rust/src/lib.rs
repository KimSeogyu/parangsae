// Generated symbolic-owned authority. Regenerate from the verified model.
//! Equality/copy contract for actual immutable UTF-8 data in one live object.
//! Keep this crate separate and use safe Rust callers. Persistent identity,
//! reset prevention, restart, restore, external effects, and application-wide
//! coverage require separate contracts. No application callback runs in send.
#![forbid(unsafe_code)]

use serde::Serialize;
use std::collections::BTreeMap;

pub const MODEL: &str = "FactWriteEnvelope";
pub const MODEL_REVISION: &str = "dcc32c6c46d16abd4b90ee573974b59aec70e95315c296a18484a3ac089ac25d";

/// Data atoms stay distinct from scalar strings, including an identical sentinel.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SymbolicValue {
    String { value: String },
    Boolean { value: bool },
    Integer { value: String },
    Data { domain: String, value: String },
}

/// The source of each simultaneous assignment, copied from the symbolic IR.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ValueExpression {
    Literal { value: SymbolicValue },
    State { variable: String },
    NextState { variable: String },
    Input { parameter: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub model: &'static str,
    pub model_revision: &'static str,
    pub revision: String,
    pub values: BTreeMap<String, SymbolicValue>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Receipt {
    pub action: String,
    pub before: Snapshot,
    pub after: Snapshot,
    pub updates: BTreeMap<String, ValueExpression>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransitionErrorReason {
    Disabled,
    Ambiguous,
    RevisionExhausted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TransitionError {
    pub reason: TransitionErrorReason,
    pub state: Snapshot,
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.reason {
            TransitionErrorReason::Disabled => {
                formatter.write_str("symbolic input is disabled in the current state")
            }
            TransitionErrorReason::Ambiguous => {
                formatter.write_str("more than one symbolic action is enabled")
            }
            TransitionErrorReason::RevisionExhausted => {
                formatter.write_str("authority revision is exhausted")
            }
        }
    }
}

impl std::error::Error for TransitionError {}

/// Private complete state. No Clone, Deserialize, setter, or restore API.
pub struct FactEnvelopeMachine {
    values: BTreeMap<String, SymbolicValue>,
    revision: u64,
}

/// An uncommitted transition that exclusively borrows its originating owner.
/// Dropping this token leaves the owner unchanged. It cannot be copied or restored.
pub struct PreparedTransition<'owner> {
    owner: &'owner mut FactEnvelopeMachine,
    values: BTreeMap<String, SymbolicValue>,
    revision: u64,
    receipt: Receipt,
}

impl PreparedTransition<'_> {
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }

    /// All validation and allocation finished in prepare; commit only moves fields.
    pub fn commit(self) -> Receipt {
        let Self {
            owner,
            values,
            revision,
            receipt,
        } = self;
        owner.values = values;
        owner.revision = revision;
        receipt
    }
}

impl FactEnvelopeMachine {
    pub fn new() -> Self {
        Self {
            revision: 0,
            values: BTreeMap::from([
                (
                    "phase".to_owned(),
                    SymbolicValue::String {
                        value: "absent".to_owned(),
                    },
                ),
                (
                    "rejected".to_owned(),
                    SymbolicValue::String {
                        value: "none".to_owned(),
                    },
                ),
                (
                    "rejectedProvenance".to_owned(),
                    SymbolicValue::String {
                        value: "none".to_owned(),
                    },
                ),
                (
                    "stored".to_owned(),
                    SymbolicValue::String {
                        value: "none".to_owned(),
                    },
                ),
                (
                    "storedProvenance".to_owned(),
                    SymbolicValue::String {
                        value: "none".to_owned(),
                    },
                ),
            ]),
        }
    }

    pub fn read(&self) -> Snapshot {
        Snapshot {
            model: MODEL,
            model_revision: MODEL_REVISION,
            revision: self.revision.to_string(),
            values: self.values.clone(),
        }
    }

    fn reject(&self, reason: TransitionErrorReason) -> TransitionError {
        TransitionError {
            reason,
            state: self.read(),
        }
    }

    /// The input is actual immutable data, never a caller-selected action.
    /// All guards and simultaneous assignment values read the old state.
    pub fn send(&mut self, input_0: &str, input_1: &str) -> Result<Receipt, TransitionError> {
        self.prepare(input_0, input_1)
            .map(PreparedTransition::commit)
    }

    /// Inspect the complete proposed receipt before the owner changes.
    /// The exclusive token must be committed or dropped before using this owner again.
    pub fn prepare(
        &mut self,
        input_0: &str,
        input_1: &str,
    ) -> Result<PreparedTransition<'_>, TransitionError> {
        let input_value_0 = SymbolicValue::Data {
            domain: "Values".to_owned(),
            value: input_0.to_owned(),
        };
        let input_value_1 = SymbolicValue::Data {
            domain: "Receipts".to_owned(),
            value: input_1.to_owned(),
        };
        let mut selected: Option<usize> = None;
        if (matches!(input_value_0.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Values")
            && matches!(input_value_1.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Receipts")
            && (self
                .values
                .get("phase")
                .expect("private generated state invariant")
                .clone()
                == SymbolicValue::String {
                    value: "absent".to_owned(),
                })
            && matches!(input_value_0.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Values")
            && matches!(input_value_1.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Receipts"))
        {
            if selected.is_some() {
                return Err(self.reject(TransitionErrorReason::Ambiguous));
            }
            selected = Some(0);
        }
        if (matches!(input_value_0.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Values")
            && matches!(input_value_1.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Receipts")
            && [
                SymbolicValue::String {
                    value: "present".to_owned(),
                },
                SymbolicValue::String {
                    value: "conflict".to_owned(),
                },
            ]
            .contains(
                &(self
                    .values
                    .get("phase")
                    .expect("private generated state invariant")
                    .clone()),
            )
            && (input_value_0.clone()
                == self
                    .values
                    .get("stored")
                    .expect("private generated state invariant")
                    .clone())
            && matches!(input_value_1.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Receipts"))
        {
            if selected.is_some() {
                return Err(self.reject(TransitionErrorReason::Ambiguous));
            }
            selected = Some(1);
        }
        if (matches!(input_value_0.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Values")
            && matches!(input_value_1.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Receipts")
            && [
                SymbolicValue::String {
                    value: "present".to_owned(),
                },
                SymbolicValue::String {
                    value: "conflict".to_owned(),
                },
            ]
            .contains(
                &(self
                    .values
                    .get("phase")
                    .expect("private generated state invariant")
                    .clone()),
            )
            && matches!(input_value_0.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Values")
            && (input_value_0.clone()
                != self
                    .values
                    .get("stored")
                    .expect("private generated state invariant")
                    .clone())
            && matches!(input_value_1.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Receipts"))
        {
            if selected.is_some() {
                return Err(self.reject(TransitionErrorReason::Ambiguous));
            }
            selected = Some(2);
        }
        let Some(selected) = selected else {
            return Err(self.reject(TransitionErrorReason::Disabled));
        };
        let Some(revision) = self.revision.checked_add(1) else {
            return Err(self.reject(TransitionErrorReason::RevisionExhausted));
        };
        let before = self.read();
        let (action, values, updates) = match selected {
            0 => (
                "Insert",
                BTreeMap::from([
                    (
                        "phase".to_owned(),
                        SymbolicValue::String {
                            value: "present".to_owned(),
                        },
                    ),
                    (
                        "rejected".to_owned(),
                        SymbolicValue::String {
                            value: "none".to_owned(),
                        },
                    ),
                    (
                        "rejectedProvenance".to_owned(),
                        SymbolicValue::String {
                            value: "none".to_owned(),
                        },
                    ),
                    ("stored".to_owned(), input_value_0.clone()),
                    ("storedProvenance".to_owned(), input_value_1.clone()),
                ]),
                BTreeMap::from([
                    (
                        "phase".to_owned(),
                        ValueExpression::Literal {
                            value: SymbolicValue::String {
                                value: "present".to_owned(),
                            },
                        },
                    ),
                    (
                        "rejected".to_owned(),
                        ValueExpression::Literal {
                            value: SymbolicValue::String {
                                value: "none".to_owned(),
                            },
                        },
                    ),
                    (
                        "rejectedProvenance".to_owned(),
                        ValueExpression::Literal {
                            value: SymbolicValue::String {
                                value: "none".to_owned(),
                            },
                        },
                    ),
                    (
                        "stored".to_owned(),
                        ValueExpression::Input {
                            parameter: "v".to_owned(),
                        },
                    ),
                    (
                        "storedProvenance".to_owned(),
                        ValueExpression::Input {
                            parameter: "provenance".to_owned(),
                        },
                    ),
                ]),
            ),
            1 => (
                "Same",
                BTreeMap::from([
                    (
                        "phase".to_owned(),
                        self.values
                            .get("phase")
                            .expect("private generated state invariant")
                            .clone(),
                    ),
                    (
                        "rejected".to_owned(),
                        self.values
                            .get("rejected")
                            .expect("private generated state invariant")
                            .clone(),
                    ),
                    (
                        "rejectedProvenance".to_owned(),
                        self.values
                            .get("rejectedProvenance")
                            .expect("private generated state invariant")
                            .clone(),
                    ),
                    (
                        "stored".to_owned(),
                        self.values
                            .get("stored")
                            .expect("private generated state invariant")
                            .clone(),
                    ),
                    (
                        "storedProvenance".to_owned(),
                        self.values
                            .get("storedProvenance")
                            .expect("private generated state invariant")
                            .clone(),
                    ),
                ]),
                BTreeMap::from([
                    (
                        "phase".to_owned(),
                        ValueExpression::State {
                            variable: "phase".to_owned(),
                        },
                    ),
                    (
                        "rejected".to_owned(),
                        ValueExpression::State {
                            variable: "rejected".to_owned(),
                        },
                    ),
                    (
                        "rejectedProvenance".to_owned(),
                        ValueExpression::State {
                            variable: "rejectedProvenance".to_owned(),
                        },
                    ),
                    (
                        "stored".to_owned(),
                        ValueExpression::State {
                            variable: "stored".to_owned(),
                        },
                    ),
                    (
                        "storedProvenance".to_owned(),
                        ValueExpression::State {
                            variable: "storedProvenance".to_owned(),
                        },
                    ),
                ]),
            ),
            2 => (
                "Clash",
                BTreeMap::from([
                    (
                        "phase".to_owned(),
                        SymbolicValue::String {
                            value: "conflict".to_owned(),
                        },
                    ),
                    ("rejected".to_owned(), input_value_0.clone()),
                    ("rejectedProvenance".to_owned(), input_value_1.clone()),
                    (
                        "stored".to_owned(),
                        self.values
                            .get("stored")
                            .expect("private generated state invariant")
                            .clone(),
                    ),
                    (
                        "storedProvenance".to_owned(),
                        self.values
                            .get("storedProvenance")
                            .expect("private generated state invariant")
                            .clone(),
                    ),
                ]),
                BTreeMap::from([
                    (
                        "phase".to_owned(),
                        ValueExpression::Literal {
                            value: SymbolicValue::String {
                                value: "conflict".to_owned(),
                            },
                        },
                    ),
                    (
                        "rejected".to_owned(),
                        ValueExpression::Input {
                            parameter: "v".to_owned(),
                        },
                    ),
                    (
                        "rejectedProvenance".to_owned(),
                        ValueExpression::Input {
                            parameter: "provenance".to_owned(),
                        },
                    ),
                    (
                        "stored".to_owned(),
                        ValueExpression::State {
                            variable: "stored".to_owned(),
                        },
                    ),
                    (
                        "storedProvenance".to_owned(),
                        ValueExpression::State {
                            variable: "storedProvenance".to_owned(),
                        },
                    ),
                ]),
            ),
            _ => unreachable!("private generated action invariant"),
        };
        let after = Snapshot {
            model: MODEL,
            model_revision: MODEL_REVISION,
            revision: revision.to_string(),
            values: values.clone(),
        };
        let receipt = Receipt {
            action: action.to_owned(),
            before,
            after,
            updates,
        };
        Ok(PreparedTransition {
            owner: self,
            values,
            revision,
            receipt,
        })
    }
}

impl ::std::default::Default for FactEnvelopeMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaustion_preserves_complete_state_and_revision() {
        let mut owner = FactEnvelopeMachine::new();
        owner.revision = u64::MAX;
        let before = owner.read();
        match owner.prepare("fresh-overflow-input-0", "fresh-overflow-input-1") {
            Err(error) => assert_eq!(error.state, before),
            Ok(_) => panic!("no preparation follows u64::MAX"),
        }
        assert_eq!(owner.read(), before);
        let error = owner
            .send("fresh-overflow-input-0", "fresh-overflow-input-1")
            .expect_err("no revision follows u64::MAX");
        assert_eq!(error.state, before);
        assert_eq!(owner.read(), before);
    }
}
