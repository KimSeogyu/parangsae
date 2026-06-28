# V9 Falsification Post-Mortem

## Decision
- suite_decision: V9_KILL_STATE_GOVERNANCE_LOCKED
- source_v9_suite_decision: NO_FAMILY_SURVIVES_STRICT_FALSIFICATION
- readiness_baseline_verdict: NOT_READY

## Negative Controls
- correlation_breakdown: FALSIFIED_SIGN_FLIP (revival requires a new pre-registered mechanism)
- volatility_dispersion: FALSIFIED_SIGN_FLIP (revival requires a new pre-registered mechanism)

## Invariant Failures
- none

## Boundary
- V10 is lineage and negative-control governance over the already-falsified V9 queue. It does not generate alpha, rerun V8/V9 selection, download data, implement a strategy, optimize weights, claim edge/profit/readiness, or permit live/testnet/account/API-key/order/fill use.
- Future research must start from a new pre-registered mechanism, not from V9 family revival.
