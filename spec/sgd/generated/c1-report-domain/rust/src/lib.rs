// Generated from a checked finite model. Do not edit.
#![forbid(unsafe_code)]

pub const MODEL: &str = "ReportCustody";
pub const MODEL_REVISION: &str = "8fcde862c6c58034079226ac693b973717a75b17a1ed7717946b34b8d64fa704";
const INITIAL_STATE: usize = 1;
const TRANSITIONS: &[(usize, usize, usize)] =
    &[(0, 3, 0), (1, 2, 2), (2, 0, 0), (2, 1, 3), (3, 3, 3)];
const OPERATION_ROLES: &[(usize, usize, usize)] = &[(2, 1, 0)];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Value {
    String(&'static str),
    Boolean(bool),
    Integer(&'static str),
}
static VALUES_0: &[(&str, Value)] = &[
    ("phase", Value::String("Failed")),
    ("ready", Value::Boolean(true)),
    ("resident", Value::Boolean(true)),
];
static VALUES_1: &[(&str, Value)] = &[
    ("phase", Value::String("Ready")),
    ("ready", Value::Boolean(true)),
    ("resident", Value::Boolean(true)),
];
static VALUES_2: &[(&str, Value)] = &[
    ("phase", Value::String("Reporting")),
    ("ready", Value::Boolean(true)),
    ("resident", Value::Boolean(true)),
];
static VALUES_3: &[(&str, Value)] = &[
    ("phase", Value::String("Succeeded")),
    ("ready", Value::Boolean(true)),
    ("resident", Value::Boolean(true)),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub node_id: &'static str,
    pub revision: u64,
    pub values: &'static [(&'static str, Value)],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Receipt {
    pub before: Snapshot,
    pub after: Snapshot,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionError {
    Disabled,
    RevisionExhausted,
}
#[derive(Debug)]
pub enum OperationResult<T, E> {
    Refused(TransitionError),
    Completed {
        outcome: Result<T, E>,
        start: Receipt,
        completion: Receipt,
    },
    CompletionRejected {
        outcome: Result<T, E>,
        start: Receipt,
        error: TransitionError,
    },
}
#[must_use = "the operation result contains its observed status and snapshot"]
#[derive(Debug)]
pub struct SyncOperation<T, E> {
    status: OperationResult<T, E>,
    snapshot: Snapshot,
}
impl<T, E> SyncOperation<T, E> {
    pub fn status(&self) -> &OperationResult<T, E> {
        &self.status
    }
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    pub fn into_parts(self) -> (OperationResult<T, E>, Snapshot) {
        (self.status, self.snapshot)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicEvent {
    Event3,
}
impl PublicEvent {
    pub fn from_model_name(name: &str) -> Option<Self> {
        match name {
            "Wait" => Some(Self::Event3),
            _ => None,
        }
    }
    fn index(self) -> usize {
        match self {
            Self::Event3 => 3,
        }
    }
}

struct Machine {
    state: usize,
    revision: u64,
}
impl Machine {
    fn new() -> Self {
        Self {
            state: INITIAL_STATE,
            revision: 0,
        }
    }
    fn read(&self) -> Snapshot {
        let (node_id, values) = match self.state {
            0 => ("s0", VALUES_0),
            1 => ("s1", VALUES_1),
            2 => ("s2", VALUES_2),
            3 => ("s3", VALUES_3),
            _ => unreachable!("invalid private graph state"),
        };
        Snapshot {
            node_id,
            revision: self.revision,
            values,
        }
    }
    fn transition(&mut self, event: usize) -> Result<Receipt, TransitionError> {
        let next = TRANSITIONS
            .iter()
            .find(|(state, action, _)| *state == self.state && *action == event)
            .map(|(_, _, target)| *target)
            .ok_or(TransitionError::Disabled)?;
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(TransitionError::RevisionExhausted)?;
        let before = self.read();
        self.state = next;
        self.revision = revision;
        Ok(Receipt {
            before,
            after: self.read(),
        })
    }
}

pub trait SyncPorts {
    type Payload;
    type Port0Input;
    type Port0Output;
    type Port0Error;
    fn writer(
        &mut self,
        payload: &Self::Payload,
        input: Self::Port0Input,
    ) -> Result<Self::Port0Output, Self::Port0Error>;
}

pub struct ReportCustodySyncDomain<P: SyncPorts> {
    machine: Machine,
    ports: P,
    payload: P::Payload,
}
impl<P: SyncPorts> ReportCustodySyncDomain<P> {
    pub fn new(ports: P, payload: P::Payload) -> Self {
        Self {
            machine: Machine::new(),
            ports,
            payload,
        }
    }
    pub fn read(&self) -> Snapshot {
        self.machine.read()
    }
    pub fn payload(&self) -> &P::Payload {
        &self.payload
    }
    pub fn into_parts(self) -> (P::Payload, P, Snapshot) {
        let snapshot = self.machine.read();
        (self.payload, self.ports, snapshot)
    }
    pub fn send(&mut self, event: PublicEvent) -> Result<Receipt, TransitionError> {
        self.machine.transition(event.index())
    }
    pub fn report(&mut self, input: P::Port0Input) -> SyncOperation<P::Port0Output, P::Port0Error> {
        if self.machine.revision > u64::MAX - 2 {
            return SyncOperation {
                status: OperationResult::Refused(TransitionError::RevisionExhausted),
                snapshot: self.machine.read(),
            };
        }
        let start = match self.machine.transition(OPERATION_ROLES[0].0) {
            Ok(receipt) => receipt,
            Err(error) => {
                return SyncOperation {
                    status: OperationResult::Refused(error),
                    snapshot: self.machine.read(),
                }
            }
        };
        let outcome = <P as SyncPorts>::writer(&mut self.ports, &self.payload, input);
        let completion_event = if outcome.is_ok() {
            OPERATION_ROLES[0].1
        } else {
            OPERATION_ROLES[0].2
        };
        let status = match self.machine.transition(completion_event) {
            Ok(completion) => OperationResult::Completed {
                outcome,
                start,
                completion,
            },
            Err(error) => OperationResult::CompletionRejected {
                outcome,
                start,
                error,
            },
        };
        SyncOperation {
            status,
            snapshot: self.machine.read(),
        }
    }
}
