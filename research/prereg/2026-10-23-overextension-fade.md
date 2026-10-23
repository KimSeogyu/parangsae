STATUS: PRE-REGISTRATION, frozen (`chmod 444`) before any run. NOT RUN; no result exists for this family. Nothing has been appended to `research/trials.jsonl`. Its sha256 must be written in each `RunStarted` record. After freezing, any change is a new file `-r2`, with N carried over.

# S05 `overextension-fade`: sweep pre-registration (BTCUSDT USD-M perp)

- Frozen at: `2026-10-23 07:39 KST` · author: Quant Researcher · queue entry: `strategy-queue-v1.md` §2 S05 · runner inputs: `sweep-runner-inputs-v2.md` sha256 `be7f5dd05d9adc9d557355e509a4ab1c33742599a123a0167812b0fbec01dc75`
- Prior looks at this idea (each counts toward N): ledger rows 2–3 (realized-vol distributions, exploratory), vol-only, counted in the 13.
- Results seen before freezing: **none**. No backtest, sweep or P&L of this rule on any data.

## 1. Hypothesis and economic rationale
- **Hypothesis:** in quiet regimes (`p_t ≤ 0.5`), H1 moves of |z| ≥ k over L hours revert over the next H hours enough to earn positive net returns after 5 bps + 2 bps slippage + funding.
- **Why the edge should exist / who is on the other side:** in low vol, sharp multi-hour moves are mostly liquidity shocks (stop runs, a single large taker) that market makers revert. The other side is impatient takers and stop-triggered flow.
- **Falsifier beyond the gates:** the fades' mean OOS return is negative in the H = 12 configs while positive in H = 4 only gross (the edge is pure noise or bid-ask). This is reported.

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
- Bars and decision: UTC-aligned H1 bars from `kline_1m`. Decision at bar close + 1 ms. Fill at the open of the first 1m bar at or after the decision with `count > 0`.
- Signal: `z_t = ln(C_t / C_{t−L}) / σ_L`, where `σ_L` = √L × sd (ddof 1) of H1 log returns over the 720 H1 bars ending at t. Quiet gate: `volregime` H1 (w = 24, k = 168) `p_t ≤ 0.5`. Event: `|z_t| ≥ k` and the quiet gate; position = −sign(z_t).
- Entry, exit, hold, flips: enter at the next tradable 1m open and hold H; exit at the first H1 decision ≥ entry + H. Signals during a hold are ignored. No entry if `d + H` passes 2025-09-30 23:59 UTC.
- Sizing: ±1 × equity notional / fill price, floored to 0.001 BTC.
- FIXED (not swept):
  - σ window 720 H1 bars
  - quiet gate p_t ≤ 0.5 (volregime w = 24, k = 168)
  - no stop-loss

## 4. Grid (exact; unchanged from strategy-queue-v1; the new window forced no change)

| Param | Values |
|---|---|
| `L (H1 bars)` | 4, 12 |
| `k` | 2.0, 2.5 |
| `H (hours)` | 4, 12 |

- Grid size **8** configs, no variants or baselines. **This family adds 8 trials.** Fee scenarios are not trials.
- **N_global at freeze** (read from the ledger): **13** (legacy exploratory rows). **Gate N = max(202, ledger N_global at evaluation)**: the full-queue global N of runner-inputs v2 §3, kept.

## 5. Evaluation protocol
- Walk-forward, rolling: train 365 d / purge 2 d / test 90 d / step 90 d.
  - Warmup 30 d (max lookback 720 h), no decisions in it.
  - First train 2021-01-17; first test 2022-01-19; folds 15 (last fold 91 d).
  - **OOS days T = 1,351** (2022-01-19..2025-09-30).
- Purge 2 d, with every train-window position closed at the train end. Embargo 1 d (CSCV only). This deviates from the backtest-design "max H + lookback" wording; the reason is in runner-inputs v2 §0 C8.
- Selection: the highest net annual Sharpe at 5 bps + 2 bps slippage + funding on each train window. Ties go to the earlier config in §4 order (L ascending, then k, then H).
- Costs: fee 2/4/5/6 bps/side (headline and selection 5), slippage 2 bps/side, **funding on**. Stress 5 + 5 is reported.
- **Sharpe denominator:** daily net returns from equity at 00:00 UTC (the close of the 23:59 bar), ddof 1, × √365, T = calendar days (flat days count as 0).
- Metrics: total return, annual Sharpe (with denominator string), max DD (per bar), turnover, n_trades, average hold, exposure, refused fills, fees, slippage and funding paid, fee_sens, breakeven fee, DSR, PBO. Format: runner-inputs v2 §5 table.
- **DSR hurdle at T = 1,351, N = 202** (DSR ≥ 0.95; SR_ann needed). These are planning values; the gate uses measured V, skew and kurt:
  - normal returns, cross-trial sd_ann 0.25 / 0.50 / 0.75 → **1.55 / 2.24 / 2.94**;
  - with skew −0.5 and kurt 8 → 1.57 / 2.28 / 2.98.
  - Formula: runner-inputs v2 §5.2.

## 6. Gates (all on stitched OOS; none is relaxed after freezing)
- [ ] DSR ≥ 0.95 at N = max(202, ledger N_global), measured cross-trial V, daily units
- [ ] PBO ≤ 0.20 (CSCV, S = 16, on the family's 8 IS daily-return columns). The queue gate is used; it is stricter than 0.5
- [ ] Net total return > 0 **and** net SR > 0 at 5 bps + 2 bps slippage + funding; net SR > 0 at 6 bps
- [ ] ≥ 100 OOS round trips
- [ ] No-lookahead: the truncation proptest is green for `volregime` H1 and the z computation; fills at the next tradable 1m open
- [ ] Holdout guard log present; no date ≥ 2025-10-01 and none < 2020-12-18 in any artifact
- [ ] Trial count = ledger count (8 for this family, aborted runs included)
- Family-specific: none.

## 7. Kill rule and next step
- On any gate FAIL, the family is closed for this pin. A re-test needs a new idea (a new file), not a new grid.
- On PASS: paper-trade the selected config (dry-run only, no live keys). Opening the holdout requires the user's approval.

## 8. Known failure modes and risks (stated before results)
- **Most cost-sensitive price strategy**: about 14 bps per round trip (fees plus slippage) against a small edge per trade. Breakeven fee is the key diagnostic.
- A quiet-regime breakout (S03 events) runs it over. Fat left tail without a stop.
- Fading crash or capitulation moves (2022) when p is still low at the start of the move.
- Events cluster, so the effective number of independent trades is lower than the count.
