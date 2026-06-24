# Binance Vision Real-Run Report

## Status
- status: success
- no profit guarantee: true
- paper semantics: bar-based holdout simulation; not exchange matching, not live fill proof
- funding fees: excluded and recorded as limitation

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

## Canonical Parameters
- strategy: `{"atr_window": 14, "fast_sma": 12, "risk_fraction": 0.95, "slow_sma": 48, "stop_atr": 2.5, "take_profit_atr": 4.0}`
- broker/costs: `{"cash": 10000.0, "commission": 0.0004, "exclusive_orders": true, "finalize_trades": true, "fractional_unit": 1e-08, "hedging": false, "margin": 1.0, "spread": 0.0002, "trade_on_close": false}`
- funding: excluded from canonical bar-based simulation

## Meaning Gate
- status: passed
- total bars: 4368
- train bars: 3057
- holdout bars: 1311
- backtest closed trades: 89
- holdout closed trades: 33

## Backtest Metrics
- closed trades: 89
- return pct: -28.498752255154887
- equity final usd: 7150.124774484511
- max drawdown pct: -33.32697929443461

## Holdout / Paper Metrics
- closed trades: 33
- return pct: -13.111998074857983
- equity final usd: 8688.800192514202
- max drawdown pct: -14.19746725941865

## Limitations
- This is a historical bar-based simulation, not Binance exchange matching.
- This is not live fill proof and not a profit guarantee.
- No Binance API key was used; no testnet/live order was submitted.
