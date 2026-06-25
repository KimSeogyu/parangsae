# Destructive Readiness Validation Report

## Status
- status: success
- run type: readiness-run
- readiness verdict: NOT_READY
- failed gates: median_stress_return_beats_cash_null, max_selected_candidate_switch_rate, validation_depth_research_grade
- gates without failure flags: min_windows, min_holdout_trades_each, max_high_corr_family_pairs, min_regime_labels, stress_return_floor_pct
- live orders submitted: false
- API keys required: false
- no profit guarantee: true
- claim boundary: readiness-run is a destructive public-historical research validation package, not live-fill proof, not trading advice, not a profit guarantee, and not permission for live orders

## Data
- symbol: BTCUSDT
- interval: 1h
- months: 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06
- rows: 4368
- first timestamp: 2024-01-01T00:00:00Z
- last timestamp: 2024-06-30T23:00:00Z
- normalized OHLCV sha256: 1be88b91ac457fbd27a56916c9a0644cf212ec874ff47bfc960bb42a6dc23542

## Source URLs
- https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-01.zip
- https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-02.zip
- https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-03.zip
- https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-04.zip
- https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-05.zip
- https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-06.zip

## Walk-forward / OOS
- window count: 5
- median base holdout return pct: -0.2821323674864984
- median stress holdout return pct: -1.359232104497523
- all holdouts have trades: True
- selected candidate switch rate: 1.0
- overfit status: failed

## Validation depth / compute profile
- validation depth grade: prototype
- research-grade workload passed: False
- failed depth checks: total_input_bars, window_count, candidate_count, candidate_window_evaluations, bars_processed_estimate
- wall-clock policy: elapsed_seconds is diagnostic only and never upgraded to a readiness pass/fail criterion
- elapsed seconds: 45.967202
- elapsed seconds role: diagnostic only; a fast run can be valid for smoke checks but is not proof of research-grade validation
- total input bars: 4368
- input span days: 181.95833333333334
- window count: 5
- candidate count: 8
- candidate-window evaluations: 40
- total strategy evaluations: 90
- bars processed estimate: 68400
- bars processed formula: `sum(candidate_count * (train_bars + validation_bars) + 2 * holdout_bars for each generated window)`

## Baselines / Costs / Funding
- cash null return pct: 0.0
- buy-hold source: backtesting_metric
- stress commission: 0.001
- stress spread: 0.001
- stress slippage proxy bps: 5.0
- slippage formula: closed_trades * 2 * dynamic_slippage_proxy_bps / 100 percentage points
- execution proxy source: ohlcv_range_trade_count_proxy
- execution proxy live-fill proof: False
- stress funding rate per 8h: 0.0001
- funding formula: exposure_time_pct / 100 * bars / 8 * funding_rate_per_8h * 100
- funding source: public_binance_usdm_funding_history
- funding rows: 546
- funding rate selection: per-window max(default stress rate, observed public funding quantile 0.75) when public funding is supplied

## Data-source coverage
- OHLCV: validated_public_data
- derivatives funding: validated_public_data
- orderbook/orderflow: blocked_requires_provider
- on-chain: blocked_requires_provider
- macro/events: blocked_requires_provider
- sentiment/news: blocked_requires_provider

## Regime split
- observed regime labels: 4
- min regime labels gate: 2

## Signal ablation / duplicate-risk checks
- ablation method: family-score Pearson IC against 24-bar forward returns, then drop-one-family recomputation
- high correlation family pairs: 3
- high correlation threshold: 0.8
- max raw signals per family/timestamp: 6

## Stress / lookahead
- stress return floor pct: -15.0
- all windows survived stress floor: True
- lookahead probe status: passed
- lookahead probe passed: True

## What failed / what is still needed before real trading
- failed gate: median_stress_return_beats_cash_null
- failed gate: max_selected_candidate_switch_rate
- failed gate: validation_depth_research_grade

## Live blockers
- No live/testnet order path was validated or authorized.
- No exchange matching or live-fill proof exists; OHLCV execution proxy is not fill proof.
- Funding history is downloaded from public Binance USDⓈ-M data, but account-specific funding cashflows are still not validated.
- Catalog-only orderbook/on-chain/macro/sentiment signals remain inactive without configured data-source validation.
- Passing this report can at most justify further paper-only validation, never live trading.

## Limitations
- This report tries to falsify a historical research workflow; it is not exchange matching.
- Passing readiness-run can at most justify additional paper-only validation.
- It is not trading advice, not live-fill proof, and not a profit guarantee.
- No Binance API key was used; no testnet/live order was submitted.
