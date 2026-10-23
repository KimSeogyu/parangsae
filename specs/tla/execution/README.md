# OrderLifecycle v3.1: TLA+ model of the execution v0 order lifecycle

v3.1 is the EL-approved follow-up to v3. It applies fixes F1, F2/F3, F5, F7, F8 and F9 as
TLA+ changes. v3 is in `../execution-tla/` and was not modified. This directory started as a
copy of v3 with the same file layout. It is still design-only: nothing was written to the
parangsae checkout and nothing was pushed.

Every number in this file comes from the TLC logs in this directory (TLC2 Version 2.19 of
08 August 2024, rev 5a47802; tla2tools.jar 1.7.4, sha256
936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88). Times are KST (UTC+9).

The original v3 README is kept unchanged below the v3.1 section. Some files it lists
(`states.dot.gz`, `OrderLifecycleDump.cfg`, `tlc-dump.log`) are not part of v3.1. See
"Regenerable, not shipped".

## What changed in v3.1

| Fix | TLA change |
|---|---|
| F1 | New event **QueryExpired**: `(Unknown, QueryExpired) = {Expired}` (`QExpired`, WF). QueryCanceled already existed. A new liveness property, `UnknownResolvesWhenVenueDone`, states that `(Unknown /\ venue order expired, canceled or filled) ~> ~Unknown`. It **passes**. |
| F2/F3 | NotFound is authoritative only after a bound. An abstract clock `o[i].el` counts ticks in Unknown since the last send of the current ClientOrderId and saturates at `Bound + 1`, where `Bound == RecvWindow + Margin`. `QNotFound` requires `el > Bound`. `QNotFoundEarly` (before the bound) leaves the order Unknown and halted, recorded as `(Unknown, QueryNotFound, Unknown)`. Physical assumption in `TickClock`: an in-flight NEW lands or is dropped before `el` passes Bound. The venue's ~3-day purge is `VPurge`: an Unknown order whose venue order is canceled or expired with no fill, and with `el > Bound`, disappears from the venue (vst = "none"), so the F2 scenario is reachable (witness `findings/W1`). |
| F7 | A retry always uses a fresh ClientOrderId `CID(i, a) == <<i, a>>` with `a = att + 1`. `Att == 0..MaxAttempts-1`. When attempts are used up, NotFound gives Rejected. The venue is modeled per (intent, attempt). `NoDuplicateSubmitPerClientOrderId` is kept. New: `NoTwoLiveVenueOrdersPerIntent` (at most one attempt per intent is open, in flight, or the current Unknown/open-local attempt) and `FillsAcrossAttemptsWithinApproved` (venue fills summed over attempts are at most the approved qty). |
| F5 | `(PendingCancel, CancelDenied)` gains Unknown. A deny on an order with no processed Ack (`~acked`) and no fill goes to Unknown, halts and queries. Before, it produced a ghost Acked. New invariant: `NoGhostAcked` (`st = Acked => acked`). |
| F8 | Every halt bumps halt_epoch. This covers recon mismatch, Unknown entry, kill, ban, RiskHalt, IllegalTransition and PayloadError. The bump is `Bump == ap' = [j |-> FALSE]` (see Abstractions). Pre-halt approvals therefore cannot be used after `OperatorReset`. `Reapprove` re-runs risk::check at the current epoch. New invariant: `ApprovalsNotUsedAcrossHalt`, a ghost flag set if any submit uses an approval issued before a halt that happened since. |
| F9 | `(PendingNew, CancelRequested)` gains Canceled. Kill cancels a not-yet-sent order locally and never submits it. New invariant: `NoSubmitOfPreKillOrders`, a ghost flag set if any order that existed at the kill is later submitted. |

Also new: `Ban` (venue ban halts, HaltG) and `RecordReachedCells`. `RecordReachedCells` is a
recorder, not a check. It uses TLC register 1 to print `<<"REACHED", from, event, result>>`
the first time each cell result is reached. It needs `-workers 1`. v3's `-dump dot` approach
does not scale to the v3.1 state space.

Normalizations that keep the state space finite and small (all sound for the checked
properties):
* Approvals are single-use: they are consumed at dispatch, and a retry needs `Reapprove`.
* The ghost `ph` is cleared at submit.
* `el` is reset when a query resolves Unknown.
* `pk` is only set for PendingNew and Unknown orders.

## Constants (OrderLifecycle.cfg) and the state-space reduction

| Constant | v3 | v3.1 pinned | Note |
|---|---|---|---|
| Intents | {i1, i2} | **{i1}** | **reduced**: see below |
| MaxQty | 2 | 2 | |
| MaxAge | 0 | 0 | |
| MaxEpoch | 1 | 1 | bounds the number of kills |
| InitPos | {0, 1} | {0, 1} | |
| InitForeign | {FALSE, TRUE} | {FALSE, TRUE} | |
| MaxAttempts | n/a | 2 | new |
| RecvWindow | n/a | 1 | new, abstract ticks |
| Margin | n/a | 1 | new, abstract ticks (Bound = 2) |

**The state space blew up with v3's constants (2 intents), so Intents was reduced from
{i1, i2} to {i1}. No other constant changed.** Diagnostic runs (safety only, no
PROPERTY, `-Xmx2g -workers 1`) were stopped while their queues were still growing:

| Run | Constants vs pinned | Stopped at (KST) | distinct / generated / depth / queue |
|---|---|---|---|
| 2 intents, Margin 1 (v3 consts + new) | Intents {i1,i2} | 10:27:31 | 4,558,872 / 24,240,608 / 22 / 919,359 |
| 2 intents, Margin 0 | Intents {i1,i2}, Margin 0 | 10:28:12 | 2,232,729 / 10,596,422 / 19 / 630,166 |
| A | Intents {i1,i2}, Margin 0, MaxQty 1 | 10:33:39 | 3,367,302 / 17,613,022 / 25 / 602,218 |
| B | Intents {i1,i2}, Margin 0, InitPos {1}, InitForeign {FALSE} | 10:33:39 | 3,400,849 / 17,681,769 / 24 / 677,129 |
| D | **Intents {i1}** (= pinned) | completed 10:31:56 | 139,379 / 910,304 / 29 / 0 |

Reducing the scalar constants did not help: the product of two intents, each with two
per-attempt venue records, the clock and the ghosts, dominates. So the pinned run uses 1
intent. With a single intent, all the new per-intent invariants (NoTwoLive, fills across
attempts, CID, epoch, kill) are fully checked. What is lost is cross-intent interleaving:
one intent's halt affecting another's in-flight order, and kill with two live orders. A
two-intent safety-only run is reported under "Two-intent safety run (partial)".

## Results (pinned run, tlc.log)

Command, the CI shape:
`java -XX:+UseParallelGC -Xmx512m -cp tla2tools.jar tlc2.TLC -workers 1 -metadir <tmp> -config OrderLifecycle.cfg OrderLifecycle.tla`

```
Model checking completed. No error has been found.
910304 states generated, 139379 distinct states found, 0 states left on queue.
The depth of the complete state graph search is 29.
Finished in 50s at (2026-10-23 10:35:42)
```

| Check | Kind | Result |
|---|---|---|
| TypeOK | invariant | holds |
| NoDuplicateSubmitPerClientOrderId | invariant | holds |
| NoTwoLiveVenueOrdersPerIntent (new, F7) | invariant | holds |
| FillsAcrossAttemptsWithinApproved (new, F7) | invariant | holds |
| NoNewOrderAfterHalt | invariant | holds |
| ReduceOnlyFlattenAlwaysApprovable | invariant | holds |
| FilledQtyNeverExceedsOrderQty | invariant | holds |
| CancelRemainingConsistent | invariant | holds |
| ApprovalsNotUsedAcrossHalt (new, F8) | invariant | holds |
| NoSubmitOfPreKillOrders (new, F9) | invariant | holds |
| NoGhostAcked (new, F5) | invariant | holds |
| FlagImpliesHalted, CellsFollowMatrix, ReduceOnlyNeverCrossesZero, UnknownHalts, CidInjective | supporting | hold |
| RecordReachedCells | recorder | always TRUE (prints REACHED lines) |
| HaltOnlyClearedByOperatorReset | action property | holds |
| KillEventuallyFlatOrFlagged | liveness | holds |
| UnknownResolvesWhenVenueDone (new, F1) | liveness | **holds** (in v3 it fails: see R1) |

The first pinned attempt failed KillEventuallyFlatOrFlagged (exit 13, 13,881 distinct). The
cause was a lasso: KillFlatten → Tick (approval ages) → Reapprove → Tick → … in which the
flatten is never dispatched. Weak fairness on the flatten dispatch is not enough once
Reapprove exists, because the dispatch is only intermittently enabled. v3.1 uses
`SF_vars(flatten Dispatch)` instead (finding N4).

Small config (`OrderLifecycleSmall.cfg`: 1 intent, MaxQty 1, InitPos {1}, InitForeign
{FALSE}, MaxAttempts 2, RecvWindow 0, Margin 0; invariants only, with `-dump dot`):
185783 generated, 31339 distinct, depth 25, 03s, no error (`tlc-small.log`).

## transitions.json (117 cells, generated) and transitions-v3-to-v3.1.md

`gen_transitions.py tlc.log transitions.json transitions-v3-to-v3.1.md` builds the table from
the 117 `CELL` lines that TLC printed and the reachability from the REACHED lines. It
compares the table against a v3 transcription and the expected v3.1 changes, and it exits
nonzero on any failed check. All checks pass:

| Check | Value |
|---|---|
| states × events | 9 × 13 (QueryExpired added) = **117** cells (v3: 108) |
| CELL lines / duplicates | 117 / none |
| all cells match expected v3.1 | true (0 mismatched) |
| legal / illegal | 30 / 87 (v3: 29 / 79) |
| vs v3 | **9 added, 3 changed, 0 removed** |
| (cell, result) triples | 126, unique |
| cells reached | 41 (53 reached (cell, result) pairs) |
| legal cells never reached | (Acked, Ack), (PendingCancel, CancelRequested) |
| legal result never reached | PartiallyFilled -Fill-> PartiallyFilled (needs qty ≥ 3; same as v3) |
| illegal cells hit by venue-consistent runs | 13 (v3: 16) |

Diff summary (details and reasons in `transitions-v3-to-v3.1.md`):
* Added: the 9 `QueryExpired` cells (F1). Only `(Unknown, QueryExpired) = {Expired}` is legal;
  the 8 others are `{}`.
* Changed: `(PendingNew, CancelRequested)` {PendingCancel} → {Canceled, PendingCancel} (F9).
  `(PendingCancel, CancelDenied)` {Acked, PartiallyFilled} → {Acked, PartiallyFilled, Unknown}
  (F5). `(Unknown, QueryNotFound)` {PendingNew} → {PendingNew, Rejected, Unknown}
  (F2/F3/F7).
* Removed: none.

Reachability changes vs v3, for cells that are themselves unchanged. To separate these from
the intent reduction, the v3 model was rerun with the same 1-intent constants: it still
reaches all of them, so the changes come from the v3.1 fixes.
* (Acked, Ack) is no longer reached. In v3 it was reached only through the ghost-Acked path,
  where a deny maps to Acked and the real Ack arrives later. That path is gone under F5.
* (PendingNew, CancelDenied), (Acked, Reject) and (Rejected, CancelDenied) are no longer hit
  as IllegalTransition. They are consequences of the F5/F7/F9 changes, not traced one by one.

## Mutants and regression variants

Each variant has the mutated `.tla`, a `.cfg` (same constants as the pinned run, TypeOK plus
its target), `mutation.diff` and `tlc.log`. They were run with the CI command shape.

| Variant | Mutation | Target | TLC result | generated / distinct / depth |
|---|---|---|---|---|
| M1 | CID ignores the attempt (`<<i, 0>>`): a retry reuses the id | NoDuplicateSubmitPerClientOrderId | exit 12, violated | 165386 / 31740 / 13 |
| M2 | gate ignores the halt latch **and** halts don't bump the epoch | NoNewOrderAfterHalt | exit 12, violated | 364 / 186 / 6 |
| M3 | RiskApproves never takes the reduce-only branch | ReduceOnlyFlattenAlwaysApprovable | exit 12, violated | 59 / 35 / 3 |
| M4 | overfill applied instead of PayloadError | FilledQtyNeverExceedsOrderQty | exit 12, violated | 185 / 69 / 5 |
| M5 | EmitCancelled without `cum = vcum` | CancelRemainingConsistent | exit 12, violated | 92114 / 20223 / 12 |
| M6 | matching Reconcile clears the latch | HaltOnlyClearedByOperatorReset | exit 13, action property violated | 7 / 7 / 2 |
| M7 (new) | QNotFound without `el > Bound` (F3 regression) | NoTwoLiveVenueOrdersPerIntent | exit 12, violated | 9868 / 3494 / 8 |
| M8 (new) | Unknown found EXPIRED is retried for full qty under a fresh id | FillsAcrossAttemptsWithinApproved | exit 12, violated | 325302 / 59046 / 16 |
| M9 (new) | `Bump == UNCHANGED ap` (F8 regression) | ApprovalsNotUsedAcrossHalt | exit 12, violated | 702 / 407 / 6 |
| M10 (new) | Kill does not bump the epoch (F9 regression) | NoSubmitOfPreKillOrders | exit 12, violated | 739 / 427 / 6 |
| M11 (new) | deny without Ack → Acked (F5 regression) | NoGhostAcked | exit 12, violated | 12486 / 4146 / 9 |
| R1 (regression) | QExpired disabled (v3 had no QueryExpired) | UnknownResolvesWhenVenueDone | exit 13, temporal property violated | 125664 / 25597 / n/a |
| R2 (regression) | definitive-NotFound retry keeps the same CID (v3 F2 behavior) | NoDuplicateSubmitPerClientOrderId | exit 12, violated | 164516 / 31259 / 13 |
| findings/W1 | witness invariant `PurgedOrderNeverRetried` (expected violated) | F2 scenario reachable | exit 12, violated as intended | 56174 / 13623 / 11 |
| findings/S1 | gate ignores the halt latch, epoch bump intact (old M2) | NoNewOrderAfterHalt | **exit 0, survives** (expected; N5) | 910874 / 139379 / 29 |

Traces:
* R1: Plan → DispatchTimeout → VExpire → TickClock×3 → DExpire (Unknown, Expire = IllegalV) →
  TimeoutEnv, looping. The order stays Unknown forever with the venue expired.
* R2: … DispatchTimeout → TickClock×3 → VExpire → **VPurge** → QNotFound → Reapprove →
  DispatchAck: a second NEW goes out under the same CID after the purge.
* W1: the same prefix up to QNotFound after VPurge.

M1 changed from v3, where it meant "QNotFound without ~infl". That role now belongs to M7,
which checks it against the stronger NoTwoLive invariant. The v3 gate-only M2 now survives;
see N5.

## Two-intent safety run (partial)

`OrderLifecycleTwoIntentSafety.cfg` uses v3's constants exactly (Intents {i1, i2}, MaxQty 2,
MaxAge 0, MaxEpoch 1, InitPos {0,1}, InitForeign {F,T}) plus MaxAttempts 2, RecvWindow 1 and
Margin 1. It checks all 16 invariants and no properties, with `-Xmx4g -workers 1`, on the
same module. It was **stopped manually at about 10:55:35 KST, so it is not a completed check**.
Last progress line (`tlc-two-intent-safety-partial.log`):

```
Progress(27) at 2026-10-23 10:55:29: 67,087,132 states generated, 12,075,999 distinct states found, 1,951,222 states left on queue.
```

No invariant violation was found in those 12,075,999 distinct states (every state up to BFS
depth 26 and part of depth 27). This is bounded evidence only.

## New findings in v3.1

* **N1 An Unknown order resting open at the venue has no resolving event.** There is no
  QueryOpen / QueryNew. A timed-out GTC order that landed and rests unfilled stays Unknown and
  halted until it fills, expires, is canceled or is killed. `UnknownResolvesWhenVenueDone`
  only covers expired, canceled and filled. Suggestion: add `(Unknown, QueryOpen) = {Acked}`,
  or `{PartiallyFilled}` with cum > 0.
* **N2 F5 increases F4 noise.** After deny → Unknown, a late Ack or Fill for that order hits
  (Unknown, Ack) or (Unknown, Fill), which is IllegalTransition and halts. It is safe but
  noisy, and the same F4 limitation as before.
* **N3 F8 plus single-use approvals: any halt during a kill invalidates the flatten's
  approval.** The flatten then needs Reapprove before it can be dispatched. Risk always
  approves a strictly reducing reduce-only order, so this is a delay, not a deadlock.
* **N4 Approval TTL vs dispatch latency livelock.** With Reapprove, approve → age → reapprove
  can repeat forever without a dispatch. TLC found this under WF. The model now assumes SF on
  the flatten dispatch, which in practice means the approval TTL must exceed dispatch
  latency, or a reapproval must be followed by an immediate dispatch. The implementation
  should guarantee this.
* **N5 NoNewOrderAfterHalt has two independent defenses.** With the epoch bump, removing the
  halt check from the dispatch gate is not observable (S1 survives). Approvals from before
  the halt are dead, and risk::check refuses non-reduce re-approval while halted. Only
  removing both defenses (M2) violates the invariant. Keep both: the gate is defense in depth.
* **N6 Scale.** With per-attempt venue records, two intents are out of reach at -Xmx512m
  (see the table above). If CI must check cross-intent interleavings, it needs a dedicated
  2-intent config with MaxAttempts 1 and no kill, or symmetry/view reduction.

## Known limits (not fixed in v3.1)

* **F4 Valid out-of-order reports are still IllegalTransition** (13 cells hit; see
  transitions.json). They halt with an alert, which is safe but noisy. F5 adds to this (N2).
* **F6 PendingNew is still overloaded.** It means both "sent, awaiting ack" and "known absent,
  awaiting (re)dispatch under a fresh CID". The model still needs a `sent` flag.
* **F10 (PendingCancel, CancelRequested) is still unreachable.** No second cancel is sent
  while one is pending.

## Abstractions (v3.1)

* **halt_epoch** is abstracted as `ap[i]` = "the approval's epoch equals the current epoch",
  and Bump sets every `ap` to FALSE. This is exact because epochs only increase and
  approvals are compared only for equality. `kills` (≤ MaxEpoch) bounds the number of kills;
  other halts are bounded by the halted latch.
* **The clock** is abstract ticks saturating at Bound+1, counted only while Unknown. Physical
  assumption: recvWindow + skew ≤ Margin, so no in-flight NEW exists once `el > Bound`.
* **Purge** is a single step, `VPurge`: canceled/expired, vcum = 0, past the bound. Real time
  (~3 days) is not modeled.
* **Ban** is modeled only as a halt cause (epoch bump), not as rate-limit timing.
* There are 2 attempts per intent. When they are used up, NotFound gives Rejected (an
  operator decision in reality).
* Pinned with 1 intent (see the reduction above).

## Regenerable, not shipped in the tarball

* `states-small.dot` (46,423,432 bytes, 31,339 states) and `states-small.svg`:
  `SMALL_GRAPH=1 ./run_tlc.sh`. Note: `dot -Tsvg` on this file did not finish within a
  15-minute timeout here, so no states-small.svg was produced for v3.1. `tlc-small.log` is
  shipped.
* A full-constant `-dump dot` (`states.dot.gz` in v3) is not produced for v3.1. Rerun the
  pinned command with `-dump dot,colorize,actionlabels states.dot` if needed (large).
* Everything else (`tlc.log`, `transitions.json`, the md diff, `lifecycle.*`, `mutants/`,
  `findings/`) is shipped and can be rebuilt with `./run_tlc.sh`.

## How to rerun

```sh
./run_tlc.sh                    # pinned run, transitions.json + diff, lifecycle.svg, mutants
SMALL_GRAPH=1 ./run_tlc.sh      # also the reduced-constant state graph (not shipped)
```

---

# v3 README (unchanged, kept for reference)


Design-only artifact. It models `reports/execution-v0-spec.md` **v3 as pinned**
(sha256 `406177757097b590a8607cd9a71d0ccd2dcef06fdf2c444cc17c1bca4b549dd7`) with exactly its
9 states and 12 events. No Rust code exists yet, so this is the design, not a refinement of code.
Nothing was written to the parangsae checkout.

All numbers below are copied from the TLC logs in this directory
(TLC2 Version 2.19 of 08 August 2024, rev 5a47802; tla2tools.jar 1.7.4,
sha256 936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88).

### Files

| File | What it is |
|---|---|
| `OrderLifecycle.tla` / `.cfg` | Module and the pinned config (all invariants and properties) |
| `tlc.log` | Pinned run, CI command shape (`-workers 1 -Xmx512m`), includes the 108 `CELL` lines printed by TLC |
| `OrderLifecycleDump.cfg`, `tlc-dump.log`, `states.dot.gz` | Same spec and constants with invariants only, `-dump dot,colorize,actionlabels`. The raw dot is 1,145,759,027 bytes (708,408 nodes, 3,247,866 edges), so it ships gzipped |
| `OrderLifecycleSmall.cfg`, `tlc-small.log`, `states-small.dot`, `states-small.svg` | **Reduced constants** (1 intent, MaxQty 1, InitPos {1}, no foreign order): a real TLC dump that graphviz can render. The full graph cannot be rendered to SVG usefully |
| `lifecycle.dot` / `lifecycle.svg` | **Abstracted** 9-state graph built from `transitions.json`: legal reached cells, legal unreached (dashed), and illegal cells hit by venue-consistent runs (red, into an IllegalTransition sink) |
| `transitions.json`, `gen_transitions.py` | The 108 cells, generated from the model (see below) |
| `gen_lifecycle_dot.py` | transitions.json → lifecycle.dot |
| `mutants/` | One mutant per required safety invariant, plus one for the action property. Each has the mutated `.tla`, a `.cfg` (TypeOK + target), `mutation.diff` and `tlc.log` |
| `findings/` | Variants that demonstrate v3 issues F1 and F2 (with logs) |
| `run_tlc.sh`, `make_mutants.py` | Rerun scripts |

### How to rerun

```sh
sudo apt-get install -y default-jre-headless graphviz
# tla2tools.jar 1.7.4 at /workspace/builder/tools/tla2tools.jar (or set JAR=...)
./run_tlc.sh                    # pinned run, dump, transitions.json, small dump, svgs
python3 make_mutants.py /workspace/builder/tools/tla2tools.jar   # mutants/ and findings/
```

The pinned run is the repo CI command:
`java -XX:+UseParallelGC -Xmx512m -cp tla2tools.jar tlc2.TLC -workers 1 -metadir <tmp> -config OrderLifecycle.cfg OrderLifecycle.tla`.
The dump run needs `-Xmx2g` (the dot writer) and about 2.5 minutes. `gen_transitions.py`
exits nonzero if any of its checks fail.

### Constants (OrderLifecycle.cfg)

| Constant | Value | Meaning |
|---|---|---|
| Intents | {i1, i2} | intent slots; ClientOrderId = `<<intent, attempt=0>>` |
| MaxQty | 2 | max abs(position) and order qty, abstract units |
| MaxAge | 0 | `max_approval_age` in ticks (an approval is good for one dispatch; Tick ages it) |
| MaxEpoch | 1 | bound on halt_epoch advances (one kill) |
| InitPos | {0, 1} | restored settled position |
| InitForeign | {FALSE, TRUE} | venue has an open order the book does not know |

### Results (pinned run, tlc.log)

```
Model checking completed. No error has been found.
3247870 states generated, 708408 distinct states found, 0 states left on queue.
The depth of the complete state graph search is 26.
Finished in 01min 51s
fingerprint collision probability: optimistic 9.8E-8, actual 4.0E-8
```

| Check | Kind | Result |
|---|---|---|
| TypeOK | invariant | holds |
| NoDuplicateSubmitPerClientOrderId | invariant | holds |
| NoNewOrderAfterHalt | invariant | holds |
| ReduceOnlyFlattenAlwaysApprovable | invariant | holds |
| FilledQtyNeverExceedsOrderQty | invariant | holds |
| CancelRemainingConsistent | invariant | holds |
| FlagImpliesHalted, CellsFollowMatrix, ReduceOnlyNeverCrossesZero, UnknownHalts, CidInjective | supporting invariants | hold |
| HaltOnlyClearedByOperatorReset | action property `[][...]_vars` | holds |
| KillEventuallyFlatOrFlagged: `killed ~> (KillDone \/ (halted /\ flag))` | liveness under WF | holds |

Dump run (invariants only): same 3247870 generated / 708408 distinct / depth 26, 02min 25s.
Small config: 21026 generated, 4634 distinct, depth 17, 01s, no error.

### What is modeled

* **Lifecycle matrix.** `Allowed(s, e)` is v3's table cell by cell. It has 29 legal cells (a
  non-empty result set) and 79 illegal cells (`{}` = typed IllegalTransition). An `ASSUME`
  prints all 108 cells into `tlc.log`. Every lifecycle step goes through `Allowed` and
  records `last = <<state, event, result>>`; `CellsFollowMatrix` checks each recorded cell.
  If a venue-consistent report hits an illegal cell, the order is left unchanged and the
  book halts with an alert (`result = "IllegalV"`). A legal cell that carries an impossible
  payload (an overfill) gives `PayloadError` and halts.
* **ClientOrderId** is deterministic, `CID(i) = <<i, 0>>`. A retry reuses the same ID, and
  only after `QueryNotFound` from Unknown. In the model that requires the NEW is no longer
  in flight (`~infl`), the venue has no record, and no fills. The venue counts NEWs per CID
  (`vsubs`).
* **Timeout → Unknown.** A dispatch timeout either lands, stays in flight or is dropped.
  Unknown halts new submits (`UnknownHalts`). It resolves by QueryFilled / QueryCanceled /
  QueryNotFound; none of these clears the latch.
* **Recon halt latch.** Init is halted (a restored book starts halted). Reconcile runs only
  when quiet. A mismatch keeps halted and raises the flag; nothing is adopted. A match does
  NOT clear the latch. Only `OperatorReset` clears halted, flag and riskHalted, and it
  requires a fresh reconcile, no Unknown order, and, if killed, KillDone.
* **Zero-crossing split.** A plan that crosses zero creates only the reduce-only close leg
  in its slot; the open leg is a separate later Plan with its own approval. Each leg checks
  `GateOK`.
* **Kill.** Kill sets the latch, advances halt_epoch (so older approvals go stale), cancels
  all known orders (CancelRequested) and foreign orders, then dispatches a reduce-only
  flatten. Risk always approves a strictly reducing, reduce-only order, even when halted.
  If the flatten cannot proceed within the bound (no slot left), `KillStuck` raises the flag.
* **RiskGate.** `GateOK(i)` requires `ApprovalFresh(i)` (`aEp = epoch /\ aAge <= MaxAge`),
  and either not halted and not riskHalted, or reduce-only, strictly reducing, with known
  exposure. `RiskHalt` models the risk side tripping independently.
* **Venue.** The venue is separate from the book (`vst`, `vcum`, `vsubs`, `infl`). It can
  land or drop an in-flight NEW, fill, expire (IOC/GTX/EXPIRED_IN_MATCH), and cancel.
  Reports are delivered later in any order (DAck, DRej, DFill, DCresp, DExpire), so
  out-of-order but valid sequences are explored.
* **Cancel remaining.** `EmitCancelled` records `rem = qty - cum` only once the book's cum
  equals the venue's cum (`CancelRemainingConsistent`).
* `Parked` is an explicit stutter for two legitimate end states: halted with an alert waiting
  for a human, and the intent bound used up while everything is quiet. Without it, TLC
  reports a deadlock in both.

### transitions.json (108 cells, generated)

`gen_transitions.py` takes the cell table from the 108 `CELL` lines that TLC itself printed
(`PrintMatrix` over `Allowed`). It takes reachability from the `last` value of every node in
`states.dot`, counting only edges labelled with lifecycle actions. A hand transcription of
v3 (`V3_TABLE`) is used **only for comparison**. Checks, all passing:

| Check | Value |
|---|---|
| matrix lines printed by TLC | 108, no duplicates |
| cell_count_is_108 / cells_unique | true / true |
| all_cells_match_v3 | true (0 mismatched cells) |
| legal / illegal | 29 / 79 |
| (cell, result) triples | 113, unique |
| cells reached in the state graph | 44 |
| legal cells never reached | (PendingCancel, CancelRequested) |
| illegal cells hit by venue-consistent runs | 16, listed under F4 |

Under the pinned constants, the result `PartiallyFilled → PartiallyFilled` of Fill is
also not reached, since MaxQty 2 allows at most one partial step.

### Mutants (each caught)

| Mutant | Mutation | TLC result | distinct / generated / depth |
|---|---|---|---|
| M1 NoDuplicateSubmitPerClientOrderId | QNotFound without `~infl` | exit 12, invariant violated | 4496 / 10242 / 8 |
| M2 NoNewOrderAfterHalt | gate ignores halted/riskHalted | exit 12, invariant violated | 239 / 336 / 6 |
| M3 ReduceOnlyFlattenAlwaysApprovable | RiskApproves never takes the reduce-only branch | exit 12, invariant violated | 27 / 37 / 3 |
| M4 FilledQtyNeverExceedsOrderQty | overfill applied (cum = qty+1) instead of PayloadError | exit 12, invariant violated | 62 / 108 / 5 |
| M5 CancelRemainingConsistent | EmitCancelled without `cum = vcum` | exit 12, invariant violated | 37924 / 108742 / 12 |
| M6 HaltOnlyClearedByOperatorReset | matching Reconcile clears halted/flag | exit 13, action property violated | 6 / 6 / 2 |

### Findings in v3 (the model keeps v3's matrix unchanged)

* **F1 Unknown + venue EXPIRED never resolves.** v3 has QueryFilled, QueryCanceled and
  QueryNotFound but no QueryExpired (or QueryRejected). An order that times out and then
  expires (IOC, GTX, EXPIRED_IN_MATCH) stays Unknown and halted forever.
  `findings/F1_UnknownExpiredNeverResolves`: the liveness property
  `UnknownExpiredResolves` fails (exit 13; 20959 distinct / 60401 generated).
* **F2 NotFound → same-ID retry duplicates after archival.** Binance stops returning
  CANCELED/EXPIRED orders with no fills after 3 days (-2013), and clientOrderId is unique only
  among *open* orders. v3's QueryNotFound → PendingNew → resubmit with the same CID then
  sends a second NEW. `findings/F2_ArchivedOrderNotFound_DuplicateSubmit` adds `VForget`:
  NoDuplicateSubmitPerClientOrderId is violated (exit 12; 9268 distinct, depth 9; trace:
  Timeout(landed) → VExpire → VForget → QNotFound → DispatchAck).
* **F3 "Authoritative" NotFound is undefined.** If a NotFound can arrive while the NEW is still
  in flight, the result is a duplicate (M1). v3 needs a rule such as: NotFound counts only
  after recvWindow plus clock skew has passed since the send, and is confirmed by a second
  query.
* **F4 Valid out-of-order reports are IllegalTransition.** These cells are illegal in v3 but
  were hit by venue-consistent report orders in the model, so each one halts with an alert:
  (PendingNew, CancelDenied), (Acked, Reject), (Filled, Ack), (Filled, CancelDenied),
  (PendingCancel, Reject), (Canceled, Ack), (Canceled, CancelConfirmed),
  (Rejected, CancelDenied), (Expired, Ack), (Expired, CancelDenied), and Unknown with Ack,
  Fill, CancelDenied, CancelConfirmed, Reject and Expire. Typical causes are a late Ack after a
  fill or cancel, a cancel-deny after a full fill, and late reports while Unknown. Late
  reports into Unknown are dropped, so fills that land in Unknown are learned only from the
  query result. This is safe (it halts) but very noisy.
* **F5 Ghost Acked.** For PendingNew → CancelRequested → PendingCancel → CancelDenied, v3
  gives Acked, even though the venue never acknowledged the order (e.g. -2011 unknown order).
  The book then has an Acked order the venue lacks, and recon flags a mismatch.
* **F6 PendingNew is overloaded.** It means both "sent, awaiting ack" and "known absent,
  awaiting (re)dispatch" (after QueryNotFound). The model needs a separate `sent` flag.
* **F7 `attempt` is dead.** v3 derives the CID from (intent, attempt), but attempt never
  changes (same-ID retry). It is always 0 in the model.
* **F8 Recon halt does not advance halt_epoch.** Only kill advances it, so approvals issued
  before a recon halt are still valid after OperatorReset if within max age. The model
  protects against this with MaxAge = 0 and the freshness rule; v3 should state the rule.
* **F9 Kill vs an unsent PendingNew is unspecified.** The model makes the epoch advance stale
  its approval (DispatchRefuse). v3 should say this explicitly.
* **F10 (PendingCancel, CancelRequested) is unreachable** under v3's own rules (no second
  cancel is sent while one is pending). It is harmless, but the cell is dead.
* **Model-bound artifact, not a v3 bug:** with only 2 intent slots, kill can run out of slots
  for the flatten. That ends in KillStuck (halted + flag), which the liveness property allows.

### Abstractions and gaps

* Not modeled: prices, filters, rate limits and backoff, risk-event replay sequence numbers,
  payload bytes, sim/paper mode, multiple symbols, and hedge mode (reduceOnly is invalid in
  hedge mode or with closePosition, so it is out of scope).
* Quantities are abstract units 0..MaxQty. One fill unit per step.
* Time is reduced to the approval-age Tick and in-flight flags; there are no clocks. The
  recvWindow is abstracted to "the in-flight NEW eventually lands or is dropped".
* Spurious or forged reports with an arbitrary state or event were removed from Next: they
  made the space blow up (more than 7.7M states). Overfill (`SpurFill`) is kept.
  Out-of-order delivery of real reports covers the IllegalTransition paths.
* At most one kill (MaxEpoch 1), 2 intents, MaxAge 0. Liveness holds only under the WF
  assumptions listed in `Fairness`; operator actions and Plan have no fairness.
* `-dump dot` is too large to render (1.1 GB). The SVGs are the reduced-constant graph and
  the abstracted lifecycle graph.

### Binance docs (supporting notes only; they did not change the matrix)

developers.binance.com served a JS bot challenge to both the fetch tool and curl, so these
facts come from search-result text of the official docs, the official connector javadoc, and
the binance-us docs mirror:

* New Order (USDⓈ-M futures, `POST /fapi/v1/order`): `newClientOrderId` is "A unique id among
  open orders", pattern `^[\.A-Z\:/a-z0-9_-]{1,36}$`. `reduceOnly` cannot be sent in Hedge
  Mode or with `closePosition`.
  https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api
* Query Order (`GET /fapi/v1/order`) needs `orderId` or `origClientOrderId`. An order is not
  found if it is CANCELED/EXPIRED with no filled trade and created more than 3 days ago, or
  created more than 90 days ago (F2).
  https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api/Query-Order
* Error codes: -1007 TIMEOUT "Send status unknown; execution status unknown" (→ Unknown);
  -2013 NO_SUCH_ORDER "Order does not exist"; -1021 timestamp outside recvWindow (→ VDrop).
  https://developers.binance.com/docs/derivatives/usds-margined-futures/error-code
* Order status: NEW, PARTIALLY_FILLED, FILLED, CANCELED, REJECTED, EXPIRED,
  EXPIRED_IN_MATCH (STP) (F1).
