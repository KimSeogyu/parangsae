STATUS: INPUT SPEC v2 for Signal Builder and Backtest Builder, 2026-10-23 07:37 KST. FROZEN (444). Design only, NOT RUN. Nothing was appended to `research/trials.jsonl`, and no performance number exists. It supersedes `sweep-runner-inputs-v1.md`, which is left unchanged. Changes from v1 come only from the EL rulings (a)–(d) and the Data Engineer's read-path v0.3 notice of 2026-10-23; none came from results.

# Sweep-runner inputs v2 (BTCUSDT USD-M perp, backtest and paper only)

## 0. Changes from v1
| # | v1 | v2 | Source |
|---|---|---|---|
| C1 | Case A/B (engine-v0 reads schema 2 only) | **One case.** The engine-v1 reader accepts schema 1, so there is no re-ingest | EL (a) |
| C2 | Funding stream "needed beyond v0" | **Funding is in the strategy view (engine-v1).** Index and spot come later, after the first table | EL (b) |
| C3 | Filler-minute fill rule was a BB request | **Fills on zero-volume filler or `count = 0` minutes are refused and move to the next minute with `count > 0`** | EL (c), DE |
| C4 | Data 2020-01-01.. | **Earliest start 2020-12-18 00:00 UTC.** Every run ends before 2025-10-01 00:00 UTC | EL (d) |
| C5 | Forced flat 2023-11-10..15 | **Dropped.** The 105 conflicts are resolved by read path v0.3 | DE |
| C6 | — | `count = 0` bars are no-trade bars: no fills, and excluded from volume features | DE |
| C7 | Generic output table | Per-strategy table with a **DSR hurdle column** (§5) | EL |
| C8 | Purge = max H + lookback | Purge = 2 d with train-end truncation (§2). Why: train strictly precedes test, so feature lookback cannot leak test labels | QR, flagged |

## 1. Hard boundaries
- Backtest and paper only. No `execution` import, no keys, no orders.
- **Holdout:** no read, query or warmup load of any row keyed ≥ **2025-10-01 00:00:00 UTC** (`HOLDOUT_START_US = 1_759_276_800_000_000`). The runner checks this twice:
  - (a) before opening, it refuses any `date=` partition ≥ 2025-10-01;
  - (b) after load, it asserts `max(open_time_us, funding_time_us) < HOLDOUT_START_US`.
  A violation aborts the run and counts as an aborted trial.
- **No reads before 2020-12-18 00:00 UTC**, warmup included (EL d).
- **End of data:**
  - no entry at a decision `d` with `d + H > 2025-09-30 23:59 UTC`;
  - every position is flattened at the open of the last tradable minute ≤ 2025-09-30 23:59 UTC;
  - the last daily equity point is the close of the 2025-09-30 23:59 bar.
- **Ledger:** one write-ahead `RunStarted` per trial. `trial_id = sha256(JCS{config_id, pinned_root, attempt})`. Aborted runs count. Fee scenarios are not trials.

## 2. Data (read path v0.3) and walk-forward

### 2.1 Read path (verified 2026-10-23 07:36 KST; sha256 of the files as seen on the box)
| Object | Path | sha256 |
|---|---|---|
| Read-path doc v0.3 | `/workspace/reports/research-read-path.md` | `d5ada80d0c5cb1b9f4ee68e39f2546400522e48ce018f6aaf545a6e015f41108` |
| Read-path SQL | `/workspace/reports/research-read-path.sql` | `9c7fbc3a2b95d0dbc52040ee82e09926f11f47883f3499537c8c0e61f54e65cc` |
| 2023-11-10 resolution overlay | `/workspace/data/parangsae/runs/resolutions/um-kline-2023-11-10.parquet` | `d5f6bcd61688cac971b6d276ea4fa0826c86d998b7adc06557686da6bdcfb8ea` |

Views used:
- `um_1m_resolved` / `kline_1m` / `kline_1h` for prices. `kline_1h` covers complete hours only (60/60), with `available_ts` = the max over its minutes, i.e. hour end + 1 ms.
- `funding`: available = greatest(stored, slot + 60 s), from `runs/funding-4db8a75/`.
- `mark_1m` for MTM and funding notional.
- H4 bars are built from `kline_1m` the same way: 240/240 minutes, UTC-aligned 00/04/…/20.
- A Rust engine reader must reproduce these expressions exactly. Funding in particular: never read the raw `available_ts_us`.

**pinned_root:** there is no published manifest pin yet (the manifest branch is unmerged). Until one exists, `pinned_root` = sha256 of the JCS list `[(path, sha256)]` covering the three objects above plus every Parquet file the run opens, sorted by path, and it is computed at run start. Any change is a new pin, and so new trials.

### 2.2 Data rules
- **`count = 0` minutes** (no-trade; 369 stored rows, minus the 99 replaced on 2023-11-10 by the overlay, leaves 270 in the store's history; the 93 that contradict aggTrades are still open):
  - **no fills**: an order whose fill minute has `count = 0` fills at the open of the next minute with `count > 0`;
  - the 6 h execution timeout still applies; if it expires the trial aborts;
  - excluded from volume features (no first-batch strategy uses volume);
  - their OHLC (a flat carry) stays in price bars exactly as the read path builds them. Disclosed: each of the 93 contradicted bars carries a stale (previous-close) price for its one minute, inside the H1/H4 OHLC.
- **No forced-flat window** anywhere. The 2023-11-10 15:06–16:45 rows come from the overlay; 2023-11-14 11:38–11:42 are the stored rows (correct).
- **Mark gap inside the window:** 2024-08-12 10:02–10:03 UTC (2 minutes, `mark_1m_gaps`). It is not a funding stamp. MTM needs a mark on every bar, so **BB must rule** (proposal: carry the last mark for ≤ 2 minutes, disclosed). Until BB rules, a trial that hits this gap aborts and counts. The 2020-12-17 gap is before the start (EL d).
- Funding: 8 h settlements, complete in-window. Settled on the pre-fill position at the 1m mark open at `funding_time`. Visible to signals at `funding_time + 60 s`.

### 2.3 Walk-forward (dates are UTC, inclusive)
- Rolling: train 365 d, purge 2 d, then test 90 d, step 90 d. `train_k = [test_k − 2 d − 365 d, test_k − 2 d)`.
- Train-window trades are flattened at the open of the last `count > 0` minute before the train end, and that flatten pays costs, so no train label uses test-period prices. Embargo is 1 d; it applies only to the CSCV blocks of §6.
- **Remainder rule:** a final remainder < 45 d is appended to the last fold. A remainder ≥ 45 d becomes its own fold.
- **Warmup:** there are no decisions in the warmup period. It is only the feature lookback, read from ≥ 2020-12-18.

| Family | Warmup | First train | First test | Folds | Last fold | **OOS days T** | OOS span |
|---|---|---|---|---:|---|---:|---|
| S01, S02, S03, S05 | 30 d (max lookback 720 h) | 2021-01-17 | 2022-01-19 | 15 | 91 d | **1,351** | 2022-01-19..2025-09-30 |
| S06 | 365 d (W = 365) | 2021-12-18 | 2022-12-20 | 11 | 116 d | **1,016** | 2022-12-20..2025-09-30 |
| later families | per family | | | | | | |

- **Selection per train window:** the highest net annual Sharpe at 5 bps + 2 bps slippage + funding. Ties go to the earlier config in grid order.
- Stitched OOS = the selected config's test-window daily returns, concatenated. Each fold starts from its own opening equity.

## 3. Grids (unchanged from v1; every point is one trial per pinned root)
Bars, decisions and fills are as in v1 §3: decision at bar close + 1 ms, fill at the open of the first 1m bar with `open_time ≥ d` and `count > 0`. Notional is 1× equity, initial cash 1,000,000 USDT, lot 0.001 BTC.

| Family | Grid | Size |
|---|---|---:|
| S01 tsmom-slow | (interval, n) ∈ {(H1,24),(H1,72),(H1,168),(H4,42),(H4,90),(H4,180)} × c ∈ {0.5, 1.0} | 12 |
| S02 donchian-breakout | N ∈ {24, 72, 168} × H ∈ {12, 24, 48} | 9 |
| S03 squeeze-breakout | θ ∈ {0.20, 0.35} × N ∈ {24, 72} × H ∈ {12, 24} | 8 |
| S05 overextension-fade | L ∈ {4, 12} × k ∈ {2.0, 2.5} × H ∈ {4, 12} | 8 |
| S06 session-seasonality | W ∈ {90, 180, 365} × k ∈ {1.0, 2.0} | 6 |
| **First batch** | | **43** |
| S07, S08 (funding now in view), S04, S09–S12 | as v1 | 12, 12, 81, 12, 9, 12, 8 |
| **Queue total (ledger trials)** | | **189** |

**Global N = 189 + 13 legacy exploratory rows = 202.** This is kept from v1. The window change did not change any grid.
- Gate N = max(202, N_global in the ledger at evaluation).
- So the first batch is held to the full-queue hurdle, even if other families never run.

## 4. Costs
- **Fee scenarios:** 2 / 4 / 5 / 6 bps per side taker. The **headline and selection use 5**. The others rerun the same decisions and are reported only.
- **Slippage:** 2 bps per side in every fee scenario, passed explicitly. Stress: 5 bps fee + 5 bps slippage on the selected path, reported, not a gate.
- **Funding: always on.**

## 5. Output: one table per strategy

### 5.1 `table_<slug>.csv`
One row per (config × window × fee_bps), plus the `OOS_stitched` rows. Columns:

| Column | Definition |
|---|---|
| `trial_id`, `config_id`, `slug`, `params` | params as canonical JSON |
| `pinned_root`, `read_path_sha` | `d5ada80d…08` / `9c7fbc3a…cc` in full |
| `window` | `IS_fold_k` / `OOS_fold_k` / `OOS_stitched`; `start`, `end` (UTC dates) |
| `fee_bps`, `slip_bps`, `funding_on` | |
| `days_T` | calendar days with a daily return (flat days = 0) |
| `total_return` | E_end / E_start − 1, net of fees, slippage and funding |
| `sharpe_annual` | mean(r_d) / sd(r_d, ddof 1) × √365, where r_d = daily net return from equity at 00:00 UTC (the close of the 23:59 bar). **Denominator printed as `"daily 00:00Z net, ddof1, sqrt365, T=<days_T>"`** in `sharpe_denominator` |
| `sharpe_daily`, `skew`, `kurt` | kurt is non-excess |
| `max_dd` | per-bar equity curve, rounded up |
| `turnover_ann` | Σ|fill notional| / mean daily equity / (T/365) |
| `n_trades`, `avg_hold_h`, `exposure_pct`, `n_refused_fills` | `n_refused_fills` = fills moved off `count = 0` minutes |
| `fees_paid`, `slippage_cost`, `funding_net` | USDT |
| `fee_sens` | OLS slope of `sharpe_annual` against fee_bps over {2, 4, 5, 6} |
| `breakeven_fee_bps` | interpolated fee at which net SR = 0 (`>6` / `<2` outside the grid) |
| `n_global` | max(202, ledger N at evaluation) |
| `sr_var_cross_trial` | V_ann over the family's configs at 5 bps, IS. At evaluation the gate uses the variance over **all** N_global trials when it is available, else the max of the family's value and 0.25 (sd 0.5) |
| **`dsr_hurdle_sr_ann`** | the smallest annual SR with DSR ≥ 0.95, given `n_global`, `sr_var_cross_trial`, `days_T`, `skew`, `kurt` (§5.2). On `OOS_stitched` rows only |
| `dsr`, `pbo` | on `OOS_stitched` at 5 bps |
| `gate_pass` | all §6 gates |

### 5.2 DSR formula and denominators
Daily units:
- `SR_d = SR_ann / √365`, `V_d = V_ann / 365`.
- `SR0 = √V_d · [(1 − γ) Φ⁻¹(1 − 1/N) + γ Φ⁻¹(1 − 1/(N·e))]`, with γ = 0.5772156649. At N = 202 the bracket is **2.7688**.
- `DSR = Φ( (SR_d − SR0) · √(T − 1) / √(1 − g3·SR_d + (g4 − 1)/4 · SR_d²) )`.
- T = `days_T` of `OOS_stitched`.
- The hurdle is found by bisection on SR_ann. Script: `scratch/queue-v1/wf_dates_v2.py` (arithmetic only).

Planning values (N = 202, g3 = 0, g4 = 3; in brackets: skew −0.5, kurt 8). These are assumptions; the table column uses measured inputs.

| T (OOS days) | families | sd_ann 0.25 | 0.50 | 0.75 |
|---:|---|---:|---:|---:|
| 1,351 | S01, S02, S03, S05 | 1.55 (1.57) | 2.24 (2.28) | 2.94 (2.98) |
| 1,016 | S06 | 1.68 (1.71) | 2.37 (2.41) | 3.07 (3.12) |

SR0 alone (annualized, N = 202) is 0.69 / 1.38 / 2.08.

## 6. Gates (per family, on `OOS_stitched` at 5 bps unless stated)
1. DSR ≥ 0.95 at N = `n_global`, i.e. `sharpe_annual ≥ dsr_hurdle_sr_ann`.
2. PBO ≤ 0.20: CSCV with S = 16 blocks over the family's IS daily-return matrix (warmup excluded), each block boundary purged by max H + 1 d embargo. A family with < 2 configs fails.
3. Net total return > 0 and net SR > 0 at 5 bps + slippage + funding. Also net SR > 0 at 6 bps.
4. Enough trades: ≥ 100 OOS round trips. For S01, ≥ 100 rebalances and ≥ 10 sign changes.
5. No lookahead: the truncation proptest is green for each signal used, and fills are at the next tradable 1m open.
6. Holdout guard log present. No date ≥ 2025-10-01 and none < 2020-12-18 in any artifact.
7. The ledger count equals the trials run, aborted runs included.

## 7. Trial count
- First batch adds **43** (S01 12, S02 9, S03 8, S05 8, S06 6).
- Nothing is logged until runs start: one `RunStarted` per trial, written before the run.
