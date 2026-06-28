# PUBLIC_DERIVATIVES_MICROSTRUCTURE_DATA_AXIS_V11 Plan

## Boundary

V11 is a data coverage and provenance loop only. It must not generate alpha,
rerun V8/V9/V10, implement a strategy, optimize weights, touch TSMOM behavior,
use private/account/API-key/order/fill data, create live/testnet paths, or claim
edge/profitability/readiness.

## Inputs

- `AGENTS.md` governs the loop.
- Frozen V10 artifact:
  `reports/loops/20260628-v9-falsification-lineage-negative-control-governance-v10/`
- Preserve V10 kill-state evidence:
  - source V9 decision
  - source V9 next queue empty
  - negative-control families
  - disallowed revival entries
  - lineage hashes

## Implementation

1. Add `src/parangsae/research/public_derivatives_microstructure.py`.
   - Define default universe with `TONUSDT` excluded and recorded.
   - Define public derivatives data families.
   - Read V10 JSON/CSV lineage lock artifacts.
   - Carry forward source V10 payload hash, V8/V9 lineage digests,
     negative-control families, disallowed revival entries, and V9 empty queue
     state into machine-readable V11 outputs.
   - Audit deterministic public cache paths for each symbol x family.
   - Record missing/unavailable public data without fabricating observations.
   - Compute checksums/provenance for existing cache files only.
   - Keep downloads plan-only by default; no network data pull is needed for V11.
   - Emit required JSON/CSV/Markdown artifacts.
   - Assert the payload safety contract before writing:
     `readiness_baseline_verdict=NOT_READY`, all private/account/API-key/order/
     fill/live/testnet flags false or no-* true as appropriate,
     `alpha_generation_performed=false`,
     `strategy_optimization_performed=false`,
     `holdout_retune_used=false`, `tsmom_state_changed=false`,
     `raw_data_files_committed=false`, and claim boundary text that forbids
     alpha, edge, profitability, readiness, live readiness, and trading use.

2. Add minimal CLI wiring in `src/parangsae/apps/cli/quantctl.py`.
   - Command: `public-derivatives-microstructure-data-axis-run`
   - No credential arguments, no live/testnet path, no alpha generation.

3. Add loop artifact discovery support in
   `src/parangsae/research/loop_artifacts.py`.
   - Detect V11 by `public_derivatives_microstructure_axis.json` plus
     `loop_namespace` containing `public_derivatives_microstructure_data_axis_v11`.
   - Write `loop.json` as an extra standard loop pointer while preserving every
     user-required V11 artifact file.

4. Add tests first in `tests/test_public_derivatives_microstructure.py`.
   - Cover V10 state carry-forward, TON exclusion, schema stability,
     unavailable data recording, no fabrication, idempotent existing-cache skip,
     readiness gate, safety flags, artifact writing, CLI importability, and
     prior V7/V8/V9/V10 command preservation.

5. Update `README.md` and `docs/backtesting-paper-trading.md`.
   - Explicitly state V11 is a public microstructure data-axis loop only.
   - State V12 should run only when `coverage_sufficient_for_future_v12=true`.

## Expected artifact

`reports/loops/20260628-public-derivatives-microstructure-data-axis-v11/`

Required files:

- `public_derivatives_microstructure_axis.json`
- `data_family_coverage.csv`
- `symbol_data_coverage.csv`
- `missing_data_plan.csv`
- `download_manifest.csv`
- `provenance_manifest.csv`
- `readiness_by_family.csv`
- `v10_lineage_lock_reference.csv`
- `report.md`
- `commands.txt`
- `changed_files.txt`

`readiness_by_family.csv` is a required filename from the user request, but its
schema must describe only data-axis coverage gates for future V12 input use. It
must not claim strategy readiness, alpha, edge, profitability, execution
readiness, live readiness, or permission to trade.

The top-level JSON must include at least:

- V10 lineage fields: `source_v10_artifact_path`,
  `source_v10_payload_hash`, `v10_kill_state_preserved`,
  `source_v9_suite_decision`, `source_v9_next_queue_empty`,
  `v9_next_queue_empty`, `negative_control_families`,
  `disallowed_revival_entries`, `source_v9_hash_manifest_digest`,
  `source_v8_hash_manifest_digest`
- Coverage fields: `requested_symbol_universe`, `usable_symbol_universe`,
  `excluded_symbols`, `data_families_requested`, `data_families_available`,
  `coverage_sufficient_for_future_v12`, download/cache counters, raw data
  file lists and commit flags
- Safety fields: `readiness_baseline_verdict`, `private_data_used`,
  `account_data_used`, `api_key_used`, `live_or_testnet_path_created`,
  `alpha_generation_performed`, `strategy_optimization_performed`,
  `holdout_retune_used`, `tsmom_state_changed`, all required `no_*` booleans,
  `claim_boundary`, `safety_gates`, `validation_commands`, and `changed_files`

## Validation

Run the required suite:

- `uv run pytest tests/test_public_derivatives_microstructure.py -q`
- `uv run pytest tests/test_public_multi_asset_panel_coverage.py -q`
- `uv run pytest tests/test_alpha_signal_factory.py -q`
- `uv run pytest tests/test_tsmom_loop.py -q`
- `uv run pytest -q`
- `uv run ruff check .`
- `uv run --with pyright pyright <changed files>`
- `git diff --check`
- `git status --short data`
- `git ls-files data`

Audit-only:

- `uv run --with pyright pyright src tests`

## Stop condition

The loop is complete when V11 artifacts exist, required validations pass or
known unrelated audit debt is isolated, no raw data is staged, the final
payload preserves all required safety and claim boundaries, and the branch is
committed/pushed if the required gate passes.

## Additional Acceptance Gates

- V11 tests must monkeypatch prior V8/V9/V10 evaluator paths to raise if called
  and prove V11 only reads frozen artifact files.
- V11 tests must verify source artifact mtimes remain unchanged.
- V11 outputs must be scanned for forbidden positive trading/readiness labels,
  allowing only `readiness_baseline_verdict=NOT_READY` and the required
  filename `readiness_by_family.csv`.
- `readiness_by_family.csv` keeps the user-required filename but uses an
  explicit coverage-gate schema: `data_family`, `coverage_gate`,
  `coverage_status`, `observed_cache_files`, `missing_items`,
  `available_symbol_count`, `included_symbol_count_for_future_v12`,
  `common_start_by_family`, `common_end_by_family`,
  `coverage_sufficient_for_future_v12`, `not_strategy_readiness`, and
  `claim_boundary`.
