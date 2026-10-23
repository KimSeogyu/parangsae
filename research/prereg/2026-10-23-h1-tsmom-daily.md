STATUS: PRE-REGISTRATION, frozen (`chmod 444`) before any run of this family. NOT RUN; no result exists for this family. Nothing has been appended to `research/trials.jsonl` for it. Its sha256 must be written in each `RunStarted` record. After freezing, any change is a new file `-r2`, with N carried over. **POST-HOC (see banner).**

# H1 `h1-tsmom-daily`: sweep pre-registration (BTCUSDT USD-M perp, second batch)

- Frozen at: `2026-10-23 12:32 KST` · author: Quant Researcher · approved by: Engineering Lead (2026-10-23, order H2, H1, H4, H5, H3; 13 trials) · queue entry: `strategy-queue-v2.md` §2 H1 (rank 2) · runner inputs: `sweep-runner-inputs-v2.md` sha256 `be7f5dd05d9adc9d557355e509a4ab1c33742599a123a0167812b0fbec01dc75`
- Family label (in `config_id`): `"H1"` · window `start_us = 1608249600000000`, `end_us = 1759276800000000` (exclusive) · `initial_cash = 100000000000000` (1,000,000 USDT in 1e8 ticks)
- `config_id` = sha256 of the JCS bytes of `{end_us, family, initial_cash, params, start_us}` (keys sorted; params are i64, non-integers in 1e8 ticks), the rule of `crates/backtest/src/batch.rs` `canonical_config_id` (reproduced for all 43 first-batch configs before use here).

> ## ⚠️ POST-HOC REGISTRATION
> This hypothesis was **shaped after seeing the 2022-01-19..2025-09-30 walk-forward OOS of attempt 1** (code 8afa8d1; evaluation notes `research/notes/2026-10-23-*-attempt1.md`). A walk-forward on the same 2020-12-18..2025-09-30 span is therefore **not clean confirmation**: the OOS here overlaps the data that suggested the idea. This file fixes the rule, grid, trial count and gates before any run of this family, so N and the gates cannot drift, but **a pass on every gate means at most "candidate for holdout"**. Whether the locked holdout (≥ 2025-10-01) is ever opened for it is **reserved for Seogyu** and is not implied by a pass. Template line 7 ("results seen before freezing: none") is knowingly not met; this is disclosed, not waived.

- Prior looks at this idea (each counts toward N): the 12 S01 trials of attempt 1 and of attempt 0 (same signal; H4 n = 180 is S01 grid points 10/11 at c = 0.5/1.0); the attempt-1 evaluation (`notes/2026-10-23-tsmom-slow-attempt1.md`), which showed S01's gross P&L was positive (+0.164 summed) but its turnover (120×/yr) made it lose. All are already in the 93.
- Results seen before freezing: **yes, S01's (see banner)**. Nothing of this daily-decision rule was computed.

## 1. Hypothesis and economic rationale
- **Hypothesis:** S01's slow H4 time-series-momentum signal, acted on only once a day at 00:00 UTC with a wide deadband (0.5), keeps enough of S01's gross trend P&L while cutting turnover so that net returns after 5 bps + 2 bps slippage + funding are positive over 2022-02-18..2025-09-30.
- **Why the edge should exist / who is on the other side:** as S01 (slow information diffusion, trend-following flows; the other side is mean-reversion liquidity). The change is execution frequency: intraday re-evaluation of a 30–60-day signal mostly trades noise, and each trade costs ≥ 7 bps/side.
- **Falsifier beyond the gates:** (i) the stitched OOS turnover is not at least 3× lower than S01's attempt-1 selected path (120× equity/yr), i.e. > 40×/yr; (ii) > 50% of stitched net P&L from one fold; (iii) beta to BTCUSDT daily returns explains it.

## 2. Data (pinned)
- Read path **v0.3** (unchanged from the first batch):
  - doc `/workspace/reports/research-read-path.md`, sha256 `d5ada80d0c5cb1b9f4ee68e39f2546400522e48ce018f6aaf545a6e015f41108`;
  - SQL `/workspace/reports/research-read-path.sql`, sha256 `9c7fbc3a2b95d0dbc52040ee82e09926f11f47883f3499537c8c0e61f54e65cc`;
  - overlay `/workspace/data/parangsae/runs/resolutions/um-kline-2023-11-10.parquet`, sha256 `d5f6bcd61688cac971b6d276ea4fa0826c86d998b7adc06557686da6bdcfb8ea`.
- **Pinned root:** `pinned_root` = sha256(JCS sorted `[(path, sha256)]` of the three objects above plus every Parquet file opened), computed at run start (runner-inputs v2 §2.1). The first batch ran on `9804991dc4dad1cb9d69c402d5672fa404349285116aa93e4826babeaf0bfc6c`. A family that opens additional datasets (H4: funding as a signal; H5: premium index) gets a different root; that is expected and is recorded, not a new trial by itself.
- **Window:** reads 2020-12-18 00:00 UTC .. 2025-09-30 23:59 UTC only. Nothing earlier (EL d) and nothing ≥ 2025-10-01 00:00 UTC (`HOLDOUT_START_US = 1_759_276_800_000_000`, holdout, double guard of runner-inputs v2 §1).

| Dataset (view) | Range read | Visibility | Gaps and handling |
|---|---|---|---|
| um-kline 1m (`kline_1m`; H1/H4 bars built from it, complete bars only) | 2020-12-18..2025-09-30 | close + 1 ms | none. `count = 0` minutes: no fills (move to the next minute with `count > 0`). The 93 aggTrades-contradicted zero bars stay as a flat carry (disclosed, open DE item) |
| mark 1m (`mark_1m`): MTM, funding notional | same | open + 60 s | 2024-08-12 10:02–10:03 UTC (2 min). **BB ruling pending**; until BB rules, a run hitting it aborts and counts |
| funding (`funding` view): cost only, not a signal input | same | greatest(stored, slot + 60 s) | none in the window (5,244 / 5,244 settlements, checked 2026-10-23 12:20–12:30 KST) |

## 3. Rule (causal)
- Bars: UTC-aligned H4 bars (240/240 minutes) from `kline_1m`.
- **Decision: once a day**, at the close of the H4 bar ending 00:00 UTC (`decision_hour_utc = 0`), + 1 ms. No decision at any other H4 close. Fill at the open of the first 1m bar at or after the decision with `count > 0`.
- Signal: `x_t = ln(C_t / C_(t−n)) / (c · 0.10 · √(n / 42))` on H4 closes; target `w_t = clamp(x_t, −1, 1)` (signals-v0 `tsmom`, S01's H4 scaling).
- Rebalance to `w_t` only when `|w_t − w_cur| ≥ 0.5` (`deadband`), where `w_cur` = current signed notional / equity at the decision. Continuous, no fixed hold. A position whose target has moved by less than 0.5 is kept, even if the target is now 0.
- Sizing: signed notional = w × equity at the decision / fill price, floored to 0.001 BTC; 1× max leverage.
- FIXED (not swept): c = 1.0; r0 = 0.10, n0 = 42 (S01 H4); deadband 0.5; decision hour 0.
- Longest lookback: n = 360 H4 bars = 1,440 h = 60 d ⇒ **warmup 60 d** (both configs use the same folds).

## 4. Grid (exact)
| grid_index | interval | n | c | deadband | decision_hour_utc | config_id |
|---|---|---|---|---|---|---|
| 0 | 240 | 180 | 100000000 | 50000000 | 0 | `36dbcbd7a258b161d956b6871e323ec2c1d17878b8249fff7265f706fd45f4e3` |
| 1 | 240 | 360 | 100000000 | 50000000 | 0 | `0bae20c00c19c281dcc5b3bbaf9c12cb75c2e71b67746c0fd1455c3b096634f0` |

Preimage of grid_index 0 (for verification): `{"end_us":1759276800000000,"family":"H1","initial_cash":100000000000000,"params":{"c":100000000,"deadband":50000000,"decision_hour_utc":0,"interval":240,"n":180},"start_us":1608249600000000}`

- Grid size **2** configs. **This family adds 2 trials.** Units: `interval` minutes, `n` H4 bars, `c` and `deadband` 1e8 ticks, `decision_hour_utc` hours.
- **N_global at freeze** (read from the ledgers, not estimated): **93** distinct trials = 13 legacy `exploratory` rows of `research/trials.jsonl` + 37 distinct `trial_id` in `research/ledger/wf-batch/attempt0-06ea184-trials.jsonl` (sha256 `9e4e782b…`) + 43 distinct `trial_id` in `runs/wf-batch-attempt1-8afa8d1/trials.jsonl` (sha256 `f93f3aa2…`); no overlap. The 5 `wf-batch-eval` rows appended to `research/trials.jsonl` on 2026-10-23 12:21 KST are evaluation records, not trials. **Gate N = max(202, distinct trials in the ledgers at evaluation)**; `strategy-queue-v2.md` keeps the planned total ≤ 202, so the gate N is 202 unless aborted re-runs push the realised count above it.

## 5. Evaluation protocol
- Walk-forward, rolling: train 365 d / purge 2 d / test 90 d / step 90 d (runner-inputs v2 §2.3).
  - Warmup 60 d (see §3 for the longest lookback), no decisions in it; reads start 2020-12-18.
  - First train 2021-02-16; first test 2022-02-18; **15 folds** (last fold 61 d, remainder rule of runner-inputs v2 §2.3).
  - **OOS days T = 1,321** (2022-02-18..2025-09-30).
- Purge 2 d, with every train-window position closed at the train end (runner-inputs v2 §0 C8). Embargo 1 d (CSCV only).
- Costs: fee 2/4/5/6 bps/side (headline and selection 5), slippage 2 bps/side, **funding on**. Stress 5 bps + 5 bps slippage on the selected path, reported, not a gate. Fee scenarios are not trials.
- **Sharpe denominator:** daily net returns from equity at 00:00 UTC (the close of the 23:59 bar), ddof 1, × √365, T = calendar days (flat days count as 0). Printed as `"daily 00:00Z net, ddof1, sqrt365, T=<days_T>"`.
- Each fold starts from its own opening equity; the stitched OOS is the compounded chain of the selected (or FLAT) test windows.
- **Selection with stay-flat (exact; second-batch amendment A1 of `strategy-queue-v2.md`):** for fold k and each config j, let `S_j` be its train-window net annual Sharpe at 5 bps fee + 2 bps slippage + funding on (daily 00:00Z net returns, ddof 1, ×√365, over the train window's calendar days, as computed by the engine). A config with no fill in the train window, or with zero variance of its daily net returns, has `S_j` undefined and is treated as `S_j = 0`. Let `J+ = {j : S_j > 0}` (strictly greater than zero, compared at the engine's precision).
  - If `J+` is empty, **fold k is FLAT**: no order is sent in test window k; every test day's net return is exactly 0; the fold's days still count in T; the fold is reported with `selected = "FLAT"`; it contributes no round trips.
  - Otherwise the selected config is `argmax_{j ∈ J+} S_j`, ties going to the lower `grid_index` (n = 180 before n = 360).
  - The FLAT option is not a grid point and not a trial. The CSCV matrix (§6 gate 2) contains the K configs only.

| Fold | Train | Test |
|---|---|---|
| 1 | 2021-02-16..2022-02-15 | 2022-02-18..2022-05-18 |
| 2 | 2021-05-17..2022-05-16 | 2022-05-19..2022-08-16 |
| 3 | 2021-08-15..2022-08-14 | 2022-08-17..2022-11-14 |
| 4 | 2021-11-13..2022-11-12 | 2022-11-15..2023-02-12 |
| 5 | 2022-02-11..2023-02-10 | 2023-02-13..2023-05-13 |
| 6 | 2022-05-12..2023-05-11 | 2023-05-14..2023-08-11 |
| 7 | 2022-08-10..2023-08-09 | 2023-08-12..2023-11-09 |
| 8 | 2022-11-08..2023-11-07 | 2023-11-10..2024-02-07 |
| 9 | 2023-02-06..2024-02-05 | 2024-02-08..2024-05-07 |
| 10 | 2023-05-07..2024-05-05 | 2024-05-08..2024-08-05 |
| 11 | 2023-08-05..2024-08-03 | 2024-08-06..2024-11-03 |
| 12 | 2023-11-03..2024-11-01 | 2024-11-04..2025-02-01 |
| 13 | 2024-02-01..2025-01-30 | 2025-02-02..2025-05-02 |
| 14 | 2024-05-01..2025-04-30 | 2025-05-03..2025-07-31 |
| 15 | 2024-07-30..2025-07-29 | 2025-08-01..2025-09-30 |

- Metrics: runner-inputs v2 §5.1 (see §6 report fields); OOS daily correlation with S01's attempt-1 stitched series reported.
- **DSR hurdle at T = 1,321, N = 202** (SR_ann needed for DSR ≥ 0.95). Planning values; the gate uses the measured V, skew and kurt:
  - normal returns, cross-trial sd_ann 0.25 / 0.50 / 0.75 → **1.56 / 2.25 / 2.95**;
  - with skew −0.5 and kurt 8 → 1.58 / 2.29 / 2.99.
  - Formula: runner-inputs v2 §5.2 (bracket 2.7688 at N = 202; script `research/scratch/second-batch-prereg/plan.py`, arithmetic only).
- **V reading (fixed):** runner-inputs v2 §5.1 says use the variance over all N_global trials when available, else max(family IS cross-config V, 0.25). Legacy rows carry no Sharpe, so "all N_global" is never available and the family rule applies: V_ann = max(variance across the family's configs of each config's annual Sharpe on the CSCV IS matrix of gate 2, 0.25).

**PBO reading (fixed; resolves the first-batch ambiguity):** the IS matrix M has one column per config and one row per calendar day, built without overlap from the train runs: rows = the full 365 train days of fold 1, followed, for each fold k ≥ 2, by the last 90 days of fold k's train window (the days not in fold k−1's train window), taken from fold k's own train run. Each calendar day appears once; T_IS = 365 + 90·(folds − 1) = **1,625**. CSCV: S = 16 contiguous blocks of ⌊T_IS/16⌋ days (the remainder goes to the last block); all C(16, 8) = 12,870 splits; in each split, the IS half drops the days within 1 d (continuous rule: hold 0 + 1 d embargo) of any boundary with an adjacent OOS block. Per half, each config's Sharpe (daily, ddof 1); an undefined Sharpe ranks last. j* = IS-best (ties → lower grid_index); ω = OOS rank of j* (1 = worst … K = best, mid-ranks for ties) / (K + 1); λ = ln(ω / (1 − ω)); **PBO = #(splits with λ ≤ 0) / 12,870**. The OOS-fold-matrix variant used as a sensitivity in the attempt-1 evaluation is reported only, never gated.

## 6. Gates (all on stitched OOS; none is relaxed after freezing)
- [ ] **Gate 1 DSR** ≥ 0.95 at N = max(202, distinct trials in the ledgers at evaluation), V as §5, T = 1,321, measured skew/kurt (non-excess) of the stitched OOS
- [ ] **Gate 2 PBO** ≤ 0.20 on the fixed reading of §5 (K = 2: λ is ± per split, so PBO is the share of splits where the IS-better config is OOS-worse).
- [ ] **Gate 3** net total return > 0 **and** net SR > 0 at 5 bps + 2 bps slippage + funding; **and** net SR > 0 at 6 bps + 2 bps slippage + funding. An all-FLAT stitched OOS has an undefined SR and fails.
- [ ] **Gate 4** ≥ 100 OOS strategy-initiated rebalances **and** ≥ 10 sign changes (continuous rule, as S01). Arithmetic bound: at most one decision per day over 1,321 days. Exits forced by the fold-end flatten or the end of data are counted in `n_trades` but **not** toward this gate (tightening; attempt 1 had ~15 such forced exits per family).
- [ ] **Gate 5 No-lookahead:** the truncation proptest is green for every signal and gate used here; fills at the next tradable 1m open
- [ ] **Gate 6 Holdout:** the runner's partition-refusal and max-key-assertion lines are present **in the run directory log** for every trial; no date ≥ 2025-10-01 and none < 2020-12-18 in any artifact (attempt 1 had no such lines in its log)
- [ ] **Gate 7** trial count = ledger count (2 for this family, aborted runs included)
- Family-specific (tightening): falsifier (i) — stitched OOS turnover ≤ 40× equity/yr.

- **Full report fields (required; attempt 1 did not write them):** `table_<slug>.csv` with every runner-inputs v2 §5.1 column (incl. `fees_paid`, `slippage_cost`, `funding_net`, `exposure_pct`, `n_trades`, `avg_hold_h`, `n_refused_fills`, `skew`, `kurt`, `max_dd` on the **per-bar** equity curve, `fee_sens`, `breakeven_fee_bps`, `dsr_hurdle_sr_ann`, `dsr`, `pbo`, `n_global`, `sharpe_denominator`), plus per fold: `selected_grid_index` / `selected_config_id` or `FLAT`, every config's train Sharpe `S_j`, `n_rebalances`, `n_sign_changes`, `n_forced_exits`, and the CSCV IS matrix as a CSV.

## 7. Kill rule and next step
- On any gate FAIL, the family is closed for this pin. A re-test needs a new idea (a new file), not a new grid.
- **On PASS: at most "candidate for holdout" (POST-HOC).** Opening the holdout is Seogyu's decision. Paper-trading (dry-run only, no live keys) only on Seogyu's instruction.

## 8. Code it depends on (being built by EL's cloud agent; none exists at freeze)
- **S01 `tsmom` decision-hour and deadband params:** `decision_hour_utc` (decide only at the H4 close at that UTC hour) and `deadband` (1e8 ticks) as strategy params, so these two configs get distinct `config_id`s from S01's. The param key names above are part of the `config_id` preimage and must be used verbatim.
- **Stay-flat selection** (amendment A1) in the walk-forward selection (`walkforward.rs` `select_train`, lines 794–802 at 8afa8d1).
- Full report fields of §6 and the PBO matrix of §5.

## 9. Known failure modes and risks (stated before results)
- **POST-HOC**: S01's H4 n = 180 configs were S01's two best full-span OOS configs on this span (+0.36 / +0.46 SR at c = 0.5 / 1.0). Picking n = 180 was informed by that.
- The rebalance gate may bind: a daily decision with a 0.5 deadband on a slow signal may give fewer than 100 rebalances over 1,321 days.
- The wide deadband leaves stale exposure (up to |0.5| away from target), so chop losses can persist.
- With K = 2, PBO is coarse, and the stay-flat rule can leave whole folds flat.
