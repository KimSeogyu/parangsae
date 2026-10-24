# Carry capital and receipt-order design model

This is a finite **design** model for the proposed offline carry coordinator.
It is not a refinement proof of Rust and does not certify Binance account
solvency, fills, or profitability. The starting source is main at
78359cc: the carry coordinator is not implemented there. The current single
symbol RiskGate and PaperEngine do not supply this paired contract.

The model checks three promises:

- Opening a perpetual short does not create spendable principal cash.
- A dispatched order keeps its outstanding-exposure reservation until a
  complete terminal cut drains all economic fills and their receipts.
- A hedge can dispatch only against fee-net spot quantity already visible
  to its controller.

It also checks one-pool capital conservation, nonnegative modeled cash,
no unbacked short in the modeled entry/exit policy, and spot exit only after
the perpetual is known flat.

## Rerun

Use the repository-pinned TLA+ 1.7.4 jar (TLC 2.19); no download or native Rust
build is performed. Choose a new output directory for each run.

~~~sh
python3 crates/risk/spec/carry/verify.py \
  --jar specs/tla/tla2tools.jar \
  --out /tmp/parangsae-carry-model-review
~~~

In a managed worktree, pass the absolute path of the already available pinned
jar if the ignored jar is not present in that checkout. The runner verifies
its pinned digest, runs one Java process at a time with one worker and a
256 MiB heap, and records logs/configs plus a machine-readable result outside
the checkout. It removes its own TLC state directories after each run. It
never edits the model or weakens a checked invariant.

The positive configuration fixes every bug switch to FALSE. The runner also
enables each intentional broken control separately and requires the named
invariant to fail. An unexpectedly green broken control fails the runner.
It separately asks TLC to violate FullCycleUnreachable, a reachability goal,
to establish that a complete full-quantity paired cycle is reachable. This
expected counterexample is not a product defect or a weakened safety rule.

## What was observed

TLC completed the positive breadth-first safety search: all seven invariants
held over 1,585 distinct states, with no queued states left, at quantity bound
2. The three broken controls each produced their expected counterexample:

| Broken control | Observed path and harm |
| --- | --- |
| Credit short principal | Visible spot receipt → hedge dispatch → short economic fill → receipt. Futures cash becomes 9 instead of 7 after a one-unit short with a fee. This invents spendable capital. |
| Release on cancel acknowledgement | Spot plan → dispatch → cancel request → acknowledgement, with no complete receipt cut. The order loses its reservation before reconciliation. |
| Use an unseen economic spot fill | Spot plan → dispatch → economic spot fill, receipt still hidden → hedge plan → dispatch. A short is authorized while visible owned spot is still zero. |

The last two controls distinguish an exchange-side fact from what the
controller has received. Cancel acknowledgement stops later economic fills;
earlier unseen fills may still arrive. Only CompleteCut releases the
reservation.

These are deliberate model mutations, not findings reproduced in new Rust
code. Read the actual TLC logs when assessing a change; the state counts alone
do not establish correspondence. The /statechart skill can turn their
counterexample paths into a reader-facing chart. A passing overview is:

~~~mermaid
stateDiagram-v2
    [*] --> SpotPending: Reserve one capital allocation
    SpotPending --> SpotKnown: Receive priced fee-net spot evidence
    SpotKnown --> HedgePending: Dispatch bounded short
    HedgePending --> PairedKnown: Drain entry evidence
    PairedKnown --> PerpClosePending: Close actual short
    PerpClosePending --> SpotClosePending: Complete perp-flat cut
    SpotClosePending --> Closed: Drain spot exit evidence
    SpotPending --> Unresolved: Unknown or incomplete cancellation
    HedgePending --> Unresolved: Unknown or incomplete cancellation
    PerpClosePending --> Unresolved: Unknown or incomplete cancellation
    Unresolved --> Unresolved: Keep liabilities until complete evidence
~~~

This chart is a reading aid, not an additional proof or an exhaustive graph.

## Bounds and correspondence obligations

| Model element | Runtime obligation to review when implementation exists |
| --- | --- |
| spotCash, perpCash, IdleReserve | CarryLedger must allocate one initial budget into disjoint physical accounts. Opening short notional must not credit a wallet. |
| venueFilled versus seenFilled | Economic event and admitted receipt are separate. Runtime EventTs, AvailableTs and ReceivedTs gates require their own tests; this model abstracts them into action order. |
| Prepare and Dispatch | A real engine must own private per-instance, payload/attempt/epoch/expiry-bound consumed authority and recheck dispatch-time limits. The model does not prove Rust token opacity. |
| held | Outstanding quantity is conservatively qty minus seenFilled, with identity retained through full receipt drain. This Boolean reservation abstraction does not verify actual cash/buffer sizing. |
| CompleteCut | The model trusts a truthful complete cut: venue stopped and all prior receipts delivered. Runtime must validate priced cuts, conflicts and cumulative quantities; a bare cancel flag cannot supply this assumption. |
| Deliver | One receipt posts accounting atomically. Runtime must stage all fallible ledger, lifecycle and risk checks before commit; this model does not prove implementation atomicity. |
| PlanPerpClose, SafeToSellSpot | Perp-first exit and no spot oversell or unbacked short. Runtime grid/dust/refusal controls remain separate obligations. |
| Unknown | New modeled dispatch halts. There is no resume, retry or eventual-recovery proof here. |

One pair, four leg order identities, at most one attempt for each leg and
quantities from 0 through 2 are modeled. A short hedge may match one or two
visible spot units. Economic fills and receipts each advance one unit, so
partial fills, delayed receipts and cancellation interleavings are explored.
Initial spot and perp cash are 8 each, idle reserve is 4, fixed spot/perp
prices are 2, and each filled unit pays a quote fee of 1. These are synthetic
integer units, not market values, account fees or trading recommendations.

At these fixed prices futures realized and unrealized P&L are zero.
OneCapitalPool checks posted cash plus known owned spot value equals the one
initial budget minus all posted fees. Hidden economic facts are covered by
retained liabilities; this is not a claim that an unreceived cash balance is
a complete external account statement.

Excluded: fixed-point overflow and rounding; actual base-asset commissions,
gross-to-net grids and dust; moving marks, basis, maintenance tiers,
liquidation and ADL; funding boundary/receipt clocks and charge marks;
fees in additional assets; transfers, borrowing and yield; receipt identity
conflicts/duplicates and capacity exhaustion; retries, multiple pairs,
epochs/resume, persistence, crashes, multiple writers, and live effects.
No fairness, liveness or guaranteed close is asserted. Quiescence is allowed.

## Evidence boundary and ownership

Risk/Portfolio owns this directory through the existing crates/risk/** path
rule. No architecture, dependency, capability, required-CI or branch-protection
change is included. Existing hosted TLC discovery scans specs/tla/; this
crate-local model has **local evidence only** until hosted execution is
separately configured and observed.

TLC: positive bounded search and all three failing controls passed the
runner's expectations. Kani: not run; no Rust proof is claimed. Cucumber:
the assigned twelve carry observer cases are implementation work, not passed
behavior evidence. Native runtime properties and separate-process replay
must be checked against the eventual code before a correspondence claim.

When carry source is ready, map each row above to its actual entry points and
guards, replay the three broken stories through those entry points, and
update this record only after independent review. Use
/maintain-formal-verification-skill when implementation changes make this
design model stale.
