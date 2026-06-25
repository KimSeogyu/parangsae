# Parangsae Quant Work Progress Summary — 2026-06-26

## Git status

- Branch: `codex/data-depth-research-grade-v1`
- Latest pushed commit before this summary: `4bf3c477b804e913d731f74d40a9017d78ff0902`
- Latest implementation commit message: `Add TSMOM stress switch funding diagnostic loop`
- Claim boundary: all work below is paper-only / diagnostic or validation research, not live readiness, not trading advice, not a profit guarantee, and not permission for live/testnet orders.

## Completed loop 1: DATA_DEPTH_RESEARCH_GRADE_V1

- Commit: `7413486` — `Implement DATA_DEPTH_RESEARCH_GRADE_V1 research-depth loop`
- Purpose: expand public historical OHLCV/funding data depth and provenance before adding more strategy logic.
- Result: `data_depth_loop_verdict: WATCH`
- Readiness baseline: `readiness_baseline_verdict: NOT_READY`
- Boundary: data-depth-only; it does not claim strategy profitability, paper readiness, or live readiness.
- Durable artifact path: `reports/loops/20260625-data-depth-research-grade/`

## Completed loop 2: TSMOM_VOL_TARGET_FUNDING_AWARE_V1

- Commit: `7f50855` — `Implement TSMOM_VOL_TARGET_FUNDING_AWARE_V1 paper loop`
- Purpose: pre-registered paper-only TSMOM validation loop with volatility targeting and public funding-aware throttles.
- Result: `tsmom_loop_verdict: WATCH`
- Readiness baseline: `readiness_baseline_verdict: NOT_READY`
- Passed gates:
  - `candidate_count`
  - `candidate_window_evaluations`
  - `bars_processed_estimate`
  - `max_drawdown_abs_pct`
  - `tail_loss_cvar_pct`
- Failed gates:
  - `median_stress_return_beats_cash_null`
  - `selected_candidate_switch_rate`
  - `funding_aware_net_return`
- Durable artifact path: `reports/loops/20260625-tsmom-vol-target-funding-aware-v1/`

## Completed loop 3: TSMOM_STRESS_SWITCH_FUNDING_DIAGNOSTIC_V1

- Commit: `4bf3c47` — `Add TSMOM stress switch funding diagnostic loop`
- Purpose: diagnostic-only follow-up explaining the three failed TSMOM gates without retuning parameters, relaxing gates, optimizing on holdout, or claiming readiness.
- Result: `tsmom_diagnostic_loop_verdict: DIAGNOSTIC_COMPLETE`
- Readiness baseline: `readiness_baseline_verdict: NOT_READY`
- Durable artifact path: `reports/loops/20260626-tsmom-stress-switch-funding-diagnostic-v1/`
- Generated artifacts:
  - `diagnostic.json`
  - `report.md`
  - `failed_windows.csv`
  - `candidate_switches.csv`
  - `funding_decomposition.csv`
  - `commands.txt`
  - `changed_files.txt`

## Diagnostic findings

### 1. `median_stress_return_beats_cash_null`

- Window count: `31`
- Failed cash/null windows: `26`
- Median net stress return: `-1.5014549259752459%`
- Mean net stress return: `-1.5551975725139542%`
- Worst net stress return: `-4.986574106354551%`
- CVaR-style tail net return: `-4.775379842171519%`

Interpretation: stress-window returns remain negative versus the cash/null baseline in most windows, so this gate remains failed.

### 2. `selected_candidate_switch_rate`

- Switch count: `27`
- Switch rate: `0.9`
- Top diagnosed causes:
  - unstable near-tie candidates
  - volatility-target changes
  - lookback sensitivity
  - exposure-cap changes
  - funding-threshold changes

Interpretation: selection is unstable across walk-forward windows; the diagnostic loop explains the instability but does not tune the grid to hide it.

### 3. `funding_aware_net_return`

- Median gross return: `-0.7648991553115825%`
- Median funding cost drag: `0.0975%`
- Median slippage drag: `0.5%`
- Median net return after funding/stress costs: `-1.5014549259752459%`
- Primary diagnosed failure modes:
  - low gross edge after funding and slippage
  - stress costs erased edge
  - funding cost drag dominates raw edge in weak windows
  - low or wrong-direction gross edge across bear/bull/sideways regimes

Interpretation: the failure is not just funding cost; the gross edge is weak before costs, and funding/slippage stress makes the net result worse.

## Validation evidence

Latest validation after diagnostic rework:

- `uv run pytest tests/test_tsmom_loop.py -q` → `9 passed`
- `uv run pytest -q` → `119 passed`
- `uv run ruff check .` → passed
- `uv run --with pyright pyright src/parangsae/apps/cli/quantctl.py src/parangsae/research/loop_artifacts.py src/parangsae/research/data_depth.py src/parangsae/research/tsmom.py tests/test_tsmom_loop.py` → `0 errors, 0 warnings`
- `git diff --check` → clean
- Code review: `APPROVE`
- Architect review: `CLEAR`
- UltraQA adversarial CLI/artifact scenarios: passed

## Current verdict

- Repository readiness remains: `NOT_READY`
- Data-depth loop: `WATCH`, data-depth-only
- TSMOM strategy loop: `WATCH`, paper-only validation loop
- TSMOM diagnostic loop: `DIAGNOSTIC_COMPLETE`, diagnostic-only

No artifact currently supports live readiness, profitability, trading advice, or live/testnet execution permission.

## Recommended next pre-registered loop

Recommended next loop candidate:

```text
TSMOM_SWITCH_STABILITY_FUNDING_THROTTLE_RISK_V1
```

Suggested purpose:

- reduce candidate-switch instability;
- test whether funding drag should throttle exposure before holdout;
- preserve public-data-only paper validation;
- do not retune on holdout;
- do not relax gates;
- do not claim readiness unless all relevant readiness gates pass in a separate readiness process.
