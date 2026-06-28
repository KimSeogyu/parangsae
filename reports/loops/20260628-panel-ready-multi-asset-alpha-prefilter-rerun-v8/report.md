# Quant Loop Report: 20260628-panel-ready-multi-asset-alpha-prefilter-rerun-v8

## Purpose
Rerun the existing V5 public multi-asset strict prefilter on the V7-ready cached panel only.

## Claim boundary
- suite_decision: NEEDS_STRICT_OOS_FALSIFICATION
- readiness_baseline_verdict: NOT_READY
- source_v7_panel_ready: True
- claim_boundary: PANEL_READY_MULTI_ASSET_ALPHA_PREFILTER_RERUN_V8 is a paper-only rerun of the existing V5 multi-asset alpha signal prefilter on the V7-ready public cached panel. It does not download or backfill data, does not generate a new strategy, does not perform portfolio construction, does not optimize signal weights or thresholds, does not use live/testnet/account/API-key/order/fill data, does not claim alpha/edge/profitability/readiness, and does not permit trading.
- multiple_testing_boundary: V8 reruns the existing V5 public multi-asset signal hypothesis prefilter only after V7 panel readiness. Thousands of diagnostics remain exposed to multiple-testing, data-snooping, duplicate-family, and survivorship risk. V8 can only create paper-only strict falsification candidates and cannot claim alpha, edge, profitability, readiness, execution readiness, or permission to trade.
- Generated signals are hypotheses, not edges, strategies, profitability evidence, readiness, or trade permission.

## Safety boundary
- no_live_orders_submitted: True
- no_testnet_orders_submitted: True
- no_account_data_used: True
- no_api_key_used: True
- no_private_order_or_fill_data_used: True
- data_download_performed: False
- backfill_performed: False
- holdout_retune_used: False
- strategy_optimization_performed: False
- live_or_testnet_path_created: False
- tsmom_state_changed: False

## V7 state
- source_v7_artifact_path: reports/loops/20260628-public-multi-asset-data-backfill-v7
- source_v7_suite_decision: BACKFILL_COMPLETED_PANEL_READY_FOR_V5_RERUN
- common_start: 2024-01-01T00:00:00Z
- common_end: 2025-12-31T23:00:00Z
- common_bar_count: 17544

## Universe
- requested: BTCUSDT, ETHUSDT, SOLUSDT, BNBUSDT, XRPUSDT, ADAUSDT, DOGEUSDT, AVAXUSDT, LINKUSDT
- used: BTCUSDT, ETHUSDT, SOLUSDT, BNBUSDT, XRPUSDT, ADAUSDT, DOGEUSDT, AVAXUSDT, LINKUSDT
- excluded_count: 1

## Generated signals
- generation_seed: 20260629
- candidate_budget: 5000
- max_depth: 4
- generated_signal_count: 486
- evaluated_signal_count: 486
- families_evaluated: cross_sectional_momentum, cross_sectional_reversal, funding_dispersion, lead_lag, beta_residual, volatility_dispersion, liquidity_volume_shock, correlation_breakdown, carry_x_trend, relative_value_interaction
- horizons_evaluated: 8, 24, 72

## Duplicate and multiple-testing controls
- duplicate_signal_count: 50
- duplicate_ratio: 0.102880658436214
- effective_signal_breadth: 436
- cannot_claim_edge_due_to_multiple_testing: True

## Shortlisted families
- carry_x_trend: WATCH_DIAGNOSTIC_ONLY (raw diagnostic metric requires future OOS falsification before any claim)
- correlation_breakdown: NEEDS_STRICT_OOS_FALSIFICATION (diagnostic OOS metric warrants paper-only stricter falsification)
- cross_sectional_momentum: WATCH_DIAGNOSTIC_ONLY (raw diagnostic metric requires future OOS falsification before any claim)
- cross_sectional_reversal: WATCH_DIAGNOSTIC_ONLY (raw diagnostic metric requires future OOS falsification before any claim)
- funding_dispersion: WATCH_DIAGNOSTIC_ONLY (raw diagnostic metric requires future OOS falsification before any claim)
- lead_lag: WATCH_DIAGNOSTIC_ONLY (raw diagnostic metric requires future OOS falsification before any claim)
- relative_value_interaction: WATCH_DIAGNOSTIC_ONLY (raw diagnostic metric requires future OOS falsification before any claim)
- volatility_dispersion: NEEDS_STRICT_OOS_FALSIFICATION (diagnostic OOS metric warrants paper-only stricter falsification)

## Failed gates / warnings
- symbol_availability: watch (1 requested symbols excluded for OHLCV/schema/min-bars reasons)
- duplicate_correlation_control: watch (50 redundant V5 signal variants marked)
- v4_queue_audit_only: passed (queue file present but blank)
- v4_state_preserved: passed (V4 source artifact is audit-only and not mutated)
- readiness_boundary: passed (readiness_baseline_verdict remains NOT_READY)
- tsmom_state_unchanged: passed (TSMOM artifacts are not modified or restored)
- multiple_testing_boundary: unclaimed (cannot_claim_edge_due_to_multiple_testing is true)

## What cannot be claimed
- alpha or edge confirmed
- profitability
- live readiness or execution readiness
- permission to trade, allocate capital, or use live/testnet/account data
- V5 automatic rerun beyond this paper-only prefilter artifact

## Next loop
Use next_strict_falsification_queue.csv only as a paper-only input to a separate future strict OOS falsification protocol. Do not infer alpha, readiness, or trade permission.
