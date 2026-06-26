# Quant Loop Report: 20260627-alpha-signal-factory-composition-harness-v1

## Purpose
Build a deterministic paper-only Alpha Signal Factory composition harness that generates, composes, diagnoses, duplicate-controls, and null-compares signal hypotheses.

## Claim boundary
- suite_decision: SIGNAL_FACTORY_INFRA_ONLY
- loop_namespace: alpha_signal_factory_composition_harness_only_not_readiness
- readiness_baseline_verdict: NOT_READY
- claim_boundary: ALPHA_SIGNAL_FACTORY_COMPOSITION_HARNESS_V1 is paper-only signal-generation, composition, duplicate-control, and null-comparison infrastructure. Generated signals are hypotheses, not proven edges, not a trading strategy, not trading advice, not profitability evidence, not readiness or live readiness, and not permission for live/testnet/account/API-key/order/fill use.
- Shortlists are paper-only research queues; they are not strategies, order intents, readiness, or profitability claims.

## Safety boundary
- no_live_orders_submitted: True
- no_testnet_orders_submitted: True
- no_account_data_used: True
- no_api_key_used: True
- no_private_order_or_fill_data_used: True
- holdout_retune_used: False
- tsmom_state_changed: False

## Signal generation
- signal_generation_seed: 20260627
- max_depth: 3
- generated_signal_count: 38
- evaluated_signal_count: 38
- families_evaluated: funding, interaction, mean_reversion, momentum, price_return, volatility, volume_liquidity
- horizons_evaluated: 8, 24, 72

## Duplicate and null controls
- duplicate_signal_count: 10
- effective_signal_breadth: 28
- null_baselines_used: zero_signal_cash_null, random_sign_signal, lagged_signal, inverse_signal, shuffled_timestamp_signal, buy_and_hold_same_window, frozen_tsmom_negative_control
- redundant duplicate rows: 10

## Shortlist
- watch_family_count: 5
- killed_signal_count: 0
- funding: WATCH_DIAGNOSTIC_ONLY (diagnostic IC survives as a paper-only hypothesis for another falsification loop)
- interaction: WATCH_DIAGNOSTIC_ONLY (diagnostic IC survives as a paper-only hypothesis for another falsification loop)
- mean_reversion: WATCH_DIAGNOSTIC_ONLY (diagnostic IC survives as a paper-only hypothesis for another falsification loop)
- momentum: WATCH_DIAGNOSTIC_ONLY (diagnostic IC survives as a paper-only hypothesis for another falsification loop)
- volatility: WATCH_DIAGNOSTIC_ONLY (diagnostic IC survives as a paper-only hypothesis for another falsification loop)

## Failed gates / unclaimed checks
- duplicate_correlation_control: watch (10 redundant signal variants marked)
- readiness_boundary: passed (readiness_baseline_verdict remains NOT_READY)
- tsmom_state_unchanged: passed (TSMOM artifacts are read-only negative-control references only)

## What cannot be claimed
- live readiness
- profitability
- execution readiness
- permission to use real money
- restoration of TSMOM active development
- that any generated signal is a durable edge

## Next loop
Use WATCH_* families only as candidates for a separate pre-registered paper-only falsification loop.
