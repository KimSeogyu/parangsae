# Parangsae Quant Constitution

This `AGENTS.md` is the repo-local constitution for agents working in `parangsae`. It is intentionally self-contained. Do not move the Loop Engineering rules into a separate framework document unless the user explicitly asks to change this constitution.

Global system/developer instructions still take precedence. Within this repository, this file is the source of truth for quant research, data, backtesting, strategy changes, result interpretation, and live/testnet boundaries.

## 1. Project identity

Parangsae is a **paper/dry-run-first Binance quant research app**.

The project exists to build a rigorous research and validation system, not to make unsupported profit claims. The latest committed readiness artifact is `NOT_READY`, so no live-readiness claim is established.

Current baseline:

```text
reports/readiness-run-20260625T053627Z/
verdict: NOT_READY
validation_depth: prototype
walk-forward windows: 5
candidate count: 8
candidate-window evaluations: 40
failed_gates:
  - median_stress_return_beats_cash_null
  - max_selected_candidate_switch_rate
  - validation_depth_research_grade
```

Treat this as the default comparison point unless a newer committed readiness artifact is explicitly named. Ignore untracked intermediate report directories unless the task explicitly targets them.

## 2. Non-negotiable boundaries

These rules dominate performance goals.

- Do **not** submit live orders during research, backtesting, readiness, or forward-paper work.
- Do **not** use API keys, balances, account state, signed endpoints, live fills, or live orders for strategy-performance claims.
- Do **not** use holdout, current forward-paper windows, testnet/live outcomes, or account state for parameter selection.
- Do **not** describe paper, backtest, or testnet success as live readiness.
- Do **not** turn diagnostic labels such as `signal_summary.latest.decision` into order intents.
- Preserve kill-switch behavior, live acknowledgements, risk approvals, and product-family blockers.
- Failed reports are evidence. Do not delete or hide them merely to make results look cleaner.

If a user requests live/testnet execution, separate the request into:

1. strategy-performance research, which stays paper/no-key; and
2. execution plumbing validation, where testnet may be useful for mechanics only.

## 3. Operating mantra

```text
Hypothesis first.
Small change.
Harsh validation.
Preserve failures.
Promote only on net OOS evidence.
Never confuse paper success with live readiness.
```

## 4. Core vocabulary

Use these meanings consistently.

- **Signal**: a causal feature or diagnostic derived from data available at a timestamp.
- **Strategy**: a complete rule that maps causal data to positions, sizing, entries, exits, and risk controls.
- **Regime Protocol**: risk/permission layer describing market state; it is not alpha proof by itself.
- **Alpha Signal Factory**: research signal library; signal count is not strategy quality.
- **Adapter**: code that turns research diagnostics into paper positions for falsification. It is not an order path.
- **Backtest**: historical simulation for falsification, not proof of future profit.
- **Forward paper**: current/recent public-data paper observation, not validation and not live readiness.
- **Testnet**: execution-mechanics environment, not strategy-performance evidence.
- **Live readiness**: a separate claim requiring execution validation and all safety gates; research success alone never grants it.

## 5. What counts as a good strategy

A good strategy is not the one with the highest single-period backtest return. A good strategy is one that is better than a declared baseline under the same data rights, costs, risk limits, and validation protocol.

Minimum definition:

```text
Better(A, B) =
  OOS_net_utility(A) > OOS_net_utility(B)
  and data_snooping_risk(A) is controlled
  and cost/slippage/funding stress survives
  and regime split has no hidden fatal failure
  and turnover/capacity/execution risk is acceptable
  and research did not use live/testnet/order/account paths for selection
```

A candidate strategy should be:

- **falsifiable** — written hypothesis, baseline, data scope, parameter grid, and expected failure modes exist before holdout;
- **causal** — it uses only information knowable at the decision timestamp;
- **net-of-cost** — evaluated after fees, spread, slippage, funding, turnover, and stress assumptions;
- **OOS-robust** — survives chronological train/validation/holdout and walk-forward windows;
- **risk-aware** — judged against drawdown, tail loss, volatility, exposure, and capital efficiency;
- **regime-aware** — does not hide catastrophic failure inside one regime;
- **non-duplicative** — adds information beyond already-correlated signal families;
- **stable** — does not require unstable candidate switching or fragile parameters;
- **implementable** — respects liquidity, order size, capacity, and execution constraints;
- **safe by default** — never converts research evidence into live authorization.

Recommended scoring direction:

```text
strategy_score =
  + OOS net return contribution
  + OOS Sharpe / Sortino after costs
  - max drawdown penalty
  - CVaR / tail-loss penalty
  - turnover penalty
  - funding drag penalty
  - slippage/spread stress penalty
  - parameter instability penalty
  - regime concentration penalty
  - data-snooping / overfit penalty
  + diversification benefit
```

## 6. Current research thesis

The preferred next research family is:

```text
TSMOM_VOL_TARGET_FUNDING_AWARE_V1
```

Meaning:

- time-series momentum / trend following first;
- realized-volatility target sizing;
- Regime Protocol risk multiplier;
- funding/OI-aware throttle where data exists;
- mean reversion only as secondary and regime-gated;
- validation by walk-forward, OOS, cost/slippage/funding stress, ablation, regime split, and anti-overfit checks.

This is a research direction, not a proven profitable strategy. Do not expand the Alpha Signal Factory merely to add more signals. Add or change signals only when the loop hypothesis states why the new information is non-duplicative and how it will be falsified.

## 7. Data governance

Treat data as part of the strategy. Bad data invalidates good code.

### Required metadata

Every durable research report must record:

- source name;
- endpoint, URL, or download path;
- symbol/product family;
- timeframe;
- timezone;
- date range;
- row count;
- content hash or normalized hash where practical;
- whether bars are closed or open;
- funding source and coverage when funding is used.

### Raw and derived data

- Keep raw data immutable when practical.
- Write cleaned/derived data as separate artifacts.
- Never silently mutate raw downloaded data to make a strategy pass.
- Preserve enough metadata to reproduce the dataset or explain why it cannot be reproduced.

### Timestamp and bar rules

- Use UTC timestamps by default.
- Use closed bars for canonical validation.
- If open bars are included, label them explicitly and do not use that run for canonical readiness claims.
- Validate monotonic sorting and duplicate timestamps.
- Detect missing bars and gaps; either fail clearly or record the gap policy.

### Causal alignment

- Align funding, volume, regime, signal, and macro/event data to the first bar where the information was knowable.
- Never forward-fill future information into past decisions.
- Rolling features must not use future bars.
- Prefix-invariance/lookahead checks are required when changing feature, regime, signal, or report-generation logic.

### New data source checklist

When adding a data source, add tests for:

- schema and required columns;
- timezone handling;
- sorting and duplicate timestamps;
- missing or partial ranges;
- closed/open bar semantics;
- causal alignment;
- failure behavior when the source is unavailable;
- metadata and hash recording.

Public no-key market data may support strategy research. Account-specific fills, fees, balances, and funding cashflows require separate execution validation and must not be mixed into research selection.

## 8. Backtesting and validation method

Backtests are falsification tools. They are not proof of future profit.

### Pre-registration

Before running holdout or readiness validation, write down:

- strategy family and stable candidate ID;
- hypothesis in one falsifiable sentence;
- baseline artifact;
- data scope;
- parameter grid;
- selection metric;
- expected failure modes;
- excluded inputs: holdout, live orders, API keys, account state.

### Split discipline

- Use chronological splits.
- Train and validation may select candidates.
- Holdout evaluates once after selection.
- Prefer walk-forward windows over a single split.
- Never retune on holdout until it passes.

### Baselines

Compare against:

- cash/null return;
- buy-and-hold where applicable;
- current committed strategy/readiness baseline;
- simpler strategy family if the new one is more complex.

A complex strategy that cannot beat a simple baseline net of costs should be parked or killed.

### Required costs and stresses

Include, at minimum:

- commission/fees;
- spread assumption;
- slippage proxy;
- funding drag for derivatives/perps when applicable;
- turnover;
- order-size or participation-impact proxy when relevant;
- stress scenario harsher than the base assumption.

### Required outputs

A serious backtest/readiness report should include:

- gross and net return;
- Sharpe/Sortino or equivalent risk-adjusted metric;
- max drawdown;
- tail-risk proxy such as CVaR when available;
- trade count and turnover;
- exposure time;
- baseline comparisons;
- regime split;
- ablation/duplicate-signal diagnostics;
- failed gates;
- selection boundary declaration;
- claim boundary declaration.

### Readiness verdict semantics

`readiness-run` CLI success means artifacts were written. It does **not** mean the strategy passed.

Allowed readiness verdicts:

```text
BLOCKED
NOT_READY
CONDITIONALLY_READY_FOR_PAPER_ONLY
```

No readiness report may return a live-readiness verdict.

## 9. Research-grade evidence gates

A run is not research-grade because it finished quickly or produced a nice report. Readiness schema v2 uses workload depth.

Current research-grade thresholds:

| Gate | Threshold |
| --- | ---: |
| total input bars | `>= 17,520` |
| walk-forward windows | `>= 12` |
| candidate count | `>= 25` |
| candidate-window evaluations | `>= 300` |
| estimated bars processed | `>= 500,000` |

Elapsed seconds are diagnostic only. Fast vectorized research is acceptable; shallow evidence remains smoke/prototype.

Depth interpretation:

- **smoke**: plumbing evidence only;
- **prototype**: useful development evidence, not research-grade;
- **research_grade**: enough workload depth to begin serious comparison, still not live proof.

## 10. Strategy addition and modification rules

When adding or modifying a strategy:

1. Create or update a loop artifact using the inline artifact schema in this file.
2. Give the candidate a stable strategy ID and version.
3. State the hypothesis in one falsifiable sentence.
4. Declare the baseline, data scope, and parameter grid before holdout.
5. Add causal feature tests and boundary tests before trusting metrics.
6. Integrate through research adapters/candidate grids, not live/order paths.
7. Run ablation or duplicate-family checks when adding signals.
8. Update docs/report schemas when output semantics change.
9. Preserve failure evidence.
10. Promote, iterate, park, or kill based on gates, not intuition.

### Strategy ID convention

Use explicit IDs that encode family and version, for example:

```text
TSMOM_VOL_TARGET_FUNDING_AWARE_V1
DONCHIAN_TREND_RISK_V1
REGIME_MEAN_REVERSION_LOWVOL_V1
```

Do not reuse a strategy ID for materially different logic. Bump the version.

### What must not happen

- Do not wire diagnostic labels directly into execution.
- Do not add dozens of near-duplicate signals without duplicate-risk checks.
- Do not hide complexity inside a candidate after seeing holdout.
- Do not call lower drawdown an improvement if the strategy simply stopped trading without an explicit capital-preservation hypothesis.

## 11. Result interpretation protocol

Interpret reports in this order:

1. **Verdict first** — `BLOCKED`, `NOT_READY`, or `CONDITIONALLY_READY_FOR_PAPER_ONLY` outranks attractive metrics.
2. **Data scope second** — check source, range, symbol, timeframe, row count, funding source, and closed/open bar status.
3. **Validation depth third** — smoke/prototype evidence cannot justify research-grade claims.
4. **Selection boundary fourth** — confirm holdout, live orders, API keys, and account state were excluded from tuning.
5. **Net metrics fifth** — prefer cost/slippage/funding-adjusted OOS and stress metrics over raw return.
6. **Baseline comparison sixth** — compare against cash, buy-and-hold, and current committed baseline.
7. **Failure gates seventh** — failed gates are the next work queue, not cosmetic warnings.
8. **Regime and ablation eighth** — check whether performance depends on one regime or one duplicated signal family.
9. **Execution boundary last** — forward paper is observation, testnet is mechanics, and neither proves live performance.

Every final interpretation must answer:

```text
What passed?
What failed?
What evidence depth was used?
What cannot be claimed yet?
What is the next loop?
```

### Failed-gate action map

- `min_windows` → data-depth loop, not strategy celebration.
- `validation_depth_research_grade` → more data/windows/candidates/evaluations before research-grade claims.
- `median_stress_return_beats_cash_null` → strategy/risk/cost loop; do not retune holdout.
- `max_selected_candidate_switch_rate` → simplify grid or improve parameter stability.
- `max_high_corr_family_pairs` → deduplicate signal families.
- `min_regime_labels` → expand data or improve regime coverage.
- `lookahead_checks` → stop promotion; fix causality bug first.
- `min_holdout_trades_each` → inspect whether the strategy is inactive, overfiltered, or data is insufficient.

## 12. Loop Engineering process

Use separate loops so failures are attributable.

### Loop types

#### Data loop

Purpose: improve data authority and coverage.

Examples:

- extend BTCUSDT history beyond prototype depth;
- add ETH/SOL for cross-sectional tests;
- add public funding history with hashes and source ranges;
- add order-book snapshots only when execution realism is being tested.

Pass condition:

- data source, date range, row count, hash, and schema are recorded;
- no future leakage or open-bar leakage;
- tests cover missing/partial data behavior.

#### Strategy loop

Purpose: test one pre-registered alpha hypothesis.

Examples:

- volatility-targeted trend-following;
- Donchian breakout vs MA slope;
- funding-aware exposure throttle;
- regime-specific trend permission.

Pass condition:

- hypothesis and parameter grid are fixed before holdout;
- train/validation selects; holdout evaluates once;
- baseline/cash/buy-hold comparison is recorded;
- failures are preserved.

#### Risk loop

Purpose: improve sizing and drawdown behavior without hiding alpha failure.

Examples:

- volatility target;
- drawdown throttle;
- regime risk multiplier;
- max turnover guard;
- exposure cap under high funding or high volatility.

Pass condition:

- risk improvement is visible net of costs;
- lower drawdown is not achieved only by never trading;
- return/risk tradeoff is explicit.

#### Cost and execution-realism loop

Purpose: make paper results harder to fake.

Examples:

- dynamic slippage proxy;
- spread/funding stress;
- participation-impact estimate;
- order-size/capacity sensitivity.

Pass condition:

- stress assumptions are explicit;
- default report gets harsher, not more optimistic;
- no live-fill claim unless actual execution validation exists.

#### Validation loop

Purpose: strengthen the falsification harness.

Examples:

- PBO / Deflated Sharpe / SPA-style checks;
- expanded walk-forward grid;
- ablation matrix;
- regime-specific failure gates;
- baseline library.

Pass condition:

- harder validation can fail existing strategies;
- schema version and report contract are frozen or migrated explicitly;
- tests prove failure gates trigger.

#### Forward-paper observation loop

Purpose: observe current market behavior safely.

Examples:

- daily/weekly `forward-paper-run`;
- compare recent paper behavior with historical readiness results;
- record drift in funding, volatility, turnover, and regime mix.

Pass condition:

- no orders submitted;
- current period is not used to retune the same candidate;
- observation report says what changed and whether it invalidates assumptions.

#### Execution loop

Purpose: validate order mechanics after research evidence is credible.

Examples:

- testnet order creation/cancel/reject handling;
- partial-fill handling;
- position reconciliation;
- kill-switch under active order conditions.

Pass condition:

- mechanics work under expected failure cases;
- logs prove no accidental live order path;
- execution evidence is not reported as strategy performance.

### Standard loop state machine

```text
1. PROPOSE
   Pre-register hypothesis, baseline, data, grid, metrics, and expected failure modes.

2. IMPLEMENT_MINIMAL
   Make the smallest code/data/report change that tests the hypothesis.

3. LOCAL_VERIFY
   Run targeted tests, schema checks, boundary checks, and lint when applicable.

4. EVIDENCE_RUN
   Run the smallest meaningful harness first; run readiness-run when justified.

5. ADVERSARIAL_REVIEW
   Ask how the result is cheating, leaking, overfitting, duplicating signals, or ignoring costs.

6. DECIDE
   PROMOTE_TO_NEXT_LOOP, ITERATE_ON_SAME_HYPOTHESIS, PARK, or KILL.

7. JOURNAL
   Save evidence, commands, changed files, failed gates, and next hypothesis.
```

## 13. Decision rules

### PROMOTE_TO_NEXT_LOOP

Allowed only when:

- safety gates remain intact;
- evidence artifacts are durable;
- the candidate beats the declared baseline under net-of-cost OOS/stress evidence;
- lookahead/prefix-invariance checks pass;
- regime split has no hidden fatal failure;
- duplicate-signal risk is acceptable;
- candidate stability is acceptable;
- the next loop is stricter or orthogonal, not looser.

### ITERATE_ON_SAME_HYPOTHESIS

Allowed only when:

- the failure is implementation/data-quality related;
- holdout has not been used for tuning;
- the next change is pre-registered, narrow, and auditable.

### PARK

Use when:

- the idea is plausible but data depth is insufficient;
- required external data is missing;
- execution realism is not adequate;
- the idea needs a portfolio universe that does not exist yet.

### KILL

Use when:

- the idea fails after stress/OOS/regime split;
- performance depends on one duplicated signal family;
- selected candidates switch too often;
- costs/funding erase the edge;
- adding complexity only hides the same failure.

## 14. Loop artifact format

Each loop should leave machine-readable and human-readable evidence.

Suggested path:

```text
reports/loops/<YYYYMMDD>-<loop-slug>/
```

Required files:

```text
loop.json
report.md
commands.txt
changed_files.txt
```

Minimum `loop.json` schema:

```json
{
  "loop_id": "YYYYMMDD-short-slug",
  "status": "PROPOSED | IMPLEMENTED | PROMOTED | PARKED | KILLED",
  "loop_type": "data | strategy | risk | cost_execution | validation | forward_paper | execution",
  "hypothesis": "One falsifiable sentence.",
  "baseline": "reports/readiness-run-20260625T053627Z/ or newer committed artifact",
  "strategy_family": "TSMOM_VOL_TARGET_FUNDING_AWARE_V1",
  "data_scope": "symbol/timeframe/source/date range",
  "selection_inputs": ["train", "validation"],
  "selection_excludes": ["holdout", "live_orders", "api_keys", "account_state"],
  "pre_registered_grid": {},
  "expected_failure_modes": [],
  "safety_gates": {
    "live_orders_submitted": false,
    "api_keys_required_for_research": false,
    "holdout_used_for_selection": false,
    "profit_or_live_ready_claim": false
  },
  "validation_commands": [],
  "evidence_artifacts": [],
  "failed_gates": [],
  "decision": "PROMOTE_TO_NEXT_LOOP | ITERATE_ON_SAME_HYPOTHESIS | PARK | KILL | undecided",
  "next_loop": ""
}
```

Minimum `report.md` structure:

```text
# Quant Loop Report: <loop_id>

## Hypothesis
## Baseline
## Change
## Data and selection boundary
## Commands
## Evidence
## Result interpretation
## Decision
## Why
## Next loop
```

## 15. Default loop order from current baseline

Because the committed baseline is still `NOT_READY`, prioritize:

1. **Data-depth loop** — collect enough public historical data to satisfy research-grade depth.
2. **Strategy loop** — implement a narrow `TSMOM_VOL_TARGET_FUNDING_AWARE_V1` candidate grid.
3. **Risk loop** — add volatility target and drawdown/funding throttles.
4. **Validation loop** — add overfit penalties and stronger candidate-stability reporting.
5. **Forward-paper loop** — observe current market behavior without tuning or orders.
6. **Execution loop** — only after paper research is credible, use testnet for order mechanics.

## 16. Commands, environment, and validation

uv is mandatory for this project. Do not silently fall back to raw `python`, virtualenv executables, or system package tools for normal project workflows. If `uv` is missing, install it first or report the environment as blocked.

Required environment checks:

```bash
uv --version
uv sync --dev
```

Use the uv-managed console entry point (`uv run quantctl ...`) as the standard. The project is packaged in editable form through `pyproject.toml`, so the entry point should be available after `uv sync --dev`.

Common command forms:

```bash
# CLI help
uv run quantctl --help

# Smoke and paper workflows
uv run quantctl backtest --fixture --strategy trend-risk
uv run quantctl paper-run --fixture --ticks 10 --event-log logs/paper-events.jsonl
uv run quantctl replay --event-log logs/paper-events.jsonl
uv run quantctl live-dry-run --fixture

# Research workflows
uv run quantctl real-run
uv run quantctl risk-run
uv run quantctl readiness-run
uv run quantctl forward-paper-run --lookback-bars 720

# Verification
uv run pytest -q
uv run ruff check .
```

For dependency changes, update `pyproject.toml`/`uv.lock` through `uv`, then run `uv sync --dev`. For small code changes, run targeted tests first, then broader checks when practical. Do not claim completion without fresh validation evidence or an explicit validation gap.

## 17. Testnet and live trading boundary

Testnet is useful for:

- order payload plumbing;
- create/cancel flow;
- reject handling;
- partial fills;
- reconciliation;
- kill-switch behavior under order conditions.

Testnet is not useful for proving strategy performance because liquidity, matching, participants, slippage, and incentives differ from live markets.

Live trading requires every live safety gate, including:

- CLI mode is `live`;
- `execution.live_enabled: true`;
- explicit operator acknowledgement environment variable;
- required Binance API credentials in runtime environment;
- inactive kill-switch;
- live-allowed product family;
- risk approval for the exact order intent;
- separate execution-readiness evidence.

Even if all of those pass, this repo’s research reports still do not guarantee profit.

## 18. Documentation and schema responsibility

When changing strategy or validation behavior, update relevant source-of-truth docs and schemas:

- `AGENTS.md` for project operating rules;
- `README.md` for user-facing overview;
- `docs/backtesting-paper-trading.md` for backtesting/report details when needed;
- generated report schema notes when output contracts change.

Schema-breaking changes must bump the relevant schema version and include a migration note. Report consumers should not have to guess whether field semantics changed.

## 19. Constitution change control

Because this file is the project constitution:

- Keep it self-contained.
- Do not weaken safety boundaries without explicit user instruction and a recorded rationale.
- Do not remove failed-baseline context merely because it is inconvenient.
- Prefer adding precise rules over vague aspirations.
- When this file changes, verify it for broken references and obvious contradictions.
- If this file conflicts with code behavior, either fix the code or explicitly document the gap.

## 20. Final response contract for agents

For quant-related work, final responses should be evidence-based and concise:

```text
Mode / loop:
Changed files:
Validation run:
What passed:
What failed or remains unclaimed:
Next loop:
```

Do not end with “ready” unless the relevant gate actually says ready, and specify ready for what: smoke, prototype, paper-only, execution-mechanics, or live.

## 21. Anti-patterns to reject

Reject these even if they make the result look better:

- Retuning on holdout until it passes.
- Calling a one-month forward-paper run strategy validation.
- Adding many signals without a falsifiable non-duplication claim.
- Reporting raw return while ignoring fees, spread, slippage, funding, turnover, capacity, or regime split.
- Treating testnet fills as performance evidence.
- Deleting failed reports to improve optics.
- Promoting because one candidate worked in one period.
- Hiding a failed gate in prose.
- Turning research diagnostics into live order instructions.
- Calling anything live-ready without separate execution-readiness evidence and live safety gates.
