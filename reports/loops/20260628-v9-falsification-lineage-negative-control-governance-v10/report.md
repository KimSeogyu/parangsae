# V9_FALSIFICATION_LINEAGE_AND_NEGATIVE_CONTROL_GOVERNANCE_V10

## Verdict
- suite_decision: V9_KILL_STATE_GOVERNANCE_LOCKED
- readiness_baseline_verdict: NOT_READY
- source_v9_artifact_path: reports/loops/20260628-multi-asset-strict-oos-fdr-falsification-v9
- source_v8_artifact_path: reports/loops/20260628-panel-ready-multi-asset-alpha-prefilter-rerun-v8

## Lineage
- source_v9_hash_manifest_digest: 8b3d98af84bd4c131bcfadb6e6b8d1502a23ef97cede54abcf9b6e8b3e7b2713
- source_v8_hash_manifest_digest: 27995865f3a099ac1aec9dfe4f77bf6486b9510b0eba99885a0d13133b0d1b96
- lineage_hash_count: 26

## Invariants
- invariant_check_count: 21
- invariant_pass_count: 21
- invariant_failure_count: 0

## Negative Controls
- correlation_breakdown: frozen_negative_control
- volatility_dispersion: frozen_negative_control

## Safety
- data_download_performed: False
- alpha_generation_performed: False
- tsmom_state_changed: False

## What cannot be claimed
- alpha or edge confirmed
- profitability
- readiness or live/testnet permission
- strategy, portfolio, or signal-weight optimization
