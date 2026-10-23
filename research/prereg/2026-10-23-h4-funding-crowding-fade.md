STATUS: PRE-REGISTRATION, frozen (`chmod 444`) before any run of this family. NOT RUN; no result exists for this family. Nothing has been appended to `research/trials.jsonl` for it. Its sha256 must be written in each `RunStarted` record. After freezing, any change is a new file `-r2`, with N carried over.

# H4 `h4-funding-crowding-fade`: sweep pre-registration (BTCUSDT USD-M perp, second batch)

- Frozen at: `2026-10-23 12:32 KST` · author: Quant Researcher · approved by: Engineering Lead (2026-10-23, order H2, H1, H4, H5, H3; 13 trials) · queue entry: `strategy-queue-v2.md` §2 H4 (rank 3; replaces S08's grid) · runner inputs: `sweep-runner-inputs-v2.md` sha256 `be7f5dd05d9adc9d557355e509a4ab1c33742599a123a0167812b0fbec01dc75`
- Family label (in `config_id`): `"H4"` · window `start_us = 1608249600000000`, `end_us = 1759276800000000` (exclusive) · `initial_cash = 100000000000000` (1,000,000 USDT in 1e8 ticks)
- `config_id` = sha256 of the JCS bytes of `{end_us, family, initial_cash, params, start_us}` (keys sorted; params are i64, non-integers in 1e8 ticks), the rule of `crates/backtest/src/batch.rs` `canonical_config_id` (reproduced for all 43 first-batch configs before use here).

- Prior looks at this idea (each counts toward N): ledger rows 7–13 of `research/trials.jsonl` (`regime-exploration`, settled funding, descriptive only: clustering, sign persistence, runs); already in the 13. The S08 draft (`prereg/drafts/2026-10-22-funding-crowding-unwind.md`) was never run. The 2026-10-23 coverage check (§2) counted settlements and read the min / median / max of `rate_ticks` to confirm units; no return, event count or P&L was computed.
- Results seen before freezing: **none** for this signal.
- **Ex-ante:** no backtest, sweep, event count or P&L of this rule or of any rule on this signal input exists on any data. The first-batch results it follows (all five families rejected, mainly on turnover × cost) motivated *low turnover* in general, not this signal.

## 1. Hypothesis and economic rationale
- **Hypothesis:** when the 24 h sum of settled BTCUSDT funding is in the top (bottom) decile of its own trailing year and price has already fallen (risen) over the next 24 h, crowded longs (shorts) are unwinding, and a position against the crowded side held 3–7 days earns positive net returns after 5 bps + 2 bps slippage + funding.
- **Why the edge should exist / who is on the other side:** persistent high funding means leveraged longs pay to stay financed, mostly to basis arbitrageurs. That positioning is fragile: an adverse move forces stop-outs and liquidations, which feed the move over days, and the position on the non-crowded side also receives funding. The other side is the crowded leveraged perp side. Funding alone does not time it; the 24 h price confirmation is the trigger.
- **Falsifier beyond the gates:** (i) funding received is negative or < 10% of gross P&L on the stitched OOS (the "paid to wait" leg is absent); (ii) one side (crowded-long fades or crowded-short fades) carries > 80% of the stitched net P&L while the other side's net is negative (reported per side); (iii) > 50% of stitched net P&L from one fold.

## 2. Data (pinned) and coverage (checked read-only, pre-holdout only, 2026-10-23 12:20–12:30 KST; script `research/scratch/second-batch-prereg/coverage.py`)
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
| **funding (`funding` view over `runs/funding-4db8a75/`): signal input and cost** | 2020-12-18 00:00..2025-09-30 16:00 UTC settlements | greatest(stored, slot + 60 s); **never the raw column** | **none.** In-window: 5,244 / 5,244 expected 8 h settlements, all spacings exactly 8 h (5,243 of 5,243), 0 off the 00/08/16 UTC grid, `interval_hours` = 8 on every row, 0 null rates. Raw `available_ts_us == funding_time_us` on all 5,244 rows (legacy stamp), hence the view's +60 s |

- Coverage facts (from `coverage.json`): store `parquet/funding-rate`: 1,748 daily files 2020-12-18..2025-09-30, sha256 over the per-file sha256 hex digests concatenated in path order `2a671281929e93afbd3f462a74ea0635060ff1ec248302815b80a81c954e1e0f`. View source `runs/funding-4db8a75/parquet/funding-rate`: 1,748 in-window files, same 5,244 settlements, concat sha256 `88f610abe369bd37e4ff4f700a0c1d560d4fe10c115637676029b7acfa939c2b`.
- **Risk recorded, not fixed:** the read-path `funding` view globs `runs/funding-4db8a75/.../*/part.parquet`, which also contains partitions ≥ 2025-10-01. Only the runner's partition refusal (runner-inputs v2 §1 a) keeps them out; gate 6 requires its log line.
- First usable decision: F24 needs 3 settlements and its quantile needs 365 d of F24 history ⇒ warmup 366 d.

## 3. Rule (causal)
- Events: each settlement s at `funding_time` ft ∈ {00, 08, 16} UTC, visible at ft + 60 s.
- `F24_s = r_s + r_(s−1) + r_(s−2)` (the last 3 settled rates; `sum_settlements = 3`).
- Tails: `Q_hi` / `Q_lo` = the (1 − q) / q quantiles (linear interpolation, type 7) of `F24` over the settlements in `[ft − 365 d, ft)` (strictly past; 1,095 values). **Long-crowded** if `F24_s > 0` and `F24_s > Q_hi` (strict); **short-crowded** if `F24_s < 0` and `F24_s < Q_lo` (strict). Ties do not qualify.
- Confirmation over C = 24 h: `R_C = ln(close of the 1m bar ending ft + 24 h / close of the 1m bar ending ft)` on `kline_1m`. Long-crowded and `R_C < 0` ⇒ **short**; short-crowded and `R_C > 0` ⇒ **long**; otherwise no trade.
- Decision at ft + 24 h + 1 ms (an H1 close). Entry at the open of the first 1m bar at or after it with `count > 0`, only if flat. At most one entry per event. Events whose decision falls while a position is open are skipped. No flips, no pyramiding.
- Exit at the first H1 decision ≥ entry + H. No entry if `d + H` passes 2025-09-30 23:59 UTC.
- Sizing: ±1 × equity notional / fill price, floored to 0.001 BTC. Funding is paid/received by the engine on the held position at each settlement.
- FIXED (not swept): q = 0.10; C = 24 h; trailing window 365 d; 3-settlement sum; the sign requirement.

## 4. Grid (exact)
| grid_index | q | confirm_h | sum_settlements | hold_h | config_id |
|---|---|---|---|---|---|
| 0 | 10000000 | 24 | 3 | 72 | `f225df71988599ef4623c16edbd2c7ea4a2fcff37f0ccf852ddbc9ba146caffe` |
| 1 | 10000000 | 24 | 3 | 168 | `7edda12efcc21de362ef8e25144c4eed6086f8a6f108997ac08e7653fb0fc4ba` |

Preimage of grid_index 0 (for verification): `{"end_us":1759276800000000,"family":"H4","initial_cash":100000000000000,"params":{"confirm_h":24,"hold_h":72,"q":10000000,"sum_settlements":3},"start_us":1608249600000000}`

- Grid size **2** configs. **This family adds 2 trials** and replaces S08's 12-point grid (queue v2). Units: `q` 1e8 ticks, `confirm_h` / `hold_h` hours.
- **N_global at freeze** (read from the ledgers, not estimated): **93** distinct trials = 13 legacy `exploratory` rows of `research/trials.jsonl` + 37 distinct `trial_id` in `research/ledger/wf-batch/attempt0-06ea184-trials.jsonl` (sha256 `9e4e782b…`) + 43 distinct `trial_id` in `runs/wf-batch-attempt1-8afa8d1/trials.jsonl` (sha256 `f93f3aa2…`); no overlap. The 5 `wf-batch-eval` rows appended to `research/trials.jsonl` on 2026-10-23 12:21 KST are evaluation records, not trials. **Gate N = max(202, distinct trials in the ledgers at evaluation)**; `strategy-queue-v2.md` keeps the planned total ≤ 202, so the gate N is 202 unless aborted re-runs push the realised count above it.

## 5. Evaluation protocol
- Walk-forward, rolling: train 365 d / purge 2 d / test 90 d / step 90 d (runner-inputs v2 §2.3).
  - Warmup 366 d (see §3 for the longest lookback), no decisions in it; reads start 2020-12-18.
  - First train 2021-12-19; first test 2022-12-21; **11 folds** (last fold 115 d, remainder rule of runner-inputs v2 §2.3).
  - **OOS days T = 1,015** (2022-12-21..2025-09-30).
- Purge 2 d, with every train-window position closed at the train end (runner-inputs v2 §0 C8). Embargo 1 d (CSCV only).
- Costs: fee 2/4/5/6 bps/side (headline and selection 5), slippage 2 bps/side, **funding on**. Stress 5 bps + 5 bps slippage on the selected path, reported, not a gate. Fee scenarios are not trials.
- **Sharpe denominator:** daily net returns from equity at 00:00 UTC (the close of the 23:59 bar), ddof 1, × √365, T = calendar days (flat days count as 0). Printed as `"daily 00:00Z net, ddof1, sqrt365, T=<days_T>"`.
- Each fold starts from its own opening equity; the stitched OOS is the compounded chain of the selected (or FLAT) test windows.
- **Selection with stay-flat (exact; second-batch amendment A1 of `strategy-queue-v2.md`):** for fold k and each config j, let `S_j` be its train-window net annual Sharpe at 5 bps fee + 2 bps slippage + funding on (daily 00:00Z net returns, ddof 1, ×√365, over the train window's calendar days, as computed by the engine). A config with no fill in the train window, or with zero variance of its daily net returns, has `S_j` undefined and is treated as `S_j = 0`. Let `J+ = {j : S_j > 0}` (strictly greater than zero, compared at the engine's precision).
  - If `J+` is empty, **fold k is FLAT**: no order is sent in test window k; every test day's net return is exactly 0; the fold's days still count in T; the fold is reported with `selected = "FLAT"`; it contributes no round trips.
  - Otherwise the selected config is `argmax_{j ∈ J+} S_j`, ties going to the lower `grid_index` (H = 72 before H = 168).
  - The FLAT option is not a grid point and not a trial. The CSCV matrix (§6 gate 2) contains the K configs only.

| Fold | Train | Test |
|---|---|---|
| 1 | 2021-12-19..2022-12-18 | 2022-12-21..2023-03-20 |
| 2 | 2022-03-19..2023-03-18 | 2023-03-21..2023-06-18 |
| 3 | 2022-06-17..2023-06-16 | 2023-06-19..2023-09-16 |
| 4 | 2022-09-15..2023-09-14 | 2023-09-17..2023-12-15 |
| 5 | 2022-12-14..2023-12-13 | 2023-12-16..2024-03-14 |
| 6 | 2023-03-14..2024-03-12 | 2024-03-15..2024-06-12 |
| 7 | 2023-06-12..2024-06-10 | 2024-06-13..2024-09-10 |
| 8 | 2023-09-10..2024-09-08 | 2024-09-11..2024-12-09 |
| 9 | 2023-12-09..2024-12-07 | 2024-12-10..2025-03-09 |
| 10 | 2024-03-08..2025-03-07 | 2025-03-10..2025-06-07 |
| 11 | 2024-06-06..2025-06-05 | 2025-06-08..2025-09-30 |

- Metrics: runner-inputs v2 §5.1 (see §6 report fields), plus per side (crowded-long fades vs crowded-short fades): n, net P&L, funding received; number of distinct funding episodes (runs of consecutive qualifying settlements).
- **DSR hurdle at T = 1,015, N = 202** (SR_ann needed for DSR ≥ 0.95). Planning values; the gate uses the measured V, skew and kurt:
  - normal returns, cross-trial sd_ann 0.25 / 0.50 / 0.75 → **1.68 / 2.38 / 3.07**;
  - with skew −0.5 and kurt 8 → 1.71 / 2.42 / 3.12.
  - Formula: runner-inputs v2 §5.2 (bracket 2.7688 at N = 202; script `research/scratch/second-batch-prereg/plan.py`, arithmetic only).
- **V reading (fixed):** runner-inputs v2 §5.1 says use the variance over all N_global trials when available, else max(family IS cross-config V, 0.25). Legacy rows carry no Sharpe, so "all N_global" is never available and the family rule applies: V_ann = max(variance across the family's configs of each config's annual Sharpe on the CSCV IS matrix of gate 2, 0.25).

**PBO reading (fixed; resolves the first-batch ambiguity):** the IS matrix M has one column per config and one row per calendar day, built without overlap from the train runs: rows = the full 365 train days of fold 1, followed, for each fold k ≥ 2, by the last 90 days of fold k's train window (the days not in fold k−1's train window), taken from fold k's own train run. Each calendar day appears once; T_IS = 365 + 90·(folds − 1) = **1,265**. CSCV: S = 16 contiguous blocks of ⌊T_IS/16⌋ days (the remainder goes to the last block); all C(16, 8) = 12,870 splits; in each split, the IS half drops the days within 8 d (max hold 168 h = 7 d, + 1 d embargo) of any boundary with an adjacent OOS block. Per half, each config's Sharpe (daily, ddof 1); an undefined Sharpe ranks last. j* = IS-best (ties → lower grid_index); ω = OOS rank of j* (1 = worst … K = best, mid-ranks for ties) / (K + 1); λ = ln(ω / (1 − ω)); **PBO = #(splits with λ ≤ 0) / 12,870**. The OOS-fold-matrix variant used as a sensitivity in the attempt-1 evaluation is reported only, never gated.

## 6. Gates (all on stitched OOS; none is relaxed after freezing)
- [ ] **Gate 1 DSR** ≥ 0.95 at N = max(202, distinct trials in the ledgers at evaluation), V as §5, T = 1,015, measured skew/kurt (non-excess) of the stitched OOS
- [ ] **Gate 2 PBO** ≤ 0.20 on the fixed reading of §5 (K = 2).
- [ ] **Gate 3** net total return > 0 **and** net SR > 0 at 5 bps + 2 bps slippage + funding; **and** net SR > 0 at 6 bps + 2 bps slippage + funding. An all-FLAT stitched OOS has an undefined SR and fails.
- [ ] **Gate 4** ≥ 100 OOS strategy-initiated round trips. Arithmetic bound (no data): with hold 168 h (+ the 24 h confirmation), 100 round trips over 1,015 days need a position on ≥ ~69% of days; with hold 72 h, ≥ ~30%. Exits forced by the fold-end flatten or the end of data are counted in `n_trades` but **not** toward this gate (tightening; attempt 1 had ~15 such forced exits per family).
- [ ] **Gate 5 No-lookahead:** the truncation proptest is green for every signal and gate used here; fills at the next tradable 1m open
- [ ] **Gate 6 Holdout:** the runner's partition-refusal and max-key-assertion lines are present **in the run directory log** for every trial; no date ≥ 2025-10-01 and none < 2020-12-18 in any artifact (attempt 1 had no such lines in its log)
- [ ] **Gate 7** trial count = ledger count (2 for this family, aborted runs included)
- Family-specific: none (falsifiers (i)–(iii) reported, not gated).

- **Full report fields (required; attempt 1 did not write them):** `table_<slug>.csv` with every runner-inputs v2 §5.1 column (incl. `fees_paid`, `slippage_cost`, `funding_net`, `exposure_pct`, `n_trades`, `avg_hold_h`, `n_refused_fills`, `skew`, `kurt`, `max_dd` on the **per-bar** equity curve, `fee_sens`, `breakeven_fee_bps`, `dsr_hurdle_sr_ann`, `dsr`, `pbo`, `n_global`, `sharpe_denominator`), plus per fold: `selected_grid_index` / `selected_config_id` or `FLAT`, every config's train Sharpe `S_j`, `n_rebalances`, `n_sign_changes`, `n_forced_exits`, and the CSCV IS matrix as a CSV.

## 7. Kill rule and next step
- On any gate FAIL, the family is closed for this pin. A re-test needs a new idea (a new file), not a new grid.
- On PASS: paper-trade the selected config (dry-run only, no live keys). Opening the holdout requires the user's approval.

## 8. Code it depends on (being built by EL's cloud agent; none exists at freeze)
- **Funding-crowding fade strategy** reading the engine-v1 `funding` view as a signal (F24, trailing-365-d quantiles, 24 h price confirmation, non-overlapping holds) with a truncation proptest on the funding stream.
- **Stay-flat selection** (amendment A1) in `walkforward.rs` `select_train`.
- Full report fields of §6, per-side split and the PBO matrix of §5.

## 9. Known failure modes and risks (stated before results)
- **The round-trip gate is likely to bind** (see the arithmetic in gate 4): tail events cluster into a few episodes, and the 3–7 d holds cap the count. This was a design choice for low turnover; it is disclosed here, not adjusted after results.
- Post-2021 funding is pinned near the 0.01% baseline, so the trailing tails may be tiny deviations, with many ties that do not qualify.
- Funding is pro-cyclical, so this shorts bull trends (2024) when longs are crowded but price keeps rising after a 24 h dip.
- Only 11 folds and T = 1,015; the DSR hurdle is higher than the 1,351-day families'.
