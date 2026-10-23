# OrderLifecycle: TLA+ model of the execution v0 order lifecycle

Design-only artifact. It models `reports/execution-v0-spec.md` **v3 as pinned**
(sha256 `406177757097b590a8607cd9a71d0ccd2dcef06fdf2c444cc17c1bca4b549dd7`) with exactly its
9 states and 12 events. No Rust code exists yet, so this is the design, not a refinement of code.
Nothing was written to the parangsae checkout.

All numbers below are copied from the TLC logs in this directory
(TLC2 Version 2.19 of 08 August 2024, rev 5a47802; tla2tools.jar 1.7.4,
sha256 936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88).

## Files

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

## How to rerun

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

## Constants (OrderLifecycle.cfg)

| Constant | Value | Meaning |
|---|---|---|
| Intents | {i1, i2} | intent slots; ClientOrderId = `<<intent, attempt=0>>` |
| MaxQty | 2 | max abs(position) and order qty, abstract units |
| MaxAge | 0 | `max_approval_age` in ticks (an approval is good for one dispatch; Tick ages it) |
| MaxEpoch | 1 | bound on halt_epoch advances (one kill) |
| InitPos | {0, 1} | restored settled position |
| InitForeign | {FALSE, TRUE} | venue has an open order the book does not know |

## Results (pinned run, tlc.log)

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

## What is modeled

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

## transitions.json (108 cells, generated)

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

## Mutants (each caught)

| Mutant | Mutation | TLC result | distinct / generated / depth |
|---|---|---|---|
| M1 NoDuplicateSubmitPerClientOrderId | QNotFound without `~infl` | exit 12, invariant violated | 4496 / 10242 / 8 |
| M2 NoNewOrderAfterHalt | gate ignores halted/riskHalted | exit 12, invariant violated | 239 / 336 / 6 |
| M3 ReduceOnlyFlattenAlwaysApprovable | RiskApproves never takes the reduce-only branch | exit 12, invariant violated | 27 / 37 / 3 |
| M4 FilledQtyNeverExceedsOrderQty | overfill applied (cum = qty+1) instead of PayloadError | exit 12, invariant violated | 62 / 108 / 5 |
| M5 CancelRemainingConsistent | EmitCancelled without `cum = vcum` | exit 12, invariant violated | 37924 / 108742 / 12 |
| M6 HaltOnlyClearedByOperatorReset | matching Reconcile clears halted/flag | exit 13, action property violated | 6 / 6 / 2 |

## Findings in v3 (the model keeps v3's matrix unchanged)

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

## Abstractions and gaps

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

## Binance docs (supporting notes only; they did not change the matrix)

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
