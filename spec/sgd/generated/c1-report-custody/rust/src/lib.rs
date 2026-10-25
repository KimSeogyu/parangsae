// Generated retained operation. Regenerate from the verified model.
//! One synchronous operation retains its concrete payload on ordinary returns.
//! The callback borrows the payload. Its actual Result selects the model outcome.
//! No promise covers panic, abort, process loss, interior mutation, caller disposal,
//! durable custody, or application-wide source coverage.
#![forbid(unsafe_code)]

mod machine;
pub use machine::{
    FiniteValue, Snapshot, TransitionError, TransitionErrorReason, MODEL, MODEL_REVISION,
};

#[derive(Debug)]
pub enum FinalizationStatus<R, E> {
    /// The model refused admission. The callback did not run.
    Refused(TransitionError),
    /// The actual callback result, retained independently of the payload.
    Completed(Result<R, E>),
    /// Preserve the observed callback result even if the completion transition fails.
    TransitionRejected {
        outcome: Result<R, E>,
        error: TransitionError,
    },
}

/// Fields are private: callers cannot construct an error-only finalization.
#[must_use = "finalization retains the original payload and the observed operation result"]
#[derive(Debug)]
pub struct Finalized<T, R, E> {
    payload: T,
    status: FinalizationStatus<R, E>,
    snapshot: Snapshot,
}

impl<T, R, E> Finalized<T, R, E> {
    pub fn payload(&self) -> &T {
        &self.payload
    }
    pub fn status(&self) -> &FinalizationStatus<R, E> {
        &self.status
    }
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// Explicitly transfer all owned parts to the caller. This is not a durable acknowledgement.
    pub fn into_parts(self) -> (T, FinalizationStatus<R, E>, Snapshot) {
        (self.payload, self.status, self.snapshot)
    }
}

/// Attempt once. A model refusal does not call the writer. Both callback outcomes
/// retain the supplied payload, including after successful reporting.
pub fn finalize<T, R, E>(
    payload: T,
    writer: impl FnOnce(&T) -> Result<R, E>,
) -> Finalized<T, R, E> {
    let mut machine = machine::ReportCustody::new();
    if let Err(error) = machine
        .send(machine::Event::from_model_name("StartReport").expect("validated operation action"))
    {
        return Finalized {
            payload,
            status: FinalizationStatus::Refused(error),
            snapshot: machine.read(),
        };
    }
    let outcome = writer(&payload);
    let event = if outcome.is_ok() {
        machine::Event::from_model_name("ReportSucceeded").expect("validated operation action")
    } else {
        machine::Event::from_model_name("ReportFailed").expect("validated operation action")
    };
    let status = match machine.send(event) {
        Ok(_) => FinalizationStatus::Completed(outcome),
        Err(error) => FinalizationStatus::TransitionRejected { outcome, error },
    };
    Finalized {
        payload,
        status,
        snapshot: machine.read(),
    }
}
