# Quant Loop Report: 20260625-data-depth-research-grade

## Hypothesis
Extending public closed-bar historical market data to research-grade bar/window depth improves validation authority before any TSMOM strategy selection.

## Baseline
- baseline: reports/readiness-run-20260625T053627Z/
- baseline verdict remains NOT_READY; this loop does not replace it.

## Change
- Added/ran a data-depth loop for public historical closed-bar OHLCV and optional public funding metadata.
- No strategy parameters were optimized and no holdout/testnet/live/account outcomes were used for selection.
- schema version: 1
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

## Commands
- `uv --version`
- `uv sync --dev`
- `uv run pytest tests/test_binance_vision_data.py tests/test_data_depth_loop.py tests/test_readiness_validation.py -q`
- `uv run pytest -q`
- `uv run ruff check .`
- `uvx pyright --pythonpath .venv/bin/python src/parangsae/apps/cli/quantctl.py src/parangsae/research/data_depth.py src/parangsae/marketdata/binance_vision.py tests/test_binance_vision_data.py tests/test_data_depth_loop.py`
- `uv run quantctl data-depth-run --output-dir reports/loops/20260625-data-depth-research-grade --overwrite`

## Evidence
- total input bars: 17544 / threshold 17520 — PASS
- walk-forward windows projected with frozen data-depth contract: 31 / threshold 12 — PASS
- data-only research-grade depth passed: True
- strategy-grid depth unclaimed: True

## Result interpretation
- What passed: public closed-bar OHLCV metadata and data-only bar/window depth gates: True.
- What failed/remains unclaimed: candidate_count, candidate_window_evaluations, bars_processed_estimate.
- Evidence depth used: public historical OHLCV plus optional public funding metadata; no live/testnet/account data.
- Cannot claim: profitability, stress-return pass, candidate stability, full research-grade strategy validation, execution mechanics, or live readiness.

## Decision
- decision: PROMOTE_TO_NEXT_LOOP

## Why
- claim boundary: DATA_DEPTH_RESEARCH_GRADE_V1 is a public-data provenance/depth loop, not a strategy-performance validation, not live-fill proof, not trading advice, not a profit guarantee, and not permission for live orders
- The loop improves data authority before alpha evaluation while preserving the committed NOT_READY baseline.

## Next loop
Narrow paper-only TSMOM_VOL_TARGET_FUNDING_AWARE_V1 strategy loop after data-depth evidence is durable.
