# C1 opportunity control

This v2 finite-owned model protects one control object held privately by the
continuous backtest actor. It owns arming, pending evaluation, reservation,
busy, temporary disabled, stopped and deferred-epoch state. Signal code owns
causal payload/window/key classification only. It has no second Armed flag.

The model has no money, price, timestamp, pair-count or trade-count variable.
Repeated cycles are graph loops. A two-cycle integration example does not cap
the actual actor at two pairs.

## Checked contract

The eight scalar variables are mode, armed, evaluation, settledIdle,
fundingPending, healthy, epochPending and stoppedEver. Every variable remains
in the exact finite owner.

An eligible evaluation reserves one intent only after a known-false evaluation
armed the idle control. It consumes that evaluation and disarms. Ordinary
pretrade size refusal does not fabricate a false financing signal.

A prospective policy/source epoch can disarm only healthy fully settled idle
state with no unpaid funding. It preserves actual native keys, money and loss
history. It does not enable temporary Disabled and cannot reset Stopped.
Busy/Reserved work defers the epoch.

fundingPending represents ANY unpaid retained boundary. The actor sends
RecordFundingSettled only when the whole actual funding map is settled.
One receipt clearing one of several boundaries is insufficient. Quantities
zero do not alone establish settled idle.

Not-yet-available input preserves arming. An actual idle Gap/Quarantined
discontinuity first uses the guarded RequestEpoch/NewEpoch source-epoch
sequence to invalidate old arming, then disables input. Active missing
indispensable input is a technical Stop.

## Native event boundary

Read api-mapping.json before integration. The actor prepares every fallible
source/key/clock/actor/numeric check before consuming a generated event. It
then applies the prevalidated native payload and Keep/Advance watermark choice
infallibly. Generated success alone does not mean the source key was consumed.

The generated owner is the only writer of its control fields. The actual
CarryEngine/CarryLedger remains the financial and order authority. No model
action proves that a cash, price, source-key or receipt classification is true.
A successful Observe may accompany halted risk and must map to Stop.

Only a successful eligible transition from armed Idle to Reserved authorizes
one attempt to call the actual protected native order path. Rejected native
Plan/Dispatch does not roll back arming or replay the same consumed key.

## Model checks and controlled failures

Root ran the installed native SGD model/build/inspect tools on the identical
private source. The positive finite exploration completed with no errors and
an empty queue. All eight variables were retained, with 25 concrete nodes
and 129 edges. This is local evidence, not a hosted workflow claim.

The positive configuration fixes both bug constants to FALSE. Separate
negative projects enable one deliberately faulty behavior:

| Project | Expected invariant failure |
| --- | --- |
| project-bad-disarm.json | ReservedBusyDisarmed catches a reserved intent whose arming was not cleared. |
| project-bad-restart.json | StoppedCannotRestart catches a reset from stopped control to idle. |

Both failures were observed through the real native TLC runner. Neither is a
reported existing application bug. Never enable these constants in the
positive project or weaken the invariants to accept a faulty model.

Use native sgd_model with project.json and the pinned repository jar for a
model-only review, and the two negative projects in separate output folders.
Use sgd_inspect to read the complete automaton. Executable generation requires
sgd_build; imported DOT is not executable authority. The local generated/
directory is model-only review output and contains no executable crate.
The executable inventory lives at spec/sgd/generated/c1-control from the
repository root. Never run model-only generation over that executable output.

The installed runner uses its supported 512 MiB Java heap and one worker.
Do not use inherited JVM overrides or label this as a 256 MiB run.

## Generated Rust publication

Root performed fresh controlled generation from this repository-path project
into spec/sgd/generated/c1-control. Its inventory and provenance are retained. The
generated Rust source matches the independently checked private crate. Do not
edit those generated files or substitute private provenance.

The actual repository-path generated package is
sgd-owned-c1opportunitycontrol-50dd0338899d. Its Rust import is
sgd_owned_c1opportunitycontrol_50dd0338899d. Both are recorded in
spec/sgd/generated/c1-control/rust/Cargo.toml and its generated README. Confirm them before adding the sole
Backtest path dependency. Ops reviews that exact module/edge and manifest.
Do not inline the owner as a mutable application module.

The standalone crate must remain outside existing Cargo workspace members.
Placing it under crates/risk, even with an exact workspace.exclude, causes
multiple workspace roots on the checked Cargo version. The root workspace
excludes the actual nonmember path spec/sgd/generated/c1-control/rust.
The earlier local executable was retired by controlled model-only generation,
not by editing its generated manifest or state owner.

The actual generated API is C1ControlMachine::new(), read() and send(Event).
Exact model action IDs are preserved. Inspect both before and after snapshots;
an EvaluateEligible accepted while unarmed is not a reservation. Rejection
preserves owner state/revision. Accepted self loops may advance revision.
Read the generated Rust README for the exact native contract.

Independent native readback constructed fresh owners only through the public
API. It checked every reachable node/event pair through canonical prefixes,
full scalar snapshots, rejection immutability, accepted self-loop revisions,
eight repeated cycles, Stop and pending-funding/epoch behavior. Deliberately
incorrect expected edges and values failed the harness, then the correct
expectations passed. This is not a test of every unbounded history or of the
native financial/event classifier.

The generated crate's own revision-exhaustion test also passed on an isolated
byte-identical copy. That test uses its internal test-only revision setup; the
independent public harness did not claim to reach u64::MAX. The temporary build
target was removed. No broad application or market test was run by this unit.

## Limits

This is finite control evidence, not a proof of monetary arithmetic,
affordability, clock inequalities, real key ordering, source truth,
historical settlement/margin, actor integration or production durability.
Do not encode an account as an opaque string and call equality an accounting
proof.

Each new() creates an independent Init object. The real actor must retain one
owner across continuous OOS folds/cycles. Reset prevention, durable history,
crash recovery, external-effect atomicity and liveness need separate
contracts and native evidence. StoppedWait/Replay are explicit self loops;
they do not promise progress.

Native RiskObserve classification, authoritative complete cuts, all-funding
settlement and exact key-watermark behavior must be tested at the actual
adapter. The existing CarryCapital model is separate fixed-price design
evidence, not a refinement proof of this control or the financial runtime.
