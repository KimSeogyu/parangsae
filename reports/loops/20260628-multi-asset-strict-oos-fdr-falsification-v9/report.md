# MULTI_ASSET_STRICT_OOS_FDR_FALSIFICATION_V9

## Verdict
- suite_decision: NO_FAMILY_SURVIVES_STRICT_FALSIFICATION
- readiness_baseline_verdict: NOT_READY
- claim_boundary: MULTI_ASSET_STRICT_OOS_FDR_FALSIFICATION_V9 freezes the V8 paper-only queue and performs stricter falsification. It is not alpha generation, not a trading strategy, not portfolio construction, not signal weight optimization, not profitability evidence, not readiness or live readiness, and not permission for live/testnet/account/API-key/order/fill use.

## Frozen V8 Queue
- source_v8_artifact_path: reports/loops/20260628-panel-ready-multi-asset-alpha-prefilter-rerun-v8
- source_v8_queue_count: 2
- frozen_queue_count: 6
- evaluated_signal_count: 6

## Strict OOS / FDR
- chronological_fold_count: 9
- purge_bars: 72
- embargo_bars: 72
- permutation_count: 1000
- block_size: equals_horizon_bars
- duplicate_adjusted_test_count: 6

## Family decisions
- correlation_breakdown h8: FALSIFIED_SIGN_FLIP (test_ic=-0.002532814448271075, q=0.847152847153)
- correlation_breakdown h24: FALSIFIED_SIGN_FLIP (test_ic=-0.03262528251897266, q=0.847152847153)
- correlation_breakdown h72: FALSIFIED_SIGN_FLIP (test_ic=0.03795435246382513, q=0.847152847153)
- volatility_dispersion h8: FALSIFIED_SIGN_FLIP (test_ic=0.03391277171778956, q=0.847152847153)
- volatility_dispersion h24: FALSIFIED_SIGN_FLIP (test_ic=0.03721457888716796, q=0.847152847153)
- volatility_dispersion h72: FALSIFIED_SIGN_FLIP (test_ic=0.06738604928254943, q=0.847152847153)

## Safety
- no_live_orders_submitted: True
- no_testnet_orders_submitted: True
- no_account_data_used: True
- no_api_key_used: True
- no_private_order_or_fill_data_used: True
- data_download_performed: False
- tsmom_state_changed: False

## What cannot be claimed
- alpha or edge confirmed
- profitability
- live readiness or execution readiness
- permission to trade, allocate capital, or use live/testnet/account data
- strategy, portfolio, signal-weight, threshold, or horizon optimization
