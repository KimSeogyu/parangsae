STATUS: PRE-REGISTRATION, frozen (`chmod 444`) before any run. NOT RUN; no result exists for this family. Nothing has been appended to `research/trials.jsonl`. Its sha256 must be written in each `RunStarted` record. After freezing, any change is a new file `-r2`, with N carried over.

# S01 `tsmom-slow`: sweep pre-registration (BTCUSDT USD-M perp)

- Frozen at: `2026-10-23 07:39 KST` · author: Quant Researcher · queue entry: `strategy-queue-v1.md` §2 S01 · runner inputs: `sweep-runner-inputs-v2.md` sha256 `be7f5dd05d9adc9d557355e509a4ab1c33742599a123a0167812b0fbec01dc75`
- Prior looks at this idea (each counts toward N): none in the ledger for momentum on BTC returns. Ledger rows 2–3 (realized-vol distributions) are vol-only and are already counted in the 13.
- Results seen before freezing: **none**. No backtest, sweep or P&L of this rule on any data.

## 1. Hypothesis and economic rationale
- **Hypothesis:** the sign of the past n-bar BTCUSDT perp log return predicts the sign of the next-period return strongly enough that a continuous, deadbanded position of clamp(scaled return, ±1) earns positive net returns after 5 bps + 2 bps slippage + funding over 2022-01..2025-09.
- **Why the edge should exist / who is on the other side:** information diffuses slowly, and trend-following flows (vol targeters, CTA-style funds, retail FOMO, short squeezes, liquidations) push in the direction of past moves. The other side is mean-reversion liquidity providers and early contrarians.
- **Falsifier beyond the gates:** the sign of OOS P&L is driven by one fold (> 50% of stitched net P&L from a single 90-d fold is reported as fragile), or long-only exposure explains it (report the beta to buy-and-hold).

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
| funding (`funding`): cost; not a signal input | same | slot + 60 s | none in the window |

- No forced-flat window. The 2023-11-10..15 conflicts are resolved by v0.3 (2023-11-10 15:06–16:45 from the overlay; 2023-11-14 stored rows are correct).

## 3. Rule (causal)
- Bars and decision: UTC-aligned H1 or H4 bars (per config) from `kline_1m`. Decision at bar close + 1 ms. Fill at the open of the first 1m bar at or after the decision with `count > 0`.
- Signal: `x_t = ln(C_t / C_{t−n}) / r_scale`, with `r_scale = c · r0 · √(n / n0)`; H1: r0 = 0.05, n0 = 24; H4: r0 = 0.10, n0 = 42. Target `w_t = clamp(x_t, −1, 1)` (signals-v0 `tsmom`).
- Entry, exit, hold, flips: continuous. Rebalance to `w_t` only when `|w_t − w_cur| ≥ 0.25`, where `w_cur` is the current signed notional / equity at the decision. There is no fixed hold.
- Sizing: signed notional = w × equity at the decision, divided by the fill price and floored to 0.001 BTC.
- FIXED (not swept):
  - deadband 0.25 (queue)
  - r0 and n0 per interval (as above)
  - 1× maximum leverage

## 4. Grid (exact; unchanged from strategy-queue-v1; the new window forced no change)

| Param | Values |
|---|---|
| `(interval, n)` | (H1,24), (H1,72), (H1,168), (H4,42), (H4,90), (H4,180) |
| `c` | 0.5, 1.0 |

- Grid size **12** configs, no variants or baselines. **This family adds 12 trials.** Fee scenarios are not trials.
- **N_global at freeze** (read from the ledger): **13** (legacy exploratory rows). **Gate N = max(202, ledger N_global at evaluation)**: the full-queue global N of runner-inputs v2 §3, kept.

## 5. Evaluation protocol
- Walk-forward, rolling: train 365 d / purge 2 d / test 90 d / step 90 d.
  - Warmup 30 d (max lookback 720 h), no decisions in it.
  - First train 2021-01-17; first test 2022-01-19; folds 15 (last fold 91 d).
  - **OOS days T = 1,351** (2022-01-19..2025-09-30).
- Purge 2 d, with every train-window position closed at the train end. Embargo 1 d (CSCV only). This deviates from the backtest-design "max H + lookback" wording; the reason is in runner-inputs v2 §0 C8.
- Selection: the highest net annual Sharpe at 5 bps + 2 bps slippage + funding on each train window. Ties go to the earlier config in §4 order (interval-n pairs as listed, then c ascending).
- Costs: fee 2/4/5/6 bps/side (headline and selection 5), slippage 2 bps/side, **funding on**. Stress 5 + 5 is reported.
- **Sharpe denominator:** daily net returns from equity at 00:00 UTC (the close of the 23:59 bar), ddof 1, × √365, T = calendar days (flat days count as 0).
- Metrics: total return, annual Sharpe (with denominator string), max DD (per bar), turnover, n_trades, average hold, exposure, refused fills, fees, slippage and funding paid, fee_sens, breakeven fee, DSR, PBO. Format: runner-inputs v2 §5 table.
- **DSR hurdle at T = 1,351, N = 202** (DSR ≥ 0.95; SR_ann needed). These are planning values; the gate uses measured V, skew and kurt:
  - normal returns, cross-trial sd_ann 0.25 / 0.50 / 0.75 → **1.55 / 2.24 / 2.94**;
  - with skew −0.5 and kurt 8 → 1.57 / 2.28 / 2.98.
  - Formula: runner-inputs v2 §5.2.

## 6. Gates (all on stitched OOS; none is relaxed after freezing)
- [ ] DSR ≥ 0.95 at N = max(202, ledger N_global), measured cross-trial V, daily units
- [ ] PBO ≤ 0.20 (CSCV, S = 16, on the family's 12 IS daily-return columns). The queue gate is used; it is stricter than 0.5
- [ ] Net total return > 0 **and** net SR > 0 at 5 bps + 2 bps slippage + funding; net SR > 0 at 6 bps
- [ ] ≥ 100 OOS rebalances **and** ≥ 10 sign changes (continuous rule, as in the queue)
- [ ] No-lookahead: the truncation proptest is green for `tsmom` (H1 and H4); fills at the next tradable 1m open
- [ ] Holdout guard log present; no date ≥ 2025-10-01 and none < 2020-12-18 in any artifact
- [ ] Trial count = ledger count (12 for this family, aborted runs included)
- Family-specific: none. S01 is the control that S02 and S03 are compared against; the OOS daily correlation with them is reported.

## 7. Kill rule and next step
- On any gate FAIL, the family is closed for this pin. A re-test needs a new idea (a new file), not a new grid.
- On PASS: paper-trade the selected config (dry-run only, no live keys). Opening the holdout requires the user's approval.

## 8. Known failure modes and risks (stated before results)
- Whipsaw in range regimes (2023 chop), where fees dominate at H1 n = 24 (prior 1–2 rebalances/day).
- BTC trend premia may have compressed after 2021, and the OOS starts in 2022 (bear, then chop).
- Long exposure in bull runs pays positive funding, so the funding drag is real at 1×.
- The continuous position carries large exposure through gap moves. Max DD can be large even if SR passes.
- 2-day purge plus train-end truncation is a lighter purge than the design-doc wording (flagged).
