# Public Derivatives Microstructure Recent Window Connector V12

- Suite decision: `MICROSTRUCTURE_PROXY_READY_FOR_V13`
- Readiness baseline verdict: `NOT_READY`
- Source V11 artifact: `reports/loops/20260628-public-derivatives-microstructure-data-axis-v11`
- V11 state preserved: `True`
- Requested symbols: BTCUSDT, ETHUSDT, SOLUSDT, BNBUSDT, XRPUSDT, ADAUSDT, DOGEUSDT, AVAXUSDT, LINKUSDT
- Usable future-V13 symbols: ADAUSDT, AVAXUSDT, BNBUSDT, BTCUSDT, DOGEUSDT, ETHUSDT, LINKUSDT, SOLUSDT, XRPUSDT
- Data families with public coverage: kline_derived_microstructure_proxy
- Historical kline proxy gate: `True`
- Recent-window endpoint gate: `False`
- Coverage sufficient for future V13: `True`
- Recent-window only: `False`
- Temporal mismatch warning: none
- Endpoint failed downloads: `0`
- Raw data files committed: `False`
- Alpha generation performed: `False`

## Endpoint Retention Boundaries

- `kline_derived_microstructure_proxy`: public_binance_vision_monthly_archive_cache_only; historical span only when raw zip cache exists
- `open_interest_statistics_recent`: latest_one_month_official_endpoint_retention
- `global_long_short_ratio_recent`: latest_30_days_official_endpoint_retention
- `top_trader_long_short_position_ratio_recent`: latest_30_days_official_endpoint_retention
- `top_trader_long_short_account_ratio_recent`: latest_30_days_official_endpoint_retention
- `taker_buy_sell_volume_recent`: latest_30_days_official_endpoint_retention
- `basis_recent`: latest_30_days_official_endpoint_retention
- `mark_price_premium_index_recent`: current_snapshot_public_endpoint; no 2024_2025 historical REST backfill assumed

## Claim Boundary

PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_CONNECTOR_V12 is public derivatives microstructure connector and provenance infrastructure only. It preserves the V11/V10 state, separates long-span kline-derived public proxy coverage from recent-window REST endpoint coverage, and can only permit a future V13 paper research input gate. It is not alpha generation, not signal generation, not a trading strategy, not portfolio construction, not signal weight optimization, not profitability or edge evidence, not readiness or live readiness, and not permission for live/testnet/account/API-key/order/fill use.

## Safety

- No live orders submitted.
- No testnet orders submitted.
- No account, API-key, private order, or fill data used.
- Existing V8/V9/V10/V11 artifacts were read only and not rerun.
- Failed download rows: `0`.
