# Operator Runbook

## Normal flow
1. Run fixture tests and paper loop.
2. Inspect report and event log.
3. Keep kill-switch available.
4. Do not enable live mode until a separate paper-trading milestone has sufficient evidence.

## Kill switch
```bash
python -m apps.cli.quantctl kill-switch enable
python -m apps.cli.quantctl kill-switch disable
```

## Live mode reminder
Milestone 1 should not submit real orders. Live mode requires explicit flags, exact acknowledgment, credentials, inactive kill-switch, live-allowed product family, and risk approval.

## Real-run backtest/paper report

Run:

```bash
uv run python -m apps.cli.quantctl real-run
```

Inspect `reports/<run-id>/report.md`, `metrics.json`, `trades.csv`, `equity.csv`, and `manifest.json`. A failed meaningfulness gate creates `failure.md`/`failure.json` instead of a success report. Do not treat either backtest or paper/holdout output as a profit guarantee.
