# Parangsae

A personal Binance-only quant trading app assembled from `parangsae`, `parangsae-engine`, and `parangsae-quant`.

The app is **paper/dry-run first and live-gated**: it can build signals, run fixture backtests, run paper trading, validate dry-run Binance payloads, and replay event logs. The latest readiness artifact is `NOT_READY`, so no live readiness is established. It does **not** guarantee profit, and it will not submit real Binance orders unless every live safety gate passes.

## Quick smoke

```bash
uv run quantctl backtest --fixture --strategy trend-risk
uv run quantctl paper-run --fixture --ticks 10 --event-log logs/paper-events.jsonl
uv run quantctl replay --event-log logs/paper-events.jsonl
uv run quantctl live-dry-run --fixture
uv run quantctl kill-switch enable
uv run quantctl live-dry-run --fixture
uv run quantctl kill-switch disable
uv run quantctl forward-paper-run --lookback-bars 240 --funding-source none
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
uv run quantctl real-run
uv run quantctl risk-run
uv run quantctl readiness-run
uv run quantctl data-depth-run
uv run quantctl tsmom-run
uv run quantctl forward-paper-run
```

`real-run`, `risk-run`, and `readiness-run` download public Binance Vision BTCUSDT USDⓈ-M Futures 1h klines for 2024-01..2024-06, run the current research harnesses, and write durable reports under `reports/`. `data-depth-run` and `tsmom-run` are separate wider public-data loop workflows described below. See `docs/backtesting-paper-trading.md`.

Latest verified report: `reports/real-run-20260624T165402Z/` (strict meaning gates passed; canonical holdout return was negative, so this is evidence, not a profit promise).

`data-depth-run` is a pre-strategy data authority loop for the
`DATA_DEPTH_RESEARCH_GRADE_V1` milestone. It deliberately does not optimize
strategy parameters, select a TSMOM candidate, retune on holdout, or improve
profitability metrics. By default it fetches public/no-key Binance Vision
BTCUSDT USDⓈ-M Futures `1h` closed-bar monthly klines for 2024-01..2025-12 and
public/no-key Binance USDⓈ-M funding history over the same UTC range, then writes
the required loop evidence under `reports/loops/<YYYYMMDD>-data-depth-research-grade/`:

- `loop.json`
- `report.md`
- `commands.txt`
- `changed_files.txt`

The loop records source URLs, symbol, product family, timeframe, UTC range, row
count, normalized OHLCV/funding hashes, closed-bar status, close-time policy,
gap/duplicate policy, funding coverage, causal funding alignment, and safety
gates showing no live/testnet orders, no API keys, and no account/signed
endpoints. `loop.json.schema_version == 2` also records explicit
`dataset_provenance`, `report_artifact_provenance`, and data-only
`coverage_analysis` for projected walk-forward windows, volatility regimes,
public funding regimes, and stress/tail market context. The data-depth contract
is frozen in `loop.json.data_depth_contract` so later readiness tuning does not
silently redefine this milestone. Binance Vision archive rows may include
zero-volume closed bars; those are source-scoped quality flags, not silently
dropped data.

Passing the bar/window data-depth gates is not a readiness or profit claim.
`data_depth_loop_verdict: WATCH`, when present, is data-depth-only and is not a
readiness verdict. The committed readiness baseline remains recorded separately
as `readiness_baseline_verdict: NOT_READY`. Candidate-count, candidate-window,
median stress return vs cash/null, candidate-switch-rate, funding-aware net
return, drawdown, and tail-loss gates remain under unclaimed strategy/performance
gates until a later pre-registered paper-only strategy/validation loop.

`tsmom-run` implements the pre-registered paper-only
`TSMOM_VOL_TARGET_FUNDING_AWARE_V1` strategy/validation loop. It is isolated from
the canonical `readiness-run` baseline and writes loop artifacts under
`reports/loops/<YYYYMMDD>-tsmom-vol-target-funding-aware-v1/`:

- `loop.json`
- `report.md`
- `commands.txt`
- `changed_files.txt`
- `tsmom_windows.csv`
- `tsmom_candidates.csv`

The frozen TSMOM grid has 48 causal long-only candidates across lookback,
realized-volatility window, target annual volatility, max exposure, and public
funding-rate throttle settings. Selection uses train/validation only with the
frozen `risk_adjusted_score_v1`; holdout is evaluated once per selected
candidate/window. Hard gates include candidate count, candidate-window
evaluations, bars processed estimate, median stress return vs cash/null,
selected-candidate switch rate, funding-aware net return, max drawdown, and 5%
tail-loss CVaR.

`tsmom_loop_verdict: WATCH`, when present, is loop-only and uses the namespace
`tsmom_strategy_validation_only_not_live_readiness`. It is not a readiness
verdict, not trading advice, not a profit guarantee, and not permission for live
orders. The readiness baseline remains separately recorded as
`readiness_baseline_verdict: NOT_READY` unless a separate readiness run passes
all readiness gates. Allowed TSMOM loop verdicts are `BLOCKED`, `WATCH`, and
`PASS_PAPER_ONLY`; none of these are live-readiness verdicts.

When the TSMOM loop is `WATCH`, run the diagnostic-only follow-up:

```bash
uv run quantctl tsmom-diagnostic-run
```

`TSMOM_STRESS_SWITCH_FUNDING_DIAGNOSTIC_V1` writes
`reports/loops/<YYYYMMDD>-tsmom-stress-switch-funding-diagnostic-v1/` with
`diagnostic.json`, `report.md`, `failed_windows.csv`,
`candidate_switches.csv`, `funding_decomposition.csv`, `commands.txt`, and
`changed_files.txt`. Its `tsmom_diagnostic_loop_verdict` is diagnostic-only:
it explains failed stress-vs-cash, selected-candidate switch-rate, and
funding-aware net-return gates by regime, stress window, funding drag, and
candidate instability. It does not retune parameters, relax gates, imply
profitability, imply live readiness, authorize live/testnet execution, or use
account/API-key/order/fill data for performance claims.

`tsmom-risk-run` implements the pre-registered paper-only
`TSMOM_SWITCH_STABILITY_FUNDING_THROTTLE_RISK_V1` follow-up risk/cost loop:

```bash
uv run quantctl tsmom-risk-run
```

It writes
`reports/loops/<YYYYMMDD>-tsmom-switch-stability-funding-throttle-risk-v1/`
with `risk_loop.json`, `report.md`, `candidate_switch_stability.csv`,
`funding_throttle_decomposition.csv`, `regime_breakdown.csv`, `commands.txt`,
and `changed_files.txt`. The loop tests two frozen risk rules before holdout
evaluation: train/validation-only near-tie sticky selection, and public
funding-drag exposure throttling. It preserves source TSMOM failed gates,
compares baseline TSMOM against the risk variant, and keeps
`readiness_baseline_verdict: NOT_READY` unless a separate readiness process
proves otherwise. It does not retune on holdout, relax gates, imply
profitability, imply live readiness, authorize live/testnet execution, or use
account/API-key/order/fill data for performance claims.

`tsmom-gross-edge-run` implements the pre-registered paper-only
`TSMOM_GROSS_EDGE_REGIME_NULL_ROBUSTNESS_V1` diagnostic/research loop:

```bash
uv run quantctl tsmom-gross-edge-run
```

It writes
`reports/loops/<YYYYMMDD>-tsmom-gross-edge-regime-null-robustness-v1/` with
`gross_edge_loop.json`, `report.md`, `gross_edge_decomposition.csv`,
`regime_edge_breakdown.csv`, `null_baseline_comparison.csv`,
`failed_edge_windows.csv`, `commands.txt`, and `changed_files.txt`. The loop
uses the frozen source TSMOM selected candidates to ask whether gross return is
positive before funding, slippage, and stress costs, whether any gross edge is
regime-concentrated, and whether simple null baselines match or beat TSMOM.
Its `tsmom_gross_edge_loop_verdict` is namespaced and loop-only; it is not a
readiness verdict. It preserves source failed gates and
`readiness_baseline_verdict: NOT_READY`, and it does not retune on holdout,
relax gates, imply profitability or live readiness, authorize live/testnet
execution, or use account/API-key/order/fill data for performance claims.

`tsmom-family-decision-run` implements the pre-registered paper-only
`TSMOM_CANDIDATE_FAMILY_DECISION_V1` research-governance loop:

```bash
uv run quantctl tsmom-family-decision-run
```

It writes `reports/loops/<YYYYMMDD>-tsmom-candidate-family-decision-v1/` with
`candidate_family_decision.json`, `report.md`, `evidence_rollup.csv`,
`failed_gate_rollup.csv`, `regime_exception_review.csv`,
`decision_rationale.md`, `commands.txt`, and `changed_files.txt`. The loop
reads the durable TSMOM source, diagnostic, risk, and gross-edge artifacts and
converts the accumulated evidence into one namespaced candidate-family decision:
`KEEP_WATCH`, `REGIME_ONLY_WATCH`, `FREEZE_AS_NEGATIVE_CONTROL`, or
`DEPRECATE_CANDIDATE_FAMILY`. It is not a strategy-optimization loop and does
not retune parameters, relax gates, hide failed gates, imply profitability or
live readiness, authorize live/testnet execution, or use account/API-key/order/
fill data for performance claims. `readiness_baseline_verdict` remains
`NOT_READY` unless a separate readiness process proves otherwise.

`tsmom-regime-only-falsification-run` implements the pre-registered paper-only
`REGIME_ONLY_TSMOM_FALSIFICATION_V1` follow-up:

```bash
uv run quantctl tsmom-regime-only-falsification-run
```

It writes `reports/loops/<YYYYMMDD>-regime-only-tsmom-falsification-v1/` with
`decision.json`, `report.md`, `evidence_rollup.csv`, `failed_gate_rollup.csv`,
`commands.txt`, and `changed_files.txt`. The loop reads existing durable
artifacts only, does not retune TSMOM parameters, and evaluates the
`market_regime=sideways` AND `volatility_regime=low-vol` exception slice
against cash/null, buy-and-hold, random sign, lagged signal, inverse signal,
and a simpler trend baseline when available. The decision is loop-only and
paper-only; `readiness_baseline_verdict` remains `NOT_READY`, and the loop does
not imply profitability, live readiness, execution readiness, or permission to
trade real money.

`research-null-baseline-harness-run` implements the strategy-agnostic
`RESEARCH_NULL_BASELINE_HARNESS_V1` validation-infrastructure loop:

```bash
uv run quantctl research-null-baseline-harness-run
```

It writes `reports/loops/<YYYYMMDD>-research-null-baseline-harness-v1/` with
`baseline_harness.json`, `baseline_comparison.csv`, `report.md`,
`commands.txt`, and `changed_files.txt`. The harness reads the durable TSMOM
source/diagnostic/risk/gross-edge/candidate-family/regime-only artifacts and
standardizes future null/baseline comparison rows for cash/null, buy-and-hold,
deterministic random sign, lagged signal, inverse signal, and a simpler trend
baseline only when durable evidence supports it. This is validation
infrastructure, not alpha, not optimization, and not a performance-improvement
feature. It records leakage defenses, deterministic random seeds, lagged-signal
no-current-bar policy, and that baseline comparison was not used for holdout
retuning. Existing TSMOM state is unchanged: broad active development is not
restored, regime-only TSMOM remains a negative-control candidate, and
`readiness_baseline_verdict` remains `NOT_READY`.

`alpha-signal-factory-composition-run` implements the paper-only
`ALPHA_SIGNAL_FACTORY_COMPOSITION_HARNESS_V1` validation-infrastructure loop:

```bash
uv run quantctl alpha-signal-factory-composition-run
```

It writes
`reports/loops/<YYYYMMDD>-alpha-signal-factory-composition-harness-v1/` with
`alpha_signal_factory.json`, `signal_catalog.jsonl`,
`signal_diagnostics.csv`, `signal_family_summary.csv`,
`signal_correlation_clusters.csv`, `null_baseline_comparison.csv`,
`shortlist.csv`, `failed_gate_rollup.csv`, `report.md`, `commands.txt`, and
`changed_files.txt`.

This command is an Alpha Signal Factory harness, not a trading strategy. It
generates deterministic SignalSpec DAGs for price-return, momentum,
mean-reversion, volatility, volume/liquidity, funding, and interaction/gated
signal families; applies bounded unary/binary/gating transforms; shifts signals
by at least one decision lag before forward-return label comparison; and records
duplicate/correlation clusters so near-identical variants do not count as
independent evidence. Generated signals are hypotheses, not proven edges.

The shortlist is paper-only and may contain only research categories such as
`KILL_SIGNAL`, `DUPLICATE_SIGNAL`, `WATCH_FAMILY_ONLY`,
`WATCH_DIAGNOSTIC_ONLY`, or `NEGATIVE_CONTROL_CANDIDATE`. It must not emit
`READY`, `TRADE`, `LIVE_READY`, `PROFITABLE`, `DEPLOY`, or
`CAPITAL_ALLOCATE`. Null/baseline and duplicate controls are mandatory:
cash/null, buy-and-hold where applicable, deterministic random sign, lagged
signal, inverse signal, shuffled timestamp signal, and frozen TSMOM
negative-control references when durable artifacts support them. The harness
does not claim profitability, readiness, live/testnet permission, capital
allocation, or execution readiness; `readiness_baseline_verdict` remains
`NOT_READY` and `tsmom_state_changed` remains `false`.

`alpha-signal-dag-expansion-run` implements the paper-only
`ALPHA_SIGNAL_DAG_COMBINATORIAL_EXPANSION_V2` harness:

```bash
uv run quantctl alpha-signal-dag-expansion-run
```

It writes
`reports/loops/<YYYYMMDD>-alpha-signal-dag-combinatorial-expansion-v2/` with
`alpha_signal_dag_expansion.json`, `signal_dag_catalog.jsonl`,
`signal_diagnostics.csv`, `signal_family_summary.csv`,
`signal_correlation_clusters.csv`, `null_baseline_comparison.csv`,
`multiple_testing_summary.csv`, `shortlist.csv`, `failed_gate_rollup.csv`,
`report.md`, `commands.txt`, and `changed_files.txt`.

V2 is a bounded combinatorial alpha signal DAG factory, not a trading strategy.
It expands public-data base signals through unary transforms, binary
interactions, regime/funding/volatility gates, and depth-2/depth-4 DAG
topologies under a deterministic candidate budget. Generated DAG signals are
hypotheses, not edges. The harness forbids full-sample normalization, shifts
every evaluated signal by a decision lag before labels, compares families and
representatives against null baselines, and chooses duplicate-cluster
representatives by deterministic non-performance rules.

Because V2 can generate many hypotheses, `multiple_testing_summary.csv` and the
top-level `multiple_testing_boundary` are mandatory. Apparent IC or hit-rate
patterns cannot be reported as alpha, profitability, readiness, or trading
permission from this loop. Shortlist labels are paper-only (`KILL_SIGNAL`,
`DUPLICATE_SIGNAL`, `WATCH_FAMILY_ONLY`, `WATCH_DIAGNOSTIC_ONLY`,
`NEGATIVE_CONTROL_CANDIDATE`, or `NEEDS_OOS_FALSIFICATION`) and only identify
families that may deserve a separate pre-registered paper-only OOS falsification
loop. `readiness_baseline_verdict` remains `NOT_READY`, and
`tsmom_state_changed` remains `false`.

`alpha-signal-oos-fdr-validation-run` implements the paper-only
`ALPHA_SIGNAL_OOS_FDR_VALIDATION_PROTOCOL_V3` harness:

```bash
uv run quantctl alpha-signal-oos-fdr-validation-run
```

It reads the frozen V2 artifact at
`reports/loops/20260627-alpha-signal-dag-combinatorial-expansion-v2/` by
default and writes
`reports/loops/<YYYYMMDD>-alpha-signal-oos-fdr-validation-protocol-v3/` with
`alpha_signal_oos_fdr_validation.json`, `frozen_candidate_universe.jsonl`,
`chronological_folds.csv`, `signal_oos_diagnostics.csv`,
`family_oos_summary.csv`, `representative_signal_summary.csv`,
`oos_null_baseline_comparison.csv`, `multiple_testing_fdr_summary.csv`,
`failed_gate_rollup.csv`, `next_falsification_queue.csv`, `report.md`,
`commands.txt`, and `changed_files.txt`.

V3 freezes the V2 candidate universe: it does not generate new Signal DAGs,
rename `signal_id`s, tune horizons, tune thresholds, optimize signal weights, or
select candidates by performance. It applies deterministic chronological folds
with purge and embargo metadata, evaluates train/validation/test IC and rank-IC
diagnostics, reports the corresponding test metric for the best validation
signal, and aggregates results by family with duplicate-adjusted breadth.

Multiple-testing and false-discovery risk remain first-class outputs. The V3
summary may only create a paper-only next-falsification queue; even OOS family
survival is not proof of alpha, profitability, readiness, execution readiness,
or permission to use live/testnet/account/API-key/order/fill data. Allowed
family decisions are limited to paper-research statuses such as `KILL_FAMILY`,
`DUPLICATE_DOMINATED`, `FAILS_OOS_CONSISTENCY`,
`FAILS_NULL_ROBUSTNESS`, `NEEDS_STRICTER_OOS_FALSIFICATION`, and
`WATCH_FAMILY_ONLY`. `readiness_baseline_verdict` remains `NOT_READY`, and
`tsmom_state_changed` remains `false`.

`alpha-signal-strict-falsification-run` implements the paper-only
`ALPHA_SIGNAL_STRICT_FALSIFICATION_PROTOCOL_V4` harness:

```bash
uv run quantctl alpha-signal-strict-falsification-run
```

It reads the frozen V3 artifact at
`reports/loops/20260627-alpha-signal-oos-fdr-validation-protocol-v3/` by
default and writes
`reports/loops/<YYYYMMDD>-alpha-signal-strict-falsification-protocol-v4/` with
`alpha_signal_strict_falsification.json`, `preregistered_hypotheses.csv`,
`frozen_candidate_universe.jsonl`, `strict_chronological_folds.csv`,
`signal_strict_oos_diagnostics.csv`, `bootstrap_significance_summary.csv`,
`family_strict_falsification_summary.csv`, `multiple_testing_fdr_summary.csv`,
`failed_gate_rollup.csv`, `next_protocol_queue.csv`, `report.md`,
`commands.txt`, and `changed_files.txt`.

V4 is a falsificationist follow-up to the V3 `next_falsification_queue.csv`. It
pre-registers each queued `(family, horizon)` hypothesis and its expected IC
sign *before* evaluation, freezes the V3 candidate universe to the queued
families without regeneration, and then tries to **reject** each hypothesis. It
applies stricter purge/embargo (`2 ×` the maximum queued horizon), computes a
block-permutation p-value per family/horizon (the label series is split into
non-overlapping blocks whose order is shuffled, with block length equal to the
horizon, so within-block label autocorrelation is preserved while the
signal-label alignment is destroyed), and Benjamini-Hochberg
corrects those p-values across the pre-registered set with an additional
selection-deflated alpha that accounts for the broad V2/V3 universe the
hypotheses were chosen from.

The protocol **defaults to FALSIFIED**: a hypothesis survives only when its
observed sign matches the pre-registration, sign/fold consistency and
null-robustness clear the `0.60` thresholds, degradation stays inside
`[0.5, 2.0]`, and its bootstrap p-value is BH-significant. Otherwise it is
labelled `FALSIFIED_SIGN_FLIP`, `FALSIFIED_OOS_INCONSISTENT`,
`FALSIFIED_NULL_FRAGILE`, `FALSIFIED_DEGRADATION`,
`FALSIFIED_NO_BOOTSTRAP_SIGNIFICANCE`, or `FALSIFIED_INSUFFICIENT_EVIDENCE`. A
`SURVIVED_STRICT_FALSIFICATION` verdict is explicitly **not** alpha
confirmation; survivors may only enter a paper-only `NEEDS_EXECUTION_AWARE_PROTOCOL`
queue. `readiness_baseline_verdict` remains `NOT_READY`, and
`tsmom_state_changed` remains `false`.

`multi-asset-alpha-generator-strict-prefilter-run` implements the paper-only
`MULTI_ASSET_ALPHA_GENERATOR_STRICT_PREFILTER_V5` harness:

```bash
uv run quantctl multi-asset-alpha-generator-strict-prefilter-run
```

It writes
`reports/loops/<YYYYMMDD>-multi-asset-alpha-generator-strict-prefilter-v5/`
with a public multi-asset panel summary, signal catalog, diagnostics,
correlation clusters, OOS prefilter rows, null-baseline comparisons,
multiple-testing summary, shortlist, failed gates, next strict-falsification
queue, report, commands, and changed-files artifacts.

V5 expands the information universe from single-symbol OHLCV/funding signals to
public multi-asset panel hypotheses: cross-sectional momentum/reversal,
funding dispersion, lead-lag, beta residual, volatility dispersion,
liquidity/volume shock, correlation breakdown, carry x trend, and relative-value
interactions. It does **not** revive V4-falsified V2/V3/V4 candidate families.
The V4 artifact is read only as lineage/audit evidence: V5 records
`source_v4_artifact_path` and whether `next_protocol_queue.csv` is empty, but it
does not seed candidates from V4 survivors, horizons, representatives, or
thresholds.

The default requested universe is
`BTCUSDT, ETHUSDT, SOLUSDT, BNBUSDT, XRPUSDT, ADAUSDT, DOGEUSDT, AVAXUSDT, LINKUSDT, TONUSDT`.
The command records requested, used, and excluded symbols deterministically.
At each timestamp, cross-sectional ranks use only symbols that have an actual
OHLCV row at that timestamp; missing symbols are coverage loss, not forward
filled. Missing or invalid public funding does not remove an OHLCV-usable
symbol; it only makes funding-dependent diagnostics unavailable for that symbol
or pair.

Generated signals are hypotheses, not edges. V5 is not a trading strategy, not
portfolio construction, not signal-weight optimization, not strategy PnL
optimization, not profitability evidence, not readiness, and not live/testnet
permission. Because the signal count increases multiple-testing risk, every
artifact records `cannot_claim_edge_due_to_multiple_testing: true`. A shortlist
entry is paper-only and can only mean a family may deserve another strict
falsification loop. `readiness_baseline_verdict` remains `NOT_READY`, and
`tsmom_state_changed` remains `false`.

`public-multi-asset-panel-coverage-run` implements the data-infrastructure-only
`PUBLIC_MULTI_ASSET_DATA_PANEL_COVERAGE_V6` loop:

```bash
uv run quantctl public-multi-asset-panel-coverage-run
```

It writes
`reports/loops/<YYYYMMDD>-public-multi-asset-data-panel-coverage-v6/` with
`public_multi_asset_panel_coverage.json`, `symbol_coverage.csv`,
`missing_data_plan.csv`, `panel_readiness.csv`, `provenance_manifest.csv`,
`report.md`, `commands.txt`, and `changed_files.txt`.

V6 exists because the V5 strict prefilter expanded the information universe to
public multi-asset data but the actual V5 artifact had only `BTCUSDT` usable and
excluded the other requested symbols for missing cached public OHLCV. V6 audits
that public OHLCV/funding panel coverage, preserves V5 excluded-symbol reasons,
records provenance, and produces a missing-public-data plan. It does not revive
V4-falsified families and does not rerun V5 automatically.

V6 is not a trading strategy, not alpha generation, not portfolio construction,
not signal-weight optimization, and not strategy PnL optimization. It makes no
profitability, edge, readiness, live-readiness, or execution-readiness claim.
`panel_ready_for_v5_rerun: true` means only that a separate future paper-only V5
rerun has enough public panel coverage to be considered. The V5 shortlist, if
created in a later run, remains paper-only; multiple-testing risk increases with
signal count and is not solved by this data coverage loop. V6 uses no
live/testnet/account/API-key/order/fill data, keeps
`readiness_baseline_verdict: NOT_READY`, and leaves `tsmom_state_changed:
false`.

`public-multi-asset-data-backfill-run` implements the public-data-only
`PUBLIC_MULTI_ASSET_DATA_BACKFILL_V7` loop:

```bash
uv run quantctl public-multi-asset-data-backfill-run
```

It reads the immutable V6 artifact, derives the default symbol universe and
target month span from that V6 source when CLI overrides are omitted, builds a
deterministic public OHLCV/funding backfill plan, skips existing cache files,
records checksums/provenance, and reruns the V6 coverage audit internally. It
does not run V5 automatically; it only reports whether
`panel_ready_for_v5_rerun` is true after the backfill attempt.

V7 writes `reports/loops/<YYYYMMDD>-public-multi-asset-data-backfill-v7/` with
`public_multi_asset_data_backfill.json`, `backfill_plan.csv`,
`download_manifest.csv`, `symbol_month_coverage.csv`,
`checksum_manifest.csv`, `failed_downloads.csv`,
`panel_readiness_after_backfill.csv`, `missing_data_remaining.csv`,
`report.md`, `commands.txt`, and `changed_files.txt`. Raw public cache files
under `data/**` are provenance inputs and should not be committed unless repo
policy explicitly allows it; V7 filters raw cache paths out of
`changed_files.txt` and records `raw_data_files_committed: false`.

V7 is not alpha generation, not a trading strategy, not portfolio construction,
not signal-weight optimization, not holdout retuning, and not a profitability,
edge, readiness, live-readiness, or execution-readiness claim. A future V5 rerun
should only be considered after `panel_ready_for_v5_rerun: true`, and any such
rerun remains paper-only with the same multiple-testing warning.

`panel-ready-multi-asset-alpha-prefilter-rerun` implements the paper-only
`PANEL_READY_MULTI_ASSET_ALPHA_PREFILTER_RERUN_V8` loop:

```bash
uv run quantctl panel-ready-multi-asset-alpha-prefilter-rerun
```

V8 reads the immutable V7 backfill artifact first and refuses to run unless
`panel_ready_for_v5_rerun: true`. It then reruns the existing V5 strict
prefilter on the V7 usable public cached universe only. For the current V7
artifact that means `BTCUSDT, ETHUSDT, SOLUSDT, BNBUSDT, XRPUSDT, ADAUSDT,
DOGEUSDT, AVAXUSDT, LINKUSDT`; `TONUSDT` remains excluded because V7 still
records missing cached OHLCV months for it.

V8 writes
`reports/loops/<YYYYMMDD>-panel-ready-multi-asset-alpha-prefilter-rerun-v8/`
with `panel_ready_multi_asset_alpha_prefilter.json` plus the V5-style panel
summary, signal catalog, diagnostics, correlation clusters, OOS prefilter,
null-baseline comparison, multiple-testing summary, shortlist, failed gates,
next strict-falsification queue, report, commands, and changed-files artifacts.
It does not download or backfill data and does not run V5 automatically outside
this explicit command.

Generated V8 signals are hypotheses, not edges. V8 is not a trading strategy,
not portfolio construction, not signal-weight optimization, not strategy PnL
optimization, not holdout retuning, not profitability evidence, not readiness,
and not live/testnet permission. Multiple-testing risk remains high because the
rerun evaluates thousands of signals; any shortlist is paper-only and can only
feed a separate future strict OOS falsification loop. `readiness_baseline_verdict`
remains `NOT_READY`, no private/account/API-key/order/fill data is used, and
`tsmom_state_changed` remains `false`.

`multi-asset-strict-oos-fdr-falsification-run` implements the paper-only
`MULTI_ASSET_STRICT_OOS_FDR_FALSIFICATION_V9` loop:

```bash
uv run quantctl multi-asset-strict-oos-fdr-falsification-run
```

V9 reads the immutable V8 artifact and freezes `next_strict_falsification_queue.csv`.
It evaluates only queued V8 families by replaying V8 catalog signal IDs; it does
not generate new signals, does not rerun V8, and does not download or backfill
data. The default command uses the V8 public cached symbol/month span in
cache-only mode. If the V8 queue is empty, V9 writes a schema-only artifact with
`suite_decision=NO_V8_QUEUE_TO_FALSIFY`.

V9 writes
`reports/loops/<YYYYMMDD>-multi-asset-strict-oos-fdr-falsification-v9/` with the
frozen queue, chronological fold metadata, strict OOS diagnostics, block
permutation null results, BH-FDR summary, regime robustness, symbol robustness,
null-baseline stress, failed gates, and next mechanism-research queue. The fixed
family rule is all-horizon survival across `8h`, `24h`, and `72h`; no best
horizon, best p-value, best IC, or result-based representative selection is
allowed. Permutation block size is pinned to each horizon.

A surviving V9 family, if any, is still not proven edge. `WATCH_FAMILY_ONLY` and
`NEEDS_MECHANISM_RESEARCH` mean paper-only follow-up. V9 is not a trading
strategy, not portfolio construction, not signal-weight optimization, not
profitability evidence, not readiness, and not live/testnet permission.
Multiple-testing risk remains central, `readiness_baseline_verdict` remains
`NOT_READY`, no private/account/API-key/order/fill data is used, and no TSMOM
state is changed.

`multi-asset-v9-falsification-lineage-governance-run` implements the paper-only
`V9_FALSIFICATION_LINEAGE_AND_NEGATIVE_CONTROL_GOVERNANCE_V10` loop:

```bash
uv run quantctl multi-asset-v9-falsification-lineage-governance-run
```

V10 does not generate alpha, rerun V8/V9, download data, implement a strategy,
or revive killed families. It reads the committed V9 strict falsification
artifact and its source V8 artifact, writes deterministic SHA-256 lineage
manifests, checks the committed V9 artifact invariants, and records
V9-falsified families as paper-only negative controls.

V10 writes
`reports/loops/<YYYYMMDD>-v9-falsification-lineage-negative-control-governance-v10/`
with V8/V9 lineage manifests, committed artifact invariants, a negative-control
registry, a disallowed-revival registry, and a post-mortem. The V9 kill state is
preserved as evidence: future work must start from a new pre-registered
mechanism with its own edge-source hypothesis, counterparty, data requirement,
and failure mode. V10 makes no profitability, edge, readiness, live/testnet, or
capital-allocation claim.

`public-derivatives-microstructure-data-axis-run` implements the public-data-only
`PUBLIC_DERIVATIVES_MICROSTRUCTURE_DATA_AXIS_V11` loop:

```bash
uv run quantctl public-derivatives-microstructure-data-axis-run
```

V11 preserves the V10 kill-state governance evidence and adds a new public
derivatives microstructure data axis for future V12 input readiness checks. It
audits existing public cache/provenance for open interest, long/short ratio,
taker buy/sell volume ratio, liquidation/force-order events, mark/index/basis
proxies, funding-rate extensions, and public depth proxies where available. It
does not generate alpha, does not rerun V8/V9/V10, does not implement a
strategy, does not optimize weights, does not download by default, and does not
use private/account/API-key/order/fill data or create live/testnet paths.

V11 writes
`reports/loops/<YYYYMMDD>-public-derivatives-microstructure-data-axis-v11/`
with per-symbol/per-family coverage, missing public data plans, download/cache
manifest rows, provenance hashes for existing cache files, V10 lineage lock
references, and a `readiness_by_family.csv` file whose required filename is used
only as a future V12 data coverage gate. Raw public data cache files should not
be committed unless repo policy explicitly allows it. V12 should only run when
`coverage_sufficient_for_future_v12=true`; that flag is not an edge,
profitability, execution, or live-readiness claim.

`public-derivatives-microstructure-recent-window-run` implements the
public-data-only
`PUBLIC_DERIVATIVES_MICROSTRUCTURE_RECENT_WINDOW_CONNECTOR_V12` loop:

```bash
uv run quantctl public-derivatives-microstructure-recent-window-run
```

V12 preserves the V11 outcome and V10 kill-state governance while building
connector/provenance infrastructure for public Binance USD-M derivatives
microstructure inputs. It does not generate alpha, does not rerun V8/V9/V10/V11,
does not implement a strategy, does not optimize portfolios or signal weights,
does not use private/account/API-key/order/fill data, and does not create
live/testnet paths.

V12 deliberately separates long-span `kline_derived_microstructure_proxy`
coverage from recent-window REST endpoint coverage. Existing public Binance
Vision raw kline cache may support historical proxy fields such as trade count,
quote volume, and taker-buy volume columns. Public `/futures/data/*` style
endpoints are treated as recent-window data with explicit retention boundaries;
they must not be mixed with 2024-2025 labels unless a future V13 protocol is
configured as recent-window-only. Raw public data cache should not be committed
unless repo policy explicitly allows it. V13 should only run when
`coverage_sufficient_for_future_v13=true`, and that flag is only a paper
research input gate, not an edge, profitability, readiness, or live/testnet
claim.

`historical-kline-microstructure-proxy-signal-factory-run` implements the
paper-only
`HISTORICAL_KLINE_MICROSTRUCTURE_PROXY_SIGNAL_FACTORY_V13` loop:

```bash
uv run quantctl historical-kline-microstructure-proxy-signal-factory-run
```

V13 is the single allowed follow-up when V12 proves long-span historical
`kline_derived_microstructure_proxy` coverage. It uses only existing public
Binance Vision raw kline cache fields such as trade count, quote volume, and
taker-buy volume columns to generate microstructure-aware signal hypotheses.
It does not use recent-window endpoint data, download or backfill data, rerun
V8/V9/V10/V11/V12, implement a strategy, optimize portfolios or signal weights,
or claim alpha, edge, profitability, readiness, execution readiness, or
live/testnet permission.

V13 writes
`reports/loops/<YYYYMMDD>-historical-kline-microstructure-proxy-signal-factory-v13/`
with `historical_kline_microstructure_signal_factory.json`,
`microstructure_panel_summary.csv`, `signal_catalog.jsonl`,
`signal_diagnostics.csv`, duplicate-correlation clusters, OOS/null/multiple
testing summaries, and `next_strict_falsification_queue.csv`. If that queue is
empty, the historical OHLCV/funding/kline-proxy chain stops without retuning.
If it is non-empty, the only next action is a frozen paper-only strict
OOS/FDR falsification loop.

Success reports now include a regime protocol layer:

- `regime.csv`: causal OHLCV-derived features, regime labels, protocol action, and risk multiplier by bar.
- `regime.json`: schema, fixed thresholds, paper-backed regime axes, protocol map, and summary.
- `metrics.json.regime_summary`: the same summary embedded for machine checks.

`regime.json.schema_version == 1` is intentionally frozen for this report contract. Breaking changes to columns, labels, thresholds, or protocol semantics must bump the schema version and include a migration note.

The regime layer is a risk/permission diagnostic (for example allow trend, reduce risk, or pause trend). It is not an alpha signal, live-fill proof, or profit guarantee.

Success reports also include an Alpha Signal Factory v1 layer:

- `signals.csv`: one causal OHLCV-derived signal row per bar/signal with `signal_id`, `asset`, `timeframe`, `category`, `family`, `role`, `lookback`, `segment`, `regime`, `value`, `direction`, `confidence`, `expiry_bars`, normalized score, weight, and weighted score.
- `signals.json`: frozen schema, computed category catalog, catalog-only external categories, duplicate-aware scoring semantics, claim boundary, and summary.
- `metrics.json.signal_summary`: the same summary embedded for automated checks.

`signals.json.schema_version == 1` is intentionally frozen. The v1 factory computes only OHLCV-backed trend, momentum, mean-reversion, volatility, volume, structure, regime-bridge, and exit diagnostics. Derivatives, orderbook/orderflow, on-chain, options, macro/events, sentiment/news, and execution-quality signals are listed as `not_computed_without_data_source` until explicit data sources and tests are added.

The signal layer is an alpha-research diagnostic and screening artifact. It is not trading advice, model-alpha proof, live-fill proof, or a profit guarantee; raw signal count is duplicate-controlled by `family`/`role` aggregation.

`signal_summary.latest.decision` is a diagnostic label only. It is not an order intent, target allocation, or portfolio instruction, and paper/live execution code must not consume it directly without a separately approved strategy adapter and validation protocol.

`risk-run` is the risk-adjusted research harness. It preserves the fixed `real-run` baseline, then uses a small predeclared product-family-aware grid, chronological train/validation/holdout selection, frozen `risk_adjusted_score_v1`, and report fields for Sharpe, MDD, candidate count, selected settings, and `holdout_used_for_selection:false`. The canonical BTCUSDT one-window result is still a research/comparison artifact, not proof of durable stable live returns.

Latest verified risk-run: `reports/risk-run-20260624T220620Z/`.

- Selected candidate: `c006-f24-s96-long-only`
- Holdout return: -1.232121126590955%
- Holdout Sharpe: -1.6176381689108235
- Holdout MDD: -2.4270383165800102%
- Baseline `real-run` holdout for the same window: return -13.111998074857983%, Sharpe -8.039068250214187, MDD -14.19746725941865%

This is a lower-drawdown/lower-loss research outcome for this window, not a profit guarantee.

`readiness-run` is the destructive validation harness for the Regime Protocol + Alpha Signal Factory stack. It is intentionally harsher than `real-run`/`risk-run`: it runs walk-forward train/validation/holdout windows, OOS holdout checks, cash and buy-hold baselines, stress commission/spread/dynamic OHLCV slippage proxy, public Binance USDⓈ-M funding-history drag when available, regime split, signal-family ablation, duplicate/correlation risk, prefix-invariance lookahead probes, selected-candidate stability checks, and live blockers.

The readiness candidate set now includes the fixed SMA/ATR bracket candidates plus a predeclared Alpha Signal Factory + Regime Protocol paper-strategy adapter. Selection still uses train/validation only; holdout is evaluated once after selection. This is a paper-only adapter for falsification, not an order intent.

`readiness.json.schema_version == 2` adds `compute_profile` and `validation_depth`.
This is a guard against mistaking a fast smoke/prototype run for a serious research
backtest: elapsed seconds are recorded as diagnostic-only metadata, while the
`validation_depth_research_grade` gate requires enough input bars, walk-forward
windows, candidates, candidate-window evaluations, and estimated bars processed.
Fast vectorized runs are allowed, but shallow runs stay `NOT_READY`/`BLOCKED` until
they clear workload-depth thresholds.

Funding is no longer only a hidden sensitivity assumption by default. `readiness-run` fetches the public no-key Binance USDⓈ-M Futures funding-rate history endpoint and records source URL, docs URL, row count, date range, hashes, and per-window funding rate used for drag. Account-specific funding cashflows are still not validated. If `--funding-source sensitivity` is used, the report explicitly falls back to sensitivity-only.

Execution cost remains non-live: the report uses commission/spread plus a deterministic OHLCV range/trade-count slippage proxy. It still is not exchange matching, not live-fill proof, and not authorization for live/testnet orders.

Success means the validation package ran and wrote evidence, not that the strategy is tradable. The report can only return `NOT_READY`, `CONDITIONALLY_READY_FOR_PAPER_ONLY`, or `BLOCKED`; there is no live-ready verdict. Success files:

- `readiness.json`
- `readiness_windows.csv`
- `readiness_ablation.csv`
- `readiness_regime.csv`
- `report.md`
- `manifest.json`

The final readiness report is expected to say what passed, what failed, and what remains before any real trading. It uses public Binance Vision klines and optional public Binance funding history only, submits no live/testnet orders, requires no API keys, and is not trading advice, live-fill proof, or a profit guarantee.

Latest verified readiness-run: `reports/readiness-run-20260625T053627Z/` — verdict `NOT_READY`, 5 walk-forward windows, 8 candidates, 40 candidate-window evaluations, failed gates `median_stress_return_beats_cash_null`, `max_selected_candidate_switch_rate`, and `validation_depth_research_grade`. Public Binance funding history was downloaded and the execution-cost proxy/depth profile were recorded, but the system still refused to promote itself to live trading.

## Live-market forward paper run

```bash
uv run quantctl forward-paper-run
```

`forward-paper-run` is the safe way to watch current/recent market behavior without placing orders. It pulls public Binance USDⓈ-M Futures recent klines (`/fapi/v1/klines`) and, by default, public funding history (`/fapi/v1/fundingRate`), then runs a long/flat paper adapter over the existing Regime Protocol + Alpha Signal Factory diagnostics.

Defaults:

- Symbol/timeframe: `BTCUSDT` / `1h`
- Lookback: 720 closed `1h` bars, roughly one month
- Initial cash: 10,000 USDT
- Fee/slippage assumptions: `0.0004` fee rate and `5` bps slippage
- Funding: `--funding-source public-binance`; use `--funding-source none` only when you explicitly want to exclude funding and record that limitation
- Latest still-open kline is excluded unless `--include-open-bar` is explicitly set; when
  closed-bar filtering is on, the command asks Binance for one extra kline so the saved
  artifact still contains the requested 720 closed bars

Success writes `reports/forward-paper-run-*` with `manifest.json`, `metrics.json`, `positions.csv`, `equity.csv`, `trades.csv`, `regime.*`, `signals.*`, and `report.md`.

This is still paper accounting: no API keys, no signed endpoints, no testnet/live orders, no exchange matching, no live-fill proof, no trading advice, and no profit guarantee. Testnet is useful later for order-plumbing validation, but it is not a performance validation environment.
