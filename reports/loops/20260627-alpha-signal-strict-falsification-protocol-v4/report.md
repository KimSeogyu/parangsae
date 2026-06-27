# Quant Loop Report: 20260627-alpha-signal-strict-falsification-protocol-v4

## Purpose
Apply a paper-only pre-registered stricter OOS falsification protocol to the frozen V3 next-falsification queue: lock hypotheses and expected signs, enlarge purge/embargo, compute moving-block permutation p-values, and Benjamini-Hochberg correct them. The protocol tries to reject and defaults to FALSIFIED.

## Claim boundary
- suite_decision: STRICT_FALSIFICATION_PROTOCOL_INFRA_ONLY
- readiness_baseline_verdict: NOT_READY
- source_v3_artifact_path: reports/loops/20260627-alpha-signal-oos-fdr-validation-protocol-v3
- claim_boundary: ALPHA_SIGNAL_STRICT_FALSIFICATION_PROTOCOL_V4 is a paper-only pre-registered stricter out-of-sample falsification protocol over the frozen V3 next-falsification queue. It tries to reject queued family/horizon hypotheses and defaults to FALSIFIED. SURVIVED_STRICT_FALSIFICATION means a hypothesis was not rejected by this protocol; it is NOT alpha confirmation, NOT a proven edge, NOT a trading strategy, NOT signal-weight optimization, NOT profitability evidence, NOT readiness or live readiness, and NOT permission for live/testnet/account/API-key/order/fill use.
- preregistration_boundary: Hypotheses, expected IC sign, fold construction, purge/embargo, bootstrap block length, resample count, false-discovery alpha, and survival thresholds are pre-registered from the frozen V3 queue before evaluation. Post-hoc sign flips and threshold retuning are forbidden and force FALSIFIED.
- multiple_testing_boundary: V4 evaluates a small frozen pre-registered hypothesis set selected from a much larger V2/V3 candidate universe. Bootstrap p-values are Benjamini-Hochberg corrected and selection-deflated; surviving the correction is still not proof of alpha because the hypotheses were chosen after broad multiple testing. Survivors may only enter a paper-only next protocol queue.
- SURVIVED_STRICT_FALSIFICATION is a paper-only next-protocol follow-up, not alpha confirmation.

## Safety boundary
- no_live_orders_submitted: True
- no_testnet_orders_submitted: True
- no_account_data_used: True
- no_api_key_used: True
- no_private_order_or_fill_data_used: True
- holdout_retune_used: False
- strategy_optimization_performed: False
- live_or_testnet_path_created: False
- tsmom_state_changed: False

## Pre-registered hypotheses (frozen from V3 queue)
- preregistered_hypothesis_count: 5
- queued_families: funding, mean_reversion, momentum, price_return, volume_liquidity
- queued_horizons: [8, 24, 72]
- frozen_candidate_count: 1712
- evaluated_signal_count: 1712

## Stricter OOS configuration
- strict_fold_count: 12
- purge_bars: 144
- embargo_bars: 144
- bootstrap_resamples: 2000
- preregistered_alpha: 0.1

## Falsification outcomes
- survived_strict_falsification_count: 0
- falsified_count: 5
- funding / h8: FALSIFIED_SIGN_FLIP (expected_sign=-1, observed_sign=1, bootstrap_p=0.9525237381309345, bh_significant=False)
- mean_reversion / h8: FALSIFIED_NULL_FRAGILE (expected_sign=-1, observed_sign=-1, bootstrap_p=0.17791104447776113, bh_significant=False)
- momentum / h24: FALSIFIED_NULL_FRAGILE (expected_sign=-1, observed_sign=-1, bootstrap_p=0.12493753123438281, bh_significant=False)
- price_return / h72: FALSIFIED_OOS_INCONSISTENT (expected_sign=-1, observed_sign=-1, bootstrap_p=0.12193903048475763, bh_significant=False)
- volume_liquidity / h72: FALSIFIED_OOS_INCONSISTENT (expected_sign=1, observed_sign=1, bootstrap_p=0.20689655172413793, bh_significant=False)

## Next protocol queue
- empty (no hypothesis survived strict falsification)

## Failed gates / unclaimed checks
- preregistration_lock: passed (5 hypotheses, expected signs, and survival thresholds locked from frozen V3 queue before evaluation)
- sign_flip_guard: watch (1 hypotheses falsified by post-hoc sign flip vs pre-registered direction)
- bootstrap_fdr_significance: unclaimed (no hypothesis is Benjamini-Hochberg significant after correction; edge cannot be claimed)
- strict_family_survival: watch (5 of 5 pre-registered hypotheses were falsified)
- readiness_boundary: passed (readiness_baseline_verdict remains NOT_READY)
- tsmom_state_unchanged: passed (TSMOM artifacts are not modified or restored)

## What cannot be claimed
- alpha confirmed
- profitability
- live readiness
- execution readiness
- permission to trade real money
- restoration of TSMOM active development
- survivors imply edge

## Next loop
Use next_protocol_queue.csv only as input to a separate paper-only execution-aware protocol; surviving falsification is not alpha confirmation and never authorizes trading or readiness.
