# Design notes

Extracts from the removed superpowers plans. Command overviews for these loops also live in `README.md` and `docs/backtesting-paper-trading.md`.

## V10 lineage governance

Source: `docs/superpowers/plans/2026-06-28-v9-falsification-lineage-governance-v10.md`

`multi-asset-v9-falsification-lineage-governance-run` is a paper-only, cache-free governance loop on the V9 strict falsification module. V8 and V9 artifacts are immutable inputs. V10 writes hashes and a post-mortem and does not edit those reports. It does not generate alpha, rerun V8/V9, download or backfill data, implement a strategy or portfolio, or change trading or readiness state. The command does not read credentials.

Flags named in the plan: `--source-v9-artifact`, `--source-v8-artifact`, `--reports-root`, `--output-dir`, `--overwrite`, `--command`, `--changed-file`.

Lineage manifest columns: `source_role`, `relative_path`, `sha256`, `byte_count`, `exists`.

The invariant check covers required V9 files, safety fields, an empty next queue when every family is killed, absence of forbidden labels, expected CSV schemas, and presence of the V8 source path. A mutated V9 next queue fails closed.

The locked suite decision named in the plan is `V9_KILL_STATE_GOVERNANCE_LOCKED`.

## V11 public derivatives microstructure data axis

Source: `docs/superpowers/plans/2026-06-28-public-derivatives-microstructure-v11.md`

V11 is a coverage and provenance loop. It does not generate alpha, rerun V8/V9/V10, implement a strategy, optimize weights, change TSMOM behavior, use private, account, API-key, order, or fill data, or create live or testnet paths. It does not claim edge, profitability, or readiness.

The frozen input is `reports/loops/20260628-v9-falsification-lineage-negative-control-governance-v10/`. V11 carries forward the V10 payload hash, V8/V9 lineage digests, the V9 suite decision, the empty V9 next queue, negative-control families, disallowed revival entries, and lineage hashes. The plan's acceptance rule is that V11 only reads those frozen artifact files and that source artifact modification times stay unchanged.

`TONUSDT` is excluded from the default universe, and the exclusion is recorded. Missing or unavailable public data is recorded and not fabricated. Checksums cover existing cache files only. Downloads stay plan-only; V11 does not require a network pull. Raw data files are not committed.

Command: `public-derivatives-microstructure-data-axis-run`.

Artifact discovery uses `public_derivatives_microstructure_axis.json` and a `loop_namespace` containing `public_derivatives_microstructure_data_axis_v11`. `loop.json` is an extra pointer. The planned artifact directory is `reports/loops/20260628-public-derivatives-microstructure-data-axis-v11/`.

`readiness_by_family.csv` keeps that filename, but the columns are a data-coverage gate for a later V12 input, not a strategy or live-readiness claim:

`data_family`, `coverage_gate`, `coverage_status`, `observed_cache_files`, `missing_items`, `available_symbol_count`, `included_symbol_count_for_future_v12`, `common_start_by_family`, `common_end_by_family`, `coverage_sufficient_for_future_v12`, `not_strategy_readiness`, `claim_boundary`.

The plan says V12 should run only when `coverage_sufficient_for_future_v12=true`.

Top-level JSON fields required by the plan:

- Lineage: `source_v10_artifact_path`, `source_v10_payload_hash`, `v10_kill_state_preserved`, `source_v9_suite_decision`, `source_v9_next_queue_empty`, `v9_next_queue_empty`, `negative_control_families`, `disallowed_revival_entries`, `source_v9_hash_manifest_digest`, `source_v8_hash_manifest_digest`.
- Coverage: `requested_symbol_universe`, `usable_symbol_universe`, `excluded_symbols`, `data_families_requested`, `data_families_available`, `coverage_sufficient_for_future_v12`, download and cache counters, raw data file lists, and commit flags.
- Safety: `readiness_baseline_verdict` (`NOT_READY`), `private_data_used`, `account_data_used`, `api_key_used`, `live_or_testnet_path_created`, `alpha_generation_performed`, `strategy_optimization_performed`, `holdout_retune_used`, `tsmom_state_changed`, the required `no_*` booleans, `claim_boundary`, `safety_gates`, `validation_commands`, `changed_files`.

Before writing, the safety contract requires `readiness_baseline_verdict=NOT_READY`, `alpha_generation_performed=false`, `strategy_optimization_performed=false`, `holdout_retune_used=false`, `tsmom_state_changed=false`, and `raw_data_files_committed=false`, with private, account, API-key, order, fill, live, and testnet flags false or the matching `no_*` flags true. Output scans allow `readiness_baseline_verdict=NOT_READY` and the filename `readiness_by_family.csv`; other positive trading or readiness labels are forbidden.
