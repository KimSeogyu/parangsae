# Quant Loop Report: 20260626-tsmom-switch-stability-funding-throttle-risk-v1

## Hypothesis
A train/validation-only near-tie sticky-selection rule plus a pre-registered public-funding-drag exposure throttle can reduce selected-candidate switch instability and improve funding-aware stress robustness without holdout tuning.

## Claim boundary
- tsmom_risk_loop_verdict: WATCH
- namespace: tsmom_switch_stability_funding_throttle_risk_only_not_readiness
- readiness_baseline_verdict: NOT_READY
- source_tsmom_loop_verdict: WATCH
- claim_boundary: TSMOM_SWITCH_STABILITY_FUNDING_THROTTLE_RISK_V1 is a pre-registered paper-only risk/cost research loop, not readiness or live readiness, not trading advice, not a profit guarantee, not execution readiness, not permission for live orders or testnet orders, and it uses no account, API-key, private order, or fill data for performance claims

## Pre-registration
- allowed_inputs: public_historical_ohlcv_closed_bars, public_historical_binance_usdm_funding_rates, train_split, validation_split, holdout_for_once_only_evaluation
- forbidden_inputs: live_orders, testnet_orders, account_state, api_keys, private_orders, fills, signed_endpoints, holdout_for_parameter_selection
- holdout_boundary: holdout is evaluated once after train/validation-only selection and is never used to tune rules

## Source failed gates preserved
- median_stress_return_beats_cash_null, selected_candidate_switch_rate, funding_aware_net_return

## Risk evaluation gates
- candidate_count: PASSED (observed=48, threshold=25)
- candidate_window_evaluations: PASSED (observed=1488, threshold=300)
- bars_processed_estimate: PASSED (observed=2388240, threshold=500000)
- median_stress_return_beats_cash_null: FAILED (observed=-1.5014549259752459, threshold=> 0.0)
- selected_candidate_switch_rate: PASSED (observed=0.7333333333333333, threshold=<= 0.75)
- funding_aware_net_return: FAILED (observed=-1.5014549259752459, threshold=> 0.0 with validated public funding)
- max_drawdown_abs_pct: PASSED (observed=5.481545539334309, threshold=<= 35.0)
- tail_loss_cvar_pct: PASSED (observed=-4.681780665426386, threshold=>= -15.0)

## Switch stability
- baseline_switch_count: 27
- baseline_switch_rate: 0.9
- stable_switch_count: 22
- stable_switch_rate: 0.7333333333333333
- runner_up_near_tie_window_count: 30
- sticky_selection_eligible_count: 8
- sticky_selection_triggered_count: 5

## Funding throttle comparison
- median_baseline_net_return_pct: -1.5014549259752459
- median_risk_net_return_pct: -1.5014549259752459
- median_delta_net_return_pct: 0.0
- median_risk_funding_drag_pct: 0.11499999999999999
- throttle_active_window_count: 11
- max_throttle_bar_pct: 91.86507936507937
- risk_tail_cvar_pct: -4.681780665426386
- risk_cash_null_failure_count: 26

## Safety
- live_orders_submitted: false
- testnet_orders_submitted: false
- api_keys_required_for_research: false
- account_or_fill_data_used: false
- strategy_parameters_retuned: false
- gates_relaxed: false
- live_trading_path_introduced: false

## Artifacts
- risk_loop.json
- candidate_switch_stability.csv
- funding_throttle_decomposition.csv
- regime_breakdown.csv
- commands.txt
- changed_files.txt

## Next loop
If risk gates remain failed, inspect gross edge and selection stability before any strategy retune; if all risk gates pass, run a separate readiness process without claiming live readiness.
