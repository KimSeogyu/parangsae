# Quant Loop Report: 20260625-tsmom-vol-target-funding-aware-v1

## Hypothesis
A causal time-series momentum strategy with volatility-targeted sizing and public funding throttles can be falsified under research-grade walk-forward validation.

## Baseline
- baseline: reports/readiness-run-20260625T053627Z/
- readiness_baseline_verdict: NOT_READY
- baseline readiness remains NOT_READY unless a separate readiness report passes all gates.

## Loop verdict boundary
- tsmom_loop_verdict: WATCH
- tsmom_loop_verdict_namespace: tsmom_strategy_validation_only_not_live_readiness
- allowed loop verdicts: BLOCKED, WATCH, PASS_PAPER_ONLY
- This is a loop-only paper validation verdict, not a readiness verdict or live authorization.

## Data and selection boundary
- source: Binance Vision public USDⓈ-M Futures monthly klines
- symbol/timeframe: BTCUSDT / 1h
- UTC range: 2024-01-01T00:00:00Z to 2025-12-31T23:00:00Z
- row count: 17544
- months: 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12
- normalized OHLCV sha256: afd05e75d20344e0cfd96923490e90a9ac3569a88392954d08f7a09b718461de
- funding coverage: validated_public_data
- funding causality: Public funding events are aligned to OHLCV bars with ffill only after the event timestamp, then shifted one decision bar. Funding is never backfilled into earlier bars and never represents account-specific funding cashflows.
- selection inputs: train, validation
- selection excludes: holdout, live_orders, testnet_orders, api_keys, account_state, fills

## Pre-registered grid
- strategy family: TSMOM_VOL_TARGET_FUNDING_AWARE_V1
- candidate count: 48
- selection metric: risk_adjusted_score_v1
- holdout used for selection: false

## Hard gates
- candidate_count: PASSED (observed=48, threshold=25, status=passed)
- candidate_window_evaluations: PASSED (observed=1488, threshold=300, status=passed)
- bars_processed_estimate: PASSED (observed=2388240, threshold=500000, status=passed)
- median_stress_return_beats_cash_null: FAILED (observed=-1.5014549259752459, threshold=> 0.0, status=failed)
- selected_candidate_switch_rate: FAILED (observed=0.9, threshold=<= 0.75, status=failed)
- funding_aware_net_return: FAILED (observed=-1.5014549259752459, threshold=> 0.0 with validated public funding, status=failed)
- max_drawdown_abs_pct: PASSED (observed=5.481545539334309, threshold=<= 35.0, status=passed)
- tail_loss_cvar_pct: PASSED (observed=-4.775379842171519, threshold=>= -15.0, status=passed)

## Commands
- `uv --version`
- `uv sync --dev`
- `uv run pytest tests/test_tsmom_loop.py -q`
- `uv run pytest -q`
- `uv run ruff check .`
- `uv run --with pyright pyright src/parangsae/apps/cli/quantctl.py src/parangsae/research/loop_artifacts.py src/parangsae/research/data_depth.py src/parangsae/research/tsmom.py tests/test_tsmom_loop.py`
- `git diff --check`

## Evidence
- walk-forward windows: 31
- candidate-window evaluations: 1488
- bars processed estimate: 2388240
- selected candidate switch rate: 0.9
- tail-loss CVaR pct: -4.775379842171519

## Result interpretation
- What passed: candidate_count, candidate_window_evaluations, bars_processed_estimate, max_drawdown_abs_pct, tail_loss_cvar_pct
- What failed: median_stress_return_beats_cash_null, selected_candidate_switch_rate, funding_aware_net_return
- Evidence depth used: public closed-bar OHLCV and public funding metadata where available.
- Cannot claim: live readiness, profit, trading advice, exchange matching, or live/testnet performance.

## TSMOM loop decision
- tsmom_loop_decision: ITERATE_ON_SAME_HYPOTHESIS

## Why
- claim boundary: TSMOM_VOL_TARGET_FUNDING_AWARE_V1 is a pre-registered paper-only strategy validation loop, not live readiness, not trading advice, not a profit guarantee, and not permission for live orders
- No live/testnet/account/API-key/order/fill data supports these performance claims.

## Next loop
Fix failed hard gates without retuning holdout, or run stricter paper validation only if all gates pass.
