# Quant Loop Report: 20260627-multi-asset-alpha-generator-strict-prefilter-v5

## Purpose
Generate a public multi-asset cross-sectional / relative-value / lead-lag / funding-dispersion signal universe and immediately apply a strict paper-only diagnostic prefilter.

## Claim boundary
- suite_decision: NO_SURVIVING_SIGNAL_FAMILY
- readiness_baseline_verdict: NOT_READY
- claim_boundary: MULTI_ASSET_ALPHA_GENERATOR_STRICT_PREFILTER_V5 is paper-only multi-asset signal factory and strict diagnostic prefilter infrastructure. Generated signals are hypotheses, not edges, not a trading strategy, not portfolio construction, not signal weight optimization, not profitability evidence, not readiness or live readiness, and not permission for live/testnet/account/API-key/order/fill use.
- multiple_testing_boundary: V5 generates a large public multi-asset signal hypothesis universe. Every metric is diagnostic and exposed to multiple-testing, data-snooping, duplicate-family, and survivorship risk. V5 may only create paper-only next falsification candidates; it cannot claim alpha, edge, profitability, readiness, execution readiness, or permission to trade.
- v4_lineage_boundary: V4 artifacts are lineage/audit inputs only. V5 records the source V4 path and queue emptiness but never seeds candidates from V4 survivors, V4 family queues, V3/V4 representatives, V4 horizons, V4 thresholds, or V4 evidence.
- Generated signals are hypotheses, not edges, strategies, profitability evidence, readiness, or trade permission.

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

## V4 state
- source_v4_artifact_path: reports/loops/20260627-alpha-signal-strict-falsification-protocol-v4
- v4_queue_empty: True
- v4_families_revived: False
- v4_candidates_reused: False

## Universe
- requested: BTCUSDT, ETHUSDT, SOLUSDT, BNBUSDT, XRPUSDT, ADAUSDT, DOGEUSDT, AVAXUSDT, LINKUSDT, TONUSDT
- used: BTCUSDT
- excluded_count: 9

## Generated signals
- generation_seed: 20260629
- candidate_budget: 5000
- max_depth: 4
- generated_signal_count: 34
- evaluated_signal_count: 34
- families_evaluated: cross_sectional_momentum, cross_sectional_reversal, funding_dispersion, lead_lag, beta_residual, volatility_dispersion, liquidity_volume_shock, correlation_breakdown, carry_x_trend, relative_value_interaction
- horizons_evaluated: 8, 24, 72

## Duplicate and multiple-testing controls
- duplicate_signal_count: 5
- duplicate_ratio: 0.14705882352941177
- effective_signal_breadth: 29
- cannot_claim_edge_due_to_multiple_testing: True

## Shortlisted families
- none

## Failed gates / warnings
- multi_asset_minimum_symbol_count: failed (1 OHLCV-usable symbols; cross-sectional evidence is limited)
- symbol_availability: watch (9 requested symbols excluded for OHLCV/schema/min-bars reasons)
- duplicate_correlation_control: watch (5 redundant V5 signal variants marked)
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
- restoration of V4-falsified single-symbol candidate families

## Next loop
Use next_strict_falsification_queue.csv only as input to a separate future paper-only strict falsification protocol.
