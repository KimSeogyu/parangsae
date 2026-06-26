# Quant Loop Report: 20260626-regime-only-tsmom-falsification-v1

## Hypothesis
If the prior TSMOM regime-only exception is worth continued paper research, the existing non-retuned TSMOM candidate rows in the sideways + low-vol slice should show sufficient sample coverage, positive cost-adjusted net return, and robustness versus null baselines.

## Edge source
behavioral trend/momentum exception under low-vol sideways market structure, falsification-only

## Claim boundary
- regime_only_tsmom_falsification_decision: FREEZE_AS_NEGATIVE_CONTROL_CANDIDATE
- namespace: regime_only_tsmom_falsification_only_not_readiness
- readiness_baseline_verdict: NOT_READY
- source_candidate_family_decision: REGIME_ONLY_WATCH
- claim_boundary: REGIME_ONLY_TSMOM_FALSIFICATION_V1 is a paper-only research-governance falsification loop for the previously allowed sideways + low-vol TSMOM exception. It does not optimize parameters, does not retune on holdout, does not relax gates, is not readiness or live readiness, is not trading advice, is not a profit guarantee, and uses no live, testnet, account, API-key, private order, or fill data.

## Regime slice evidence
- slice_window_count: 3
- total_window_count: 31
- regime_coverage_pct: 9.67741935483871
- median_gross_return_pct: 1.3819203383031775
- median_net_return_pct: -0.6503531796520268
- median_funding_drag_pct: 0.075
- median_slippage_drag_pct: 0.5
- median_stress_drag_pct: 0.9887926365135691
- median_turnover: 5.0
- total_trade_count: 13
- worst_drawdown_pct: -1.3330229672273042
- net_positive_window_count: 1

## Baseline / null comparison
- cash_null: status=available, count=3, median_baseline_return_pct=0.0, median_tsmom_minus_baseline_return_pct=1.3819203383031775, baseline_beats_or_matches_tsmom_count=1
- buy_and_hold: status=available, count=3, median_baseline_return_pct=0.11594309586437923, median_tsmom_minus_baseline_return_pct=1.2659772424387983, baseline_beats_or_matches_tsmom_count=1
- random_sign: status=available, count=3, median_baseline_return_pct=-0.6006780473893913, median_tsmom_minus_baseline_return_pct=1.9825983856925689, baseline_beats_or_matches_tsmom_count=1
- lagged_signal: status=available, count=3, median_baseline_return_pct=-0.11836058420958695, median_tsmom_minus_baseline_return_pct=-0.0004309028450055763, baseline_beats_or_matches_tsmom_count=2
- inverse_signal: status=available, count=3, median_baseline_return_pct=-1.3851363826343333, median_tsmom_minus_baseline_return_pct=2.7670567209375108, baseline_beats_or_matches_tsmom_count=1
- simpler_trend_baseline: status=unavailable_unclaimed, count=0, median_baseline_return_pct=None, median_tsmom_minus_baseline_return_pct=None, baseline_beats_or_matches_tsmom_count=None

## Decision gates
- readiness_boundary_not_live_ready: PASSED (observed=NOT_READY, threshold=must remain NOT_READY and not emit live readiness)
- source_family_decision_is_regime_only_watch: PASSED (observed=REGIME_ONLY_WATCH, threshold=REGIME_ONLY_WATCH)
- no_parameter_retune_or_holdout_reselection: PASSED (observed={'parameter_retune': False, 'holdout_reselection': False}, threshold=no retune and no holdout reselection)
- regime_slice_sample_size: FAILED (observed=3, threshold=>= 5 windows for a regime-only exception)
- median_gross_return_positive: PASSED (observed=1.3819203383031775, threshold=> 0.0)
- cost_adjusted_net_return_beats_cash_null: FAILED (observed=-0.6503531796520268, threshold=> 0.0 after funding/slippage/stress costs)
- net_positive_hit_rate: FAILED (observed=0.3333333333333333, threshold=> 0.5)
- baseline_null_robustness: FAILED (observed={'cash_null': {'median_delta': 1.3819203383031775, 'baseline_beats_or_matches_tsmom_count': 1}, 'buy_and_hold': {'median_delta': 1.2659772424387983, 'baseline_beats_or_matches_tsmom_count': 1}, 'random_sign': {'median_delta': 1.9825983856925689, 'baseline_beats_or_matches_tsmom_count': 1}, 'lagged_signal': {'median_delta': -0.0004309028450055763, 'baseline_beats_or_matches_tsmom_count': 2}, 'inverse_signal': {'median_delta': 2.7670567209375108, 'baseline_beats_or_matches_tsmom_count': 1}}, threshold=all available nulls median delta > 0 and no null beats/matches in a majority of slice windows)
- simpler_trend_baseline_available: UNCLAIMED (observed=unavailable_unclaimed, threshold=available, or explicitly unclaimed if unavailable)
- failed_gates_preserved: PASSED (observed=['median_stress_return_beats_cash_null', 'selected_candidate_switch_rate', 'funding_aware_net_return'], threshold=source failed gates remain recorded and unrenamed)

## Failed gates preserved
- source_tsmom_loop: median_stress_return_beats_cash_null
- source_tsmom_loop: selected_candidate_switch_rate
- source_tsmom_loop: funding_aware_net_return
- tsmom_diagnostic_loop: median_stress_return_beats_cash_null
- tsmom_diagnostic_loop: selected_candidate_switch_rate
- tsmom_diagnostic_loop: funding_aware_net_return
- tsmom_risk_loop: median_stress_return_beats_cash_null
- tsmom_risk_loop: funding_aware_net_return
- tsmom_gross_edge_loop: median_gross_return_beats_cash_null
- tsmom_gross_edge_loop: gross_edge_hit_rate
- tsmom_gross_edge_loop: median_gross_beats_random_sign_baseline
- tsmom_gross_edge_loop: median_gross_beats_lagged_signal_baseline
- tsmom_gross_edge_loop: median_gross_beats_inverse_signal_baseline
- tsmom_gross_edge_loop: median_gross_beats_buy_and_hold_baseline
- candidate_family_decision_loop: active_keep_watch_gross_edge
- candidate_family_decision_loop: active_keep_watch_null_robustness
- candidate_family_decision_loop: deprecate_candidate_family
- regime_only_tsmom_falsification_loop: regime_slice_sample_size
- regime_only_tsmom_falsification_loop: cost_adjusted_net_return_beats_cash_null
- regime_only_tsmom_falsification_loop: net_positive_hit_rate
- regime_only_tsmom_falsification_loop: baseline_null_robustness
- regime_only_tsmom_falsification_loop: simpler_trend_baseline_available

## What cannot be claimed
- live readiness
- profitability
- execution readiness
- permission to use real money
- validation of broad TSMOM active development

## Next loop
Use this slice only as a negative-control benchmark; prioritize non-TSMOM hypotheses or data/validation hardening.
