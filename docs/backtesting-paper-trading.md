# Binance Vision Backtesting and Paper/Holdout Run

This milestone adds a no-credential `real-run` workflow for a meaningful historical BTCUSDT USDⓈ-M Futures backtest plus bar-based holdout/paper simulation.

## Command

```bash
uv run quantctl real-run
uv run quantctl risk-run
uv run quantctl readiness-run
uv run quantctl data-depth-run
uv run quantctl tsmom-run
uv run quantctl forward-paper-run
```

Default canonical dataset:

- Symbol: `BTCUSDT`
- Interval: `1h`
- Months: `2024-01` through `2024-06`
- Source: Binance Vision monthly futures klines under `https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1h/`

`data-depth-run` intentionally uses a wider default archive window, 2024-01
through 2025-12, because its only purpose is to close the data-depth blocker
before serious TSMOM candidate selection. It writes loop artifacts under
`reports/loops/<YYYYMMDD>-data-depth-research-grade/` rather than replacing the
canonical readiness baseline.

## Engine and order-surface choices

- Backtesting/paper simulation engine: `backtesting.py` with `backtesting.lib.FractionalBacktest` for fractional BTC sizing.
- Exchange/order integration surface: `ccxt`, isolated in `src/parangsae/execution/ccxt_binance.py` and not imported by `real-run`.

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

## Regime protocol artifacts

Successful `real-run` and `risk-run` reports include a deterministic regime protocol overlay:

- `regime.csv` has one row per OHLCV bar with segment, close, causal rolling features, regime label, protocol action, risk multiplier, and reason.
- `regime.json` records schema version, fixed thresholds, protocol map, paper-backed regime axes, claim boundary, and summary.
- `metrics.json.regime_summary` mirrors `regime.json.summary` for automated checks.

`regime.json.schema_version == 1` is the frozen v1 report contract. Consumers can rely on the v1 columns and labels staying stable; any breaking change to columns, thresholds, labels, or protocol semantics must use a new schema version and include a migration note. The realized-volatility columns are horizon-scaled (`24h` by `sqrt(24)`, `168h` by `sqrt(168)`), while `vol_ratio` is the unscaled 24h rolling return standard deviation divided by the unscaled 168h rolling return standard deviation.

The classifier is intentionally simple and causal: each timestamp may use only OHLCV rows available at or before that timestamp. It does not read PnL, selected-candidate performance, holdout outcomes, credentials, or execution/order state.

Protocol labels are used as a risk/permission overlay:

- `normal_trend`: trend-following is permitted at baseline research size.
- `high_vol_trend`: trend exists but risk should be reduced.
- `chop` / `high_vol_chop`: trend-following is paused unless a separate mean-reversion strategy is validated.
- `liquidity_stress`: trend-following is paused because volume/illiquidity stress overrides signal permission.
- `insufficient_data`: no permission until enough causal history is available.

This layer comes from the research protocol: standard/null baselines, switching regimes, change-point awareness, volatility/correlation models, trend-vs-chop diagnostics, liquidity/funding stress, and overfit controls. It is not an alpha signal, not a live-fill proof, and not a profit guarantee.

## Alpha Signal Factory artifacts

Successful `real-run` and `risk-run` reports also include an Alpha Signal Factory v1 overlay:

- `signals.csv` has one row per timestamp/signal. The frozen v1 columns are `Timestamp`, `signal_id`, `asset`, `timeframe`, `category`, `family`, `role`, `lookback`, `segment`, `regime`, `value`, `direction`, `confidence`, `expiry_bars`, `normalized_value`, `weight`, `weighted_score`, and `description`.
- `signals.json` records schema version, method details, config, computed/catalog-only category catalog, scoring weights, claim boundary, and summary.
- `metrics.json.signal_summary` mirrors `signals.json.summary`.
- `report.md` includes `## Alpha Signal Factory` with the latest direction, entry, risk, exit, final score, diagnostic decision, signal count, family count, duplicate-family counts, and execution-boundary text.

`signals.json.schema_version == 1` freezes the v1 report contract. Breaking changes to columns, top-level JSON keys, scoring semantics, or computed category meanings require a schema-version bump and migration note.

V1 computes only deterministic OHLCV-backed diagnostics:

- trend / moving average distance, slope, and stack,
- momentum ROC and RSI state,
- mean-reversion z-score,
- volatility ATR% and realized-volatility ratio,
- volume z-score confirmation,
- Donchian structure breakouts,
- regime permission bridge from the regime protocol,
- exit/trailing diagnostics.

The broader signal taxonomy is preserved as catalog-only until data sources exist. Derivatives/funding/OI, orderbook/orderflow, on-chain, options, macro/events, sentiment/news, and execution/slippage categories are marked `not_computed_without_data_source` in `signals.json`.

The signal scores are duplicate-aware diagnostics: individual rows normalize to `[-1, 1]`, then summary scores aggregate by `family` and `role` so many similar indicators cannot dominate only because there are more of them. This is still a heuristic screening artifact, not trading advice, not proof of predictive alpha, not live-fill proof, and not a profit guarantee.

`signal_summary.latest.decision` is intentionally diagnostic. It is not an executable order intent, target allocation, or portfolio instruction; any paper/live execution use requires a separate approved strategy adapter plus validation protocol.

## Data-depth research-grade loop

`data-depth-run` implements the `DATA_DEPTH_RESEARCH_GRADE_V1` loop. It is a
public/no-key data governance and validation-depth workflow, not a
strategy-performance loop:

```bash
uv run quantctl data-depth-run
```

Default data scope:

- Symbol/timeframe: `BTCUSDT` / `1h`
- OHLCV source: public Binance Vision USDⓈ-M Futures monthly klines
- Months: `2024-01` through `2025-12`
- Funding source: public Binance USDⓈ-M funding-rate history over the OHLCV UTC
  range; `--funding-source none` records the explicit omission
- Bar handling: closed monthly archive bars only; `close_time` must equal
  `open_time + interval - 1ms`

Required loop artifacts:

- `loop.json`
- `report.md`
- `commands.txt`
- `changed_files.txt`

The payload records source URLs/downloads, symbol, product family, timeframe,
UTC range, row count, closed/open bar status, normalized OHLCV hash, gap policy,
duplicate policy, optional funding coverage/hash, funding causal-alignment
policy, and no-order/no-key/no-account safety gates. Its `data_depth_contract`
is a frozen data-loop snapshot rather than a live import of `readiness-run`
defaults, so later readiness tuning does not silently change the meaning of this
artifact. Source-verified Binance Vision archive rows may include zero-volume
closed bars; those are recorded as quality flags and are not deleted or treated
as live execution evidence.

`loop.json.schema_version == 2` adds explicit provenance and coverage fields:

- `dataset_provenance` for OHLCV and public funding inputs, including URLs,
  hashes, UTC ranges, row counts, and no-key/no-account boundaries.
- `report_artifact_provenance` for the generated loop artifacts.
- `coverage_analysis.walk_forward_windows` for frozen-contract projected
  train/validation/holdout windows.
- `coverage_analysis.volatility_regimes` from a 24-bar rolling standard
  deviation of public hourly close returns.
- `coverage_analysis.stress_periods` for public-market context only: underlying
  drawdown, hourly 5% CVaR, worst 24h return, high-volatility bars, and stress
  bars. These are not strategy PnL.
- `coverage_analysis.funding_regimes` for public funding-rate sign and high
  absolute-rate coverage when funding is downloaded.

This loop can pass the public data bar/window portions of the research-grade
depth requirements. `data_depth_loop_verdict: WATCH`, when present, is a
data-depth-only status in `loop.json`/`report.md`; it is not a readiness verdict
and does not change the committed readiness baseline. The baseline is recorded
separately as `readiness_baseline_verdict: NOT_READY`. The loop deliberately
leaves candidate count, candidate-window evaluations, bars processed by a
candidate grid, median stress return vs cash/null, candidate switch rate,
funding-aware net return, drawdown, and tail loss under unclaimed
strategy/performance gates until a later pre-registered paper-only
strategy/validation loop. Do not use this command to retune TSMOM parameters or
promote live readiness.

## TSMOM paper-only strategy/validation loop

`tsmom-run` implements the pre-registered
`TSMOM_VOL_TARGET_FUNDING_AWARE_V1` loop. It is the first paper-only strategy
loop after `DATA_DEPTH_RESEARCH_GRADE_V1`; it does not replace or mutate the
canonical `readiness-run` baseline:

```bash
uv run quantctl tsmom-run
```

Default data scope:

- Symbol/timeframe: `BTCUSDT` / `1h`
- OHLCV source: public Binance Vision USDⓈ-M Futures monthly klines
- Months: `2024-01` through `2025-12`
- Funding source: public Binance USDⓈ-M funding-rate history over the OHLCV UTC
  range; `--funding-source none` makes the funding-aware gate unclaimed/failed
- Live/testnet/account/API-key/order/fill data: not used for performance claims

Required loop artifacts are written under
`reports/loops/<YYYYMMDD>-tsmom-vol-target-funding-aware-v1/`:

- `loop.json`
- `report.md`
- `commands.txt`
- `changed_files.txt`
- `tsmom_windows.csv`
- `tsmom_candidates.csv`

The frozen candidate grid has 48 long-only candidates:

- `lookback_bars=(168, 336, 672)`
- `vol_window_bars=(168, 336)`
- `target_annual_volatility=(0.25, 0.40)`
- `max_exposure=(0.50, 0.80)`
- `funding_abs_rate_limit=(0.00025, 0.00050)`

Selection is train/validation-only via `risk_adjusted_score_v1`. Tie-breakers
are higher validation return, smaller absolute validation drawdown, lower
validation trade/turnover proxy, then lexicographically smaller candidate ID.
Holdout does not participate in selection.

The loop records these hard gates:

- `candidate_count`
- `candidate_window_evaluations`
- `bars_processed_estimate`
- `median_stress_return_beats_cash_null`
- `selected_candidate_switch_rate`
- `funding_aware_net_return`
- `max_drawdown_abs_pct`
- `tail_loss_cvar_pct`

Funding is causal: public funding events are forward-filled only after the event
timestamp and shifted one decision bar; they are never backfilled into earlier
bars. Missing public funding coverage means the funding-aware gate is not
claimable.

`tsmom_loop_verdict` is loop-only and uses
`tsmom_loop_verdict_namespace:
tsmom_strategy_validation_only_not_live_readiness`. It is not a readiness
verdict. The committed readiness baseline remains separately recorded as
`readiness_baseline_verdict: NOT_READY` unless a separate readiness report passes
all readiness gates. Allowed TSMOM loop verdicts are `BLOCKED`, `WATCH`, and
`PASS_PAPER_ONLY`; none of these are live-readiness verdicts.

## TSMOM failed-gate diagnostic loop

`tsmom-diagnostic-run` is the diagnostic-only follow-up for a `WATCH`
`TSMOM_VOL_TARGET_FUNDING_AWARE_V1` loop. It does not select a new strategy,
retune parameters, relax gates, or promote the repo to readiness. It reruns the
same frozen public-data TSMOM grid and writes
`reports/loops/<YYYYMMDD>-tsmom-stress-switch-funding-diagnostic-v1/` with:

- `diagnostic.json`
- `report.md`
- `failed_windows.csv`
- `candidate_switches.csv`
- `funding_decomposition.csv`
- `commands.txt`
- `changed_files.txt`

The diagnostic contract is scoped to
`TSMOM_STRESS_SWITCH_FUNDING_DIAGNOSTIC_V1`. Its namespaced
`tsmom_diagnostic_loop_verdict` explains the previously failed
stress-vs-cash, selected-candidate switch-rate, and funding-aware net-return
gates by regime, stress window, public funding drag, and candidate instability.
It must not be rendered as a generic `verdict` or `loop_verdict`, and the
readiness baseline remains separately recorded as
`readiness_baseline_verdict: NOT_READY`.

The diagnostic artifacts decompose each holdout window by bull/bear/sideways
market regime, high/normal/low volatility regime, stress-window status, and
positive/negative/high-cost funding regime where public funding data is
available. `failed_windows.csv` records the stress-window return distribution
against cash/null, `candidate_switches.csv` records the selected candidate per
window plus inferable switch causes, and `funding_decomposition.csv` separates
gross return, funding cost, stress cost, slippage cost, and net return.

This loop remains paper-only and public-data-only. It is not trading advice, not
a profitability claim, not live readiness, and not permission for live/testnet
orders or account/API-key/fill data use.

## TSMOM switch-stability funding-throttle risk loop

`tsmom-risk-run` is the pre-registered paper-only risk/cost follow-up to
`TSMOM_STRESS_SWITCH_FUNDING_DIAGNOSTIC_V1`. It does not create a live order
path, retune on holdout, relax failed gates, or promote the repo to readiness.
It reruns the frozen TSMOM train/validation selection surface and evaluates two
pre-registered risk rules:

- **near-tie sticky selection** — if the prior stable candidate remains eligible
  and the current train/validation top candidate beats it by no more than the
  frozen score margin, keep the prior candidate for stability;
- **funding-drag exposure throttle** — on decision bars where causally aligned
  public funding abs(rate) exceeds the frozen threshold, multiply selected
  exposure by the frozen throttle multiplier.

The command writes
`reports/loops/<YYYYMMDD>-tsmom-switch-stability-funding-throttle-risk-v1/`
with:

- `risk_loop.json`
- `report.md`
- `candidate_switch_stability.csv`
- `funding_throttle_decomposition.csv`
- `regime_breakdown.csv`
- `commands.txt`
- `changed_files.txt`

`risk_loop.json` records the pre-registration contract: hypothesis, allowed
inputs, forbidden inputs, candidate risk rules, evaluation gates, holdout
boundary, claim boundary, and expected failure modes. The loop's
`tsmom_risk_loop_verdict` is namespaced to
`tsmom_switch_stability_funding_throttle_risk_only_not_readiness`; it is not a
generic readiness verdict. `readiness_baseline_verdict` remains `NOT_READY`
unless a separate readiness process proves otherwise, and the source TSMOM
failed gates remain recorded rather than hidden.

The risk artifacts compare baseline selected TSMOM holdout rows with the risk
variant by gross return, funding drag, slippage drag, stress net return,
drawdown, CVaR-style tail loss, cash/null comparison, switch count/rate, score
margins, switch causes, and public-data-derived regime labels. This remains
paper-only research, not trading advice, not a profitability claim, not live or
execution readiness, and not permission for live/testnet orders or
account/API-key/fill data use.

## TSMOM gross-edge regime/null robustness loop

`tsmom-gross-edge-run` is the pre-registered paper-only diagnostic/research
follow-up to the risk loop. It keeps the source TSMOM family frozen and asks
whether the selected TSMOM signal has positive gross edge before funding,
slippage, and stress costs; whether any edge is limited to specific public-data
regimes; and whether simple null baselines match or beat TSMOM. It does not
create a live order path, retune on holdout, relax failed gates, or promote the
repo to readiness.

The command writes
`reports/loops/<YYYYMMDD>-tsmom-gross-edge-regime-null-robustness-v1/` with:

- `gross_edge_loop.json`
- `report.md`
- `gross_edge_decomposition.csv`
- `regime_edge_breakdown.csv`
- `null_baseline_comparison.csv`
- `failed_edge_windows.csv`
- `commands.txt`
- `changed_files.txt`

`gross_edge_loop.json` records the pre-registration contract: hypothesis,
allowed inputs, forbidden inputs, regime definitions, null baselines,
evaluation gates, holdout boundary, claim boundary, and expected failure modes.
The loop's `tsmom_gross_edge_loop_verdict` is namespaced to
`tsmom_gross_edge_regime_null_robustness_only_not_readiness`; it is not a
generic readiness verdict. `readiness_baseline_verdict` remains `NOT_READY`
unless a separate readiness process proves otherwise, and source TSMOM failed
gates remain recorded rather than hidden.

The gross-edge artifacts report per-window gross return before costs, net return
after funding/slippage/stress, funding drag, slippage drag, stress drag, cash
and buy-and-hold deltas, direction correctness, exposure, turnover, drawdown,
CVaR-style tail loss, market/volatility/funding/funding-drag/trend regimes, and
null comparisons against cash, deterministic random sign, lagged signal, inverse
signal, and buy-and-hold. This remains paper-only research, not trading advice,
not a profitability claim, not live or execution readiness, and not permission
for live/testnet orders or account/API-key/fill data use.

## TSMOM candidate-family decision loop

`tsmom-family-decision-run` is the pre-registered paper-only
`TSMOM_CANDIDATE_FAMILY_DECISION_V1` research-governance loop. It does not
evaluate a new strategy, optimize parameters, retune on holdout, relax failed
gates, or create a live order path. Instead, it reads the durable TSMOM source,
stress/switch/funding diagnostic, switch-stability/funding-throttle risk, and
gross-edge/regime/null robustness artifacts and emits an explicit
candidate-family queue decision.

The command writes
`reports/loops/<YYYYMMDD>-tsmom-candidate-family-decision-v1/` with:

- `candidate_family_decision.json`
- `report.md`
- `evidence_rollup.csv`
- `failed_gate_rollup.csv`
- `regime_exception_review.csv`
- `decision_rationale.md`
- `commands.txt`
- `changed_files.txt`

Allowed decisions are:

- `KEEP_WATCH`
- `REGIME_ONLY_WATCH`
- `FREEZE_AS_NEGATIVE_CONTROL`
- `DEPRECATE_CANDIDATE_FAMILY`

`candidate_family_decision.json` records the pre-registration contract:
decision purpose, allowed decisions, evidence inputs, forbidden inputs,
decision gates, regime exception rules, deprecation rules, claim boundary, and
readiness boundary. The namespaced `tsmom_candidate_family_decision` is a
research-governance decision only; it is not a readiness verdict and does not
claim positive expected return. `readiness_baseline_verdict` remains
`NOT_READY` unless a separate readiness process proves otherwise.

The decision loop preserves failed gates across prior artifacts and reports
whether TSMOM remains broad WATCH, is narrowed to regime-only WATCH, is frozen
as a negative-control benchmark, or is deprecated from the active candidate
queue. It remains paper-only research governance, not trading advice, not a
profitability claim, not live or execution readiness, and not permission for
live/testnet orders or account/API-key/fill data use.

## Regime-only TSMOM falsification loop

`tsmom-regime-only-falsification-run` is the pre-registered paper-only
`REGIME_ONLY_TSMOM_FALSIFICATION_V1` follow-up to the candidate-family decision.
It does not implement a new strategy, optimize parameters, retune on holdout,
relax failed gates, or create a live/testnet order path. It reads existing
durable artifacts and evaluates only the intersection slice where
`market_regime=sideways` and `volatility_regime=low-vol`.

The command writes
`reports/loops/<YYYYMMDD>-regime-only-tsmom-falsification-v1/` with:

- `decision.json`
- `report.md`
- `evidence_rollup.csv`
- `failed_gate_rollup.csv`
- `commands.txt`
- `changed_files.txt`

The loop reports gross/net returns, funding drag, slippage drag, stress drag,
turnover, drawdown, trade count, sample size, regime coverage, and null/baseline
comparisons against cash/null, buy-and-hold, deterministic random sign, lagged
signal, inverse signal, and a simpler trend baseline when the durable artifacts
make it available. If the exception fails on sample size, net-of-cost return, or
null robustness, it is marked as a negative-control/deprecation candidate rather
than restored to broad active WATCH.

This remains paper-only research governance. `readiness_baseline_verdict`
remains `NOT_READY`; the report is not trading advice, not a profitability
claim, not live or execution readiness, and not permission for live/testnet
orders or account/API-key/fill data use.

## Research null/baseline harness

`research-null-baseline-harness-run` implements
`RESEARCH_NULL_BASELINE_HARNESS_V1`, a strategy-agnostic validation
infrastructure loop. It is deliberately not a new alpha strategy and does not
retune TSMOM parameters, retune holdout, relax gates, or restore any TSMOM
candidate status.

The command writes
`reports/loops/<YYYYMMDD>-research-null-baseline-harness-v1/` with:

- `baseline_harness.json`
- `baseline_comparison.csv`
- `report.md`
- `commands.txt`
- `changed_files.txt`

The common `baseline_comparison.csv` schema records, at minimum:
`baseline_name`, strategy gross/net return, baseline gross/net return, gross/net
beat flags, sample size, trade count, turnover, fees/slippage/funding drag,
max drawdown, optional regime filter, evidence source, and claim boundary. The
baseline families are cash/null, buy-and-hold, deterministic random sign,
lagged signal, inverse signal, and a simpler trend baseline only when durable
evidence supports it.

Leakage defenses are first-class artifact fields: forward-return label overlap
is explicitly recorded, random sign uses deterministic seeds, lagged signal
baselines are required to avoid current-bar information, and the harness records
that baseline comparison is evaluation-only and not holdout retuning input.

This harness makes future strategy claims harder. It does not change TSMOM
decisions: broad TSMOM active development remains failed/not restored,
regime-only TSMOM remains a negative-control candidate, and
`readiness_baseline_verdict` remains `NOT_READY`.

## Alpha Signal Factory composition harness

`alpha-signal-factory-composition-run` implements
`ALPHA_SIGNAL_FACTORY_COMPOSITION_HARNESS_V1`, a paper-only validation
infrastructure loop for generating, composing, diagnosing, duplicate-controlling,
and null-comparing alpha signal hypotheses. It is not a trading strategy, not a
strategy PnL optimizer, not live/testnet readiness, and not a profitability
claim.

```bash
uv run quantctl alpha-signal-factory-composition-run
```

The command writes
`reports/loops/<YYYYMMDD>-alpha-signal-factory-composition-harness-v1/` with:

- `alpha_signal_factory.json`
- `signal_catalog.jsonl`
- `signal_diagnostics.csv`
- `signal_family_summary.csv`
- `signal_correlation_clusters.csv`
- `null_baseline_comparison.csv`
- `shortlist.csv`
- `failed_gate_rollup.csv`
- `report.md`
- `commands.txt`
- `changed_files.txt`

Each generated `SignalSpec` is JSON-serializable and records `signal_id`,
`signal_family`, base inputs, operators, lookback, decision lag, max depth,
funding/volume usage, future-data flag, deterministic seed, and claim boundary.
Default generation is deterministic and bounded: primitives use max depth 1,
transforms use max depth 2, and interactions/gates use max depth 3. Recursive
unbounded signal generation is not allowed.

Generated signals are hypotheses, not edges. The harness computes diagnostic
metrics only: coverage, missingness, mean/std, autocorrelation, turnover proxy,
forward-return IC/rank-IC for 8h/24h/72h where labels exist, signal-sign hit
rate, decay profile, regime breakdown, duplicate cluster ID, max absolute
correlation to prior signals, null comparison status, and leakage status. It
does not optimize signal weights, thresholds, horizons, capital allocation, or
strategy PnL.

Leakage controls are mandatory. Signals may use only public OHLCV/funding data
available at or before the bar timestamp, rolling/z-score transforms use
historical windows rather than full-sample normalization, and every signal is
shifted by at least one decision lag before forward-return label comparison.
Null/baseline controls are mandatory too: zero/cash-null, buy-and-hold where
applicable, deterministic random sign, lagged signal, inverse signal, shuffled
timestamp signal, and frozen TSMOM negative-control references when durable
artifacts support them.

Duplicate/correlation control is part of the evidence contract. Pairwise signal
correlations are clustered where feasible, absolute correlation above `0.95`
marks redundant variants, and `effective_signal_breadth` excludes duplicates so
many near-identical signals cannot count as independent evidence.

Allowed shortlist labels are limited to `KILL_SIGNAL`, `DUPLICATE_SIGNAL`,
`WATCH_FAMILY_ONLY`, `WATCH_DIAGNOSTIC_ONLY`, and
`NEGATIVE_CONTROL_CANDIDATE`. Forbidden labels include `READY`, `TRADE`,
`LIVE_READY`, `PROFITABLE`, `DEPLOY`, and `CAPITAL_ALLOCATE`.
`WATCH_*` means only that a signal family deserves another paper-only
falsification loop. It does not imply profitability, readiness, execution
readiness, live/testnet permission, or permission to use real money.
`readiness_baseline_verdict` remains `NOT_READY` and `tsmom_state_changed`
remains `false`.

## Alpha Signal DAG combinatorial expansion harness

`alpha-signal-dag-expansion-run` implements
`ALPHA_SIGNAL_DAG_COMBINATORIAL_EXPANSION_V2`, a paper-only combinatorial Signal
DAG factory. It is validation infrastructure for falsifying many signal
hypotheses; it is not a trading strategy, not signal weight optimization, not
strategy PnL optimization, not live/testnet readiness, and not a profitability
claim.

```bash
uv run quantctl alpha-signal-dag-expansion-run
```

The command writes
`reports/loops/<YYYYMMDD>-alpha-signal-dag-combinatorial-expansion-v2/` with:

- `alpha_signal_dag_expansion.json`
- `signal_dag_catalog.jsonl`
- `signal_diagnostics.csv`
- `signal_family_summary.csv`
- `signal_correlation_clusters.csv`
- `null_baseline_comparison.csv`
- `multiple_testing_summary.csv`
- `shortlist.csv`
- `failed_gate_rollup.csv`
- `report.md`
- `commands.txt`
- `changed_files.txt`

Each `SignalDagSpec` is JSON-serializable and records `signal_id`, `dag_id`,
`signal_family`, `root_operator`, `nodes`, `edges`, `base_inputs`,
`operator_sequence`, depth, max depth, lookback, decision lag, price/volume/
funding/regime-gate usage, future-data flag, generation seed, generation rule,
complexity score, and claim boundary. Default generation is deterministic with
`max_depth=4`, `candidate_budget=2000`, and a hard candidate-budget maximum of
`10000`. Generation uses only public OHLCV/funding base inputs and never uses
forward returns, labels, holdout feedback, or performance to select candidates.

The operator grammar includes unary transforms (`delay`, `delta`,
`rolling_mean`, `rolling_std`, `zscore`, `rank`, `winsorize`, `sign`, `abs`,
`negate`, `clip`, `ewma`, `decay_linear`, `volatility_scale`, `rolling_min`,
`rolling_max`, and `rolling_quantile_position`), binary operators (`add`,
`subtract`, `multiply`, `ratio_safe`, `spread`, `interaction`, `rolling_corr`,
`rolling_beta_proxy`, `min`, and `max`), and regime/funding/volume gates
(`gate_low_volatility`, `gate_high_volatility`, `gate_positive_funding`,
`gate_negative_funding`, `gate_extreme_funding`, `gate_volume_spike`,
`gate_range_bound`, and `gate_trending_proxy`). Template families include
momentum/volatility, momentum/funding, mean-reversion/low-vol,
mean-reversion/funding-extreme, volume-shock/return-direction,
breakout/volume-confirmation, funding-extreme/volatility,
funding-change/return-reversal, range-position/realized-volatility, and
carry-proxy/trend-filter interactions.

Evaluation remains diagnostic only. For each DAG signal the harness records
coverage, missingness, mean/std, autocorrelation, turnover proxy, max absolute
signal, duplicate cluster ID, max absolute correlation, effective-independent
flag, 8h/24h/72h forward-return IC and rank-IC where labels exist, signal-sign
hit rates, decay profile, regime breakdown, null-comparison status, leakage
status, and failure reason. Every evaluated signal is shifted by at least one
decision lag before label comparison; same-bar label usage and full-sample
normalization are forbidden.

Null/baseline comparison covers family means and deterministic cluster
representatives: zero/cash-null, buy-and-hold same-window where applicable,
deterministic random sign, lagged signal, inverse signal, shuffled timestamp
signal, family-permuted signal, and frozen TSMOM negative-control references
when durable artifacts support them. Duplicate/correlation control marks
absolute correlation `>= 0.95` as redundant and selects representatives by
lowest `complexity_score`, then smallest depth, then lexicographic signal ID;
representatives are not chosen by performance.

Multiple-testing controls are mandatory because many DAG hypotheses are scanned.
`multiple_testing_summary.csv` reports generated/evaluated counts, null
comparison counts, family best/median raw IC, family hit rate, duplicate-adjusted
signal count, effective signal breadth, an approximate false-discovery warning,
a selection-bias warning, and `cannot_claim_edge_due_to_multiple_testing=True`.
This loop cannot claim a discovered edge; any WATCH or
`NEEDS_OOS_FALSIFICATION` output only means a family may deserve a separate
pre-registered paper-only OOS falsification loop.

Allowed shortlist labels are limited to `KILL_SIGNAL`, `DUPLICATE_SIGNAL`,
`WATCH_FAMILY_ONLY`, `WATCH_DIAGNOSTIC_ONLY`,
`NEGATIVE_CONTROL_CANDIDATE`, and `NEEDS_OOS_FALSIFICATION`. Forbidden labels
include `READY`, `TRADE`, `LIVE_READY`, `PROFITABLE`, `DEPLOY`, and
`CAPITAL_ALLOCATE`. `readiness_baseline_verdict` remains `NOT_READY`,
`holdout_retune_used` remains `false`, and `tsmom_state_changed` remains
`false`.

## Alpha Signal OOS/FDR validation protocol

`alpha-signal-oos-fdr-validation-run` implements
`ALPHA_SIGNAL_OOS_FDR_VALIDATION_PROTOCOL_V3`, a paper-only OOS and
false-discovery-control validation protocol over the frozen V2 Signal DAG
candidate universe. It is not new alpha generation, not strategy optimization,
not signal weight optimization, not live/testnet readiness, and not a
profitability claim.

```bash
uv run quantctl alpha-signal-oos-fdr-validation-run
```

The command reads the existing V2 artifact directory and writes a new artifact
directory:

- `alpha_signal_oos_fdr_validation.json`
- `frozen_candidate_universe.jsonl`
- `chronological_folds.csv`
- `signal_oos_diagnostics.csv`
- `family_oos_summary.csv`
- `representative_signal_summary.csv`
- `oos_null_baseline_comparison.csv`
- `multiple_testing_fdr_summary.csv`
- `failed_gate_rollup.csv`
- `next_falsification_queue.csv`
- `report.md`
- `commands.txt`
- `changed_files.txt`

V3 freezes the V2 candidate universe exactly for validation. It does not
generate new DAGs, change `signal_id`s, retune horizons, retune thresholds,
optimize weights, or add candidates based on performance. Chronological folds
record train/validation/test boundaries, purge bars, embargo bars, horizon bars,
sample counts, event counts, and purged observation counts. Forward-return label
overlap across fold boundaries is treated as a leakage risk and is purged before
diagnostics are reported.

For each frozen `signal_id` by horizon and fold, V3 records train/validation/test
IC, rank-IC, hit rate, coverage, turnover proxy, missingness, sign consistency,
degradation, null status, and leakage status. Family summaries use
duplicate-adjusted breadth and report the corresponding test metric for the
best validation signal rather than picking the best test metric after the fact.
Representative signals come from V2 cluster representatives where available, or
from deterministic non-performance rules: lowest complexity score, then
smallest depth, then lexicographic `signal_id`.

Multiple-testing risk is mandatory in every V3 artifact. The protocol reports
total signal/family/horizon tests, duplicate-adjusted test count, effective
signal breadth, best raw metric warnings, family median metrics, corresponding
OOS metrics, conservative false-discovery warnings, and
`cannot_claim_edge_due_to_multiple_testing=True` when statistical precision is
not justified. It must not manufacture pseudo-significant p-values.

Null/baseline comparison is OOS-aware and covers zero/cash-null, deterministic
random sign, lagged signal, inverse signal, shuffled timestamp signal,
family-permuted signal, buy-and-hold same-window where applicable, and frozen
TSMOM negative-control references when durable evidence exists. In-sample null
wins are not OOS evidence.

Allowed family decisions are limited to `KILL_FAMILY`,
`DUPLICATE_DOMINATED`, `FAILS_OOS_CONSISTENCY`,
`FAILS_NULL_ROBUSTNESS`, `NEEDS_STRICTER_OOS_FALSIFICATION`, and
`WATCH_FAMILY_ONLY`. Allowed suite decisions are paper-only infrastructure or
paper-only follow-up statuses. Forbidden labels include `READY`, `TRADE`,
`LIVE_READY`, `PROFITABLE`, `DEPLOY`, `CAPITAL_ALLOCATE`, and
`EDGE_CONFIRMED`. `WATCH_FAMILY_ONLY` means only that a family may deserve a
separate pre-registered paper-only falsification loop; it does not imply alpha,
edge, profitability, readiness, execution readiness, live/testnet permission, or
permission to trade real money. `readiness_baseline_verdict` remains
`NOT_READY`, `holdout_retune_used` remains `false`, and `tsmom_state_changed`
remains `false`.

## Alpha Signal strict falsification protocol

`alpha-signal-strict-falsification-run` implements
`ALPHA_SIGNAL_STRICT_FALSIFICATION_PROTOCOL_V4`, a paper-only pre-registered
stricter OOS falsification protocol over the frozen V3
`next_falsification_queue.csv`. It is not new alpha generation, not strategy
optimization, not signal weight optimization, not live/testnet readiness, and
not a profitability claim. Its purpose is to *reject* hypotheses; the default
outcome is FALSIFIED.

```bash
uv run quantctl alpha-signal-strict-falsification-run
```

The command reads the existing V3 artifact directory and writes a new artifact
directory:

- `alpha_signal_strict_falsification.json`
- `preregistered_hypotheses.csv`
- `frozen_candidate_universe.jsonl`
- `strict_chronological_folds.csv`
- `signal_strict_oos_diagnostics.csv`
- `bootstrap_significance_summary.csv`
- `family_strict_falsification_summary.csv`
- `multiple_testing_fdr_summary.csv`
- `failed_gate_rollup.csv`
- `next_protocol_queue.csv`
- `report.md`
- `commands.txt`
- `changed_files.txt`

V4 pre-registers, before evaluation, every queued `(family, horizon)` hypothesis
and its expected IC sign (taken from the sign of the V3 family median test IC),
the fold construction, the purge/embargo, the bootstrap block length, the
resample count, the false-discovery alpha, and the survival thresholds. These
are written to `preregistered_hypotheses.csv` and may not be retuned after
results are seen. The protocol freezes the V3 candidate universe to the queued
families without generating new DAGs, changing `signal_id`s, or selecting by
performance.

Stricter OOS controls extend V3: purge and embargo are `2 ×` the maximum queued
horizon, and folds are chronological with purged forward-return label overlap.
For each pre-registered hypothesis the family-mean signal is correlated with the
forward-return label over the pooled test windows, and significance is assessed
with a block-permutation test: the label series is partitioned into
non-overlapping contiguous blocks whose order is randomly shuffled. The block
length equals the horizon so that within-block label autocorrelation is
preserved while signal-label alignment is destroyed; the one-sided p-value is
computed in the pre-registered direction
(two-sided when the expected sign is zero) with a deterministic seed and an
add-one correction. P-values across the pre-registered set are Benjamini-Hochberg
corrected at the pre-registered alpha, and a selection-deflated alpha additionally
accounts for the broad V2/V3 effective breadth the hypotheses were chosen from.

A hypothesis is labelled `SURVIVED_STRICT_FALSIFICATION` only when the observed
test sign matches the pre-registration, sign-consistency, fold-consistency, and
null-robustness each clear `0.60`, validation-to-test degradation stays inside
`[0.5, 2.0]`, and its bootstrap p-value is Benjamini-Hochberg significant.
Otherwise it is `FALSIFIED_SIGN_FLIP`, `FALSIFIED_OOS_INCONSISTENT`,
`FALSIFIED_NULL_FRAGILE`, `FALSIFIED_DEGRADATION`,
`FALSIFIED_NO_BOOTSTRAP_SIGNIFICANCE`, or `FALSIFIED_INSUFFICIENT_EVIDENCE`.
Surviving falsification is explicitly not alpha confirmation; survivors may only
enter the paper-only `NEEDS_EXECUTION_AWARE_PROTOCOL` queue in
`next_protocol_queue.csv`. Forbidden labels extend the V3 set with
`ALPHA_CONFIRMED` and `EDGE_PROVEN`. `readiness_baseline_verdict` remains
`NOT_READY`, `holdout_retune_used` remains `false`, and `tsmom_state_changed`
remains `false`.

## Multi-asset alpha generator strict prefilter

`multi-asset-alpha-generator-strict-prefilter-run` implements
`MULTI_ASSET_ALPHA_GENERATOR_STRICT_PREFILTER_V5`, a paper-only public
multi-asset signal factory and strict diagnostic prefilter. It is not a trading
strategy, not portfolio construction, not signal-weight optimization, not
strategy PnL optimization, not live/testnet readiness, and not a profitability
claim.

```bash
uv run quantctl multi-asset-alpha-generator-strict-prefilter-run
```

The command writes a new artifact directory:

- `multi_asset_alpha_factory.json`
- `multi_asset_panel_summary.csv`
- `signal_catalog.jsonl`
- `signal_diagnostics.csv`
- `signal_family_summary.csv`
- `signal_correlation_clusters.csv`
- `oos_prefilter_summary.csv`
- `null_baseline_comparison.csv`
- `multiple_testing_summary.csv`
- `shortlist.csv`
- `failed_gate_rollup.csv`
- `next_strict_falsification_queue.csv`
- `report.md`
- `commands.txt`
- `changed_files.txt`

V5 deliberately changes the information axis after V4 falsified the prior
single-symbol V2/V3/V4 family queue. It does not reuse V4 candidates, V4
representatives, V4 horizons, V4 thresholds, or V4 evidence. The
`--source-v4-artifact` argument is lineage/audit-only: V5 records the source V4
path and whether `next_protocol_queue.csv` is empty, then generates a new
multi-asset panel hypothesis universe independently.

The requested universe defaults to the user-declared public Binance symbols:
`BTCUSDT`, `ETHUSDT`, `SOLUSDT`, `BNBUSDT`, `XRPUSDT`, `ADAUSDT`, `DOGEUSDT`,
`AVAXUSDT`, `LINKUSDT`, and `TONUSDT`. The panel records requested, used, and
excluded symbols. Symbol inclusion is controlled by OHLCV availability,
schema validity, and minimum bar count. Missing, invalid, or unalignable public
funding never excludes an otherwise OHLCV-usable symbol; funding-dependent
families record unavailable diagnostics while OHLCV-only families still run.

Cross-sectional ranks are point-in-time. At timestamp `t`, ranks and universe
medians use only symbols with actual OHLCV rows at `t`; missing symbols are not
forward-filled and do not affect historical ranks before they have data. Every
signal is shifted by at least one decision lag before 8h/24h/72h forward-return
label comparison.

The fixed V5 family taxonomy is:

- `cross_sectional_momentum`
- `cross_sectional_reversal`
- `funding_dispersion`
- `lead_lag`
- `beta_residual`
- `volatility_dispersion`
- `liquidity_volume_shock`
- `correlation_breakdown`
- `carry_x_trend`
- `relative_value_interaction`

Null/baseline comparison covers zero/cash-null, buy-and-hold same-window,
deterministic random sign, lagged signal, inverse signal, shuffled timestamp
signal, family-permuted signal, symbol-permuted signal, universe-median signal,
and frozen TSMOM negative-control availability. Duplicate control uses
vectorized matrix correlation; absolute correlation `>= 0.95` marks redundant
signals, and representatives are selected by lowest complexity, smallest depth,
then lexicographic `signal_id`, never by performance.

Multiple-testing risk increases with the generated signal count, so V5 records
`cannot_claim_edge_due_to_multiple_testing: true`. Allowed signal labels are
paper-only research labels such as `KILL_SIGNAL`, `DUPLICATE_SIGNAL`,
`WATCH_FAMILY_ONLY`, `WATCH_DIAGNOSTIC_ONLY`, and
`NEEDS_STRICT_OOS_FALSIFICATION`. A shortlist is only a paper-only queue for a
future pre-registered stricter falsification loop. It is not alpha, edge,
profitability, readiness, execution readiness, live/testnet permission, or
permission to trade. `readiness_baseline_verdict` remains `NOT_READY`, and
`tsmom_state_changed` remains `false`.

## Public multi-asset panel coverage loop

`public-multi-asset-panel-coverage-run` implements
`PUBLIC_MULTI_ASSET_DATA_PANEL_COVERAGE_V6`, a public-data coverage and
provenance loop for the V5 multi-asset panel. It is not an alpha generator, not
a trading strategy, not portfolio construction, not signal-weight optimization,
not strategy PnL optimization, not live/testnet readiness, and not a
profitability claim.

```bash
uv run quantctl public-multi-asset-panel-coverage-run
```

The command writes a new artifact directory:

- `public_multi_asset_panel_coverage.json`
- `symbol_coverage.csv`
- `missing_data_plan.csv`
- `panel_readiness.csv`
- `provenance_manifest.csv`
- `report.md`
- `commands.txt`
- `changed_files.txt`

V6 exists because the V5 multi-asset strict prefilter changed the information
axis to a public multi-asset panel but the durable V5 artifact had only
`BTCUSDT` usable. The other requested symbols were excluded for missing cached
public OHLCV. V6 therefore audits the requested public Binance universe,
records which symbols have usable OHLCV, records public funding availability
when present, preserves V5 excluded-symbol reasons, and writes a missing public
data plan. It does not modify V5, does not revive V4-falsified families, and
does not automatically rerun V5.

The default requested universe remains:
`BTCUSDT`, `ETHUSDT`, `SOLUSDT`, `BNBUSDT`, `XRPUSDT`, `ADAUSDT`, `DOGEUSDT`,
`AVAXUSDT`, `LINKUSDT`, and `TONUSDT`. A symbol is usable only when public
OHLCV for the requested monthly range is present and validates through the
existing Binance Vision loader. Missing cache files are recorded as coverage
loss; V6 does not fabricate rows. Public funding is optional for panel
inclusion: missing or invalid funding is recorded in `symbol_coverage.csv` and
`missing_data_plan.csv`, but it does not exclude an OHLCV-usable symbol.

`panel_ready_for_v5_rerun: true` is a narrow data-readiness flag for a separate
future paper-only V5 rerun. It requires at least the configured minimum number
of OHLCV-usable symbols and a common chronological public OHLCV span. It is not
an alpha, edge, profitability, strategy, portfolio, execution, testnet, or live
readiness claim. Any later V5 shortlist remains paper-only, and
multiple-testing risk still increases with the generated signal count.

V6 uses only public/no-key Binance Vision OHLCV cache/download paths and the
public/no-key Binance USDⓈ-M funding history path when funding is enabled. It
does not read account state, API keys, orders, fills, signed endpoints, testnet,
or live data. `readiness_baseline_verdict` remains `NOT_READY`, no holdout
retune is used, no strategy optimization is performed, and
`tsmom_state_changed` remains `false`.

## Public multi-asset data backfill loop

`public-multi-asset-data-backfill-run` implements
`PUBLIC_MULTI_ASSET_DATA_BACKFILL_V7`, a public-data backfill and provenance
loop for the V6 missing data plan. It is not an alpha generator, not a trading
strategy, not portfolio construction, not signal-weight optimization, not
strategy PnL optimization, not live/testnet readiness, and not a profitability
claim.

```bash
uv run quantctl public-multi-asset-data-backfill-run
```

The command reads the immutable V6 artifact directory, especially
`missing_data_plan.csv`, and derives its default symbol universe, interval, and
target month span from that V6 source when CLI overrides are omitted. It builds
a deterministic public Binance Vision OHLCV month plan plus public Binance
funding span plan, skips existing cache files, records `skipped_existing`,
records successful and failed downloads, writes checksums/provenance where cache
files exist, and then reruns the V6 coverage audit in cache-only mode. It does
not run V5 automatically.

The command writes a new artifact directory:

- `public_multi_asset_data_backfill.json`
- `backfill_plan.csv`
- `download_manifest.csv`
- `symbol_month_coverage.csv`
- `checksum_manifest.csv`
- `failed_downloads.csv`
- `panel_readiness_after_backfill.csv`
- `missing_data_remaining.csv`
- `report.md`
- `commands.txt`
- `changed_files.txt`

If public network/downloads fail, V7 records failed rows and preserves any
successful cache files for resumable reruns. It does not fabricate OHLCV or
funding data. Rerunning without `--force` should skip existing cache files.
Raw public cache files under `data/**` are not report artifacts and should not
be committed unless repo policy explicitly allows it. V7 records created raw
cache paths for audit only, filters raw cache paths and raw `.metadata.json`
sidecars out of `changed_files.txt`, and records
`raw_data_files_committed: false`.

`panel_ready_for_v5_rerun: true` is only a data-readiness flag for a separate
future paper-only V5 rerun. It requires at least the configured minimum number
of OHLCV-usable symbols, a non-empty common public OHLCV span, enough common
bars for 8h/24h/72h labels, public provenance, and intact safety gates. It is
not an alpha, edge, profitability, strategy, portfolio, execution, testnet, or
live readiness claim. Generated V5 signals, if V5 is run later, remain
hypotheses rather than edges, and multiple-testing risk still increases with
the signal count.

## Panel-ready multi-asset alpha prefilter rerun

`panel-ready-multi-asset-alpha-prefilter-rerun` implements
`PANEL_READY_MULTI_ASSET_ALPHA_PREFILTER_RERUN_V8`, a paper-only rerun of the
existing V5 public multi-asset strict prefilter after V7 panel readiness. It is
not a new alpha generator, not a trading strategy, not portfolio construction,
not signal-weight optimization, not strategy PnL optimization, not holdout
retuning, not live/testnet readiness, and not a profitability claim.

```bash
uv run quantctl panel-ready-multi-asset-alpha-prefilter-rerun
```

The command reads the immutable V7 artifact first and refuses to run unless
`panel_ready_for_v5_rerun: true` and the V7 safety fields remain public/no-key/
no-account/no-live. It uses exactly the V7 `usable_symbol_universe_after` as the
V5 rerun universe. For the current V7 artifact that is `BTCUSDT, ETHUSDT,
SOLUSDT, BNBUSDT, XRPUSDT, ADAUSDT, DOGEUSDT, AVAXUSDT, LINKUSDT`; `TONUSDT`
stays excluded because V7 still records missing cached public OHLCV months.

V8 is cache-only. It does not download or backfill missing public files and does
not call the V7 backfill loop. If a V7-usable symbol can no longer be loaded from
the local public cache, V8 fails safely rather than fabricating data or changing
the universe. Raw public cache files under `data/**` remain non-report inputs and
should not be committed unless repo policy explicitly allows it.

The command writes a new artifact directory:

- `panel_ready_multi_asset_alpha_prefilter.json`
- `multi_asset_panel_summary.csv`
- `signal_catalog.jsonl`
- `signal_diagnostics.csv`
- `signal_family_summary.csv`
- `signal_correlation_clusters.csv`
- `oos_prefilter_summary.csv`
- `null_baseline_comparison.csv`
- `multiple_testing_summary.csv`
- `shortlist.csv`
- `failed_gate_rollup.csv`
- `next_strict_falsification_queue.csv`
- `report.md`
- `commands.txt`
- `changed_files.txt`

Generated V8 signals are hypotheses rather than edges. The shortlist is
paper-only and may only feed a future separate strict OOS falsification loop.
Multiple-testing risk remains high because thousands of signals and horizons are
diagnosed; `cannot_claim_edge_due_to_multiple_testing` remains true.
`readiness_baseline_verdict` remains `NOT_READY`, and V8 makes no claim of
alpha, edge, profitability, readiness, execution readiness, live/testnet
permission, or capital allocation permission.

## Multi-asset strict OOS FDR falsification

`multi-asset-strict-oos-fdr-falsification-run` implements
`MULTI_ASSET_STRICT_OOS_FDR_FALSIFICATION_V9`, a paper-only strict
falsification loop over the frozen V8 queue. It is not signal generation, not a
trading strategy, not portfolio construction, not signal-weight optimization,
not strategy PnL optimization, not holdout retuning, not live/testnet readiness,
and not a profitability or alpha-confirmation claim.

```bash
uv run quantctl multi-asset-strict-oos-fdr-falsification-run
```

The command reads the immutable V8 artifact first and requires
`source_v7_panel_ready: true`. It freezes `next_strict_falsification_queue.csv`
and evaluates only queued V8 families/signals by replaying V8 `signal_catalog.jsonl`
signal IDs. It does not create new alpha candidates, does not rerun V8, and does
not add families, horizons, operators, thresholds, or representatives based on
V9 outcomes.

V9 is cache-only and does not download or backfill public data. It derives the
symbol universe, interval, and month span from the V8 artifact and refuses to
fabricate missing rows. If the V8 queue is empty, V9 writes schema-only artifacts
with `suite_decision=NO_V8_QUEUE_TO_FALSIFY` and terminates without signal
generation.

Strict diagnostics include chronological train/validation/test folds, purge and
embargo at least the max fixed horizon, same-bar leakage checks, deterministic
block permutation nulls, BH-FDR q-values, null-baseline stress, regime
robustness, and symbol robustness. The family decision rule is pre-registered:
all fixed horizons `8h`, `24h`, and `72h` must clear. V9 cannot promote a family
by picking the best horizon, best IC, best hit rate, best p-value, or best
q-value. Permutation block size is pinned to `horizon_bars`, so block-size
tuning cannot be used to alter p-values after seeing results.

The command writes a new artifact directory:

- `multi_asset_strict_oos_fdr_falsification.json`
- `loop.json`
- `frozen_v8_queue.csv`
- `chronological_folds.csv`
- `signal_strict_oos_diagnostics.csv`
- `family_strict_summary.csv`
- `permutation_null_results.csv`
- `bh_fdr_summary.csv`
- `regime_robustness.csv`
- `symbol_robustness.csv`
- `null_baseline_stress.csv`
- `failed_gate_rollup.csv`
- `next_mechanism_research_queue.csv`
- `report.md`
- `commands.txt`
- `changed_files.txt`

A family that reaches `WATCH_FAMILY_ONLY` or `NEEDS_MECHANISM_RESEARCH`, if one
ever does, is still only a paper-only follow-up hypothesis. Surviving V9 does not
prove edge, alpha, profitability, readiness, execution readiness, live/testnet
permission, capital allocation permission, or permission to trade. Multiple
testing risk remains central; `cannot_claim_edge_due_to_multiple_testing` stays
true and `readiness_baseline_verdict` remains `NOT_READY`.

## V9 lineage and negative-control governance

`multi-asset-v9-falsification-lineage-governance-run` implements
`V9_FALSIFICATION_LINEAGE_AND_NEGATIVE_CONTROL_GOVERNANCE_V10`, a paper-only
governance loop over the committed V9 strict falsification artifact. It is not
signal generation, not a V8/V9 rerun, not data download/backfill, not strategy
implementation, not portfolio construction, not signal-weight optimization, and
not a profitability, edge, readiness, or live/testnet claim.

```bash
uv run quantctl multi-asset-v9-falsification-lineage-governance-run
```

The command reads the V9 artifact and the V8 artifact named by V9, computes
deterministic SHA-256 lineage manifests for both artifact directories, validates
the committed V9 kill-state invariants, and records V9-falsified families as
negative controls. It does not modify V8 or V9 artifacts. It does not read
private/account/API-key/order/fill data and does not touch public data loaders.

V10 writes a new artifact directory:

- `v9_falsification_lineage_governance.json`
- `loop.json`
- `v8_lineage_manifest.csv`
- `v9_lineage_manifest.csv`
- `committed_artifact_invariants.csv`
- `negative_control_registry.csv`
- `disallowed_revival_registry.csv`
- `post_mortem.md`
- `report.md`
- `commands.txt`
- `changed_files.txt`

If V9 killed every queued family and the next mechanism queue is empty, V10
locks that evidence as governance state. Killed V9 families cannot be revived by
horizon cherry-picking, p-value picking, representative changes, or threshold
changes. Future research must start from a new pre-registered mechanism with a
declared edge source, counterparty, data requirement, validation rule, and
failure mode before metrics are inspected.

## Public derivatives microstructure data axis

`public-derivatives-microstructure-data-axis-run` implements
`PUBLIC_DERIVATIVES_MICROSTRUCTURE_DATA_AXIS_V11`, a public derivatives
microstructure data coverage and provenance loop. It is not alpha generation,
not a V8/V9/V10 rerun, not data-derived signal construction, not strategy
implementation, not portfolio construction, not signal-weight optimization, and
not a profitability, edge, readiness, or live/testnet claim.

```bash
uv run quantctl public-derivatives-microstructure-data-axis-run
```

The command reads the immutable V10 governance artifact and carries forward the
V9 kill state, empty next queue, negative-control families, disallowed revival
entries, and lineage hashes. It audits existing public no-key cache/provenance
for new derivatives microstructure families: open interest, long/short ratio,
taker buy/sell volume ratio, liquidation or force-order events, mark/index/basis
proxies, funding-rate extensions, and public depth proxies where a safe existing
cache exists. Missing or unstable sources are recorded as missing plan rows
rather than fabricated data.

V11 writes a new artifact directory:

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

The `readiness_by_family.csv` filename is retained because the loop contract
requires it, but the file is only a future-V12 data coverage gate. It is not
strategy readiness, alpha confirmation, execution readiness, or permission to
trade. V11 does not run V12 automatically; V12 should only run after
`coverage_sufficient_for_future_v12=true`.

Raw public data cache files should not be committed unless repo policy
explicitly allows it. The V11 artifact records created/committed raw data status
and filters raw cache paths out of `changed_files.txt`. Existing V10/V9/V8
artifacts are read-only inputs and must not be modified.

## Risk-adjusted research run

`risk-run` is separate from `real-run` so the fixed baseline stays comparable. It uses the same canonical public Binance Vision data and writes `reports/risk-run-*` with:

- chronological train / validation / holdout split metadata,
- small predeclared candidate grid,
- product-family-aware direction rules (`spot_crypto` candidates are long-only),
- frozen `risk_adjusted_score_v1`,
- `candidate_count`, all candidate train/validation rows, selected settings, and `holdout_used_for_selection:false`,
- train, validation, and holdout/paper Sharpe and max drawdown (MDD),
- regime protocol artifacts (`regime.csv`, `regime.json`, `metrics.json.regime_summary`),
- alpha signal factory artifacts (`signals.csv`, `signals.json`, `metrics.json.signal_summary`),
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

## Destructive readiness validation

`readiness-run` is the deliberately adversarial validation layer for the current Regime Protocol + Alpha Signal Factory stack. It does not try to present a good single-period result. It tries to break the claim before any live expansion by writing `reports/readiness-run-*` with:

- walk-forward train / validation / holdout windows;
- OOS holdout metrics after candidate selection;
- cash-null and buy-and-hold baselines;
- stress commission, spread, dynamic OHLCV execution-cost proxy, and public/no-key Binance USDⓈ-M funding-history drag when available;
- regime split of holdout trades;
- signal-family ablation using 24-bar forward-return IC;
- duplicate/correlation risk across signal families;
- selected-candidate switch-rate as an overfit/data-snooping warning;
- prefix-invariance lookahead probes for regime, signal, and ablation-prefix rows;
- permanent live blockers and no-profit/no-live disclaimers.

Frozen readiness artifacts:

- `readiness.json` — schema version, method, config, compute profile, validation-depth gates, summaries, blockers, and claim boundary.
- `readiness_windows.csv` — exact per-window train/validation/holdout, selected candidate, baseline, stress, funding, and pass/fail fields.
- `readiness_ablation.csv` — signal-family IC and drop-one-family deltas.
- `readiness_regime.csv` — holdout trade PnL grouped by causal entry-time regime.
- `report.md` — human-readable readiness report: what passed, what failed, and what is still required before real trading.

`readiness.json.schema_version == 2` freezes the v2 top-level keys and CSV schemas.
The v2 migration adds `compute_profile` and `validation_depth` so fast runs are
explicitly classified as smoke/prototype/research-grade by workload depth rather
than by wall-clock time. `elapsed_seconds` is diagnostic only. Breaking changes
require another schema-version bump and migration note.

The readiness verdict enum is intentionally limited:

```text
NOT_READY
CONDITIONALLY_READY_FOR_PAPER_ONLY
BLOCKED
```

There is no live-ready verdict. A CLI exit code of `0` only means validation artifacts were written. It does not mean the strategy is profitable, robust, or authorized for live trading.

Stress math is explicit:

```text
slippage_drag_pct = closed_trades * 2 * dynamic_slippage_proxy_bps / 100
funding_drag_pct = exposure_time_pct / 100 * bars / 8 * funding_rate_per_8h * 100
```

By default `readiness-run` downloads public Binance USDⓈ-M funding-rate history through the no-key `/fapi/v1/fundingRate` market-data endpoint and records source URL, docs URL, row count, date range, normalized hash, and the per-window funding rate used for drag. This closes the prior “funding sensitivity only” report issue for public exchange funding history, but it still does not validate account-specific funding cashflows. `--funding-source sensitivity` keeps the older sensitivity-only mode explicit.

Slippage is still a conservative post-metric proxy. The dynamic proxy combines a base bps assumption, median OHLCV high-low range, and closed-trade count. It is not exchange matching, not order-book simulation, and not live-fill proof.

The readiness candidate set includes the fixed SMA/ATR bracket candidates plus a predeclared Alpha Signal Factory + Regime Protocol paper-strategy adapter. Candidate selection remains train/validation-only; holdout rows are not used for tuning. If the median stress-vs-cash gate still fails, the correct output remains `NOT_READY` rather than retuning the same run until it passes.

## Live-market forward paper run

`forward-paper-run` is a public-data forward paper workflow for observing the current system without submitting orders:

```bash
uv run quantctl forward-paper-run --lookback-bars 720
```

`--lookback-bars` means saved output bars. With the default closed-bar filter, `720`
therefore writes 720 fully closed `1h` bars, roughly one month; the exchange request
uses one extra kline to absorb the latest still-open bar.

It differs from `real-run`/`risk-run`/`readiness-run`:

- Data source: recent public Binance USDⓈ-M Futures REST klines, not Binance Vision monthly archives.
- Default symbol/timeframe: `BTCUSDT` / `1h`.
- Default funding: public Binance funding history over the same window; if unavailable, the command fails rather than silently assuming zero. `--funding-source none` is an explicit operator choice and is written to the report.
- Default bar handling: the latest still-open kline is dropped. `--include-open-bar` is explicit and recorded in `manifest.json`.
- Default lookback: 720 closed `1h` bars, not 719 bars after dropping an open candle.
- Strategy adapter: diagnostic signal rows are converted by a separate paper adapter, not by the live order-intent path.

The exact v1 exposure formula is fixed:

```text
raw_score = 0.45 * direction_score + 0.25 * entry_score + 0.20 * risk_score - 0.10 * max(exit_score, 0)
target_exposure = long/flat threshold result scaled by regime risk_multiplier
applied_exposure[t] = target_exposure[t-1]
```

Costs are bar-level accounting:

```text
fee_cost = abs(delta_exposure) * equity_after_gross * fee_rate
slippage_cost = abs(delta_exposure) * equity_after_gross * slippage_bps / 10000
funding_cost = equity_after_gross * applied_exposure * sum(funding_rate events in (prev_bar, current_bar])
```

Positive funding reduces a long paper position; negative funding increases it. This is not account-specific funding cashflow validation.

Success artifacts under `reports/forward-paper-run-*`:

- `manifest.json`: public source URL/docs URL, request params, closed-bar flag, row counts, normalized OHLCV hash, funding metadata, no-order/no-key booleans.
- `positions.csv`: per-bar score, regime, target/applied exposure, costs, equity, drawdown.
- `equity.csv`: compact equity/cost curve.
- `trades.csv`: closed/open paper trade audit rows.
- `regime.csv/json` and `signals.csv/json`: same diagnostic artifacts used by the existing research reports.
- `report.md`: human-readable paper report and limitations.

This command is meant for forward paper observation. It does not submit testnet or live orders, does not read Binance credentials, does not use signed/account endpoints, does not prove exchange fills, does not prove profitability, and does not promote the readiness verdict.

## Current executed result

Successful run directory:

- `reports/real-run-20260624T165402Z/`
- `reports/real-run-20260624T220621Z/` (regression re-run after adding risk-run)
- `reports/risk-run-20260624T220620Z/`
- `reports/readiness-run-20260625T053627Z/`

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

Latest readiness-run result for the same canonical window:

- Verdict: `NOT_READY`
- Walk-forward windows: 5
- Candidate count: 8
- Candidate-window evaluations: 40
- Validation depth grade: `prototype`
- Research-grade workload: false
- Failed gates: `median_stress_return_beats_cash_null`, `max_selected_candidate_switch_rate`, `validation_depth_research_grade`
- Median base holdout return: -0.2821323674864984%
- Median stress/funding/slippage-adjusted holdout return: -1.359232104497523%
- Funding source: public Binance USDⓈ-M funding history, 546 observed rows
- Execution proxy: OHLCV range + volume participation + trade count; not live-fill proof
- Lookahead probe: passed
- Duplicate high-correlation family pairs: 3
- Live orders submitted: false

This is an evidence package, not a success claim. The stack survived artifact generation and causal checks, but failed the median stress-return-vs-cash gate, the selected-candidate stability gate, and the research-grade validation-depth gate. More predeclared paper-only research on untouched OOS data is required before any live trading discussion.

Initial failed diagnostic preserved:

- `reports/real-run-20260624T163947Z/`

That failed run is intentionally retained because the default integer-unit `Backtest` path could not trade BTCUSDT with 10,000 USDT cash and produced zero closed trades. The accepted fix was to use `FractionalBacktest` without changing the canonical strategy parameters or data window.

## Credential boundary

`real-run`, `risk-run`, and `readiness-run` do not read `BINANCE_API_KEY` or `BINANCE_API_SECRET`, do not import `ccxt` in their command paths, and perform no Binance API/demo/testnet/order calls. They only download public Binance Vision zip files and write local reports.

Creating Binance API keys, logging into Binance demo/testnet pages, or submitting testnet/live orders remains manual and credential-gated. Any future submit path must require explicit `allow_submit=True`, credentials, sandbox mode, the exact live-risk acknowledgment, product policy, kill-switch checks, and `LiveTradingGuard` approval.
