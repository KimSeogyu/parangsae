# Owned execution outcomes and evidence lifetime

This ordinary bounded TLC safety model separates native execution, persistence
and owned evidence. It does not add a generated SDK owner, Rust dependency,
monetary writer or shared mutable file. Rust study and Python wrapper retain
separate concrete invocation owners.

## State and contract

Two execution units have distinct run-start witnesses. `TrialOf` maps them to
charged trial identities: the same-trial cfg maps both to A; the distinct-trials
cfg maps them to A/B. Both durable unit witnesses remain. `chargedTrials` is
exactly their set image, not the unit count and not a claim/PID/exit count.
The actual binding retains each run_id and its actual trial_id. Partial/corrupt
or unreadable ledger evidence remains uncertain and cannot invent a charge.

All facts have the uniform shape `<<integer, string>>`. Unit0 is a sentinel for
spawn attempt/observed process facts; units1/2 own entry/outcome facts. The ghost
fact/payload/start histories express retention obligations. They are not restored
or serialized application owners.

| Invariant | Obligation |
| --- | --- |
| KnownFactsRetained | A later failure cannot erase known attempt/process/entry/outcome facts. |
| KnownPayloadRetained | An observed complete/technical payload stays resident or has truthful custody acknowledgement. |
| DurableUnitStartsRetained | Every known actual unit-start witness remains. |
| ChargeIsTrialImage | Charged trial set is precisely the image of actual durable unit starts. |
| BothUnitStartWitnessesRetained | Deduplicating charge never drops either unit witness. |
| TwoUnitChargeCardinality | Two starts charge1 for A/A and2 for A/B. |
| KnownEntryCannotBecomePreSpawn | Known attempt/entry cannot become pre-spawn refusal. |
| NoAutomaticRetry | One invocation has at most one spawn attempt. |
| OutcomeNotInvented | A unit cannot become both known Complete and Technical. |

`ExecutionComplete` means native completion known, not durable reports. I/O failure
is an independent axis. A failed stream/result/terminal append must retain known
PID/exit/outcome and owned payload. Spawn attempted without a returned PID remains
unresolved. Final failure can return the evidence owner; it cannot reset truth or
automatically retry. Only a truthful acknowledgement permits resident release.

## Checks

Root actually ran this exact model and three cfgs in isolated directories with
TLC2.19/TLA+1.7.4, `-Xmx512m`, one worker, seed/fingerprint0. Same-trial and
distinct-trial baselines completed with all invariants and no queued state.
The deliberate BadFallback failed `KnownEntryCannotBecomePreSpawn` after an
attempted spawn, I/O failure and false pre-spawn report. It is a model fault
control, not a native product RED. Private raw source/cfg/commands/logs/results
are bound by native-binding.json. A prior heterogeneous string/tuple-set model
representation error is retained separately and is not reported as a PASS.

The denominator is two finite units, two possible charged identities, fixed
outcome tags, one optional process attempt and bounded sets. Reachable-state
counts are retained in evidence, not used as a quality measure or unbounded proof.
Set-valued variables and parameterized actions make this an ordinary TLC model,
not SDK v2 scalar IR or an emitter input. No generated owner is proposed.

The initial source patch syntax-checked the included runner separately from the
launcher used for the model evidence above. The maintenance integration now
executes it from `npm --prefix spec/sgd run verify`, with a fresh owned output
directory. Both baselines and the named fallback fault expectation passed through
that entry point. After EL assigns a Java lane, it can also run directly:

```sh
python3 crates/risk/spec/evidence-lifecycle/verify.py \
  --jar specs/tla/tla2tools.jar \
  --out /an/approved/new/private/evidence-lifetime-check
```

It verifies the pinned JAR, rejects JVM override variables, atomically claims a
fresh output directory and runs three cases sequentially with 512MiB heap,
one worker and120s timeout per case. It requires full baseline completion or the
exact named invariant error; parser/runtime failure cannot satisfy the bad case.
It records source/cfg identities, command, PID/exit when observed and raw logs.
No auto retry or output reuse. Expected-case TLC state caches alone are removed;
source/cfg/log/result evidence remains. Unexpected cases preserve their state dir.
The512MiB limit is the Java heap, not total process memory/offheap.

## Concrete binding and limits

Read native-binding.json. Rust must keep the actual unit outcome before terminal
logging and retain previous units when a later start write fails. Python must keep
one monotone Invocation across stream/result/fsync errors. The actual synthetic
filesystem/child fault observers are separate evidence, not consequences of TLC.

The model assumes truthful entry, outcome, complete ledger-start identity and
custody acknowledgement. It does not prove file bytes/fsync, crash durability,
eventual output, fairness, OS/process supervision, source clocks, actor scheduling,
financial arithmetic or formal runtime refinement. If all evidence sinks fail,
retain owned payload/error/uncertainty rather than claim guaranteed persistence.
It does not change strategy semantics or establish statistical edge/readiness.

ci-registration-proposal.json preserves the original proposal for the tla job.
The selected integration instead uses the existing verifier. No workflow,
required-check, SDK, generated source or branch-protection change is included.
