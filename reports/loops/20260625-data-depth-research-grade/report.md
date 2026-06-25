# Quant Loop Report: 20260625-data-depth-research-grade

## Hypothesis
Extending public closed-bar historical market data to research-grade bar/window depth improves validation authority before any TSMOM strategy selection.

## Baseline
- baseline: reports/readiness-run-20260625T053627Z/
- readiness baseline verdict: NOT_READY
- baseline verdict remains NOT_READY; this loop does not replace it.

## Loop verdict boundary
- data_depth_loop_verdict: WATCH
- WATCH is data-depth-only when present; it is not a readiness verdict, not a performance verdict, and not live authorization.
- data-depth-run does not change readiness verdict semantics, strategy-performance claims, or any live/trading authorization surface.

## Change
- Added/ran a data-depth loop for public historical closed-bar OHLCV and optional public funding metadata.
- No strategy parameters were optimized and no holdout/testnet/live/account outcomes were used for selection.
- schema version: 2
- data-depth contract is frozen in `loop.json.data_depth_contract` and does not import readiness-run defaults.

## Data and selection boundary
- source: Binance Vision public USDⓈ-M Futures monthly klines
- symbol/timeframe: BTCUSDT / 1h
- UTC range: 2024-01-01T00:00:00Z to 2025-12-31T23:00:00Z
- row count: 17544
- months: 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06, 2024-07, 2024-08, 2024-09, 2024-10, 2024-11, 2024-12, 2025-01, 2025-02, 2025-03, 2025-04, 2025-05, 2025-06, 2025-07, 2025-08, 2025-09, 2025-10, 2025-11, 2025-12
- closed bar only: True
- open bar included: False
- close-time policy: binance_monthly_close_time_equals_open_time_plus_1h_minus_1ms
- volume policy: source_allows_zero_volume_bars_recorded_as_quality_flag
- zero-volume bars: 1
- normalized OHLCV sha256: afd05e75d20344e0cfd96923490e90a9ac3569a88392954d08f7a09b718461de
- gap policy: `{"expected_gap_seconds": 3600, "gap_tolerance_seconds": 0, "missing_bars_allowed": false}`
- duplicate policy: `{"duplicates": 0, "duplicates_allowed": false}`
- funding source: public_binance_usdm_funding_history
- funding rows: 2193
- funding alignment policy: Public funding events are aligned causally to the first OHLCV bar whose timestamp is greater than or equal to the funding timestamp; funding is never forward-filled into earlier decision bars. This validates public metadata alignment only, not account-specific funding cashflows.
- selection excludes: holdout, live_orders, api_keys, account_state, testnet_outcomes, forward_paper_current_window

## Dataset provenance
- ohlcv: source=Binance Vision public USDⓈ-M Futures monthly klines; rows=17544; hash=afd05e75d20344e0cfd96923490e90a9ac3569a88392954d08f7a09b718461de; no-key-public=True; account/signed=False
- funding: source=public_binance_usdm_funding_history; rows=2193; hash=bbe246bc37c90cc1ed987047eecc8d16a9400a6a3bbbd91adcd94954da51cc70; no-key-public=True; account/signed=False

## Coverage analysis
- projected walk-forward windows: 31
- volatility regimes: `{"high_count": 5782, "high_threshold": 0.005117749561302827, "insufficient_history_count": 24, "low_count": 5782, "low_threshold": 0.0034137117456426165, "method": "24_bar_rolling_std_of_hourly_close_returns", "normal_count": 5956, "total_bars": 17544}`
- stress periods: `{"context_only_not_strategy_pnl": true, "high_vol_bar_count": 5782, "max_underlying_drawdown_pct": -34.763624529709645, "return_series": "hourly_close_pct_change", "stress_bar_count": 6091, "stress_rule": "high_vol_regime_or_hourly_return_at_or_below_5th_percentile", "tail_loss_cvar_5_pct": -1.2713440581365616, "worst_24h_return_pct": -18.41984532704155}`
- funding regimes: `{"abs_rate_75pct_threshold": 0.0001, "account_cashflows_validated": false, "coverage_status": "validated_public_data", "first_timestamp": "2024-01-01T00:00:00Z", "high_abs_funding_count": 867, "last_timestamp": "2025-12-31T16:00:00.001000Z", "negative_count": 233, "positive_count": 1960, "rows": 2193, "zero_count": 0}`

## Gates
- minimum sample depth: PASS
- walk-forward coverage: PASS (data-only projected windows, not candidate stability)
- strategy_performance_gates_unclaimed: median_stress_return_beats_cash_null, selected_candidate_switch_rate, funding_aware_net_return, max_drawdown_abs_pct, tail_loss_cvar_pct

## Commands
- `uv --version`
- `uv sync --dev`
- `uv run pytest tests/test_data_depth_loop.py -q`
- `uv run pytest tests/test_binance_vision_data.py tests/test_data_depth_loop.py tests/test_readiness_validation.py -q`
- `uv run pytest -q`
- `uv run ruff check .`
- `uvx pyright --pythonpath .venv/bin/python src/parangsae/research/data_depth.py src/parangsae/apps/cli/quantctl.py tests/test_data_depth_loop.py`
- `uv run quantctl data-depth-run --output-dir reports/loops/20260625-data-depth-research-grade --overwrite`

## Evidence
- total input bars: 17544 / threshold 17520 — PASS
- walk-forward windows projected with frozen data-depth contract: 31 / threshold 12 — PASS
- data-depth loop verdict: WATCH (data-depth-only)
- data-only research-grade depth passed: True
- strategy-grid depth unclaimed: True

## Result interpretation
- What passed: public closed-bar OHLCV metadata and data-only bar/window depth gates: True.
- What failed/remains unclaimed: candidate_count, candidate_window_evaluations, bars_processed_estimate, median_stress_return_beats_cash_null, selected_candidate_switch_rate, funding_aware_net_return, max_drawdown_abs_pct, tail_loss_cvar_pct.
- Evidence depth used: public historical OHLCV plus optional public funding metadata; no live/testnet/account data.
- Cannot claim: profitability, stress-return pass, candidate stability, full research-grade strategy validation, execution mechanics, or live readiness.

## Decision
- decision: PROMOTE_TO_NEXT_LOOP

## Why
- claim boundary: DATA_DEPTH_RESEARCH_GRADE_V1 is a public-data provenance/depth loop, not a strategy-performance validation, not live-fill proof, not trading advice, not a profit guarantee, and not permission for live orders
- The loop improves data authority before alpha evaluation while preserving the committed NOT_READY baseline.

## Next loop
Narrow paper-only TSMOM_VOL_TARGET_FUNDING_AWARE_V1 strategy loop after data-depth evidence is durable.
