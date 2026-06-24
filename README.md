# Parangsae Unified Quant

A personal Binance-only quant trading app assembled from `parangsae`, `parangsae-engine`, and `parangsae-quant`.

The app is **live-ready but default-disabled**: it can build signals, run fixture backtests, run paper trading, validate dry-run Binance payloads, and replay event logs. It does **not** guarantee profit, and it will not submit real Binance orders unless every live safety gate passes.

## Quick smoke

```bash
python -m apps.cli.quantctl backtest --fixture --strategy trend-risk
python -m apps.cli.quantctl paper-run --fixture --ticks 10 --event-log logs/paper-events.jsonl
python -m apps.cli.quantctl replay --event-log logs/paper-events.jsonl
python -m apps.cli.quantctl live-dry-run --fixture
python -m apps.cli.quantctl kill-switch enable
python -m apps.cli.quantctl live-dry-run --fixture
python -m apps.cli.quantctl kill-switch disable
```

## Live safety contract

Real order submission requires all gates:

- CLI mode is `live`.
- `execution.live_enabled: true`.
- `PARANGSAE_LIVE_TRADING_ACK=I_UNDERSTAND_THIS_CAN_LOSE_MONEY`.
- `BINANCE_API_KEY` and `BINANCE_API_SECRET` are present in the runtime environment.
- Kill-switch is inactive.
- Product family is explicitly live-allowed.
- Risk approval is `APPROVED` for the exact order intent.

Milestone 1 implements adapters and guards, but verification uses dry-run/test endpoints or mocks only.

## Real Binance Vision run

```bash
uv run python -m apps.cli.quantctl real-run
uv run python -m apps.cli.quantctl risk-run
```

This downloads public Binance Vision BTCUSDT USDⓈ-M Futures 1h klines for 2024-01..2024-06, runs a `backtesting.py`/`FractionalBacktest` SMA/ATR strategy, runs the holdout as bar-based paper simulation, and writes a durable report under `reports/`. See `docs/backtesting-paper-trading.md`.

Latest verified report: `reports/real-run-20260624T165402Z/` (strict meaning gates passed; canonical holdout return was negative, so this is evidence, not a profit promise).

`risk-run` is the risk-adjusted research harness. It preserves the fixed `real-run` baseline, then uses a small predeclared product-family-aware grid, chronological train/validation/holdout selection, frozen `risk_adjusted_score_v1`, and report fields for Sharpe, MDD, candidate count, selected settings, and `holdout_used_for_selection:false`. The canonical BTCUSDT one-window result is still a research/comparison artifact, not proof of durable stable live returns.

Latest verified risk-run: `reports/risk-run-20260624T220620Z/`.

- Selected candidate: `c006-f24-s96-long-only`
- Holdout return: -1.232121126590955%
- Holdout Sharpe: -1.6176381689108235
- Holdout MDD: -2.4270383165800102%
- Baseline `real-run` holdout for the same window: return -13.111998074857983%, Sharpe -8.039068250214187, MDD -14.19746725941865%

This is a lower-drawdown/lower-loss research outcome for this window, not a profit guarantee.
