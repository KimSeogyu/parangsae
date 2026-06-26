# Quant Loop Report: 20260626-tsmom-gross-edge-regime-null-robustness-v1

## Hypothesis
Before any retuning, the current frozen TSMOM family should show positive gross return before funding, slippage, and stress costs, and should beat simple null baselines across public-data-derived regimes; otherwise the family remains WATCH or should be narrowed or deprecated.

## Claim boundary
- tsmom_gross_edge_loop_verdict: WATCH
- namespace: tsmom_gross_edge_regime_null_robustness_only_not_readiness
- readiness_baseline_verdict: NOT_READY
- source_tsmom_loop_verdict: WATCH
- claim_boundary: TSMOM_GROSS_EDGE_REGIME_NULL_ROBUSTNESS_V1 is a pre-registered paper-only diagnostic/research loop for gross-edge and null-baseline falsification, not readiness or live readiness, not trading advice, not a profit guarantee, not execution readiness, not permission for live orders or testnet orders, and it uses no live, testnet, account, API-key, private order, or fill data for performance claims

## Pre-registration
- allowed_inputs: public_historical_ohlcv_closed_bars, public_historical_binance_usdm_funding_rates, train_split, validation_split, holdout_for_once_only_evaluation, source_tsmom_selected_candidates
- forbidden_inputs: live_orders, testnet_orders, account_state, api_keys, private_orders, fills, signed_endpoints, holdout_for_parameter_selection, live_or_testnet_performance
- holdout_boundary: holdout is evaluated once using the source TSMOM train/validation-selected candidate; null comparisons and regime labels are diagnostic only and never tune source parameters

## Source failed gates preserved
- median_stress_return_beats_cash_null, selected_candidate_switch_rate, funding_aware_net_return

## Gross edge summary
- window_count: 31
- median_gross_return_pct: -0.5318158824126007
- gross_edge_hit_rate: 0.3870967741935484
- median_net_return_pct: -1.5014549259752459
- cash_null_failure_count: 19
- median_difference_vs_buy_and_hold_pct: -0.7353385016150904
- cvar_tail_gross_return_pct: -2.6269630157115365

## Gross edge gates
- median_gross_return_beats_cash_null: FAILED (observed=-0.5318158824126007, threshold=> 0.0)
- gross_edge_hit_rate: FAILED (observed=0.3870967741935484, threshold=> 0.5)
- median_gross_beats_random_sign_baseline: FAILED (observed=-0.041693984777968485, threshold=> 0.0)
- median_gross_beats_lagged_signal_baseline: FAILED (observed=-0.0004309028450055763, threshold=> 0.0)
- median_gross_beats_inverse_signal_baseline: FAILED (observed=-0.9239983623700931, threshold=> 0.0)
- median_gross_beats_buy_and_hold_baseline: FAILED (observed=-0.7353385016153124, threshold=> 0.0)
- source_failed_gates_preserved: PASSED (observed=['median_stress_return_beats_cash_null', 'selected_candidate_switch_rate', 'funding_aware_net_return'], threshold=source failed gates must remain present and unrenamed)

## Null robustness
- buy_and_hold: median_baseline_return_pct=0.11594309586437923, median_tsmom_minus_baseline_return_pct=-0.7353385016153124, baseline_beats_or_matches_tsmom_count=16
- cash_null: median_baseline_return_pct=0.0, median_tsmom_minus_baseline_return_pct=-0.5318158824126007, baseline_beats_or_matches_tsmom_count=19
- inverse_signal: median_baseline_return_pct=0.39218247995749245, median_tsmom_minus_baseline_return_pct=-0.9239983623700931, baseline_beats_or_matches_tsmom_count=19
- lagged_signal: median_baseline_return_pct=0.0, median_tsmom_minus_baseline_return_pct=-0.0004309028450055763, baseline_beats_or_matches_tsmom_count=18
- random_sign: median_baseline_return_pct=0.0, median_tsmom_minus_baseline_return_pct=-0.041693984777968485, baseline_beats_or_matches_tsmom_count=18

## Regime gross edge
- funding_drag_regime=high-funding-drag: count=4, median_gross=-0.5876158166163159, hit_rate=0.25, gate=failed_or_unclaimed
- funding_drag_regime=low-funding-drag: count=27, median_gross=-0.05272471494669473, hit_rate=0.4074074074074074, gate=failed_or_unclaimed
- funding_regime=high-funding-cost: count=1, median_gross=-2.098521394225561, hit_rate=0.0, gate=failed_or_unclaimed
- funding_regime=negative-funding: count=1, median_gross=0.0, hit_rate=0.0, gate=failed_or_unclaimed
- funding_regime=positive-funding: count=29, median_gross=-0.5318158824126007, hit_rate=0.41379310344827586, gate=failed_or_unclaimed
- market_regime=bear: count=13, median_gross=-0.6393130051843943, hit_rate=0.15384615384615385, gate=failed_or_unclaimed
- market_regime=bull: count=12, median_gross=-0.08231083286764918, hit_rate=0.5, gate=failed_or_unclaimed
- market_regime=sideways: count=6, median_gross=0.4463493722155709, hit_rate=0.6666666666666666, gate=passed
- trend_regime=trend_mixed: count=4, median_gross=-0.06278761041214609, hit_rate=0.5, gate=failed_or_unclaimed
- trend_regime=trend_persistence: count=14, median_gross=-0.31965650259219713, hit_rate=0.35714285714285715, gate=failed_or_unclaimed
- trend_regime=trend_reversal: count=13, median_gross=-0.5318158824126007, hit_rate=0.38461538461538464, gate=failed_or_unclaimed
- volatility_regime=low-vol: count=6, median_gross=1.5945640120275995, hit_rate=0.6666666666666666, gate=passed
- volatility_regime=normal-vol: count=25, median_gross=-0.6004568388549902, hit_rate=0.32, gate=failed_or_unclaimed

## Safety
- live_orders_submitted: false
- testnet_orders_submitted: false
- api_keys_required_for_research: false
- account_or_fill_data_used: false
- strategy_parameters_retuned: false
- gates_relaxed: false
- live_trading_path_introduced: false

## Artifacts
- gross_edge_loop.json
- gross_edge_decomposition.csv
- regime_edge_breakdown.csv
- null_baseline_comparison.csv
- failed_edge_windows.csv
- commands.txt
- changed_files.txt

## Next loop
If gross edge remains weak versus null baselines, pre-register either a narrower regime-specific falsification loop or park/deprecate this TSMOM family; do not retune on holdout.
