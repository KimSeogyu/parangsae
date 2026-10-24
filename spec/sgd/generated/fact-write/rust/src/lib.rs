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
        if matches!(input_value_0.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Values")
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
            && matches!(input_value_1.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Receipts")
        {
            if selected.is_some() {
                return Err(self.reject(TransitionErrorReason::Ambiguous));
            }
            selected = Some(0);
        }
        if matches!(input_value_0.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Values")
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
            && matches!(input_value_1.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Receipts")
        {
            if selected.is_some() {
                return Err(self.reject(TransitionErrorReason::Ambiguous));
            }
            selected = Some(1);
        }
        if matches!(input_value_0.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Values")
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
            && matches!(input_value_1.clone(), SymbolicValue::Data { ref domain, .. } if domain == "Receipts")
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

// Separate live finite controllers can never be injected into the private fold.
pub mod partition_control {
    // Generated by specification-guided-development. Regenerate from the verified model.
    //! Exact finite transition authority for "PartitionMergeControl".
    //!
    //! This contract covers each live authority object in callers that use safe Rust.
    //! Keep this crate as a separate dependency. Changing generated source, replacing
    //! the crate, unsafe caller code, and foreign code are outside this contract.
    //! Each new() starts a new object at Init. Persistent entity identity, restart,
    //! restore, database effects, effect atomicity, and application refinement need
    //! separate contracts. No callback executes during dispatch.
    #![forbid(unsafe_code)]

    use serde::Serialize;
    use std::collections::BTreeMap;

    pub const MODEL: &str = "PartitionMergeControl";
    pub const MODEL_REVISION: &str =
        "030db4d6fef0c8de3dc20a42172a295a0aefd7693d3e83029b8a2343ef205cd0";

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
    #[allow(non_camel_case_types)]
    pub enum Event {
        #[serde(rename = "Clash")]
        Clash,
        #[serde(rename = "CloseNoop")]
        CloseNoop,
        #[serde(rename = "CloseRefused")]
        CloseRefused,
        #[serde(rename = "CloseWrite")]
        CloseWrite,
        #[serde(rename = "ConsumeNoop")]
        ConsumeNoop,
        #[serde(rename = "ConsumeRefused")]
        ConsumeRefused,
        #[serde(rename = "ConsumeWrite")]
        ConsumeWrite,
        #[serde(rename = "Insert")]
        Insert,
        #[serde(rename = "Same")]
        Same,
    }

    impl Event {
        /// The exact action label in the verified model, without identifier changes.
        pub fn model_name(self) -> &'static str {
            match self {
                Self::Clash => "Clash",
                Self::CloseNoop => "CloseNoop",
                Self::CloseRefused => "CloseRefused",
                Self::CloseWrite => "CloseWrite",
                Self::ConsumeNoop => "ConsumeNoop",
                Self::ConsumeRefused => "ConsumeRefused",
                Self::ConsumeWrite => "ConsumeWrite",
                Self::Insert => "Insert",
                Self::Same => "Same",
            }
        }

        /// Unknown labels cannot be dispatched as an Event.
        pub fn from_model_name(name: &str) -> Option<Self> {
            match name {
                "Clash" => Some(Self::Clash),
                "CloseNoop" => Some(Self::CloseNoop),
                "CloseRefused" => Some(Self::CloseRefused),
                "CloseWrite" => Some(Self::CloseWrite),
                "ConsumeNoop" => Some(Self::ConsumeNoop),
                "ConsumeRefused" => Some(Self::ConsumeRefused),
                "ConsumeWrite" => Some(Self::ConsumeWrite),
                "Insert" => Some(Self::Insert),
                "Same" => Some(Self::Same),
                _ => None,
            }
        }
    }

    pub const ALL_EVENTS: &[Event] = &[
        Event::Clash,
        Event::CloseNoop,
        Event::CloseRefused,
        Event::CloseWrite,
        Event::ConsumeNoop,
        Event::ConsumeRefused,
        Event::ConsumeWrite,
        Event::Insert,
        Event::Same,
    ];

    /// Every supported concrete model value. Integers remain exact decimal strings.
    #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
    #[serde(tag = "kind", content = "value", rename_all = "lowercase")]
    pub enum FiniteValue {
        String(String),
        Boolean(bool),
        Integer(String),
    }

    /// An owned copy. Editing a snapshot cannot change the live authority.
    #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Snapshot {
        pub model: &'static str,
        pub model_revision: &'static str,
        pub node_id: &'static str,
        pub revision: String,
        pub values: BTreeMap<String, FiniteValue>,
    }

    #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
    pub struct Receipt {
        pub event: Event,
        pub before: Snapshot,
        pub after: Snapshot,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
    #[serde(rename_all = "kebab-case")]
    pub enum TransitionErrorReason {
        Disabled,
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
                TransitionErrorReason::Disabled => formatter
                    .write_str("TLC001: action is disabled in the current concrete model state"),
                TransitionErrorReason::RevisionExhausted => {
                    formatter.write_str("authority revision is exhausted")
                }
            }
        }
    }

    impl std::error::Error for TransitionError {}

    /// The only owner of node and revision. It has no Clone, Deserialize, mutable
    /// state reference, raw-state constructor, or restore API.
    pub struct FactEnvelopeMachineControl {
        node: usize,
        revision: u64,
    }

    impl FactEnvelopeMachineControl {
        /// Start a distinct live object at the unique verified Init state.
        pub fn new() -> Self {
            Self {
                node: 1,
                revision: 0,
            }
        }

        pub fn read(&self) -> Snapshot {
            let (node_id, values) = match self.node {
                0 => (
                    "s0",
                    BTreeMap::from([
                        ("hasConflict".to_owned(), FiniteValue::Boolean(false)),
                        ("hasInserted".to_owned(), FiniteValue::Boolean(false)),
                        (
                            "phase".to_owned(),
                            FiniteValue::String("Consumed".to_owned()),
                        ),
                    ]),
                ),
                1 => (
                    "s1",
                    BTreeMap::from([
                        ("hasConflict".to_owned(), FiniteValue::Boolean(false)),
                        ("hasInserted".to_owned(), FiniteValue::Boolean(false)),
                        ("phase".to_owned(), FiniteValue::String("Open".to_owned())),
                    ]),
                ),
                2 => (
                    "s10",
                    BTreeMap::from([
                        ("hasConflict".to_owned(), FiniteValue::Boolean(true)),
                        ("hasInserted".to_owned(), FiniteValue::Boolean(true)),
                        ("phase".to_owned(), FiniteValue::String("Open".to_owned())),
                    ]),
                ),
                3 => (
                    "s11",
                    BTreeMap::from([
                        ("hasConflict".to_owned(), FiniteValue::Boolean(true)),
                        ("hasInserted".to_owned(), FiniteValue::Boolean(true)),
                        (
                            "phase".to_owned(),
                            FiniteValue::String("ReadyRefused".to_owned()),
                        ),
                    ]),
                ),
                4 => (
                    "s2",
                    BTreeMap::from([
                        ("hasConflict".to_owned(), FiniteValue::Boolean(false)),
                        ("hasInserted".to_owned(), FiniteValue::Boolean(false)),
                        (
                            "phase".to_owned(),
                            FiniteValue::String("ReadyNoop".to_owned()),
                        ),
                    ]),
                ),
                5 => (
                    "s3",
                    BTreeMap::from([
                        ("hasConflict".to_owned(), FiniteValue::Boolean(false)),
                        ("hasInserted".to_owned(), FiniteValue::Boolean(true)),
                        (
                            "phase".to_owned(),
                            FiniteValue::String("Consumed".to_owned()),
                        ),
                    ]),
                ),
                6 => (
                    "s4",
                    BTreeMap::from([
                        ("hasConflict".to_owned(), FiniteValue::Boolean(false)),
                        ("hasInserted".to_owned(), FiniteValue::Boolean(true)),
                        ("phase".to_owned(), FiniteValue::String("Open".to_owned())),
                    ]),
                ),
                7 => (
                    "s5",
                    BTreeMap::from([
                        ("hasConflict".to_owned(), FiniteValue::Boolean(false)),
                        ("hasInserted".to_owned(), FiniteValue::Boolean(true)),
                        (
                            "phase".to_owned(),
                            FiniteValue::String("ReadyWrite".to_owned()),
                        ),
                    ]),
                ),
                8 => (
                    "s6",
                    BTreeMap::from([
                        ("hasConflict".to_owned(), FiniteValue::Boolean(true)),
                        ("hasInserted".to_owned(), FiniteValue::Boolean(false)),
                        (
                            "phase".to_owned(),
                            FiniteValue::String("Consumed".to_owned()),
                        ),
                    ]),
                ),
                9 => (
                    "s7",
                    BTreeMap::from([
                        ("hasConflict".to_owned(), FiniteValue::Boolean(true)),
                        ("hasInserted".to_owned(), FiniteValue::Boolean(false)),
                        ("phase".to_owned(), FiniteValue::String("Open".to_owned())),
                    ]),
                ),
                10 => (
                    "s8",
                    BTreeMap::from([
                        ("hasConflict".to_owned(), FiniteValue::Boolean(true)),
                        ("hasInserted".to_owned(), FiniteValue::Boolean(false)),
                        (
                            "phase".to_owned(),
                            FiniteValue::String("ReadyRefused".to_owned()),
                        ),
                    ]),
                ),
                11 => (
                    "s9",
                    BTreeMap::from([
                        ("hasConflict".to_owned(), FiniteValue::Boolean(true)),
                        ("hasInserted".to_owned(), FiniteValue::Boolean(true)),
                        (
                            "phase".to_owned(),
                            FiniteValue::String("Consumed".to_owned()),
                        ),
                    ]),
                ),
                _ => unreachable!("private generated node invariant"),
            };
            Snapshot {
                model: MODEL,
                model_revision: MODEL_REVISION,
                node_id,
                revision: self.revision.to_string(),
                values,
            }
        }

        /// Apply the exact full-node/action edge synchronously. A failure leaves
        /// both private fields unchanged. No application callback runs here.
        pub fn send(&mut self, event: Event) -> Result<Receipt, TransitionError> {
            let successor = match (self.node, event) {
                (1, Event::Clash) => Some(9),
                (1, Event::CloseNoop) => Some(4),
                (1, Event::Insert) => Some(6),
                (1, Event::Same) => Some(1),
                (10, Event::ConsumeRefused) => Some(8),
                (2, Event::Clash) => Some(2),
                (2, Event::CloseRefused) => Some(3),
                (2, Event::Insert) => Some(2),
                (2, Event::Same) => Some(2),
                (3, Event::ConsumeRefused) => Some(11),
                (4, Event::ConsumeNoop) => Some(0),
                (6, Event::Clash) => Some(2),
                (6, Event::CloseWrite) => Some(7),
                (6, Event::Insert) => Some(6),
                (6, Event::Same) => Some(6),
                (7, Event::ConsumeWrite) => Some(5),
                (9, Event::Clash) => Some(9),
                (9, Event::CloseRefused) => Some(10),
                (9, Event::Insert) => Some(2),
                (9, Event::Same) => Some(9),
                _ => None,
            };
            let Some(node) = successor else {
                return Err(TransitionError {
                    reason: TransitionErrorReason::Disabled,
                    state: self.read(),
                });
            };
            let Some(revision) = self.revision.checked_add(1) else {
                return Err(TransitionError {
                    reason: TransitionErrorReason::RevisionExhausted,
                    state: self.read(),
                });
            };
            let before = self.read();
            self.node = node;
            self.revision = revision;
            Ok(Receipt {
                event,
                before,
                after: self.read(),
            })
        }
    }

    impl Default for FactEnvelopeMachineControl {
        fn default() -> Self {
            Self::new()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn revision_exhaustion_does_not_change_the_authority() {
            let mut authority = FactEnvelopeMachineControl::new();
            authority.revision = u64::MAX;
            let before = authority.read();
            for event in ALL_EVENTS {
                let error = authority
                    .send(*event)
                    .expect_err("no revision can follow u64::MAX");
                assert_eq!(error.state, before);
                assert_eq!(authority.read(), before);
            }
        }
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
