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
