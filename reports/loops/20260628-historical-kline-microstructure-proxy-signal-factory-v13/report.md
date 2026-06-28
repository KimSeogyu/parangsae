# HISTORICAL_KLINE_MICROSTRUCTURE_PROXY_SIGNAL_FACTORY_V13

## Verdict
- suite_decision: NO_SURVIVING_SIGNAL_FAMILY
- readiness_baseline_verdict: NOT_READY
- claim_boundary: HISTORICAL_KLINE_MICROSTRUCTURE_PROXY_SIGNAL_FACTORY_V13 is a paper-only signal factory over historical kline-derived public microstructure proxies. Generated signals are hypotheses, not edges, not a trading strategy, not portfolio construction, not signal weight optimization, not profitability evidence, not readiness or live readiness, and not permission for live/testnet/account/API-key/order/fill use.

## Source V12
- source_v12_artifact_path: reports/loops/20260628-public-derivatives-microstructure-recent-window-v12
- source_v12_coverage_sufficient_for_future_v13: True
- source_v12_historical_microstructure_proxy_readiness: True
- source_v12_recent_window_microstructure_readiness: False
- recent_window_data_used: False

## Universe
- symbol_universe_used: ADAUSDT, AVAXUSDT, BNBUSDT, BTCUSDT, DOGEUSDT, ETHUSDT, LINKUSDT, SOLUSDT, XRPUSDT
- common_start: 2024-01-01T00:00:00Z
- common_end: 2025-12-31T00:00:00Z
- common_bar_count: 17521

## Signal Factory
- generated_signal_count: 5000
- evaluated_signal_count: 5000
- effective_signal_breadth: 1540
- duplicate_ratio: 0.692
- candidate_budget: 5000
- max_depth: 4

## Family Summary
- taker_imbalance_momentum: killed_oos_inconsistent (median_oos=-6.550009849809136e-05, null=null_fragile)
- taker_imbalance_reversal: killed_null_fragile (median_oos=0.003062461794600615, null=null_fragile)
- trade_count_pressure: killed_oos_inconsistent (median_oos=0.010757309445956207, null=null_fragile)
- quote_volume_pressure: killed_oos_inconsistent (median_oos=0.010622105079047614, null=null_fragile)
- avg_trade_size_proxy: killed_oos_inconsistent (median_oos=0.016206804494518008, null=null_fragile)
- microstructure_divergence: killed_oos_inconsistent (median_oos=0.010027598597255436, null=null_fragile)
- cross_sectional_microstructure_rank: killed_oos_inconsistent (median_oos=0.00039639393524538026, null=null_fragile)
- lead_lag_microstructure: killed_no_signals (median_oos=None, null=missing_null_comparison)
- microstructure_x_funding: killed_no_signals (median_oos=None, null=missing_null_comparison)
- microstructure_x_volatility_regime: killed_no_signals (median_oos=None, null=missing_null_comparison)

## Shortlist
- taker_imbalance_momentum: KILL_SIGNAL (family failed strict prefilter gates)
- taker_imbalance_reversal: KILL_SIGNAL (family failed strict prefilter gates)
- trade_count_pressure: KILL_SIGNAL (family failed strict prefilter gates)
- quote_volume_pressure: KILL_SIGNAL (family failed strict prefilter gates)
- avg_trade_size_proxy: KILL_SIGNAL (family failed strict prefilter gates)
- microstructure_divergence: KILL_SIGNAL (family failed strict prefilter gates)
- cross_sectional_microstructure_rank: KILL_SIGNAL (family failed strict prefilter gates)
- lead_lag_microstructure: KILL_SIGNAL (family failed strict prefilter gates)
- microstructure_x_funding: KILL_SIGNAL (family failed strict prefilter gates)
- microstructure_x_volatility_regime: KILL_SIGNAL (family failed strict prefilter gates)

## Multiple Testing

V13 generates a large historical kline-derived microstructure proxy signal hypothesis universe from existing public Binance Vision raw kline cache. Every metric is diagnostic and exposed to multiple-testing, data-snooping, duplicate-family, and survivorship risk. V13 may only create a paper-only strict falsification queue; it cannot claim alpha, edge, profitability, readiness, execution readiness, or permission to trade.

## Safety

- No live orders submitted.
- No testnet orders submitted.
- No account, API-key, private order, or fill data used.
- No data download or backfill performed.
- TSMOM state was not changed.

## What Cannot Be Claimed

- No alpha, edge, profitability, readiness, execution readiness, live/testnet permission, strategy, portfolio, or signal weight optimization claim.