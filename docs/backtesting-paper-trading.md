# Binance Vision Backtesting and Paper/Holdout Run

This milestone adds a no-credential `real-run` workflow for a meaningful historical BTCUSDT USDⓈ-M Futures backtest plus bar-based holdout/paper simulation.

## Command

```bash
uv run python -m apps.cli.quantctl real-run
uv run python -m apps.cli.quantctl risk-run
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

## Risk-adjusted research run

`risk-run` is separate from `real-run` so the fixed baseline stays comparable. It uses the same canonical public Binance Vision data and writes `reports/risk-run-*` with:

- chronological train / validation / holdout split metadata,
- small predeclared candidate grid,
- product-family-aware direction rules (`spot_crypto` candidates are long-only),
- frozen `risk_adjusted_score_v1`,
- `candidate_count`, all candidate train/validation rows, selected settings, and `holdout_used_for_selection:false`,
- train, validation, and holdout/paper Sharpe and max drawdown (MDD),
- explicit one-window claim boundary and no-profit/no-live-fill disclaimers.

Frozen score formula:

```text
validation_sharpe
- abs(validation_max_drawdown_pct) / 25
- abs(train_sharpe - validation_sharpe) * 0.25
- max(0, validation_closed_trades - 40) * 0.01
```

The holdout segment is evaluated only after candidate selection. A good holdout result must not be used to retune the same run. A single BTCUSDT 1h Jan-Jun 2024 result is a reproducible research harness and comparison report, not proof of durable stable live returns. Cross-regime claims require a later multi-window/walk-forward phase.

Sharpe ratio is reported because it is useful, but it is statistically noisy; annualized Sharpe can be misleading when returns are serially correlated or non-normal. MDD is reported as a risk constraint and penalty, not as a guarantee that future losses are capped.

## Current executed result

Successful run directory:

- `reports/real-run-20260624T165402Z/`
- `reports/real-run-20260624T220621Z/` (regression re-run after adding risk-run)
- `reports/risk-run-20260624T220620Z/`

Summary:

- Rows: 4,368 from `2024-01-01T00:00:00Z` to `2024-06-30T23:00:00Z`
- Backtest closed trades: 89
- Holdout/paper closed trades: 33
- Backtest return: -28.498752255154887%
- Holdout/paper return: -13.111998074857983%

This is meaningful because it used public historical Binance data, passed the strict sample/trade gates, and recorded trades/equity/manifest hashes. It is not profitable in this canonical run, not a profit guarantee, not live fill proof, and not Binance exchange matching.

Latest risk-run comparison for the same canonical window:

- Selected candidate: `c006-f24-s96-long-only`
- Candidate count: 6
- Holdout return: -1.232121126590955%
- Holdout Sharpe: -1.6176381689108235
- Holdout MDD: -2.4270383165800102%
- Baseline holdout return: -13.111998074857983%
- Baseline holdout Sharpe: -8.039068250214187
- Baseline holdout MDD: -14.19746725941865%

This is a lower-risk/lower-loss result for this one canonical holdout, not proof of durable stable future returns.

Initial failed diagnostic preserved:

- `reports/real-run-20260624T163947Z/`

That failed run is intentionally retained because the default integer-unit `Backtest` path could not trade BTCUSDT with 10,000 USDT cash and produced zero closed trades. The accepted fix was to use `FractionalBacktest` without changing the canonical strategy parameters or data window.

## Credential boundary

`real-run` does not read `BINANCE_API_KEY` or `BINANCE_API_SECRET`, does not import `ccxt`, and performs no Binance API/demo/testnet/order calls. It only downloads public Binance Vision zip files and writes local reports.

Creating Binance API keys, logging into Binance demo/testnet pages, or submitting testnet/live orders remains manual and credential-gated. Any future submit path must require explicit `allow_submit=True`, credentials, sandbox mode, the exact live-risk acknowledgment, product policy, kill-switch checks, and `LiveTradingGuard` approval.
