# Execution v0: Rust implementation spec (from TLA+ v3.1)

Status: spec only, for ONE Cloud Agent run launched by Engineering Lead. Written 2026-10-23 (KST).
Repository-relative paths unless prefixed `reports/`. The Cloud Agent writes code; this spec ships none.

**Sources, in precedence order for the lifecycle:**
1. `specs/tla/execution/OrderLifecycle.tla` / `.cfg` (v3.1), committed on `spec/execution-tla-v31` @ `1d95097c`.
   Local reference copy: sha256 of the `.tla` is `a6142149…e981` and of the `.cfg` `f77d907b…35b8`.
2. `specs/tla/execution/transitions.json`: 117 cells, sha256 `dc39b071806026f93c1c7f50f7211b80e3be6282ecd458091c7b974de82caeae`.
3. `reports/execution-v0-spec.md` (v3, sha256 `40617775…9dd7`) for everything outside the lifecycle: the risk boundary, typed
   config, target planning and filters, ClientOrderId encoding, reconciliation, rate limits, kill steps,
   MockVenue tests, paper integration, and the v0.1 boundary. Where v3 and v3.1 disagree, **the TLA+ wins**
   (see §11).

**Repo facts, read-only at `main` = `9c34170`:**
- Workspace crates: `core`, `collector`, `signals`, `regime`, `execution`, `sim`, `backtest`.
- **There is no `crates/risk` yet.**
- `crates/execution` is a scaffold: `PaperBook` plus `tests/no_live_client.rs`, with no dependencies.
- `core` exports `Price`, `Qty`, `Notional` (i64 ticks, `SCALE = 1e8`) and `UtcMicros`.
- `sim` exports `Ledger::apply_fill(MarketOrder, Price, &Costs, UtcMicros)`, `sim::Side`, `PositionQty` and `Cash`.
- The CI `tla` job runs TLC 2.19 with `-workers 1 -Xmx512m`.

Before starting, the agent re-checks main and the TLA+ branch. If either sha above differs, stop and report.

## 1. Outcome and hard lines

**Outcome.** Implement the order lifecycle as a pure transition function that mirrors `Allowed(s, e)` cell for cell. Around it, implement:
- the order book and dispatch path behind `risk::check` and `RiskGate`;
- the NotFound authority bound and fresh-ID retry;
- reconciliation, kill (cancel, then flatten, then stop) and deterministic backoff;
- MockVenue tests for every TLA+ invariant and property.

**Hard lines:**
- **Venues: paper and offline MockVenue only.** Production exposes only PaperVenue. MockVenue is `#[cfg(test)]`/dev-only, scripted, in-process.
- **No HTTP/WS client, sockets, signing, keys, env reads, URL routing or exchange endpoints**, including any "demo" path.
  `crates/execution/tests/no_live_client.rs` stays **byte-for-byte untouched** and passing. Add a separate
  `tests/source_guards.rs` that bans `f32`, `f64`, `std::env` and `as f` in `src`.
- **Every order goes through `risk::check -> ApprovedOrder` plus a `RiskGate` check immediately before dispatch.**
  A risk veto is final: there is no re-plan around it, no downsizing and no retry without a new `risk::check`. Cancel needs no approval.
- Keep `#![forbid(unsafe_code)]`, release overflow checks and `#![deny(clippy::float_arithmetic)]` in execution.
- No edits to signals, collector, backtest, sim internals, CI workflows, required checks or `specs/tla/**`.
- Do not change the TLA+ model. If code and model disagree, report it as a finding. Do not edit the model to match the code.

## 2. Crate layout and dependencies

```
crates/execution/src/
  lib.rs           // re-exports; forbid(unsafe_code); deny(clippy::float_arithmetic)
  lifecycle.rs     // OrderState, Event, OrderProgress, transition(), TransitionError   (commit 1)
  ids.rs           // IntentId, Attempt, MaxAttempts, ClientOrderId                      (commit 1)
  authority.rs     // NotFoundAuthority, Mono, Elapsed                                    (commit 1)
  config.rs        // ExecutionConfig, ApprovalTiming (N4), budgets, backoff               (commit 1 types, 2 use)
  book.rs          // OrderBook: apply(report), dispatch, retry, halt plumbing            (commit 2)
  gate.rs          // pure GateDecision::evaluate over plain inputs                        (commit 2)
  reconcile.rs, kill.rs, throttle.rs, venue.rs (sealed Venue), dto.rs                     (commit 2)
  paper.rs         // stub boundary only (commit 3; see §9)
crates/execution/tests/
  lifecycle_matrix_117.rs, invariants.rs, properties.rs, findings.rs, n4_config.rs,
  source_guards.rs, no_live_client.rs (untouched)
```

**Dependencies:**
- Normal: `parangsae-core`, `serde`, `serde_json` (DTOs and the fixture); `parangsae-risk` from commit 2.
- Dev: `proptest = "1.6"` (same as core/sim) and `sha2` (fixture pin).
- `sim` only in commit 3, and only if that commit is unblocked.

**Risk prerequisite.** `crates/risk` is absent. Commit 1 has **no** risk dependency, because the lifecycle is pure. Commit 2 needs
the risk skeleton with exactly this API (from v3 §Risk, plus the v3.1 epoch rule):

```rust
risk::check(&Limits, &PortfolioSnapshot, Permission, OrderIntent, DecisionTime) -> Result<ApprovedOrder, Rejection>;
// ApprovedOrder: private fields, risk-only constructor, no Clone/Copy/Default/Deserialize.
//   getters: intent_id, symbol, side, qty, order_type, limit_price, time_in_force, reduce_only,
//            approved_at, risk_state_version, halt_epoch
RiskGate::is_halted(&self) -> bool;  RiskGate::halt_epoch(&self) -> HaltEpoch;
RiskGate::latch(&mut self, cause: HaltCause) -> HaltEpoch;   // v3.1: EVERY halt bumps the epoch
RiskGate::operator_reset(&mut self, token: OperatorResetToken) -> Result<(), ResetRefused>;
pub enum HaltCause { ReconMismatch, UnknownOrder, Kill, Ban, RiskTrip, IllegalTransition, PayloadError }
```

If `crates/risk` is still absent when commit 2 starts, the agent stops after commit 1 and reports. It does **not**
write a stub risk crate, unless EL's launch prompt assigns the risk skeleton to this run by name.

## 3. Types first (commit 1)

### 3.1 OrderState and Event: closed enums, TLA+ names exactly

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OrderState { PendingNew, Acked, PartiallyFilled, Filled, PendingCancel, Canceled, Rejected, Expired, Unknown }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event { Ack, Fill, CancelRequested, CancelDenied, CancelConfirmed, Reject, Expire, Timeout,
                 QueryFilled, QueryCanceled, QueryExpired, QueryNotFound, RaceFill }
impl OrderState { pub const ALL: [OrderState; 9]; pub const fn name(self) -> &'static str; } // exhaustive match
impl Event      { pub const ALL: [Event; 13];     pub const fn name(self) -> &'static str; } // exhaustive match
```

- The `name()` strings must equal the TLA+ strings (`"PendingNew"`, …, `"QueryExpired"`).
- Every `match` is exhaustive with no `_` arm, so a new variant fails to compile until it is handled.
- `ALL` is checked against `name()` by the drift test (§4).
- The model's `"None"` slot is not a state. It is `Option<OrderProgress>` in the book.

### 3.2 The transition function is the only way state changes

**Choice: private fields plus one pure function. No typestate.** Justification:
- State is runtime data: it is driven by venue reports, restored from checkpoints, and replayed over 117 runtime cells.
  Typestate would need 9 types, dynamic dispatch on restore, and could not be iterated by the table test.
- The guarantee comes from visibility. `OrderProgress` fields are private to `lifecycle`, and there is no setter, `Default` or public constructor
  except `OrderProgress::new_pending(...)` (initial `PendingNew`, `cum = 0`, `attempt = Attempt::FIRST`).
- `rustdoc` `compile_fail` doctests prove that code outside the module cannot write a field or build an `OrderProgress`.

```rust
pub struct OrderProgress { state, qty: Qty /*>0, immutable*/, cum: Qty, attempt: Attempt, sent: bool,
                           acked: bool, last_sent_at: Option<Mono>, /* evidence ids, frozen final cum */ }
pub struct CellContext<'a> { now: Mono, authority: &'a NotFoundAuthority, max_attempts: MaxAttempts }
pub fn transition(p: &OrderProgress, e: &ValidatedEvent, cx: &CellContext) -> Result<OrderProgress, TransitionError>;
pub enum TransitionError {
    IllegalTransition { from: OrderState, event: Event },  // the cell is {} in transitions.json
    Overfill, ReportConflict, MissingEvidence, InvalidQuantity, InconsistentQuery,  // payload errors
}
```

**Rules:**
- `ValidatedEvent` carries payloads: fill qty and fill id, the query result's cum and evidence, and `elapsed_since_last_send`.
- Payload validation is separate from cell legality.
- An illegal cell returns `IllegalTransition`, and a bad payload in a legal cell returns a payload error.
  This mirrors the model's `IllegalV` and `PayloadError`.
- No panic, no clamp, and the input progress is unchanged on error.

**Result selection inside legal cells.** Each must equal the model's guard:

| Cell | Result |
|---|---|
| (PendingNew, CancelRequested) | `!sent` → `Canceled`, with no venue request (F9). `sent` → `PendingCancel` |
| (PendingCancel, CancelDenied) | `cum > 0` → `PartiallyFilled`. Else `acked` → `Acked`. Else → `Unknown`, and the book halts (F5) |
| (Unknown, QueryExpired) | → `Expired`. The query's cum is processed once (F1) |
| (Unknown, QueryNotFound) | Not authoritative (`elapsed <= bound`) → `Unknown`, halt kept (F3).<br>Authoritative and `cum == 0`: next attempt exists → `PendingNew` with `attempt.next()`, `sent = false`, `acked = false`, `last_sent_at = None` (F7). No next attempt → `Rejected`.<br>Authoritative and `cum > 0` → `InconsistentQuery` (the model has no step there) |
| Fill / RaceFill | `cum + q <= qty` required, else `Overfill`. Reaching `qty` → `Filled`. In PendingCancel, a partial fill stays `PendingCancel` |
| Ack | Sets `acked = true`. PendingNew → Acked. Acked, PartiallyFilled and PendingCancel stay where they are |

### 3.3 ClientOrderId from (intent, attempt), with a bounded Attempt

```rust
pub struct IntentId(u128);
pub struct MaxAttempts(u8);              // validated 1..=16 at config; no Default
pub struct Attempt(u8);                  // private; Attempt::FIRST = 0; next(self, MaxAttempts) -> Option<Attempt>
pub struct ClientOrderId(String);        // private; only ClientOrderId::from_parts(IntentId, Attempt)
```

- **Encoding** (v3 format): `e{26 uppercase base32 of the intent}-{8 lowercase hex of the attempt}`.
  That is exactly 36 ASCII bytes from `[.A-Z:/a-z0-9_-]`, with no clock, randomness or hash.
- `from_parts` is injective (TLA+ `CidInjective`).
- **v3.1 change:** every retry after an authoritative NotFound bumps the attempt, so every retry has a fresh ID (F7).
  The same attempt is never sent twice.
- `ClientOrderId::parse` round-trips. Vectors are in `client_id_vectors`; add one vector with attempt 1.

### 3.4 NotFoundAuthority: a typed config value

```rust
pub struct Mono(u64);                            // injected monotonic micros; never read from the OS here
pub struct NotFoundAuthority { recv_window_ms: u32 /*1..=60_000*/, margin_ms: u32 /*>=1*/ }
impl NotFoundAuthority {
  pub fn new(recv_window_ms: u32, margin_ms: u32) -> Result<Self, ConfigError>;
  pub fn bound_us(&self) -> u64;                 // checked (recv_window + margin) * 1000
  pub fn is_authoritative(&self, since_last_send: Elapsed) -> bool { since_last_send.us() > self.bound_us() }
}
```

- `Elapsed` is measured from the **last send of that ClientOrderId** (`last_sent_at`), never from the intent's first send.
- A time that runs backwards (`now < last_sent_at`) is `InconsistentQuery` plus a halt.
- `recv_window_ms` must equal the recvWindow that a future adapter sends. v0 sends nothing, so it is config only.
- The margin covers clock skew, matching the model's physical assumption that no NEW is in flight once `el > Bound`.

### 3.5 Money: i64 only

- Qty, price and notional use core `Qty`/`Price`/`Notional` (i64 ticks, `SCALE = 1e8`) with checked i128 intermediates.
- Positions and the pending exposure sum are `i64` with `checked_*`. Overflow is a typed error plus a halt, never a wrap.
- No floats anywhere in `src`; `source_guards.rs` enforces this.
- Target, filters and rounding follow v3 §Target planning verbatim.

## 4. Fixture replay: all 117 cells (commit 1)

**Choice: one source.** The test reads `specs/tla/execution/transitions.json` in place, via
`concat!(env!("CARGO_MANIFEST_DIR"), "/../../specs/tla/execution/transitions.json")`, and checks its sha256 against the
pinned `dc39b071…caeae` (`sha2` dev-dep). There is no copy under `fixtures/execution/`. Justification:
- The JSON is generated from TLC output next to the model that CI checks. A second copy would need its own sync check and could still be stale.
- The sha pin makes a model regeneration a deliberate two-place change: it fails loudly, never silently.
- `publish = false`, so reading outside the crate dir is fine for the workspace test.

`tests/lifecycle_matrix_117.rs`:
1. `fixture_sha_is_pinned`: the sha256 of the bytes equals the constant.
2. `fixture_has_117_cells`: `cells.len() == 117 == OrderState::ALL.len() * Event::ALL.len()`, and every (state, event) pair is unique.
3. `fixture_and_enums_agree_both_ways`: there are two checks.
   - The set of `state` names in the JSON equals `{s.name() for s in OrderState::ALL}`, and the same for events. A new JSON cell, state or event fails.
   - Every Rust variant reached through the exhaustive `name()` match appears in the JSON. A new Rust variant fails.
   - Plus `all_is_exhaustive`: a `match` with no wildcard maps each variant to its index in `ALL`.
4. `every_cell_matches_model`: for each cell, enumerate a fixed `CellContext` grid:
   - qty 2; cum ∈ {0, 1}; acked, sent ∈ {F, T}; attempt ∈ {FIRST, last};
   - elapsed ∈ {bound, bound + 1 µs}; fill qty ∈ {1, 2}; query cum ∈ {0, 1, 2}.

   The expected outcomes:
   - **Legal cell** (`model_allowed` non-empty): the **set** of `Ok` results over the grid must equal `model_allowed` exactly.
     Payload errors are allowed and excluded.
   - **Illegal cell**: every grid point returns `Err(IllegalTransition { from, event })` with the exact pair.
   - Expected values come only from the JSON. There is no hand-written table.
5. `illegal_cell_halts_book`: for each of the 87 illegal cells, `OrderBook::apply` on a book in that state checks four things:
   - the order is unchanged;
   - `RiskGate::latch(HaltCause::IllegalTransition)` was called, so the epoch increments;
   - a typed `Alert` is emitted;
   - the book is halted.

   This test moves to commit 2 if `RiskGate` is not yet available. In commit 1 it uses the execution-local
   `HaltSink` trait that commit 2 binds to `RiskGate`.

## 5. Invariant mirror: one test per TLA+ invariant and property (commits 1–2)

**Harness.**
- `tests/invariants.rs` and `tests/properties.rs` drive `OrderBook` plus `MockVenue` from proptest-generated scripts.
- Scripts mirror the model's environment: dispatch ack/reject/timeout (landed, inflight or dropped), venue fill, expire, cancel and purge, report reordering, `Tick`, ban, risk trip, kill, reconcile and operator reset.
- MockVenue keeps the ground truth `vst`, `vcum`, `vsubs` and `infl` per ClientOrderId.
- Cases: 512 by default, with `PROPTEST_CASES` honoured. Keep minimized regression seeds in `proptest-regressions/`.
- Property tests are bounded: they use a fair scheduler where every pending report and query is delivered within K = 8 steps.
- Each test's name is the TLA+ name in snake_case.

**Fail-first rule.** For every row:
1. Apply the named mutation in `src`.
2. Run the test and record the failure (test name, assertion and seed).
3. Revert the mutation.
4. Rerun green.

Attach both logs. A row whose mutation does not fail is a finding to report, not a test to weaken. M-numbers refer to the TLC mutants in `specs/tla/execution`.

| # | TLA+ name | Rust test | Mutation that must make it fail |
|---|---|---|---|
| 1 | TypeOK | `type_ok` (all accessors in domain: 0≤cum≤qty, attempt<max, rem∈{None,0..=qty}) | `Attempt::next` drops the `< max` check |
| 2 | NoDuplicateSubmitPerClientOrderId | `no_duplicate_submit_per_client_order_id` (MockVenue `vsubs[cid] <= 1`) | M1: `from_parts` ignores the attempt |
| 3 | NoTwoLiveVenueOrdersPerIntent | `no_two_live_venue_orders_per_intent` (open, in-flight, or current Unknown/open; at most 1 per intent) | M7: drop `is_authoritative` in QueryNotFound |
| 4 | FillsAcrossAttemptsWithinApproved | `fills_across_attempts_within_approved` (Σ venue fills over attempts ≤ approved qty) | M8: Unknown + QueryExpired with cum>0 retries the full qty under a fresh ID |
| 5 | NoNewOrderAfterHalt | `no_new_order_after_halt` + unit `gate_refuses_new_exposure_while_halted` on `GateDecision::evaluate` | M2: gate ignores halted **and** the epoch check. The gate-only mutation must fail the unit test (it is masked end-to-end, per TLC S1/N5) |
| 6 | ReduceOnlyFlattenAlwaysApprovable | `reduce_only_flatten_always_approvable` (risk approves a strict reducing RO while halted) | M3: risk's reduce-only branch disabled (temporary edit in `crates/risk`, reverted) |
| 7 | FilledQtyNeverExceedsOrderQty | `filled_qty_never_exceeds_order_qty` | M4: skip the Overfill check |
| 8 | CancelRemainingConsistent | `cancel_remaining_consistent` (Cancelled.remaining = qty − cum, only after drain) | M5: emit Cancelled without `cum == venue cum` |
| 9 | ApprovalsNotUsedAcrossHalt | `approvals_not_used_across_halt` (no dispatch with `approval.halt_epoch() != gate.halt_epoch()`; every halt cause bumps) | M9: `HaltCause::ReconMismatch` latches without bumping |
| 10 | NoSubmitOfPreKillOrders | `no_submit_of_pre_kill_orders` | M10: kill latches without bumping the epoch |
| 11 | NoGhostAcked | `no_ghost_acked` (`Acked ⇒ acked`) | M11: deny with no ack and no fill → `Acked` |
| 12 | FlagImpliesHalted | `flag_implies_halted` (any `Alert` ⇒ halted) | IllegalTransition path emits an Alert but skips `latch` |
| 13 | CellsFollowMatrix | `cells_follow_matrix` (every applied (from, event, to) ∈ the JSON's allowed set) | let (Filled, Ack) return `Ok(Filled)` |
| 14 | ReduceOnlyNeverCrossesZero | `reduce_only_never_crosses_zero` | StrictReduce uses `<=` instead of `<` |
| 15 | UnknownHalts | `unknown_halts` (any Unknown ⇒ halted) | Timeout → Unknown without `latch(UnknownOrder)` |
| 16 | CidInjective | `cid_injective` (proptest: distinct (intent, attempt) → distinct IDs; parse round-trip) | encode only the low 4 bits of the attempt |
| P1 | HaltOnlyClearedByOperatorReset | `halt_only_cleared_by_operator_reset` (no step except `operator_reset` clears halted, alert or risk halt) | M6: a matching reconcile clears the latch |
| P2 | KillEventuallyFlatOrFlagged | `kill_eventually_flat_or_flagged` (fair script: within N=64 steps, KillDone or halted + Alert) | drop the N4 immediate dispatch after reapproval (flatten re-ages forever) |
| P3 | UnknownResolvesWhenVenueDone | `unknown_resolves_when_venue_done` (venue expired, canceled or filled ⇒ leaves Unknown within K) | R1: map a query EXPIRED to `InconsistentQuery` (stays Unknown) |

**Finding regressions** (`tests/findings.rs`, scripted MockVenue, exact traces from the TLC counterexamples):

| Finding | Test | Mutation that must fail it |
|---|---|---|
| F1 | `f1_timeout_then_venue_expired_resolves_via_query_expired` | R1 (as P3) |
| F2 | `f2_purged_order_not_found_retries_under_fresh_id`. Script: Timeout(landed) → expire → purge → NotFound after the bound → reapprove → dispatch. Assert `vsubs` = 1 per ID and attempt = 1 | R2: retry keeps the attempt |
| F3 | `f3_not_found_before_bound_stays_unknown_and_halted`. Script: elapsed = bound exactly, then bound + 1 µs | M7 |
| F5 | `f5_cancel_denied_without_ack_goes_unknown_and_queries` | M11 |
| F7 | `f7_retry_bumps_attempt_then_rejects_when_exhausted` (MaxAttempts 2: second authoritative NotFound → Rejected, no third send) | `Attempt::next` unbounded |
| F8 | `f8_recon_halt_invalidates_prior_approval_after_reset`. Every `HaltCause` variant is table-driven | M9 |
| F9 | `f9_kill_cancels_unsent_locally_and_never_submits`. Assert: no cancel request to the venue, Cancelled{remaining = qty}, `vsubs` = 0 | KillCancel sends a venue cancel or leaves the order dispatchable |

## 6. Dispatch, retry and the halt epoch (commit 2)

- **Gate.** `GateDecision::evaluate` is a pure function of `{approval epoch, gate epoch, approved_at, now, max_approval_age, halted, risk_halted, reduce_only, strict_reduce, exposure_known}`.
  It must pass immediately before **every** dispatch, including retries. This is the model's `GateOK`.
- **Single-use approvals.** `submit(approved: ApprovedOrder, gate, now)` consumes the approval.
  `DispatchResult` hands back a `SpentApproval` that serves as evidence of the ID/payload binding but cannot be dispatched again.
  Any retry or re-dispatch needs a **new** `risk::check` (the model's `Reapprove`) at the current epoch, for the same intent, side and
  remaining qty. The order is still `PendingNew` and unsent, and its reservation is carried over, never duplicated.
- **Every halt bumps the epoch (F8).** Unknown entry, recon mismatch, kill, ban (418/-1003), risk trip,
  IllegalTransition and PayloadError all go through `RiskGate::latch(cause)`. No code path sets `halted` without the bump.
- **Unknown.** Timeout or a deny-without-ack moves the order to Unknown: latch, keep the reservation, no speculative risk event, then query by the **current**
  ClientOrderId.
  - QueryFilled, QueryCanceled and QueryExpired need complete, consistent receipts. Missing receipts process once.
  - QueryNotFound follows §3.2. None of these clears the latch.
  - Working or ambiguous answers leave the order Unknown (N1).
- **Definite refusal.** An unsent `PendingNew` whose approval went stale, with no reapproval, gives `Rejected`. This is the model's `DispatchRefuse`, which v3 keeps.
  Never emit Rejected for an ambiguously sent order.

## 7. Reconciliation, kill and MockVenue (commit 2)

**Reconciliation:**
- Same as v3 §Startup/periodic reconciliation: start halted, one coherent cut, drain first, compare everything, and never repair or adopt.
- A match keeps the latch; only `operator_reset` clears it.
- v3.1 additions:
  - a mismatch latches with `HaltCause::ReconMismatch`, which bumps the epoch;
  - a match also requires that **no earlier attempt** of any intent is open at the venue (the model's `Matches`).

**Kill** (v3 steps, plus):
1. Latch with `HaltCause::Kill`, which bumps the epoch and makes every pre-kill approval stale.
2. Cancel:
   - unsent `PendingNew` → local `Canceled` through the (PendingNew, CancelRequested) cell, `Cancelled{remaining = qty}`, reservation released, never submitted (F9);
   - sent orders → `PendingCancel`;
   - Unknown → `CancelRequested` stays Unknown (cancel by ID is still attempted);
   - foreign orders: cancel-all.
3. Drain and reconcile.
4. Flatten with a fresh risk-approved, strictly reduce-only order, which is dispatched immediately after approval (N4).
5. Any veto, ban, timeout or residual exposure stays halted with an Alert (KillStuck).

**MockVenue** (`#[cfg(test)]`, behind the sealed `Venue`):
- scriptable land, drop and in-flight NEWs;
- fills, expire and EXPIRED_IN_MATCH, cancel, and **purge**: an unfilled canceled/expired order disappears, so by-ID queries return -2013;
- 429 with `retry_after`, and 418;
- report reordering.

**v3 mock tests carried over** (each keeps its v3 fail-first mutation; renamed where v3.1 changed them):
`timeout_then_filled`, `failed_query_stays_unknown`, `duplicate_submit`, `timeout_existing_order_no_double`, `id_payload_conflict`,
`not_found_retry_fresh_id` (was `not_found_retry_same_id`), `restart_mid_order_match_requires_reset`, `restart_mid_order_mismatch_alert`,
`periodic_reconcile_due`, `target_grid_before_delta`, `negative_target_boundaries`, `price_rounding_less_aggressive`, `filter_min_max`,
`mock_429_backoff_and_budgets`, `ban_418_halts`, `isolated_leverage_bounds`, `mode_leverage_reconcile_halts`, `close_is_reduce_only`,
`reversal_two_distinct_approvals`, `kill_mid_partial_fill`, `flatten_risk_veto_stays_halted`, `approval_private_compile_fail`,
`risk_gate_epoch_age`, `risk_event_replay_conflict`, `cancel_remaining_conflict`, `cancellation_race_once`, `client_id_vectors`,
`json_doc_examples`, `capacity_no_eviction`.

## 8. N4: approval lifetime vs dispatch latency (commit 1 type, commit 2 use)

TLC found that approve → age → reapprove can repeat forever with no dispatch. The model therefore assumes strong fairness on the flatten dispatch. The implementation must make that assumption true:

```rust
pub struct ApprovalTiming { max_approval_age: PositiveMicros, max_dispatch_latency: PositiveMicros }
impl ApprovalTiming {
  pub fn new(max_approval_age: PositiveMicros, max_dispatch_latency: PositiveMicros) -> Result<Self, ConfigError>;
  // Err(ConfigError::ApprovalTtlNotAboveDispatchLatency) unless max_approval_age > max_dispatch_latency
}
```

- `ExecutionConfig::new` takes `ApprovalTiming` and `Limits::max_approval_age()` must equal its `max_approval_age`.
  A mismatch is `ConfigError::ApprovalAgeMismatch`. Startup fails, so there is no running book with a bad config.
- **Rule:** a reapproval is dispatched in the **same** `submit` call that receives it. No queue, timer or await sits between
  approval and dispatch. That makes internal latency 0, and `max_dispatch_latency` bounds only the venue round-trip.
- Tests in `tests/n4_config.rs`:
  - `n4_config_rejects_ttl_not_above_dispatch_latency`, covering both the equal and the less-than case. Mutation: `>=` → `>`, or remove the check.
  - `n4_reapproval_dispatched_in_same_step`. Mutation: queue the reapproved order for the next step. P2 must also fail under this mutation.

## 9. Commits

1. **Types, transition and the 117-cell replay.**
   - Contents: `lifecycle.rs`, `ids.rs`, `authority.rs`, the `config.rs` types (`NotFoundAuthority`, `MaxAttempts`, `ApprovalTiming`), `lifecycle_matrix_117.rs`, `source_guards.rs`, `n4_config.rs` (config test only), and the pure-lifecycle invariant tests (rows 1, 7, 11, 13, 16).
   - No risk dependency.
2. **Reconciliation, kill and MockVenue tests.**
   - Contents: `book.rs`, `gate.rs`, `venue.rs` (sealed), `reconcile.rs`, `kill.rs`, `throttle.rs`, `dto.rs` plus `fixtures/execution/` (Binance doc examples with `PROVENANCE.md`, per v3), the remaining invariant/property/finding tests, and the v3 mock tests.
   - Requires `crates/risk` (§2).
3. **Paper sim integration: blocked, so it ships as a stub boundary.**
   - Backtest Builder's paper-driver names (`Clock`, `EventSource`, `FeedEvent`, `OrderIn`, `FillModel`, `FillEvent`) are
     still DRAFT in v3 and not pinned anywhere in the repo or reports.
   - Commit 3 therefore adds only `paper.rs`:
     - a sealed `PaperVenue` whose `submit` returns a definitive `VenueError::PaperNotWired` (→ `Rejected`, reservation released);
     - doc comments naming the intended mapping to the **existing** `sim::Ledger::apply_fill(MarketOrder, Price, &Costs, UtcMicros)`
       with an explicit `core::Side` ↔ `sim::Side` conversion;
     - the bijective `ClientOrderId` ↔ sim key map to be pinned later.
   - Test `paper_stub_refuses_definitively`.
   - Do not claim a working paper adapter. The real commit 3 gets its own delta once the names are pinned.

## 10. Acceptance

Run from the repository root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Also required:
- **The CI `tla` step for `specs/tla/execution` stays unchanged and green.** That is TLC 2.19, `-workers 1`, `-Xmx512m`, OrderLifecycle.cfg. The `RecordReachedCells`
  register needs one worker. Reference run: 139,379 distinct states, 910,304 generated, depth 29, no error.
- `tests/no_live_client.rs` is unchanged: `git diff --exit-code` on it is empty.
- Attach:
  - the matrix test output showing 117 cells (30 legal, 87 illegal);
  - each §5 row's fail-first and restored logs;
  - proptest case counts and seeds;
  - the list of any mutation that did **not** fail (a finding).
- **Kani is optional for v0.** If added, it goes in `crates/execution/src/lifecycle.rs` under `#[cfg(kani)] mod proofs`. Candidate proofs:
  - `transition` never yields `cum > qty`;
  - `Attempt::next` never exceeds `MaxAttempts`;
  - `NotFoundAuthority::bound_us` never overflows.

  Running it would need `cargo kani -p parangsae-execution`, a CI change that EL must approve separately. It is not part of this run.

## 11. v3 → v3.1 contradictions, resolved for the TLA+

| # | v3 said | v3.1 / this spec |
|---|---|---|
| C1 | "v0 dispatch uses attempt zero; retry never changes it". Retry reuses the same ID and payload; "no automatic new-ID fallback" | A retry after an authoritative NotFound uses `attempt + 1`, a fresh ID; exhausted attempts → Rejected (F2/F7) |
| C2 | The venue returns the original token, and the retry moves the **same** approval through dispatch | Approvals are single-use. A retry needs a new `risk::check` at the current epoch (Reapprove). The spent token is binding evidence only |
| C3 | "Authoritative absence result" was left undefined | `elapsed since last send of that CID > recvWindow + margin`. Earlier → stays Unknown and halted (F3) |
| C4 | QueryNotFound "after known acceptance … is a conflict" | The model permits the retry when `cum == 0` even if an Ack was seen (canceled or expired unfilled, then purged). Only `cum > 0` is `InconsistentQuery`. **Flagged for EL**: the model wins here, and if EL wants the stricter v3 rule, the model must change first |
| C5 | (PendingCancel, CancelDenied) → Acked or PartiallyFilled by cum | No ack and no fill → Unknown plus a query (F5) |
| C6 | Only kill advances the halt generation | Every halt cause bumps it (F8) |
| C7 | Kill cancels venue open orders. A refusal of an unsent intent emits Rejected | Kill on an unsent order → local Canceled with `Cancelled{remaining = qty}` (F9). A stale-approval refusal outside kill stays Rejected |
| C8 | No QueryExpired, and the matrix test was `lifecycle_matrix_108_cells` | QueryExpired added (F1); the test is `lifecycle_matrix_117` |
| C9 | TLA+, Kani and cucumber deferred, "add none" | TLA+ is now committed and checked in CI. This run adds no TLA+, keeps Kani optional, adds no cucumber |

## 12. Known limits carried forward

- **F4.** Valid out-of-order reports still hit IllegalTransition: 13 cells were reached in TLC. They halt with an Alert, which is safe but noisy.
- **F6.** `PendingNew` is overloaded ("sent, awaiting ack" vs "absent, awaiting dispatch"); the `sent` flag disambiguates.
- **F10.** (PendingCancel, CancelRequested) is unreachable but legal.
- **N1.** No QueryOpen: an Unknown order resting open at the venue stays Unknown and halted until it fills, expires, is canceled or is killed.
- **N2.** F5 adds F4 noise: a late Ack or Fill after deny → Unknown is IllegalTransition.
- **N3.** Any halt during a kill invalidates the flatten's approval, so it needs a reapproval. This is a delay, not a deadlock.
- **Cross-intent interleaving is not model-checked in CI.** The pinned TLC config has 1 intent. A 2-intent safety run was stopped
  at 12,075,999 distinct states (depth 26) with no violation. That is bounded evidence, not a completed check.
  Rust proptests use 2 intents to partly cover this, but they do not discharge it.

## 13. Later / v0.1

- v0.1 is a **separate demo adapter crate** with its own spec, using v3 §v0.1 as is:
  - a closed base-URL enum (demo host only);
  - read-only signed queries first;
  - submit and cancel last, behind a flag that defaults off;
  - HTTP mock-server tests only.
- Seogyu's "ㅇㅋ" (07:28 KST, relayed by EL) authorizes developing that code. **Any network order submission, demo included,
  needs Seogyu's own-words OK for that specific act.**
- The current scope is code development only: no sends, no keys, no network.
- Other later items: TWAP and slicing, a maker policy, a QueryOpen cell (N1), the paper driver once its names are pinned, a 2-intent TLC config, and Kani in CI.
