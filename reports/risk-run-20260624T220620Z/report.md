# Binance Vision Risk-Adjusted Research Run Report

## Status
- status: success
- run type: risk-run
- no profit guarantee: true
- live orders submitted: false
- holdout used for selection: false
- claim boundary: canonical BTCUSDT 1h Jan-Jun 2024 risk-run is a reproducible research harness and comparison report, not proof of durable stable live returns

## Data
- symbol: BTCUSDT
- interval: 1h
- months: 2024-01, 2024-02, 2024-03, 2024-04, 2024-05, 2024-06
- product family: spot_crypto
- data venue: binance_usds_futures_public_klines
- rows: 4368
- first timestamp: 2024-01-01T00:00:00Z
- last timestamp: 2024-06-30T23:00:00Z

## Source URLs
- https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-01.zip
- https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-02.zip
- https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-03.zip
- https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-04.zip
- https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-05.zip
- https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/BTCUSDT-1h-2024-06.zip

## Selection
- selected candidate: c006-f24-s96-long-only
- candidate count: 6
- objective: risk_adjusted_score_v1
- formula: `validation_sharpe - abs(validation_max_drawdown_pct)/25 - abs(train_sharpe-validation_sharpe)*0.25 - max(0, validation_closed_trades-40)*0.01`
- selected settings: `{"allow_short": false, "atr_window": 14, "fast_sma": 24, "risk_fraction": 0.4, "slow_sma": 96, "stop_atr": 3.0, "take_profit_atr": 5.0}`

## Split / Meaning Gate
- chronological split verified: True
- train bars: 2184
- validation bars: 873
- holdout bars: 1311
- train closed trades: 11
- validation closed trades: 4
- holdout closed trades: 6

## Risk Metrics
| segment | return pct | Sharpe | max drawdown pct | closed trades |
| --- | ---: | ---: | ---: | ---: |
| train | 0.6925141433617501 | 0.43312980057845907 | -2.138404430795271 | 11 |
| validation | 2.7088472710232967 | 2.533008745734335 | -2.3066872532512606 | 4 |
| holdout/paper | -1.232121126590955 | -1.6176381689108235 | -2.4270383165800102 | 6 |

## Limitations
- This is a historical bar-based simulation, not Binance exchange matching.
- This is not live fill proof and not a profit guarantee.
- Sharpe annualization can be misleading under serial correlation or non-normal returns.
- The canonical one-window BTCUSDT result is not proof of durable stable live returns.
- Funding fees are excluded and recorded as a limitation.
- No Binance API key was used; no testnet/live order was submitted.
