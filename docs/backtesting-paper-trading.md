# Binance Vision Backtesting and Paper/Holdout Run

This milestone adds a no-credential `real-run` workflow for a meaningful historical BTCUSDT USDⓈ-M Futures backtest plus bar-based holdout/paper simulation.

## Command

```bash
uv run python -m apps.cli.quantctl real-run
```

Default canonical dataset:

- Symbol: `BTCUSDT`
- Interval: `1h`
- Months: `2024-01` through `2024-06`
- Source: Binance Vision monthly futures klines under `https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/`

## Engine and order-surface choices

- Backtesting/paper simulation engine: `backtesting.py` with `backtesting.lib.FractionalBacktest` for fractional BTC sizing.
- Exchange/order integration surface: `ccxt`, isolated in `packages/execution/ccxt_binance.py` and not imported by `real-run`.

License note: `ccxt` is MIT. `backtesting.py` is AGPL-3.0; this is acceptable for this personal local artifact, but must be reevaluated before distribution, commercial packaging, or SaaS/network-service release.

## Meaning gates

A success report is written only when all gates pass:

- Total bars >= 720
- Train/backtest bars >= 480
- Holdout/paper bars >= 168
- Backtest closed trades >= 3
- Holdout/paper closed trades >= 2
- UTC monotonic unique 1h candles with zero missing-candle tolerance

Failed runs write only `failure.md`, `failure.json`, and `manifest.json`; they do not write a success `report.md`.

## Current executed result

Successful run directory:

- `reports/real-run-20260624T165402Z/`

Summary:

- Rows: 4,368 from `2024-01-01T00:00:00Z` to `2024-06-30T23:00:00Z`
- Backtest closed trades: 89
- Holdout/paper closed trades: 33
- Backtest return: -28.498752255154887%
- Holdout/paper return: -13.111998074857983%

This is meaningful because it used public historical Binance data, passed the strict sample/trade gates, and recorded trades/equity/manifest hashes. It is not profitable in this canonical run, not a profit guarantee, not live fill proof, and not Binance exchange matching.

Initial failed diagnostic preserved:

- `reports/real-run-20260624T163947Z/`

That failed run is intentionally retained because the default integer-unit `Backtest` path could not trade BTCUSDT with 10,000 USDT cash and produced zero closed trades. The accepted fix was to use `FractionalBacktest` without changing the canonical strategy parameters or data window.

## Credential boundary

`real-run` does not read `BINANCE_API_KEY` or `BINANCE_API_SECRET`, does not import `ccxt`, and performs no Binance API/demo/testnet/order calls. It only downloads public Binance Vision zip files and writes local reports.

Creating Binance API keys, logging into Binance demo/testnet pages, or submitting testnet/live orders remains manual and credential-gated. Any future submit path must require explicit `allow_submit=True`, credentials, sandbox mode, the exact live-risk acknowledgment, product policy, kill-switch checks, and `LiveTradingGuard` approval.
