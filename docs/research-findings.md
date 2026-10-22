# Research findings

Recorded results kept from the removed superpowers plans. This is not a new validation run.

## No V9 family survived strict falsification

Source: `docs/superpowers/plans/2026-06-28-v9-falsification-lineage-governance-v10.md`

The V10 plan treats the committed V9 artifact as kill-state evidence:

- The suite decision to preserve is `NO_FAMILY_SURVIVES_STRICT_FALSIFICATION`.
- When every queued family is killed, the next mechanism queue is empty.
- A mutated next queue fails closed.
- Killed families are recorded as negative controls, with disallowed revival entries.
- V10 locks that evidence as `suite_decision=V9_KILL_STATE_GOVERNANCE_LOCKED`.

The plan does not treat that lock as alpha, profitability, edge, readiness, or live or testnet permission.
