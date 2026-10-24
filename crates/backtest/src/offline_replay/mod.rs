//! Durable scripted paper replay. This is an engineering fixture, not a strategy.
//!
//! Each input is fsynced before the pure engine observes it. Recovery rebuilds
//! actual Risk approvals and accounting from the initial session and original
//! injected times. Approval credentials are never serialized. One process owns
//! a run; concurrent writers and power-loss durability are outside this scope.

use parangsae_execution::{
    PaperAction, PaperCommand, PaperEngine, PaperError, PaperOutcome, PaperSession, PaperSnapshot,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

mod journal;
use journal::{Journal, JournalError};

const VERSION: u16 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaperFixture {
    pub session: PaperSession,
    pub commands: Vec<PaperCommand>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
enum Record {
    Session { version: u16, session: PaperSession },
    Input { command: PaperCommand },
}

#[derive(Debug)]
pub enum ReplayError {
    Io(std::io::Error),
    CorruptJournal { record: u64, reason: &'static str },
    Capacity,
    NeedsRecovery,
    Codec(serde_json::Error),
    InvalidHeader,
    FixtureMismatch,
    Engine(PaperError),
}
impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "paper replay I/O: {error}"),
            Self::CorruptJournal { record, reason } => {
                write!(f, "paper journal record {record}: {reason}")
            }
            Self::Capacity => write!(f, "paper replay capacity exceeded"),
            Self::NeedsRecovery => {
                write!(f, "paper replay must reopen its journal after failed I/O")
            }
            Self::Codec(error) => write!(f, "paper replay input: {error}"),
            Self::InvalidHeader => write!(
                f,
                "paper replay requires one supported initial session header"
            ),
            Self::FixtureMismatch => write!(
                f,
                "paper fixture differs from the journal session or durable input prefix"
            ),
            Self::Engine(error) => write!(f, "paper session: {error}"),
        }
    }
}
impl std::error::Error for ReplayError {}
impl From<JournalError> for ReplayError {
    fn from(error: JournalError) -> Self {
        match error {
            JournalError::Io(error) => Self::Io(error),
            JournalError::Corrupt { record, reason } => Self::CorruptJournal { record, reason },
            JournalError::Capacity => Self::Capacity,
            JournalError::UncertainAppend => Self::NeedsRecovery,
            JournalError::Codec(error) => Self::Codec(error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaperStep {
    pub outcome: Result<PaperOutcome, PaperError>,
    pub snapshot: PaperSnapshot,
}

pub struct OfflineReplay {
    journal: Journal<Record>,
    engine: PaperEngine,
    session: PaperSession,
    inputs: Vec<PaperCommand>,
}

impl OfflineReplay {
    /// Creates a new journal file. Existing files are never overwritten.
    pub fn create(path: &Path, session: PaperSession) -> Result<Self, ReplayError> {
        let engine = PaperEngine::new(session.clone()).map_err(ReplayError::Engine)?;
        let mut journal = Journal::create(path)?;
        journal.append(&Record::Session {
            version: VERSION,
            session: session.clone(),
        })?;
        Ok(Self {
            journal,
            engine,
            session,
            inputs: Vec::new(),
        })
    }

    /// Validates all durable inputs before reconstructing a private engine.
    pub fn resume(path: &Path) -> Result<Self, ReplayError> {
        let (journal, records) = Journal::open(path)?;
        let mut records = records.into_iter();
        let session = match records.next() {
            Some(Record::Session {
                version: VERSION,
                session,
            }) => session,
            _ => return Err(ReplayError::InvalidHeader),
        };
        let mut engine = PaperEngine::new(session.clone()).map_err(ReplayError::Engine)?;
        let mut inputs = Vec::new();
        for record in records {
            match record {
                Record::Input { command } => {
                    // Refusals and latched protocol errors are deterministic
                    // engine observations and must be replayed as well.
                    let _ = engine.step(&command);
                    inputs.push(command);
                }
                Record::Session { .. } => return Err(ReplayError::InvalidHeader),
            }
        }
        Ok(Self {
            journal,
            engine,
            session,
            inputs,
        })
    }

    pub fn apply(&mut self, command: PaperCommand) -> Result<PaperStep, ReplayError> {
        if matches!(&command.action, PaperAction::Reconcile(cut)
            if cut.value().receipts.len() > self.session.limits.capacity())
        {
            return Err(ReplayError::Capacity);
        }
        self.journal.append(&Record::Input {
            command: command.clone(),
        })?;
        let outcome = self.engine.step(&command);
        self.inputs.push(command);
        Ok(PaperStep {
            outcome,
            snapshot: self.engine.snapshot(),
        })
    }

    pub fn snapshot(&self) -> PaperSnapshot {
        self.engine.snapshot()
    }
    pub fn command_count(&self) -> usize {
        self.inputs.len()
    }

    /// A restarted CLI must preserve its session and durable command prefix.
    pub fn validate_fixture(&self, fixture: &PaperFixture) -> Result<(), ReplayError> {
        if self.session != fixture.session || !fixture.commands.starts_with(&self.inputs) {
            return Err(ReplayError::FixtureMismatch);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{OfflineReplay, PaperFixture, Record, ReplayError};
    use crate::paper_cases;

    #[test]
    fn a_durable_input_before_engine_application_is_applied_once_on_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paper.journal");
        let (session, commands) = paper_cases::full_fill();
        let mut uninterrupted = parangsae_execution::PaperEngine::new(session.clone()).unwrap();
        for command in &commands {
            uninterrupted.step(command).unwrap();
        }
        let mut run = OfflineReplay::create(&path, session).unwrap();
        for command in &commands[..commands.len() - 1] {
            run.apply(command.clone()).unwrap().outcome.unwrap();
        }
        run.journal
            .append(&Record::Input {
                command: commands.last().unwrap().clone(),
            })
            .unwrap();
        drop(run);
        let resumed = OfflineReplay::resume(&path).unwrap();
        assert_eq!(resumed.snapshot(), uninterrupted.snapshot());
    }

    #[test]
    fn oversized_terminal_evidence_is_refused_before_cloning_or_journaling() {
        use parangsae_execution::PaperTerminal;
        use parangsae_risk::Limits;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paper.journal");
        let mut session = paper_cases::session();
        session.limits = Limits::new(
            session.limits.max_order(),
            session.limits.max_position(),
            session.limits.max_notional(),
            session.limits.max_session_loss(),
            session.limits.max_approval_age_us(),
            session.limits.max_mark_age_us(),
            1,
        )
        .unwrap();
        let mut run = OfflineReplay::create(&path, session).unwrap();
        let before = run.snapshot();
        let bytes = std::fs::read(&path).unwrap();
        let oversized = paper_cases::cut(
            10,
            PaperTerminal::Filled,
            vec![
                paper_cases::fill(4, 1, 100_000),
                paper_cases::fill(5, 2, 300_000),
            ],
        );
        assert!(matches!(run.apply(oversized), Err(ReplayError::Capacity)));
        assert_eq!(run.snapshot(), before);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn continuation_refuses_changed_session_or_durable_input_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paper.journal");
        let (session, commands) = paper_cases::full_fill();
        let mut run = OfflineReplay::create(&path, session.clone()).unwrap();
        run.apply(commands[0].clone()).unwrap().outcome.unwrap();
        drop(run);
        let run = OfflineReplay::resume(&path).unwrap();
        let mut fixture = PaperFixture { session, commands };
        run.validate_fixture(&fixture).unwrap();
        fixture.session.costs.taker_fee.0 += 1;
        assert!(matches!(
            run.validate_fixture(&fixture),
            Err(ReplayError::FixtureMismatch)
        ));
        fixture.session.costs.taker_fee.0 -= 1;
        fixture.commands[0].now = parangsae_execution::Mono::from_micros(99);
        assert!(matches!(
            run.validate_fixture(&fixture),
            Err(ReplayError::FixtureMismatch)
        ));
    }
}
