# Quant Loop Report: 20260626-research-null-baseline-harness-v1

## Purpose
Implement a strategy-agnostic null/baseline evaluation harness so future research loops must compare claims against the same cash, buy-and-hold, random sign, lagged signal, inverse signal, and durable-evidence-only simpler trend baselines.

## Claim boundary
- research_null_baseline_harness_decision: VALIDATION_INFRA_ONLY_TSMOM_UNCHANGED
- namespace: research_null_baseline_harness_only_not_readiness
- readiness_baseline_verdict: NOT_READY
- claim_boundary: RESEARCH_NULL_BASELINE_HARNESS_V1 is strategy-agnostic paper-only validation infrastructure. It makes future strategy claims harder by standardizing null/baseline comparison rows, leakage metadata, deterministic random signs, and holdout-retune boundaries. It is not alpha, not strategy optimization, not readiness or live readiness, not trading advice, not a profit guarantee, and uses no live, testnet, account, API-key, private order, or fill data.

## Schema
- required_columns: baseline_name, strategy_return_gross, strategy_return_net, baseline_return_gross, baseline_return_net, strategy_beats_baseline_gross, strategy_beats_baseline_net, sample_size, trade_count, turnover, fees_drag, slippage_drag, funding_drag, max_drawdown, regime_filter, evidence_source, claim_boundary
- baseline_families: cash_null, buy_and_hold, random_sign, lagged_signal, inverse_signal, simpler_trend_baseline
- comparison_row_count: 172
- missing_or_unclaimed_baselines: simpler_trend_baseline

## Leakage defense
- forward_return_label_overlap_recorded: True
- forward_return_label_overlap_count: 0
- lagged_baseline_uses_current_bar_info: False
- random_sign_seed_policy: 1729 + sum(ord(char) for window_id)
- holdout_retune_used_for_baseline_comparison: False

## TSMOM state preserved
- source_tsmom_loop_verdict: WATCH
- candidate_family_decision: REGIME_ONLY_WATCH
- regime_only_tsmom_falsification_decision: FREEZE_AS_NEGATIVE_CONTROL_CANDIDATE
- broad_tsmom_active_development_restored: False
- regime_only_tsmom_active_candidate_restored: False

## Baseline family counts
- cash_null: 34
- buy_and_hold: 34
- random_sign: 34
- lagged_signal: 34
- inverse_signal: 34
- simpler_trend_baseline: 2

## What cannot be claimed
- live readiness
- profitability
- execution readiness
- permission to use real money
- restoration of broad or regime-only TSMOM active development

## Next loop
Apply this harness to future non-TSMOM hypotheses before making strategy-performance claims.
