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
