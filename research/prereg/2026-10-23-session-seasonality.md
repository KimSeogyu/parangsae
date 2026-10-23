STATUS: PRE-REGISTRATION, frozen (`chmod 444`) before any run. NOT RUN; no result exists for this family. Nothing has been appended to `research/trials.jsonl`. Its sha256 must be written in each `RunStarted` record. After freezing, any change is a new file `-r2`, with N carried over.

# S06 `session-seasonality`: sweep pre-registration (BTCUSDT USD-M perp)

- Frozen at: `2026-10-23 07:39 KST` · author: Quant Researcher · queue entry: `strategy-queue-v1.md` §2 S06 · runner inputs: `sweep-runner-inputs-v2.md` sha256 `be7f5dd05d9adc9d557355e509a4ab1c33742599a123a0167812b0fbec01dc75`
- Prior looks at this idea (each counts toward N): ledger row 4 (exploratory: hour-of-day/weekday relative realized vol and quote volume by year, minute profile). It looked at session **vol and volume**, not returns, and is counted in the 13. Disclosed as a prior look at session structure.
- Results seen before freezing: **none**. No backtest, sweep or P&L of this rule on any data.

## 1. Hypothesis and economic rationale
- **Hypothesis:** the sign of the trailing W-day mean return of each 4 h UTC block, when its t-stat is at least k, predicts that block's return in the next day, with positive net returns after 5 bps + 2 bps slippage + funding.
- **Why the edge should exist / who is on the other side:** timezone-bound flows recur at fixed UTC times: Asia open, EU/US equity hours, US-close ETF creations, hedging around the 00/08/16 funding settlements. The other side is price-insensitive flows constrained by schedule.
- **Falsifier beyond the gates:** OOS block-return signs are unstable: fewer than 4 of the 6 blocks keep the same sign of m_b across the OOS folds. This is reported.

## 2. Data (pinned)
- Read path **v0.3**:
  - doc `/workspace/reports/research-read-path.md`, sha256 `d5ada80d0c5cb1b9f4ee68e39f2546400522e48ce018f6aaf545a6e015f41108`;
  - SQL `/workspace/reports/research-read-path.sql`, sha256 `9c7fbc3a2b95d0dbc52040ee82e09926f11f47883f3499537c8c0e61f54e65cc`;
  - overlay `/workspace/data/parangsae/runs/resolutions/um-kline-2023-11-10.parquet`, sha256 `d5f6bcd61688cac971b6d276ea4fa0826c86d998b7adc06557686da6bdcfb8ea`.
- **Pinned root:** there is no published manifest pin yet. `pinned_root` = sha256(JCS sorted `[(path, sha256)]` of the three objects above plus every Parquet file opened), computed at run start (runner-inputs v2 §2.1). Any change is a new pin, and so new trials.
- `available_ts` rules: klines close + 1 ms (1m: open + 60 s, measured on every row); funding view greatest(stored, slot + 60 s); never the raw funding column.
- **Window:** reads 2020-12-18 00:00 UTC .. 2025-09-30 23:59 UTC. Nothing earlier (EL d) and nothing ≥ 2025-10-01 00:00 UTC (holdout, double guard). There is one data case: the engine-v1 reader accepts schema 1 (EL a).

| Dataset (view) | Range read | Visibility | Gaps and handling |
|---|---|---|---|
| um-kline 1m (`kline_1m`; H1/H4 built from it, complete bars only) | 2020-12-18..2025-09-30 | close + 1 ms | none. `count = 0` minutes: no fills (move to the next minute with `count > 0`) and no volume features. The 93 aggTrades-contradicted zero bars stay as a flat carry (disclosed, open DE item) |
| mark 1m (`mark_1m`): MTM, funding notional | same | open + 60 s | 2024-08-12 10:02–10:03 UTC (2 min). **BB ruling pending.** Until BB rules, a run hitting it aborts and counts |
| funding (`funding`): cost; not a signal input; settlements at 00/08/16 UTC fall on block boundaries, and funding settles on the pre-fill position | same | slot + 60 s | none in the window |

- No forced-flat window. The 2023-11-10..15 conflicts are resolved by v0.3 (2023-11-10 15:06–16:45 from the overlay; 2023-11-14 stored rows are correct).

## 3. Rule (causal)
- Bars and decision: UTC-aligned H4 bars (00, 04, …, 20) from `kline_1m`. Decision at bar close + 1 ms. Fill at the open of the first 1m bar at or after the decision with `count > 0`.
- Signal: for block b, `m_b` = mean of the block-b log returns `ln(C_close/C_open)` over the previous W days, strictly before the current day's block b. `sd_b` is their sd (ddof 1). `t_b = m_b / (sd_b / √W)`. At the decision that closes the block before b: target = sign(m_b) if `|t_b| ≥ k`, else 0.
- Entry, exit, hold, flips: hold exactly block b. If the next block's target has the same sign, keep the position with no re-trade. If its target is 0 or the opposite sign, exit or flip at the boundary fill. No entry whose block ends after 2025-09-30 23:59 UTC.
- Sizing: ±1 × equity notional / fill price, floored to 0.001 BTC.
- FIXED (not swept):
  - six 4 h UTC blocks starting at 00:00
  - returns open to close of the H4 bar

## 4. Grid (exact; unchanged from strategy-queue-v1; the new window forced no change)

| Param | Values |
|---|---|
| `W (days)` | 90, 180, 365 |
| `k` | 1.0, 2.0 |

- Grid size **6** configs, no variants or baselines. **This family adds 6 trials.** Fee scenarios are not trials.
- **N_global at freeze** (read from the ledger): **13** (legacy exploratory rows). **Gate N = max(202, ledger N_global at evaluation)**: the full-queue global N of runner-inputs v2 §3, kept.

## 5. Evaluation protocol
- Walk-forward, rolling: train 365 d / purge 2 d / test 90 d / step 90 d.
  - Warmup 365 d (W = 365), no decisions in it.
  - First train 2021-12-18; first test 2022-12-20; folds 11 (last fold 116 d).
  - **OOS days T = 1,016** (2022-12-20..2025-09-30).
- Purge 2 d, with every train-window position closed at the train end. Embargo 1 d (CSCV only). This deviates from the backtest-design "max H + lookback" wording; the reason is in runner-inputs v2 §0 C8.
- Selection: the highest net annual Sharpe at 5 bps + 2 bps slippage + funding on each train window. Ties go to the earlier config in §4 order (W ascending, then k).
- Costs: fee 2/4/5/6 bps/side (headline and selection 5), slippage 2 bps/side, **funding on**. Stress 5 + 5 is reported.
- **Sharpe denominator:** daily net returns from equity at 00:00 UTC (the close of the 23:59 bar), ddof 1, × √365, T = calendar days (flat days count as 0).
- Metrics: total return, annual Sharpe (with denominator string), max DD (per bar), turnover, n_trades, average hold, exposure, refused fills, fees, slippage and funding paid, fee_sens, breakeven fee, DSR, PBO. Format: runner-inputs v2 §5 table.
- **DSR hurdle at T = 1,016, N = 202** (DSR ≥ 0.95; SR_ann needed). These are planning values; the gate uses measured V, skew and kurt:
  - normal returns, cross-trial sd_ann 0.25 / 0.50 / 0.75 → **1.68 / 2.37 / 3.07**;
  - with skew −0.5 and kurt 8 → 1.71 / 2.41 / 3.12.
  - Formula: runner-inputs v2 §5.2.

## 6. Gates (all on stitched OOS; none is relaxed after freezing)
- [ ] DSR ≥ 0.95 at N = max(202, ledger N_global), measured cross-trial V, daily units
- [ ] PBO ≤ 0.20 (CSCV, S = 16, on the family's 6 IS daily-return columns). The queue gate is used; it is stricter than 0.5
- [ ] Net total return > 0 **and** net SR > 0 at 5 bps + 2 bps slippage + funding; net SR > 0 at 6 bps
- [ ] ≥ 100 OOS round trips
- [ ] No-lookahead: the truncation proptest is green for the block-statistic computation (strictly past days); fills at the next tradable 1m open
- [ ] Holdout guard log present; no date ≥ 2025-10-01 and none < 2020-12-18 in any artifact
- [ ] Trial count = ledger count (6 for this family, aborted runs included)
- Family-specific: none.

## 7. Kill rule and next step
- On any gate FAIL, the family is closed for this pin. A re-test needs a new idea (a new file), not a new grid.
- On PASS: paper-trade the selected config (dry-run only, no live keys). Opening the holdout requires the user's approval.

## 8. Known failure modes and risks (stated before results)
- Non-stationarity: session patterns changed with the 2024 spot-ETF era. The OOS starts later (2022-12-20, T = 1,016), so the hurdle is higher.
- **The window forces a cost:** W = 365 needs a 365-d warmup, which shortens the family's OOS for all six configs. The grid is unchanged, and this is flagged.
- Cost drag at k = 1: up to about 2–4 position changes/day.
- A position held through 00/08/16 pays funding at that boundary even when it exits there, because settlement happens on the pre-fill position.
- Six blocks inside one statistic is implicit multiplicity that N does not capture.
