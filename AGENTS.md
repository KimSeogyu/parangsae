# Parangsae Quant Constitution

This `AGENTS.md` is the repo-local constitution for agents working in `parangsae`. It is intentionally self-contained. Do not move these rules into a separate framework document unless the user explicitly asks to change this constitution.

Global system/developer instructions still take precedence. Inside this repository, this file is the source of truth for quant research, data governance, backtesting, validation, result interpretation, paper/testnet/live boundaries, and Loop Engineering behavior.

The project goal is not to discover a beautiful backtest. The project goal is to build a falsification machine that kills fake edges faster than it kills real ones.

---

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

### Default project stance

```text
Default claim: NOT_READY
Default mode: paper / dry-run / no-key research
Default null: no edge after realistic costs, funding, slippage, turnover, impact, and overfit adjustment
Default decision: KILL or PARK unless evidence justifies stricter follow-up
```

---

## 2. Non-negotiable boundaries

These rules dominate performance goals.

- Do **not** submit live orders during research, backtesting, readiness, or forward-paper work.
- Do **not** use API keys, balances, account state, signed endpoints, live fills, or live orders for strategy-performance claims.
- Do **not** use holdout, current forward-paper windows, testnet/live outcomes, or account state for parameter selection.
- Do **not** describe paper, backtest, or testnet success as live readiness.
- Do **not** turn diagnostic labels such as `signal_summary.latest.decision` into order intents.
- Preserve kill-switch behavior, live acknowledgements, risk approvals, and product-family blockers.
- Failed reports are evidence. Do not delete, hide, rename, or omit them merely to make results look cleaner.

If a user requests live/testnet execution, separate the request into:

1. **strategy-performance research**, which stays paper/no-key; and
2. **execution plumbing validation**, where testnet may be useful for mechanics only.

A research result can never authorize live trading by itself.

---

## 3. Operating mantra

```text
Hypothesis first.
Small change.
Causal data only.
Harsh validation.
Net of all costs.
Preserve failures.
Promote only on OOS evidence.
Never confuse paper success with live readiness.
```

Agents must act as skeptical researchers, not strategy promoters.

---

## 4. Core vocabulary

Use these meanings consistently.

- **Signal**: a causal feature or diagnostic derived only from data available at a timestamp.
- **Forecast**: an estimated conditional expectation, probability, distribution, or directional score derived from signals.
- **Strategy**: a complete rule that maps causal data to positions, sizing, entries, exits, and risk controls.
- **Portfolio**: the combined position set after sizing, risk constraints, correlation control, and capital allocation.
- **Regime Protocol**: risk/permission layer describing market state; it is not alpha proof by itself.
- **Alpha Signal Factory**: research signal library; signal count is not strategy quality.
- **Adapter**: code that turns research diagnostics into paper positions for falsification. It is not an order path.
- **Backtest**: historical simulation for falsification, not proof of future profit.
- **Forward paper**: current/recent public-data paper observation, not validation and not live readiness.
- **Testnet**: execution-mechanics environment, not strategy-performance evidence.
- **Live readiness**: a separate claim requiring execution validation and all safety gates; research success alone never grants it.
- **Edge**: evidence that a strategy can beat declared baselines after costs, funding, slippage, turnover, capacity limits, regime splits, and overfit controls.

---

## 5. Quant research operating system

The correct mental model is:

```text
market data
-> causal state estimate
-> fair value / forecast / risk estimate
-> desired position
-> portfolio constraints
-> order decision
-> execution simulation
-> fills / assumed fills
-> inventory and risk state
-> updated research evidence
```

Never collapse these layers into one backtest return number.

A good research system separates:

- data quality;
- causal feature construction;
- forecast quality;
- strategy logic;
- position sizing;
- portfolio construction;
- execution realism;
- risk control;
- statistical validation;
- operational safety.

Never let a good execution simulator hide a weak signal. Never let a good signal hide impossible execution. Never let attractive PnL hide leakage, overfitting, or unmodeled costs.

---

## 6. Edge definition

Raw return is not edge. High Sharpe from many trials is not edge. A signal that cannot survive costs is not edge. A backtest that cannot explain who loses, why the effect persists, and why it is not arbitraged away is not edge.

Minimum definition:

```text
Edge =
  causal forecast
  + implementable strategy rule
  + disciplined sizing
  + realistic execution
  + cost/funding/slippage/impact adjustment
  + capacity constraint
  + regime robustness
  + multiple-testing control
  + post-selection OOS evidence
```

A candidate can be considered promising only when:

```text
Better(A, B) =
  OOS_net_utility(A) > OOS_net_utility(B)
  and data_snooping_risk(A) is controlled
  and cost/slippage/funding stress survives
  and regime split has no hidden fatal failure
  and turnover/capacity/execution risk is acceptable
  and parameters are stable enough
  and duplicate-signal risk is acceptable
  and research did not use live/testnet/order/account paths for selection
```

Default null hypothesis:

```text
After realistic fees, spread, slippage, funding, turnover, capacity, market impact,
regime split, and overfit adjustment, the strategy has no edge over cash,
buy-and-hold, and a simpler baseline from the same strategy family.
```

---

## 7. Six legitimate sources of possible edge

Every strategy hypothesis must name at least one edge source before implementation.

### 7.1 Behavioral / slow reaction

Examples: trend, momentum, under-reaction, over-reaction reversal, forced behavior, crowded unwinds.

Required explanation:

```text
Which participants are slow or constrained?
Why does the effect not disappear immediately?
When should the behavior reverse or fail?
```

### 7.2 Risk premium

Examples: compensation for crash risk, liquidity risk, volatility risk, funding risk, basis risk, leverage constraint, carry risk.

Required explanation:

```text
What risk is being warehoused?
Why is the expected return compensation rather than free alpha?
What loss scenario earns the premium ex ante?
```

### 7.3 Structural / microstructure

Examples: venue fragmentation, funding mechanics, liquidation cascades, inventory imbalance, market design, order-book pressure, calendar effects.

Required explanation:

```text
What market mechanism creates the effect?
Which rule, constraint, or participant behavior sustains it?
What execution assumption is needed to capture it?
```

### 7.4 Information processing

Examples: cleaner transformation of public data, faster causal alignment, better state estimation, better noise filtering.

Required explanation:

```text
What information is public but poorly processed?
What transformation creates incremental forecast value?
Why is it not merely a duplicate of existing signals?
```

### 7.5 Execution edge

Examples: lower slippage, better order timing, lower adverse selection, lower market impact, better participation sizing.

Required explanation:

```text
How does execution improve net returns?
Which assumptions are realistic without live fills?
How does the advantage degrade with size?
```

### 7.6 Portfolio construction edge

Examples: combining weak signals, diversification, volatility targeting, correlation control, factor-neutral allocation, drawdown control.

Required explanation:

```text
What weak edges combine into a stronger portfolio?
Which risks are diversified versus merely hidden?
How does sizing improve utility rather than suppress trading?
```

A hypothesis that cannot name its edge source is not research-ready.

---

## 8. Hypothesis contract

Every loop must start with this structure:

```text
I believe <mechanism> causes <observable effect>
in <market/regime/timeframe>
because <economic, behavioral, structural, or microstructure reason>.
It should beat <baseline>
after <fees/spread/slippage/funding/turnover/impact>
and fail when <declared failure modes>.
```

Required fields:

- edge source;
- target symbol/product family/timeframe;
- who is on the other side;
- why the counterparty trades despite losing to this strategy;
- why the effect should persist;
- why the effect is not already arbitraged away;
- expected decay speed;
- capacity limit;
- required data;
- required cost model;
- baseline;
- parameter grid;
- selection metric;
- expected failure modes;
- disallowed selection inputs;
- claim boundary.

The hypothesis must be falsifiable before holdout or readiness validation.

---

## 9. Current research thesis

The preferred next research family is:

```text
TSMOM_VOL_TARGET_FUNDING_AWARE_V1
```

Meaning:

- time-series momentum / trend following first;
- realized-volatility target sizing;
- Regime Protocol risk multiplier;
- funding/OI-aware throttle where data exists;
- cost, spread, slippage, turnover, funding, and capacity included by default;
- mean reversion only as secondary and regime-gated;
- validation by walk-forward, OOS, cost/slippage/funding stress, ablation, regime split, and anti-overfit checks.

Reason:

```text
Trend/momentum is one of the few broad anomaly families worth testing first,
but in Binance perpetuals it must be treated as a net-of-funding, net-of-cost,
volatility-targeted, regime-conditioned portfolio problem.
```

This is a research direction, not a proven profitable strategy. Do not expand the Alpha Signal Factory merely to add more signals. Add or change signals only when the loop hypothesis states why the new information is non-duplicative and how it will be falsified.

Default baseline comparisons:

- cash/null return;
- buy-and-hold where applicable;
- current committed readiness baseline;
- simpler time-series momentum baseline;
- same strategy without funding throttle;
- same strategy without volatility targeting;
- same strategy under harsher cost assumptions.

---

## 10. Research pipeline

Use this order:

```text
raw data
-> cleaned causal dataset
-> features
-> labels / forward returns
-> forecast
-> strategy rule
-> position sizing
-> portfolio construction
-> execution simulation
-> cost/funding/slippage/impact stress
-> validation
-> decision
```

Do not skip directly from feature to PnL.

Each stage must have a separable artifact or test when practical. If a stage is merged for engineering convenience, the report must still interpret it separately.

---

## 11. Data governance

Treat data as part of the strategy. Bad data invalidates good code.

### 11.1 Required metadata

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
- funding source and coverage when funding is used;
- missing-bar policy;
- duplicate timestamp policy;
- causal alignment policy.

### 11.2 Raw and derived data

- Keep raw data immutable when practical.
- Write cleaned/derived data as separate artifacts.
- Never silently mutate raw downloaded data to make a strategy pass.
- Preserve enough metadata to reproduce the dataset or explain why it cannot be reproduced.
- Treat undocumented data cleaning as a validation failure.

### 11.3 Timestamp and bar rules

- Use UTC timestamps by default.
- Use closed bars for canonical validation.
- If open bars are included, label them explicitly and do not use that run for canonical readiness claims.
- Validate monotonic sorting and duplicate timestamps.
- Detect missing bars and gaps; either fail clearly or record the gap policy.

### 11.4 Causal alignment

- Align funding, volume, open interest, regime, signal, and macro/event data to the first bar where the information was knowable.
- Never forward-fill future information into past decisions.
- Rolling features must not use future bars.
- Prefix-invariance/lookahead checks are required when changing feature, regime, signal, or report-generation logic.
- Any information with publication delay must be shifted to the first tradable timestamp after release.

### 11.5 New data source checklist

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

---

## 12. Backtesting and validation method

Backtests are falsification tools. They are not proof of future profit.

### 12.1 Pre-registration

Before running holdout or readiness validation, write down:

- strategy family and stable candidate ID;
- hypothesis in one falsifiable sentence;
- edge source;
- baseline artifact;
- data scope;
- parameter grid;
- selection metric;
- expected failure modes;
- excluded inputs: holdout, live orders, API keys, account state.

### 12.2 Split discipline

- Use chronological splits.
- Train and validation may select candidates.
- Holdout evaluates once after selection.
- Prefer walk-forward windows over a single split.
- Never retune on holdout until it passes.
- Use purging/embargo where labels or features overlap across split boundaries.
- Preserve the first failed holdout report.

### 12.3 Baselines

Compare against:

- cash/null return;
- buy-and-hold where applicable;
- current committed strategy/readiness baseline;
- simpler strategy family if the new one is more complex;
- lower-turnover version where turnover is material;
- lower-cost assumption stress and harsher-cost stress.

A complex strategy that cannot beat a simple baseline net of costs should be parked or killed.

### 12.4 Required costs and stresses

Include, at minimum:

- commission/fees;
- spread assumption;
- slippage proxy;
- funding drag for derivatives/perps when applicable;
- turnover;
- order-size or participation-impact proxy when relevant;
- stress scenario harsher than the base assumption.

For Binance perpetual research, funding is not a footnote. It is part of the product structure and must be modeled whenever the strategy carries exposure across funding windows.

### 12.5 Required outputs

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
- parameter stability diagnostics;
- candidate switch-rate diagnostics;
- selection boundary declaration;
- claim boundary declaration.

### 12.6 Readiness verdict semantics

`readiness-run` CLI success means artifacts were written. It does **not** mean the strategy passed.

Allowed readiness verdicts:

```text
BLOCKED
NOT_READY
CONDITIONALLY_READY_FOR_PAPER_ONLY
```

No readiness report may return a live-readiness verdict.

---

## 13. Validation hierarchy

Evidence ranks as follows:

```text
blocked data quality
< smoke test
< prototype backtest
< chronological OOS
< walk-forward OOS
< purged/embargoed CV where labels overlap
< cost/funding/slippage stressed OOS
< regime-split OOS
< ablation and duplicate-signal tests
< multiple-testing-adjusted evidence
< forward paper observation
< execution-mechanics validation
```

Important boundaries:

- Forward paper is observation, not strategy validation.
- Testnet is mechanics, not performance evidence.
- Live is never authorized by research evidence alone.
- A higher layer cannot repair a failure in a lower layer. For example, forward paper observation cannot repair lookahead leakage.

---

## 14. Research-grade evidence gates

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

---

## 15. Statistical skepticism and overfitting doctrine

Assume the first good result is false.

A candidate is suspicious when:

- it was found after many unrecorded trials;
- performance comes from one period;
- performance comes from one symbol;
- performance comes from one regime;
- performance disappears after costs;
- parameters are sharp rather than stable;
- selected candidates switch often;
- trades are too few;
- turnover is too high;
- drawdown is hidden by inactivity;
- one duplicated signal family explains most of the return;
- performance depends on a single funding or volatility event;
- holdout influenced any implementation detail.

Every report must record:

- number of tried ideas when known;
- number of tried parameter combinations;
- candidate count;
- candidate-window evaluations;
- selection metric;
- rejected candidates;
- failed gates;
- whether holdout influenced any change;
- whether current forward-paper data influenced any change.

### 15.1 Anti-overfit tools

Prefer adding these validation mechanisms when the harness supports them:

- Deflated Sharpe Ratio or equivalent multiple-testing Sharpe adjustment;
- Probability of Backtest Overfitting or equivalent combinatorial validation;
- Reality Check / SPA-style comparison for strategy families;
- walk-forward stability heatmaps;
- ablation matrix;
- duplicate-signal correlation matrix;
- turnover-adjusted and capacity-adjusted scoring.

These tools are not magic. They are additional ways to falsify a candidate, not ways to rescue a weak candidate.

---

## 16. Cost and execution doctrine

A strategy is not evaluated at candle close prices. It is evaluated at feasible execution prices.

Minimum execution realism:

- taker/maker fee;
- spread;
- slippage;
- funding;
- turnover;
- latency assumption where relevant;
- partial-fill assumption where relevant;
- market-impact proxy;
- liquidation and margin risk for derivatives;
- capacity estimate.

A strategy that only works at zero cost is not a strategy.

### 16.1 Execution realism ladder

```text
close-price prototype
< next-bar execution assumption
< spread/slippage adjusted simulation
< funding-aware simulation
< turnover and capacity stress
< order-size/participation impact proxy
< testnet mechanics validation
< live execution readiness review
```

Testnet belongs near the end because it validates mechanics, not expected alpha.

### 16.2 Capacity and market impact

Every promoted candidate must answer:

- How much capital can plausibly be deployed?
- What turnover is required?
- How does performance degrade with order size?
- What spread/slippage/impact stress kills the strategy?
- Is the strategy earning alpha or merely assuming infinite liquidity?

---

## 17. Portfolio construction doctrine

A signal is not a portfolio.

Portfolio construction must handle:

- volatility targeting;
- max leverage;
- max position;
- symbol concentration;
- factor correlation;
- funding concentration;
- regime concentration;
- turnover budget;
- drawdown throttle;
- kill switch;
- capital preservation mode.

Lower drawdown is valid only if capital was intentionally preserved, not because the strategy accidentally stopped trading.

A portfolio improvement must show at least one of:

- better net OOS utility;
- lower drawdown without unacceptable return collapse;
- lower tail risk;
- lower turnover for similar utility;
- diversification benefit across symbols/regimes;
- improved stability under stress.

---

## 18. Strategy scoring direction

Recommended scoring direction:

```text
strategy_score =
  + OOS net return contribution
  + OOS Sharpe / Sortino after costs
  + diversification benefit
  - max drawdown penalty
  - CVaR / tail-loss penalty
  - turnover penalty
  - funding drag penalty
  - slippage/spread stress penalty
  - capacity / market-impact penalty
  - parameter instability penalty
  - selected-candidate switch penalty
  - regime concentration penalty
  - duplicate-signal penalty
  - data-snooping / overfit penalty
```

Do not optimize a single scalar score blindly. Use the score to rank candidates, then interpret failed gates and failure modes adversarially.

---

## 19. Strategy addition and modification rules

When adding or modifying a strategy:

1. Create or update a loop artifact using the inline artifact schema in this file.
2. Give the candidate a stable strategy ID and version.
3. State the hypothesis in one falsifiable sentence.
4. Name the edge source.
5. Declare the baseline, data scope, and parameter grid before holdout.
6. Add causal feature tests and boundary tests before trusting metrics.
7. Integrate through research adapters/candidate grids, not live/order paths.
8. Run ablation or duplicate-family checks when adding signals.
9. Update docs/report schemas when output semantics change.
10. Preserve failure evidence.
11. Promote, iterate, park, or kill based on gates, not intuition.

### 19.1 Strategy ID convention

Use explicit IDs that encode family and version, for example:

```text
TSMOM_VOL_TARGET_FUNDING_AWARE_V1
DONCHIAN_TREND_RISK_V1
REGIME_MEAN_REVERSION_LOWVOL_V1
FUNDING_CARRY_THROTTLED_V1
```

Do not reuse a strategy ID for materially different logic. Bump the version.

### 19.2 What must not happen

- Do not wire diagnostic labels directly into execution.
- Do not add dozens of near-duplicate signals without duplicate-risk checks.
- Do not hide complexity inside a candidate after seeing holdout.
- Do not call lower drawdown an improvement if the strategy simply stopped trading without an explicit capital-preservation hypothesis.
- Do not make a strategy more complex unless the added complexity has a falsifiable purpose.

---

## 20. Result interpretation protocol

Interpret reports in this order:

1. **Verdict first** — `BLOCKED`, `NOT_READY`, or `CONDITIONALLY_READY_FOR_PAPER_ONLY` outranks attractive metrics.
2. **Data scope second** — check source, range, symbol, timeframe, row count, funding source, and closed/open bar status.
3. **Validation depth third** — smoke/prototype evidence cannot justify research-grade claims.
4. **Selection boundary fourth** — confirm holdout, live orders, API keys, and account state were excluded from tuning.
5. **Edge source fifth** — confirm the hypothesis names a real mechanism.
6. **Net metrics sixth** — prefer cost/slippage/funding-adjusted OOS and stress metrics over raw return.
7. **Baseline comparison seventh** — compare against cash, buy-and-hold, current committed baseline, and simpler family baseline.
8. **Failure gates eighth** — failed gates are the next work queue, not cosmetic warnings.
9. **Regime and ablation ninth** — check whether performance depends on one regime or one duplicated signal family.
10. **Execution boundary last** — forward paper is observation, testnet is mechanics, and neither proves live performance.

Every final interpretation must answer:

```text
What passed?
What failed?
What evidence depth was used?
What cannot be claimed yet?
What is the next loop?
```

### 20.1 Failed-gate action map

- `min_windows` -> data-depth loop, not strategy celebration.
- `validation_depth_research_grade` -> more data/windows/candidates/evaluations before research-grade claims.
- `median_stress_return_beats_cash_null` -> strategy/risk/cost loop; do not retune holdout.
- `max_selected_candidate_switch_rate` -> simplify grid or improve parameter stability.
- `max_high_corr_family_pairs` -> deduplicate signal families.
- `min_regime_labels` -> expand data or improve regime coverage.
- `lookahead_checks` -> stop promotion; fix causality bug first.
- `min_holdout_trades_each` -> inspect whether the strategy is inactive, overfiltered, or data is insufficient.
- `cost_stress_failure` -> do not reduce cost assumptions to pass; improve edge, execution realism, or turnover.
- `funding_drag_failure` -> inspect holding period, directionality, and funding-aware throttle.
- `capacity_failure` -> reduce size, improve execution model, or park until order-book/liquidity data exists.

---

## 21. Loop Engineering process

Use separate loops so failures are attributable.

### 21.1 Loop types

#### Data loop

Purpose: improve data authority and coverage.

Examples:

- extend BTCUSDT history beyond prototype depth;
- add ETH/SOL for cross-sectional tests;
- add public funding history with hashes and source ranges;
- add open interest only with causal timestamp rules;
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
- order-size/capacity sensitivity;
- latency and next-bar assumptions.

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

### 21.2 Standard loop state machine

```text
1. PROPOSE
   Pre-register hypothesis, edge source, baseline, data, grid, metrics, and expected failure modes.

2. IMPLEMENT_MINIMAL
   Make the smallest code/data/report change that tests the hypothesis.

3. LOCAL_VERIFY
   Run targeted tests, schema checks, boundary checks, and lint when applicable.

4. EVIDENCE_RUN
   Run the smallest meaningful harness first; run readiness-run when justified.

5. ADVERSARIAL_REVIEW
   Ask how the result is cheating, leaking, overfitting, duplicating signals, ignoring costs,
   assuming impossible execution, or hiding regime failure.

6. DECIDE
   PROMOTE_TO_NEXT_LOOP, ITERATE_ON_SAME_HYPOTHESIS, PARK, or KILL.

7. JOURNAL
   Save evidence, commands, changed files, failed gates, and next hypothesis.
```

---

## 22. Decision rules

### 22.1 PROMOTE_TO_NEXT_LOOP

Allowed only when:

- safety gates remain intact;
- evidence artifacts are durable;
- the candidate beats the declared baseline under net-of-cost OOS/stress evidence;
- lookahead/prefix-invariance checks pass;
- regime split has no hidden fatal failure;
- duplicate-signal risk is acceptable;
- candidate stability is acceptable;
- cost/funding/slippage/impact stress is acceptable;
- the next loop is stricter or orthogonal, not looser.

A promoted strategy becomes subject to harsher validation, not easier validation.

### 22.2 ITERATE_ON_SAME_HYPOTHESIS

Allowed only when:

- the failure is implementation/data-quality related;
- holdout has not been used for tuning;
- the next change is pre-registered, narrow, and auditable;
- the iteration does not silently weaken costs, validation, or safety boundaries.

### 22.3 PARK

Use when:

- the idea is plausible but data depth is insufficient;
- required external data is missing;
- execution realism is not adequate;
- the idea needs a portfolio universe that does not exist yet;
- the idea may be valid but current infrastructure cannot fairly test it.

### 22.4 KILL

Use when:

- the idea fails after stress/OOS/regime split;
- performance depends on one duplicated signal family;
- selected candidates switch too often;
- costs/funding erase the edge;
- adding complexity only hides the same failure;
- the edge source cannot be defended;
- capacity assumptions are unrealistic.

---

## 23. Research portfolio

Do not run infinite random loops. Maintain a portfolio of hypotheses.

Default allocation:

```text
70%: boring robust families
20%: adjacent improvements
10%: speculative ideas
```

For this repo, default priority is:

1. data depth and causal integrity;
2. time-series momentum / trend;
3. volatility targeting;
4. funding-aware throttle;
5. regime risk control;
6. cost and slippage hardening;
7. cross-symbol diversification;
8. anti-overfit validation;
9. forward-paper observation;
10. execution mechanics only after research credibility.

Speculative loops are allowed only when they are cheap, isolated, and cannot contaminate baseline selection.

---

## 24. Loop artifact format

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
  "edge_source": "behavioral | risk_premium | structural_microstructure | information_processing | execution | portfolio_construction",
  "baseline": "reports/readiness-run-20260625T053627Z/ or newer committed artifact",
  "strategy_family": "TSMOM_VOL_TARGET_FUNDING_AWARE_V1",
  "data_scope": "symbol/timeframe/source/date range",
  "selection_inputs": ["train", "validation"],
  "selection_excludes": ["holdout", "live_orders", "api_keys", "account_state", "current_forward_paper"],
  "pre_registered_grid": {},
  "expected_failure_modes": [],
  "cost_model": {
    "fees": "declared",
    "spread": "declared",
    "slippage": "declared",
    "funding": "declared when applicable",
    "turnover": "declared",
    "impact_or_capacity": "declared when relevant"
  },
  "safety_gates": {
    "live_orders_submitted": false,
    "api_keys_required_for_research": false,
    "holdout_used_for_selection": false,
    "current_forward_paper_used_for_selection": false,
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
## Edge source
## Baseline
## Change
## Data and selection boundary
## Cost / execution assumptions
## Commands
## Evidence
## Result interpretation
## Failed gates
## Decision
## Why
## What cannot be claimed
## Next loop
```

---

## 25. Default loop order from current baseline

Because the committed baseline is still `NOT_READY`, prioritize:

1. **Data-depth loop** — collect enough public historical data to satisfy research-grade depth.
2. **Strategy loop** — implement a narrow `TSMOM_VOL_TARGET_FUNDING_AWARE_V1` candidate grid.
3. **Risk loop** — add volatility target and drawdown/funding throttles.
4. **Cost/execution-realism loop** — make slippage, spread, funding, turnover, and capacity assumptions harsher.
5. **Validation loop** — add overfit penalties and stronger candidate-stability reporting.
6. **Portfolio loop** — add cross-symbol diversification only after per-symbol evidence is interpretable.
7. **Forward-paper loop** — observe current market behavior without tuning or orders.
8. **Execution loop** — only after paper research is credible, use testnet for order mechanics.

---

## 26. Commands, environment, and validation

`uv` is mandatory for this project. Do not silently fall back to raw `python`, virtualenv executables, or system package tools for normal project workflows. If `uv` is missing, install it first or report the environment as blocked.

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

---

## 27. Testnet and live trading boundary

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

Even if all of those pass, this repo's research reports still do not guarantee profit.

### Live claim rule

Do not say:

```text
ready
live-ready
profitable
safe to trade
validated alpha
```

unless the exact scope is stated and the relevant gate actually supports it, such as:

```text
ready for smoke testing
ready for prototype comparison
conditionally ready for paper-only observation
ready for testnet execution-mechanics validation
```

This repository's readiness reports must not emit live-readiness verdicts.

---

## 28. Documentation and schema responsibility

When changing strategy or validation behavior, update relevant source-of-truth docs and schemas:

- `AGENTS.md` for project operating rules;
- `README.md` for user-facing overview;
- `docs/backtesting-paper-trading.md` for backtesting/report details when needed;
- generated report schema notes when output contracts change.

Schema-breaking changes must bump the relevant schema version and include a migration note. Report consumers should not have to guess whether field semantics changed.

---

## 29. Constitution change control

Because this file is the project constitution:

- Keep it self-contained.
- Do not weaken safety boundaries without explicit user instruction and a recorded rationale.
- Do not remove failed-baseline context merely because it is inconvenient.
- Prefer adding precise rules over vague aspirations.
- When this file changes, verify it for broken references and obvious contradictions.
- If this file conflicts with code behavior, either fix the code or explicitly document the gap.

---

## 30. Agent behavior contract

Agents must think like skeptical quant researchers.

Before implementation, ask internally:

- What exact edge source is being tested?
- What baseline must it beat?
- What data right does it use?
- What would falsify it?
- What costs could kill it?
- What regime could break it?
- What hidden leakage could inflate it?
- What simpler model could explain the same result?
- What duplicate signal family could explain the return?
- What evidence would justify parking or killing it?

Agents must not sell performance. They must state claim boundaries.

---

## 31. Final response contract for agents

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

---

## 32. Anti-patterns to reject

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
- Reducing costs, slippage, or funding assumptions after seeing results.
- Making the grid larger without recording increased multiple-testing risk.
- Calling inactivity risk control without a capital-preservation hypothesis.
- Confusing more indicators with more information.
- Confusing a plausible story with validated edge.
