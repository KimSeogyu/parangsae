# Quant Loop Report: 20260626-tsmom-stress-switch-funding-diagnostic-v1

## Hypothesis
The failed TSMOM stress, switch-rate, and funding-aware net-return gates can be explained without retuning by decomposing selected walk-forward holdouts by regime, funding drag, and candidate-selection instability.

## Claim boundary
- tsmom_diagnostic_loop_verdict: DIAGNOSTIC_COMPLETE
- namespace: tsmom_stress_switch_funding_diagnostic_only_not_readiness
- readiness_baseline_verdict: NOT_READY
- This is diagnostic-only, not readiness; claim_boundary: TSMOM_STRESS_SWITCH_FUNDING_DIAGNOSTIC_V1 is a paper-only diagnostic loop, not readiness or live readiness, not trading advice, not a profit guarantee, not strategy parameter optimization, not permission for live orders or testnet orders, and it uses no account, API-key, order, or fill data for performance claims

## Source failed gates
- source_tsmom_loop_verdict: WATCH
- source_tsmom_loop_decision: ITERATE_ON_SAME_HYPOTHESIS
- analyzed_failed_gates: median_stress_return_beats_cash_null, selected_candidate_switch_rate, funding_aware_net_return

## Median stress return vs cash/null
- count: 31
- median_net_return_pct: -1.5014549259752459
- mean_net_return_pct: -1.5551975725139542
- worst_net_return_pct: -4.986574106354551
- cvar_tail_net_return_pct: -4.775379842171519
- failed_window_count: 26

## Candidate switch diagnostic
- switch_count: 27
- switch_rate: 0.9
- top_switch_causes: unstable_near_tie_candidates, volatility_target_change, lookback_sensitivity, exposure_cap_change, funding_threshold_change

## Funding-aware net return diagnostic
- median_gross_return_pct: -0.7648991553115825
- median_funding_cost_drag_pct: 0.0975
- median_slippage_drag_pct: 0.5
- median_net_return_pct: -1.5014549259752459
- primary_failure_modes: funding_cost_drag_dominates_raw_edge, low_gross_edge_after_funding_and_slippage, low_or_wrong_direction_gross_edge_bear, low_or_wrong_direction_gross_edge_bull, low_or_wrong_direction_gross_edge_sideways, stress_costs_erased_edge

## Regime decomposition
- funding_regime:high-funding-cost: count=1, median_net=-4.564185577988487, failure_rate=1.0
- funding_regime:negative-funding: count=1, median_net=0.0, failure_rate=1.0
- funding_regime:positive-funding: count=29, median_net=-1.5014549259752459, failure_rate=0.8275862068965517
- market_regime:bear: count=13, median_net=-1.5014549259752459, failure_rate=0.9230769230769231
- market_regime:bull: count=12, median_net=-1.6748523687322259, failure_rate=0.75
- market_regime:sideways: count=6, median_net=-1.7017092614583071, failure_rate=0.8333333333333334
- volatility_regime:low-vol: count=6, median_net=-0.4845676838391529, failure_rate=0.6666666666666666
- volatility_regime:normal-vol: count=25, median_net=-1.8773954702830684, failure_rate=0.88

## Artifacts
- diagnostic.json
- failed_windows.csv
- candidate_switches.csv
- funding_decomposition.csv
- commands.txt
- changed_files.txt

## Safety
- live_orders_submitted: false
- testnet_orders_submitted: false
- api_keys_required_for_research: false
- account_or_fill_data_used: false
- strategy_parameters_retuned: false

## Next pre-registered loop recommendation
Pre-register a risk/cost loop that reduces candidate-switch instability and explicitly tests whether funding drag should throttle exposure before holdout, without relaxing gates or claiming readiness.
