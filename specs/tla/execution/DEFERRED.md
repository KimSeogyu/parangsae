# Deferred from execution-v0

Commit 2 was not written. `crates/risk` does not exist, and this tree does not stub it. The pure lifecycle and the paper stub that refuses orders are in `crates/execution`. These items stay open.

## Invariant rows not implemented

- Row 2, `NoDuplicateSubmitPerClientOrderId` (`no_duplicate_submit_per_client_order_id`).
- Row 3, `NoTwoLiveVenueOrdersPerIntent` (`no_two_live_venue_orders_per_intent`).
- Row 4, `FillsAcrossAttemptsWithinApproved` (`fills_across_attempts_within_approved`).
- Row 5, `NoNewOrderAfterHalt` (`no_new_order_after_halt`, `gate_refuses_new_exposure_while_halted`).
- Row 6, `ReduceOnlyFlattenAlwaysApprovable` (`reduce_only_flatten_always_approvable`).
- Row 8, `CancelRemainingConsistent` (`cancel_remaining_consistent`).
- Row 9, `ApprovalsNotUsedAcrossHalt` (`approvals_not_used_across_halt`).
- Row 10, `NoSubmitOfPreKillOrders` (`no_submit_of_pre_kill_orders`).
- Row 12, `FlagImpliesHalted` (`flag_implies_halted`).
- Row 14, `ReduceOnlyNeverCrossesZero` (`reduce_only_never_crosses_zero`).
- Row 15, `UnknownHalts` (`unknown_halts`).

## Properties not implemented

- P1, `HaltOnlyClearedByOperatorReset` (`halt_only_cleared_by_operator_reset`).
- P2, `KillEventuallyFlatOrFlagged` (`kill_eventually_flat_or_flagged`).
- P3, `UnknownResolvesWhenVenueDone` (`unknown_resolves_when_venue_done`).

## Findings not implemented

- F2, `f2_purged_order_not_found_retries_under_fresh_id` (venue submit count, reapproval, dispatch).
- F8, `f8_recon_halt_invalidates_prior_approval_after_reset` (every `HaltCause`, `RiskGate::latch`).
- F9 kill driver, `f9_kill_cancels_unsent_locally_and_never_submits` (no venue cancel, `vsubs` = 0). The transition-level unsent cancel is already tested. The kill driver is not.

## N4 same-step dispatch

`n4_reapproval_dispatched_in_same_step` is not implemented. The config check that approval age is strictly above dispatch latency is in place. The same-step reapproval dispatch is not.

## MockVenue gaps

There is no `MockVenue`. Nothing records `vst`, `vcum`, `vsubs`, or `infl`. The scripted paths for the rows, properties, and findings above have no venue to drive.

## Must-fix before commit 2 or any persistence

1. Paper submit must take `ApprovedOrder` and `&RiskGate`. Today `PaperVenue::submit` (`paper.rs:55`) and `PaperOrder::new` (`paper.rs:96`) take a client id and a quantity only. `Venue::submit` takes a `PaperOrder` and nothing else. A later paper adapter cannot submit around risk.
2. Decide an exact duplicate fill id before fill evidence is persisted. `apply_fill` returns `ReportConflict` when `fill_ids` already contains the id (`lifecycle.rs`, the `fill_ids.contains` branch), and the book latches that as a halt. The open choice is a no-op or a halt. Do not persist fill ids until that choice is fixed.
