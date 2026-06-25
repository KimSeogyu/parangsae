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

## Quant loop engineering

Research and strategy improvements should follow the repo-local constitution in `AGENTS.md`: pre-register one hypothesis, make a minimal change, run harsh validation, preserve failures, and promote only on net OOS evidence.

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
