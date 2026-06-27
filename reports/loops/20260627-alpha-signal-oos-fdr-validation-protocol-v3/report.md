# Quant Loop Report: 20260627-alpha-signal-oos-fdr-validation-protocol-v3

## Purpose
Validate the frozen V2 Signal DAG universe with paper-only chronological OOS, purge/embargo, duplicate-adjusted, and multiple-testing/FDR-style diagnostics.

## Claim boundary
- suite_decision: OOS_FDR_PROTOCOL_INFRA_ONLY
- readiness_baseline_verdict: NOT_READY
- source_v2_artifact_path: reports/loops/20260627-alpha-signal-dag-combinatorial-expansion-v2
- claim_boundary: ALPHA_SIGNAL_OOS_FDR_VALIDATION_PROTOCOL_V3 is paper-only OOS/FDR validation infrastructure over a frozen V2 Signal DAG candidate universe. It is not new alpha generation, not a trading strategy, not signal-weight optimization, not profitability evidence, not readiness or live readiness, and not permission for live/testnet/account/API-key/order/fill use.
- multiple_testing_boundary: V3 evaluates a frozen large candidate universe with duplicate-adjusted breadth and conservative false-discovery warnings. It may only create a paper-only next falsification queue; OOS consistency is not proof of alpha without separate future protocols and live/execution-readiness evidence.
- WATCH_FAMILY_ONLY or NEEDS_STRICTER_OOS_FALSIFICATION is a paper-only follow-up queue, not alpha confirmation.

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

## Frozen candidate universe
- source_v2_generated_signal_count: 2000
- frozen_candidate_count: 2000
- evaluated_signal_count: 2000
- signal IDs preserved: True

## OOS folds
- chronological_fold_count: 9
- horizon_count: 3
- purge_bars: 72
- embargo_bars: 72

## Multiple-testing / FDR boundary
- total_signal_tests: 18000
- effective_signal_breadth: 727
- duplicate_adjusted_test_count: 6543
- cannot_claim_edge_due_to_multiple_testing: True

## Family decisions
- funding / h8: NEEDS_STRICTER_OOS_FALSIFICATION (median_test_ic=-0.004114803990316047, sign_consistency_rate=0.5606060606060606)
- funding / h24: FAILS_OOS_CONSISTENCY (median_test_ic=0.0019544470200105473, sign_consistency_rate=0.4494949494949495)
- funding / h72: FAILS_OOS_CONSISTENCY (median_test_ic=0.002588220116273889, sign_consistency_rate=0.5515151515151515)
- mean_reversion / h8: NEEDS_STRICTER_OOS_FALSIFICATION (median_test_ic=-0.014554386712849603, sign_consistency_rate=0.6041666666666666)
- mean_reversion / h24: FAILS_OOS_CONSISTENCY (median_test_ic=-0.014002191591710109, sign_consistency_rate=0.4895833333333333)
- mean_reversion / h72: FAILS_NULL_ROBUSTNESS (median_test_ic=-0.011720581469784632, sign_consistency_rate=0.5416666666666666)
- momentum / h8: FAILS_OOS_CONSISTENCY (median_test_ic=-0.005580207615636003, sign_consistency_rate=0.5149017121116043)
- momentum / h24: NEEDS_STRICTER_OOS_FALSIFICATION (median_test_ic=-0.014088866091081325, sign_consistency_rate=0.6759670259987318)
- momentum / h72: FAILS_NULL_ROBUSTNESS (median_test_ic=-0.001557329466173827, sign_consistency_rate=0.4800253646163602)
- price_return / h8: FAILS_OOS_CONSISTENCY (median_test_ic=-0.004367641192329474, sign_consistency_rate=0.6279761904761905)
- price_return / h24: FAILS_OOS_CONSISTENCY (median_test_ic=-0.02085664476270356, sign_consistency_rate=0.6934523809523809)
- price_return / h72: NEEDS_STRICTER_OOS_FALSIFICATION (median_test_ic=-0.01336158927074645, sign_consistency_rate=0.47023809523809523)
- volatility / h8: FAILS_OOS_CONSISTENCY (median_test_ic=0.0041853852293224315, sign_consistency_rate=0.5768229166666666)
- volatility / h24: FAILS_OOS_CONSISTENCY (median_test_ic=0.0002660494352126337, sign_consistency_rate=0.6302083333333334)
- volatility / h72: FAILS_OOS_CONSISTENCY (median_test_ic=0.0076997112883954565, sign_consistency_rate=0.50390625)
- volume_liquidity / h8: FAILS_OOS_CONSISTENCY (median_test_ic=0.0015724572446294601, sign_consistency_rate=0.5703125)
- volume_liquidity / h24: FAILS_OOS_CONSISTENCY (median_test_ic=0.0026817350760581027, sign_consistency_rate=0.5546875)
- volume_liquidity / h72: NEEDS_STRICTER_OOS_FALSIFICATION (median_test_ic=0.007588408066938109, sign_consistency_rate=0.4739583333333333)

## Next falsification queue
- funding / h8: NEEDS_STRICTER_OOS_FALSIFICATION (paper-only OOS diagnostics warrant stricter falsification, not trading)
- mean_reversion / h8: NEEDS_STRICTER_OOS_FALSIFICATION (paper-only OOS diagnostics warrant stricter falsification, not trading)
- momentum / h24: NEEDS_STRICTER_OOS_FALSIFICATION (paper-only OOS diagnostics warrant stricter falsification, not trading)
- price_return / h72: NEEDS_STRICTER_OOS_FALSIFICATION (paper-only OOS diagnostics warrant stricter falsification, not trading)
- volume_liquidity / h72: NEEDS_STRICTER_OOS_FALSIFICATION (paper-only OOS diagnostics warrant stricter falsification, not trading)

## Failed gates / unclaimed checks
- duplicate_adjusted_breadth: watch (1273 V2 duplicate candidates remain duplicate-adjusted)
- multiple_testing_fdr_boundary: unclaimed (FDR-style warnings prevent edge/profit/readiness claims)
- family_oos_survival: watch (13 family-horizon rows failed or were killed)
- readiness_boundary: passed (readiness_baseline_verdict remains NOT_READY)
- tsmom_state_unchanged: passed (TSMOM artifacts are not modified or restored)

## What cannot be claimed
- alpha confirmed
- profitability
- live readiness
- execution readiness
- permission to trade real money
- restoration of TSMOM active development

## Next loop
Use next_falsification_queue.csv only as input to a separate paper-only pre-registered falsification loop; do not trade or claim readiness.
