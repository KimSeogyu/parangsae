# Quant Loop Report: 20260626-tsmom-candidate-family-decision-v1

## Decision purpose
Decide whether the current TSMOM candidate family should remain broad WATCH, be narrowed to specific regimes, be frozen as a negative-control benchmark, or be deprecated from active candidate development based only on accumulated paper-only durable artifacts.

## Claim and readiness boundary
- tsmom_candidate_family_decision: REGIME_ONLY_WATCH
- namespace: tsmom_candidate_family_decision_only_not_readiness
- readiness_baseline_verdict: NOT_READY
- source_tsmom_loop_verdict: WATCH
- tsmom_risk_loop_verdict: WATCH
- tsmom_gross_edge_loop_verdict: WATCH
- claim_boundary: TSMOM_CANDIDATE_FAMILY_DECISION_V1 is a paper-only research-governance loop that converts prior public-data paper research artifacts into a candidate-family queue decision. It is not strategy optimization, not readiness or live readiness, not trading advice, not a profit guarantee, not execution readiness, not permission for live orders or testnet orders, and it uses no live, testnet, account, API-key, private order, or fill data for performance claims.

## Evidence rollup
- median_gross_return_pct: -0.5318158824126007
- gross_edge_hit_rate: 0.3870967741935484
- median_net_stress_return_pct: -1.5014549259752459
- diagnostic_cash_null_failure_windows: 26
- gross_edge_cash_null_failure_windows: 19
- baseline_switch_rate: 0.9
- stable_switch_rate: 0.7333333333333333
- null_baselines_matching_or_beating_tsmom: ['buy_and_hold', 'cash_null', 'inverse_signal', 'lagged_signal', 'random_sign']
- regime_exception_candidates: market_regime=sideways, volatility_regime=low-vol

## Decision gates
- readiness_boundary_not_live_ready: PASSED (observed=NOT_READY, threshold=must remain NOT_READY and not emit live readiness)
- active_keep_watch_gross_edge: FAILED (observed={'median_gross_return_pct': -0.5318158824126007, 'gross_edge_hit_rate': 0.3870967741935484}, threshold=median gross > 0 and hit rate >= 0.50)
- active_keep_watch_null_robustness: FAILED (observed={'buy_and_hold': 16, 'cash_null': 19, 'inverse_signal': 19, 'lagged_signal': 18, 'random_sign': 18}, threshold=no null baseline beats or matches TSMOM in >= 16 of 31 windows)
- regime_only_watch_exception: PASSED (observed=['market_regime=sideways', 'volatility_regime=low-vol'], threshold=>=1 regime with >=5 windows, median gross > 0, hit rate >= 0.60)
- negative_control_utility: PASSED (observed=True, threshold=weak aggregate evidence plus durable artifacts for future pipeline validation)
- deprecate_candidate_family: FAILED (observed=False, threshold=broad WATCH fails, no regime exception, and no benchmark utility)
- failed_gates_preserved: PASSED (observed=['median_stress_return_beats_cash_null', 'selected_candidate_switch_rate', 'funding_aware_net_return'], threshold=source failed gates remain recorded and unrenamed)

## Candidate queue status
- TSMOM is removed from broad active development and may only continue in pre-registered regime-only WATCH tests.

## Safety
- live_orders_submitted: false
- testnet_orders_submitted: false
- api_keys_required_for_research: false
- account_or_fill_data_used: false
- holdout_used_for_selection: false
- strategy_parameters_retuned: false
- gates_relaxed: false
- live_trading_path_introduced: false

## Artifacts
- candidate_family_decision.json
- evidence_rollup.csv
- failed_gate_rollup.csv
- regime_exception_review.csv
- decision_rationale.md
- commands.txt
- changed_files.txt

## Next loop
Only if desired, pre-register a narrow regime-only falsification loop for the qualifying sideways/low-vol style regimes; otherwise freeze TSMOM as a negative-control benchmark.
