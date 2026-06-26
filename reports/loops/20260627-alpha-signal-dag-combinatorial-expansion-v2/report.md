# Quant Loop Report: 20260627-alpha-signal-dag-combinatorial-expansion-v2

## Purpose
Build a bounded deterministic paper-only combinatorial Signal DAG expansion harness for alpha-signal hypothesis generation, duplicate pruning, null comparison, and multiple-testing-aware family summaries.

## Claim boundary
- suite_decision: SIGNAL_DAG_INFRA_ONLY
- loop_namespace: alpha_signal_dag_combinatorial_expansion_v2_only_not_readiness
- readiness_baseline_verdict: NOT_READY
- claim_boundary: ALPHA_SIGNAL_DAG_COMBINATORIAL_EXPANSION_V2 is paper-only combinatorial Signal DAG generation, diagnostic evaluation, duplicate-control, multiple-testing warning, and null-comparison infrastructure. Generated DAG signals are hypotheses, not proven edges, not a trading strategy, not trading advice, not profitability evidence, not readiness or live readiness, and not permission for live/testnet/account/API-key/order/fill use.
- multiple_testing_boundary: This loop generates many signal hypotheses and therefore has high data-snooping and false-discovery risk. It may only produce paper-only family follow-up queues; it cannot claim edge without a separate future pre-registered OOS falsification protocol.
- Shortlists are paper-only research queues; they are not strategies, order intents, readiness, profitability claims, or live/testnet permission.

## Safety boundary
- no_live_orders_submitted: True
- no_testnet_orders_submitted: True
- no_account_data_used: True
- no_api_key_used: True
- no_private_order_or_fill_data_used: True
- holdout_retune_used: False
- tsmom_state_changed: False

## DAG generation
- generation_seed: 20260628
- candidate_budget: 2000
- max_depth: 4
- generated_signal_count: 2000
- evaluated_signal_count: 2000
- families_evaluated: funding, mean_reversion, momentum, price_return, volatility, volume_liquidity
- operator_families_used: binary, gate, unary

## Duplicate and null controls
- duplicate_signal_count: 1273
- effective_signal_breadth: 727
- duplicate_ratio: 0.6365
- null_baselines_used: zero_signal_cash_null, random_sign_signal, lagged_signal, inverse_signal, shuffled_timestamp_signal, family_permuted_signal, buy_and_hold_same_window, frozen_tsmom_negative_control
- redundant duplicates marked: 1273

## Multiple-testing warning
- cannot_claim_edge_due_to_multiple_testing: True
- approximate_false_discovery_warning: many generated DAG hypotheses inflate false-discovery risk; any apparent IC requires a separate future OOS falsification protocol.

## Paper-only shortlist
- funding: NEEDS_OOS_FALSIFICATION (diagnostic IC only; requires separate pre-registered paper-only OOS falsification)
- mean_reversion: NEEDS_OOS_FALSIFICATION (diagnostic IC only; requires separate pre-registered paper-only OOS falsification)
- momentum: NEEDS_OOS_FALSIFICATION (diagnostic IC only; requires separate pre-registered paper-only OOS falsification)
- price_return: NEEDS_OOS_FALSIFICATION (diagnostic IC only; requires separate pre-registered paper-only OOS falsification)
- volatility: NEEDS_OOS_FALSIFICATION (diagnostic IC only; requires separate pre-registered paper-only OOS falsification)
- volume_liquidity: NEEDS_OOS_FALSIFICATION (diagnostic IC only; requires separate pre-registered paper-only OOS falsification)

## Failed gates / unclaimed checks
- duplicate_correlation_control: watch (1273 redundant DAG signal variants marked)
- multiple_testing_edge_claim: unclaimed (combinatorial DAG scan cannot claim edge without separate OOS protocol)
- readiness_boundary: passed (readiness_baseline_verdict remains NOT_READY)
- tsmom_state_unchanged: passed (TSMOM artifacts are not modified or restored)

## What cannot be claimed
- live readiness
- profitability
- execution readiness
- permission to use real money
- restoration of TSMOM active development
- that any generated DAG signal is a durable edge
- that any shortlist family is tradable without a separate pre-registered paper-only OOS falsification loop

## Next loop
Use any WATCH/NEEDS_OOS family only as input to a separate pre-registered paper-only OOS falsification loop.
