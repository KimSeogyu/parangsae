# C1 adapter obligations

Status: EL reviewed the source and ran the native SGD model checker. The finite
baseline completed, and both deliberate regressions failed their named
invariants. No generated owner or runtime refinement proof was produced.

This product checks one supported closing episode at the adapter boundary.
It does not replace CarryLedger, CarryEngine or C1ControlMachine. No new Rust
consumer, arming state, monetary writer, dependency or effect is introduced.
The project files select SGD v2 only for an exact finite model-only witness.
The Rust target/class name is schema configuration, not permission to build
or integrate a second generated owner.

## Product and scheduled work

The independent lifetimes are:

| Variable | Meaning |
| --- | --- |
| position | Open, Closing or Settled physical position with authoritative cuts. |
| capturedSettlement | Pending if ANY captured boundary remains unpaid; Paid if the unpaid projection of the whole map is empty. Paid history remains retained. |
| turn | Required risk timer, ready work, boundary drain, drained decision or completed episode. |
| observationCut | Receipt knowledge before E, at E or still unpublished at E. |
| economicBeforeE / sourceBeforeE | Physical fill and used source keys are each strictly before exclusive E. |
| closeRequested | The required timer turn requested the supported native close. |
| cutApplied / drained / outcome | Observations of this episode, not application capabilities. |

Init includes an already open position and a captured unpaid boundary. The
environment can pay the map independently, including after position settlement.
A payment leaving another boundary unpaid is an explicit stuttering event.
The model never interprets a single payment as proof that the whole map is paid.

ProcessRequiredRiskTick is required before any boundary work can advance.
It must request close even while capturedSettlement is Pending. Each boundary
case must then pass DrainAlreadyAdmittedCut before DecideAtSemanticE. This
program counter expresses the scheduling obligation; a merely enabled optional
BeginExit transition would not catch the old wait.

Only known exposure and a supported close request are in this episode's
precondition. Missing indispensable inputs, an unresolved order, unaffordable
close or another unsupported native case is not converted into a guaranteed
dispatch. Those cases retain their native technical refusal obligations.

An admitted receipt with R <= E may settle a physical fill and source key < E.
The actual model decision is at semantic E. A cut still unpublished at E cannot
be applied later to repair that decision. The E + 1 ms valuation flush is outside
the closing episode and does not extend it.

Closed requires both position Settled and the entire unpaid settlement
projection empty. MayReportSettledIdle describes eligibility for native
AllSettledIdle feedback. It is not a second arming owner. The existing generated
opportunity owner still controls what that feedback permits.

## Checks and faults

| Invariant | Obligation |
| --- | --- |
| RiskTickRequestsClose | A processed required risk turn cannot leave the supported close unrequested. |
| BoundaryDecisionFollowsDrain | No boundary result precedes required receipt/cut drain. |
| ClosedRequiresSettledProduct | Physical settlement alone cannot clear captured cash liability. |
| LawfulEqualityCompletes | A fully settled admitted equality case cannot be falsely rejected by a strict observation cut. |
| UnpublishedCannotRescue | R > E never becomes an applied cut in this episode. |
| PhysicalAndSourceCutExclusive | Inclusive knowledge never authorizes physical/source keys at or after E. |

C1AdapterObligationsBadWait.cfg restores the former payment wait and must fail
RiskTickRequestsClose. C1AdapterObligationsBadStrict.cfg preserves lawful cut
drain but restores the former strict observation result and must fail
LawfulEqualityCompletes. Both keep every baseline invariant.

The finite variables are all scalar. There is one initial state, deterministic
successor per named action and a finite graph. Payments, source classifications
and completion are abstract events. No amount, full map, receipt identity,
unbounded clock, quantity, floating point or account is encoded as an opaque
equality token.

The model has one final-close episode. That is a model scope, not a two-pair or
one-pair runtime cap. It does not duplicate the existing repeated-cycle
opportunity model.

## Native correspondence

Read native-binding.json before interpreting results. It maps the model to the
exact actor fixture names and guards at the actual 31-case source checkpoint.
It distinguishes author-passed native results from final independent acceptance.

The model assumes truthful classification of required timer turns, source
cuts, complete position settlement and the entire unpaid map. It cannot force
an arbitrary caller to emit those events truthfully. The mapping is a review
and behavior-test obligation, not an AST binding or a refinement proof.

The raw-to-executed price owner, financial arithmetic, qS capture, asset fees,
margin, reservations, liquidity and fixed-point rounding remain in the native
owners. Existing CarryCapital and C1OpportunityControl evidence stays separate.
Neither old model proves the added adapter obligations.

There is no fairness or liveness claim. In particular Spec permits stuttering;
the scheduler contract prohibits advancing work past an unprocessed required
turn, but does not prove that an external caller supplies every timer event.
No I/O, persistence, restart, multiple-writer, exchange or readiness claim follows.

## Actual model evidence

The native model-only run completed breadth-first exploration with all ten
scalar variables retained: 34 nodes, 66 labeled edges and no queued state.
The old-wait control failed RiskTickRequestsClose immediately after the
required timer was processed. The strict-observation control failed
LawfulEqualityCompletes after an admitted equality cut and all captured
settlements were paid. These are invariant counterexamples, not parser errors.

Read-back of the exported graph also found settled positions with pending
payment, lawful equality completion, unpublished technical refusal and
invalid physical/source-cut refusal. Those cases are reachable rather than
vacuously excluded. The evidence is in the approved private phase directory:
adapter-contract-native/native-responses.json and positive/{tlc.log,
verification.json,automaton.json}.

The launcher's command cap was 512 MiB with one worker. The actual TLC header
reported 455 MB heap and 64 MB offheap. No 256 MiB or hosted execution is claimed.
Kani and new native application tests were not run by this model unit. The
separate actor receipt binds its actual 31 cases; the graph does not prove that
their native classifications are truthful.

## Proposed repeatable hook

The included runner is a proposed repeatable hook, not the launcher used for
the model evidence above. It has been syntax-checked, but not executed here.
It uses the pinned JAR, 512 MiB heap, one worker and a 120 second timeout per
case. EL must allocate the Java lane before another local invocation:

    python3 crates/risk/spec/c1-adapter/verify.py \
      --jar specs/tla/tla2tools.jar \
      --out /an/approved/new/private/c1-adapter-evidence

The three project files were passed to native sgd_model in separate model-only
attempts. Do not use sgd_build or generate a Rust consumer under this
authorization. Preserve the actual TLC/inspection receipts; no repeat run is
needed merely to reproduce their counts.

ci-registration-proposal.json is a proposal for Ops/EL. No workflow or
required-check change is included here. This crate-local path is not currently
discovered by the hosted tla job.
