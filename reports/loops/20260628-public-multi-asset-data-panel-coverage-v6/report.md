# Quant Loop Report: 20260628-public-multi-asset-data-panel-coverage-v6

## Purpose
Audit public multi-asset OHLCV/funding coverage and provenance for a future paper-only V5 rerun.

## Suite decision
- suite_decision: PANEL_NOT_READY_INSUFFICIENT_SYMBOLS
- readiness_baseline_verdict: NOT_READY
- panel_ready_for_v5_rerun: False

## V5 state
- source_v5_artifact_path: reports/loops/20260627-multi-asset-alpha-generator-strict-prefilter-v5
- v5_state: present
- v5_excluded_symbols_preserved: True

## Symbol universe
- requested_symbol_count: 10
- usable_symbol_count: 1
- usable_symbol_universe: BTCUSDT
- common_start: 2024-01-01T00:00:00Z
- common_end: 2025-12-31T23:00:00Z
- common_bar_count: 17544

## Coverage
- BTCUSDT: included=True ohlcv=True funding=True reason=available
- ETHUSDT: included=False ohlcv=False funding=False reason=missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- SOLUSDT: included=False ohlcv=False funding=False reason=missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- BNBUSDT: included=False ohlcv=False funding=False reason=missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- XRPUSDT: included=False ohlcv=False funding=False reason=missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- ADAUSDT: included=False ohlcv=False funding=False reason=missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- DOGEUSDT: included=False ohlcv=False funding=False reason=missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- AVAXUSDT: included=False ohlcv=False funding=False reason=missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- LINKUSDT: included=False ohlcv=False funding=False reason=missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- TONUSDT: included=False ohlcv=False funding=False reason=missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06

## Missing data plan
- ETHUSDT ohlcv: missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- SOLUSDT ohlcv: missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- BNBUSDT ohlcv: missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- XRPUSDT ohlcv: missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- ADAUSDT ohlcv: missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- DOGEUSDT ohlcv: missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- AVAXUSDT ohlcv: missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- LINKUSDT ohlcv: missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06
- TONUSDT ohlcv: missing cached OHLCV month(s): 2024-01,2024-02,2024-03,2024-04,2024-05,2024-06

## Safety
- no live orders submitted: true
- no testnet orders submitted: true
- no account/API-key/private order/fill data used: true
- TSMOM state changed: false
- holdout retune used: false
- strategy optimization performed: false

## Claim boundary
PUBLIC_MULTI_ASSET_DATA_PANEL_COVERAGE_V6 is public-data coverage and provenance infrastructure only. It audits OHLCV/funding availability for a future paper-only V5 rerun. It is not alpha generation, not a trading strategy, not portfolio construction, not signal weight optimization, not profitability evidence, not readiness evidence, and not permission for live/testnet/account/API-key/order/fill use.
