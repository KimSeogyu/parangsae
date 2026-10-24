#[path = "common/paper_cases.rs"]
mod cases;

use cases::*;
use cucumber::{then, when, World as _};
use parangsae_backtest::offline_replay::{OfflineReplay, PaperFixture, PaperStep, ReplayError};
use parangsae_core::{EventTs, Observation, ReceivedTs, Timestamps, UtcMicros};
use parangsae_execution::{
    PaperAction, PaperCommand, PaperEngine, PaperReport, PaperSnapshot, PaperTerminal,
};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug, Default, cucumber::World)]
struct InputWorld {
    files: Option<tempfile::TempDir>,
    before: Option<PaperSnapshot>,
    after: Option<PaperSnapshot>,
    before_bytes: Vec<u8>,
    after_bytes: Vec<u8>,
    before_count: usize,
    after_count: usize,
    applied: Option<Result<PaperStep, ReplayError>>,
    recovered: Option<Result<PaperSnapshot, String>>,
    continued: Option<PaperSnapshot>,
    wire_refusal: String,
    cli: Option<Output>,
}

impl InputWorld {
    fn journal(&mut self) -> PathBuf {
        self.files
            .get_or_insert_with(|| tempfile::tempdir().unwrap())
            .path()
            .join("paper.journal")
    }

    fn fixture(&mut self) -> PathBuf {
        let path = self.journal().with_extension("fixture.json");
        let (session, commands) = full_fill();
        fs::write(
            &path,
            serde_json::to_vec(&PaperFixture { session, commands }).unwrap(),
        )
        .unwrap();
        path
    }
}

fn altered_fill(
    at: u64,
    id: u64,
    ticks: i64,
    field: &str,
) -> Observation<parangsae_execution::PaperFill> {
    let receipt = fill(at, id, ticks);
    let mut value = receipt.value().clone();
    match field {
        "fill id" | "later receipt id" => value.fill_id = 0,
        "fill quantity" | "later receipt quantity" => value.qty = qty(0),
        "fill price" | "later receipt price" => value.reference_price = price(0),
        _ => unreachable!(),
    }
    receipt.with_value(value)
}

fn invalid_command(field: &str) -> PaperCommand {
    match field {
        "fill id" | "fill quantity" | "fill price" => {
            deliver(4, &altered_fill(4, 1, 400_000, field))
        }
        "later receipt id" | "later receipt quantity" | "later receipt price" => cut(
            6,
            PaperTerminal::Filled,
            vec![fill(4, 1, 100_000), altered_fill(5, 2, 300_000, field)],
        ),
        "mark price" => {
            let mut value = command(4, PaperAction::ReconcilePortfolio);
            value.mark = value.mark.with_value(price(0));
            value
        }
        "plan quantity" => {
            let mut value = plan(4, 2);
            let PaperAction::Plan { qty: quantity, .. } = &mut value.action else {
                unreachable!()
            };
            *quantity = qty(0);
            value
        }
        _ => unreachable!(),
    }
}

#[when(regex = r"^a native command has invalid (.+)$")]
fn invalid_native(world: &mut InputWorld, field: String) {
    let path = world.journal();
    let mut run = OfflineReplay::create(&path, session()).unwrap();
    for value in prefix() {
        run.apply(value).unwrap().outcome.unwrap();
    }
    let value = invalid_command(&field);
    let wire = serde_json::to_vec(&value).unwrap();
    world.wire_refusal = serde_json::from_slice::<PaperCommand>(&wire)
        .unwrap_err()
        .to_string();
    world.before = Some(run.snapshot());
    world.before_count = run.command_count();
    world.before_bytes = fs::read(&path).unwrap();
    world.applied = Some(run.apply(value));
    world.after = Some(run.snapshot());
    world.after_count = run.command_count();
    world.after_bytes = fs::read(&path).unwrap();
    drop(run);
    world.recovered = Some(match OfflineReplay::resume(&path) {
        Ok(mut restored) => {
            let state = restored.snapshot();
            restored
                .apply(deliver(4, &fill(4, 1, 400_000)))
                .unwrap()
                .outcome
                .unwrap();
            let continued = restored.snapshot();
            drop(restored);
            let reopened = OfflineReplay::resume(&path).unwrap();
            assert_eq!(reopened.snapshot(), continued);
            world.continued = Some(reopened.snapshot());
            Ok(state)
        }
        Err(error) => Err(error.to_string()),
    });
}

#[then("the invalid native command is refused before journaling")]
fn native_refused(world: &mut InputWorld) {
    let outcome = world
        .applied
        .as_ref()
        .unwrap()
        .as_ref()
        .map(|step| &step.outcome);
    let Some(Err(ReplayError::InvalidInput(error))) = &world.applied else {
        panic!("native apply={outcome:?}; resume={:?}", world.recovered);
    };
    assert!(world.wire_refusal.starts_with(&error.to_string()));
}

#[then("the journal bytes, command count and engine snapshot are unchanged")]
fn unchanged(world: &mut InputWorld) {
    assert_eq!(world.after_bytes, world.before_bytes);
    assert_eq!(world.after_count, world.before_count);
    assert_eq!(world.after, world.before);
}

#[then("the journal resumes and a valid fill can still settle once")]
fn recoverable(world: &mut InputWorld) {
    assert_eq!(
        world.recovered.as_ref().unwrap().as_ref().unwrap(),
        world.before.as_ref().unwrap()
    );
    let result = world.continued.as_ref().unwrap();
    assert_eq!(result.cash.0, 987_991_598_800);
    assert_eq!(result.fees_paid.0, 6_001_200);
    assert_eq!(result.position.0, 400_000);
    assert_eq!(result.fills.len(), 1);
}

fn schema_valid_input(kind: &str) -> PaperCommand {
    match kind {
        "a valid fill" => deliver(4, &fill(4, 1, 400_000)),
        "a future event mark" | "a future receipt mark" | "a stale mark" => {
            let mut input = command(4, PaperAction::ReconcilePortfolio);
            let mut clocks = input.mark.timestamps();
            match kind {
                "a future event mark" => {
                    clocks.event_ts = EventTs::from_micros(UtcMicros::from_micros(AT + 100))
                }
                "a future receipt mark" => {
                    clocks.received_ts = ReceivedTs::from_micros(UtcMicros::from_micros(AT + 100))
                }
                _ => {
                    let old = UtcMicros::from_micros(AT - 60_000_001);
                    clocks = Timestamps {
                        event_ts: EventTs::from_micros(old),
                        available_ts: parangsae_core::AvailableTs::from_micros(old),
                        received_ts: ReceivedTs::from_micros(old),
                    };
                }
            }
            input.mark = Observation::new(clocks, *input.mark.value());
            input
        }
        "backward clocks" => command(2, PaperAction::ReconcilePortfolio),
        "an unknown order" => command(
            4,
            PaperAction::Deliver(observation(
                PaperReport::Ack {
                    client_order_id: id(2),
                },
                4,
            )),
        ),
        "zero cumulative" => cut(4, PaperTerminal::Filled, vec![]),
        "zero evidence id" | "an incomplete cut" => {
            let mut input = cut(4, PaperTerminal::Filled, vec![fill(4, 1, 400_000)]);
            let PaperAction::Reconcile(observed) = &mut input.action else {
                unreachable!()
            };
            let mut value = observed.value().clone();
            if kind == "zero evidence id" {
                value.evidence_id = 0;
            } else {
                value.complete = false;
            }
            *observed = observed.with_value(value);
            input
        }
        "extreme fill values" => {
            let observed = fill(4, u64::MAX, i64::MAX);
            let mut value = observed.value().clone();
            value.reference_price = price(i64::MAX);
            deliver(4, &observed.with_value(value))
        }
        _ => unreachable!(),
    }
}

#[when(regex = r"^the native replay receives (.+)$")]
fn valid_native(world: &mut InputWorld, kind: String) {
    let path = world.journal();
    let initial = session();
    let mut commands = prefix();
    let mut run = OfflineReplay::create(&path, initial.clone()).unwrap();
    for input in &commands {
        run.apply(input.clone()).unwrap().outcome.unwrap();
    }
    let input = schema_valid_input(&kind);
    let restored: PaperCommand =
        serde_json::from_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    assert_eq!(restored, input);
    let step = run.apply(input.clone()).unwrap();
    if kind == "a valid fill" {
        assert!(step.outcome.is_ok());
    } else {
        assert!(step.outcome.is_err(), "{kind}: {:?}", step.outcome);
    }
    world.after = Some(step.snapshot);
    world.after_count = run.command_count();
    commands.push(input);
    drop(run);
    let restored = OfflineReplay::resume(&path).unwrap();
    restored
        .validate_fixture(&PaperFixture {
            session: initial,
            commands,
        })
        .unwrap();
    world.before_count = restored.command_count();
    world.recovered = Some(Ok(restored.snapshot()));
}

#[then("the schema-valid command is durable and restores its original clocks and fields")]
fn valid_durable(world: &mut InputWorld) {
    assert_eq!(world.after_count, 4);
    assert_eq!(world.before_count, world.after_count);
}

#[then("its result and economics survive restart")]
fn valid_recovered(world: &mut InputWorld) {
    assert_eq!(
        world.recovered.as_ref().unwrap().as_ref().unwrap(),
        world.after.as_ref().unwrap()
    );
}

#[when(
    regex = r"^the (partial|cancel race|unknown|halt) script is replayed through native admission$"
)]
fn existing_script(world: &mut InputWorld, kind: String) {
    let (initial, commands) = match kind.as_str() {
        "partial" => partial(),
        "cancel race" => cancel_race(),
        "unknown" => unknown(),
        "halt" => halted(),
        _ => unreachable!(),
    };
    let path = world.journal();
    let mut model = PaperEngine::new(initial.clone()).unwrap();
    let mut run = OfflineReplay::create(&path, initial.clone()).unwrap();
    for input in &commands {
        assert_eq!(run.apply(input.clone()).unwrap().outcome, model.step(input));
    }
    world.after = Some(model.snapshot());
    drop(run);
    let resumed = OfflineReplay::resume(&path).unwrap();
    assert_eq!(resumed.command_count(), commands.len());
    resumed
        .validate_fixture(&PaperFixture {
            session: initial,
            commands,
        })
        .unwrap();
    world.recovered = Some(Ok(resumed.snapshot()));
}

fn cli(mode: &str, fixture: &Path, journal: &Path, stop: usize) -> Output {
    Command::new(env!("CARGO_BIN_EXE_paper_replay"))
        .arg(mode)
        .arg(fixture)
        .arg(journal)
        .arg("--stop-after")
        .arg(stop.to_string())
        .output()
        .unwrap()
}

#[when("a fresh CLI run requests a stop count beyond its fixture")]
fn invalid_stop(world: &mut InputWorld) {
    let fixture = world.fixture();
    let journal = world.journal();
    world.cli = Some(cli("run", &fixture, &journal, 7));
}

#[then("the CLI refuses without creating a journal")]
fn no_header(world: &mut InputWorld) {
    let result = world.cli.as_ref().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("stop count"));
    assert!(
        !world.journal().exists(),
        "invalid --stop-after created a journal"
    );
}

#[then("correcting the same fresh CLI command succeeds")]
fn corrected_run(world: &mut InputWorld) {
    let fixture = world.fixture();
    let journal = world.journal();
    let output = cli("run", &fixture, &journal, 6);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        OfflineReplay::resume(&journal)
            .unwrap()
            .snapshot()
            .fills
            .len(),
        1
    );
}

#[when("a fresh CLI run requests zero commands")]
fn zero_stop(world: &mut InputWorld) {
    let fixture = world.fixture();
    let journal = world.journal();
    let output = cli("run", &fixture, &journal, 0);
    assert!(output.status.success());
    assert_eq!(OfflineReplay::resume(&journal).unwrap().command_count(), 0);
}

#[then("its header-only journal resumes to the valid complete result")]
#[then("correcting the resume command succeeds")]
fn corrected_resume(world: &mut InputWorld) {
    let fixture = world.fixture();
    let journal = world.journal();
    let output = cli("resume", &fixture, &journal, 6);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = OfflineReplay::resume(&journal).unwrap().snapshot();
    assert_eq!(result.cash.0, 987_991_598_800);
    assert_eq!(result.fills.len(), 1);
}

#[when(
    regex = r"^a CLI resume requests an invalid (upper|lower|upper with partial tail) stop count$"
)]
fn invalid_resume(world: &mut InputWorld, bound: String) {
    let fixture = world.fixture();
    let journal = world.journal();
    assert!(cli("run", &fixture, &journal, 4).status.success());
    if bound == "upper with partial tail" {
        fs::OpenOptions::new()
            .append(true)
            .open(&journal)
            .unwrap()
            .write_all(b"PR")
            .unwrap();
    }
    world.before_bytes = fs::read(&journal).unwrap();
    world.cli = Some(cli(
        "resume",
        &fixture,
        &journal,
        if bound.starts_with("upper") { 7 } else { 3 },
    ));
    world.after_bytes = fs::read(&journal).unwrap();
}

#[then("the CLI refuses and its existing journal bytes stay unchanged")]
fn resume_unchanged(world: &mut InputWorld) {
    assert!(!world.cli.as_ref().unwrap().status.success());
    assert_eq!(world.after_bytes, world.before_bytes);
}

#[tokio::main]
async fn main() {
    InputWorld::cucumber()
        .run_and_exit(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/paper_input_admission.feature"),
        )
        .await;
}
