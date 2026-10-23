STATUS: PRE-REGISTRATION, frozen (`chmod 444`) before any run of this family. NOT RUN; no result exists for this family. Nothing has been appended to `research/trials.jsonl` for it. Its sha256 must be written in each `RunStarted` record. After freezing, any change is a new file `-r2`, with N carried over. **CONTINGENT: may not start (no `RunStarted`) until the Data Engineer confirms in writing that the premium-index-kline dataset below is the Binance premiumIndex 1m kline for BTCUSDT, its `tick_domain = 'ratio'` 1e8 scaling, its `available_ts` rule, its gaps as listed, and that the engine-v1 reader accepts its schema-1 days.**

# H5 `h5-premium-reversal`: sweep pre-registration (BTCUSDT USD-M perp, second batch)

- Frozen at: `2026-10-23 12:32 KST` · author: Quant Researcher · approved by: Engineering Lead (2026-10-23, order H2, H1, H4, H5, H3; 13 trials) · queue entry: `strategy-queue-v2.md` §2 H5 (rank 4; replaces S09) · runner inputs: `sweep-runner-inputs-v2.md` sha256 `be7f5dd05d9adc9d557355e509a4ab1c33742599a123a0167812b0fbec01dc75`
- Family label (in `config_id`): `"H5"` · window `start_us = 1608249600000000`, `end_us = 1759276800000000` (exclusive) · `initial_cash = 100000000000000` (1,000,000 USDT in 1e8 ticks)
- `config_id` = sha256 of the JCS bytes of `{end_us, family, initial_cash, params, start_us}` (keys sorted; params are i64, non-integers in 1e8 ticks), the rule of `crates/backtest/src/batch.rs` `canonical_config_id` (reproduced for all 43 first-batch configs before use here).

- Prior looks at this idea (each counts toward N): the `regime-exploration` legacy rows used a perp−index basis as a funding **proxy** (descriptive only; already in the 13). The S09 draft (`prereg/drafts/2026-10-22-basis-dislocation-reversal.md`) was never run. The 2026-10-23 coverage check (§2) read premium-index levels only to confirm units (0.1% / 50% / 99.9% quantiles of 1m `close_ticks`); no return, event count, z-score or P&L was computed.
- Results seen before freezing: **none** for this signal.
- **Ex-ante:** no backtest, sweep, event count or P&L of this rule or of any rule on this signal input exists on any data. The first-batch results it follows (all five families rejected, mainly on turnover × cost) motivated *low turnover* in general, not this signal.

## 1. Hypothesis and economic rationale
- **Hypothesis:** when the daily mean of the BTCUSDT premium index is ≥ k (≤ −k) standard deviations above (below) its own previous-90-day mean, the perp underperforms (outperforms) over the next 3–7 days, so a position against the premium (short when rich, long when cheap) earns positive net returns after 5 bps + 2 bps slippage + funding.
- **Why the edge should exist / who is on the other side:** a persistently rich perp means one-sided leveraged demand that pays through funding (Binance funding tracks the premium) and that unwinds once arbitrage capacity and funding costs bite. The other side is urgent leveraged perp takers. Holding days rather than hours is meant to clear the 2 × (5 + 2) bps round-trip cost that sinks the bps-scale hourly version (S09's main risk).
- **Falsifier beyond the gates:** (i) net funding received by the strategy on the stitched OOS is ≤ 0 (fading premium extremes should sit on the funding-receiving side, since funding tracks the premium); (ii) one side carries > 80% of stitched net P&L while the other is negative; (iii) > 50% of stitched net P&L from one fold.

## 2. Data (pinned) and coverage (checked read-only, pre-holdout only, 2026-10-23 12:20–12:30 KST; script `research/scratch/second-batch-prereg/coverage.py`)
- Read path **v0.3** (unchanged from the first batch):
  - doc `/workspace/reports/research-read-path.md`, sha256 `d5ada80d0c5cb1b9f4ee68e39f2546400522e48ce018f6aaf545a6e015f41108`;
  - SQL `/workspace/reports/research-read-path.sql`, sha256 `9c7fbc3a2b95d0dbc52040ee82e09926f11f47883f3499537c8c0e61f54e65cc`;
  - overlay `/workspace/data/parangsae/runs/resolutions/um-kline-2023-11-10.parquet`, sha256 `d5f6bcd61688cac971b6d276ea4fa0826c86d998b7adc06557686da6bdcfb8ea`.
- **Pinned root:** `pinned_root` = sha256(JCS sorted `[(path, sha256)]` of the three objects above plus every Parquet file opened), computed at run start (runner-inputs v2 §2.1). The first batch ran on `9804991dc4dad1cb9d69c402d5672fa404349285116aa93e4826babeaf0bfc6c`. A family that opens additional datasets (H4: funding as a signal; H5: premium index) gets a different root; that is expected and is recorded, not a new trial by itself.
- **Window:** reads 2020-12-18 00:00 UTC .. 2025-09-30 23:59 UTC only. Nothing earlier (EL d) and nothing ≥ 2025-10-01 00:00 UTC (`HOLDOUT_START_US = 1_759_276_800_000_000`, holdout, double guard of runner-inputs v2 §1).

| Dataset | Range read | Visibility | Gaps and handling |
|---|---|---|---|
| **premium-index-kline 1m** (`parquet/premium-index-kline`, `tick_domain = 'ratio'`, 1e8 ticks): signal | 2020-12-18 00:00..2025-09-30 23:59 UTC | open + 60 s (stored `available_ts_us` = open + 60 s, checked on sample rows); a day's mean is usable at the next 00:00:00 UTC | **66 missing minutes** of 2,517,120 in 7 gaps: 2022-07-12 12:56→2022-07-12 12:58 UTC (1 min); 2022-07-12 13:13→2022-07-12 13:22 UTC (8 min); 2022-07-12 13:51→2022-07-12 14:32 UTC (40 min); 2022-07-13 06:58→2022-07-13 07:00 UTC (1 min); 2023-11-10 03:37→2023-11-10 03:42 UTC (4 min); 2023-11-10 03:58→2023-11-10 04:09 UTC (10 min); 2024-08-12 10:01→2024-08-12 10:04 UTC (2 min). **224 `count = 0` rows** (flat-carry fillers) on 10 days (largest 2023-08-29: 86 min; 2021-03-02: 59; 2023-04-10: 33). Handling: a day needs ≥ 1,380 of 1,440 minutes with `count > 0`, else its daily value is missing (only **2023-08-29** fails: 1,354). 2 hours have < 45 valid minutes (unused at daily resolution). **Schema 1 on 472 days** (all of 2020-12-18..2022-01-31 and 62 days in 2022-02..05), schema 2 on 1,276 |
| index-price-kline 1m (`parquet/index-price-kline`, `tick_domain = 'price'`): **diagnostic only** (perp/index − 1 cross-check), not a signal input | same | open + 60 s | 7 missing minutes in 2 gaps: 2022-07-12 13:16→2022-07-12 13:22 UTC (5 min); 2024-08-12 10:01→2024-08-12 10:04 UTC (2 min); 103 `count = 0` rows on 4 days (2022-07-12: 38); 0 hours < 45 valid minutes. Schema 1 on 479 days |
| um-kline 1m (`kline_1m`; H1/H4 bars built from it, complete bars only) | 2020-12-18..2025-09-30 | close + 1 ms | none. `count = 0` minutes: no fills (move to the next minute with `count > 0`). The 93 aggTrades-contradicted zero bars stay as a flat carry (disclosed, open DE item) |
| mark 1m (`mark_1m`): MTM, funding notional | same | open + 60 s | 2024-08-12 10:02–10:03 UTC (2 min). **BB ruling pending**; until BB rules, a run hitting it aborts and counts |
| funding (`funding` view): cost only, not a signal input | same | greatest(stored, slot + 60 s) | none in the window (5,244 / 5,244 settlements, checked 2026-10-23 12:20–12:30 KST) |

- Coverage facts (from `coverage.json`): premium-index 1,748 daily files, concat sha256 `0cc64e093297f2a2ece84075f93680e3b337917192e1cec455223305f9b2d785`; index-price 1,748 files, concat sha256 `50a17f4c0071de8f2950c9a09dfd1b5b393c700525ccbc44aa69c5b2ab3c923f` (sha256 over the per-file sha256 hex digests concatenated in path order). Both have 0 null closes and no gap at the window's head or tail. No dataset gap was filled or repaired; they are reported only.
- **Not in the read path yet:** read path v0.3 has no premium-index view. The DE confirmation (STATUS line) must include the view expression; this prereg's rule assumes exactly the per-minute `close_ticks` / 1e8 with the validity rule above.

## 3. Rule (causal)
- Daily premium: `P_d` = mean of the 1m premium-index `close_ticks / 1e8` over the minutes of UTC day d with `count > 0`, if there are ≥ 1,380 such minutes; else `P_d` is missing.
- `z_d = (P_d − mean(P_(d−90..d−1))) / sd(P_(d−90..d−1), ddof 1)` over the previous 90 calendar days' non-missing values; requires ≥ 80 of them, else no signal. `lookback_d = 90`.
- Decision at 00:00:00.001 UTC of day d + 1. If flat: `z_d ≥ k` ⇒ **short**; `z_d ≤ −k` ⇒ **long**. Fill at the open of the first 1m `kline_1m` bar at or after the decision with `count > 0`.
- Hold H, exit at the first H1 decision ≥ entry + H. Signals during a hold are ignored (no extension, no flip, no pyramiding). No entry if `d + H` passes 2025-09-30 23:59 UTC.
- Sizing: ±1 × equity notional / fill price, floored to 0.001 BTC.
- FIXED (not swept): lookback 90 d; validity thresholds 1,380 / 1,440 minutes and 80 / 90 days.
- Longest lookback: 90 d of daily values + the decision day ⇒ **warmup 91 d**.

## 4. Grid (exact)
| grid_index | k | hold_h | lookback_d | config_id |
|---|---|---|---|---|
| 0 | 200000000 | 72 | 90 | `03789be2fab62f87bbc7cb915a1a5f03e4dccdeca2a73ade9e2fc0ad5240934a` |
| 1 | 200000000 | 168 | 90 | `7a2a4bc53ad9388e4b419d3696e7c6a94c719d2f5b8cc540d4dd6c702c3dd562` |
| 2 | 300000000 | 72 | 90 | `7941ab21e268f1d2cf94b63d2680f2ca5c519a445419639b80a6298658ebfa91` |
| 3 | 300000000 | 168 | 90 | `74d0945a99c6772441a9bf8532e2eecd9d5472337edb14e541a7554702f0a487` |

Preimage of grid_index 0 (for verification): `{"end_us":1759276800000000,"family":"H5","initial_cash":100000000000000,"params":{"hold_h":72,"k":200000000,"lookback_d":90},"start_us":1608249600000000}`

- Grid size **4** configs (k ∈ {2, 3} × H ∈ {72 h, 168 h}). **This family adds 4 trials** and replaces S09's 12. Units: `k` 1e8 ticks, `hold_h` hours, `lookback_d` days.
- **N_global at freeze** (read from the ledgers, not estimated): **93** distinct trials = 13 legacy `exploratory` rows of `research/trials.jsonl` + 37 distinct `trial_id` in `research/ledger/wf-batch/attempt0-06ea184-trials.jsonl` (sha256 `9e4e782b…`) + 43 distinct `trial_id` in `runs/wf-batch-attempt1-8afa8d1/trials.jsonl` (sha256 `f93f3aa2…`); no overlap. The 5 `wf-batch-eval` rows appended to `research/trials.jsonl` on 2026-10-23 12:21 KST are evaluation records, not trials. **Gate N = max(202, distinct trials in the ledgers at evaluation)**; `strategy-queue-v2.md` keeps the planned total ≤ 202, so the gate N is 202 unless aborted re-runs push the realised count above it.

## 5. Evaluation protocol
- Walk-forward, rolling: train 365 d / purge 2 d / test 90 d / step 90 d (runner-inputs v2 §2.3).
  - Warmup 91 d (see §3 for the longest lookback), no decisions in it; reads start 2020-12-18.
  - First train 2021-03-19; first test 2022-03-21; **14 folds** (last fold 120 d, remainder rule of runner-inputs v2 §2.3).
  - **OOS days T = 1,290** (2022-03-21..2025-09-30).
- Purge 2 d, with every train-window position closed at the train end (runner-inputs v2 §0 C8). Embargo 1 d (CSCV only).
- Costs: fee 2/4/5/6 bps/side (headline and selection 5), slippage 2 bps/side, **funding on**. Stress 5 bps + 5 bps slippage on the selected path, reported, not a gate. Fee scenarios are not trials.
- **Sharpe denominator:** daily net returns from equity at 00:00 UTC (the close of the 23:59 bar), ddof 1, × √365, T = calendar days (flat days count as 0). Printed as `"daily 00:00Z net, ddof1, sqrt365, T=<days_T>"`.
- Each fold starts from its own opening equity; the stitched OOS is the compounded chain of the selected (or FLAT) test windows.
- **Selection with stay-flat (exact; second-batch amendment A1 of `strategy-queue-v2.md`):** for fold k and each config j, let `S_j` be its train-window net annual Sharpe at 5 bps fee + 2 bps slippage + funding on (daily 00:00Z net returns, ddof 1, ×√365, over the train window's calendar days, as computed by the engine). A config with no fill in the train window, or with zero variance of its daily net returns, has `S_j` undefined and is treated as `S_j = 0`. Let `J+ = {j : S_j > 0}` (strictly greater than zero, compared at the engine's precision).
  - If `J+` is empty, **fold k is FLAT**: no order is sent in test window k; every test day's net return is exactly 0; the fold's days still count in T; the fold is reported with `selected = "FLAT"`; it contributes no round trips.
  - Otherwise the selected config is `argmax_{j ∈ J+} S_j`, ties going to the lower `grid_index` (grid order: k ascending, then H ascending).
  - The FLAT option is not a grid point and not a trial. The CSCV matrix (§6 gate 2) contains the K configs only.

| Fold | Train | Test |
|---|---|---|
| 1 | 2021-03-19..2022-03-18 | 2022-03-21..2022-06-18 |
| 2 | 2021-06-17..2022-06-16 | 2022-06-19..2022-09-16 |
| 3 | 2021-09-15..2022-09-14 | 2022-09-17..2022-12-15 |
| 4 | 2021-12-14..2022-12-13 | 2022-12-16..2023-03-15 |
| 5 | 2022-03-14..2023-03-13 | 2023-03-16..2023-06-13 |
| 6 | 2022-06-12..2023-06-11 | 2023-06-14..2023-09-11 |
| 7 | 2022-09-10..2023-09-09 | 2023-09-12..2023-12-10 |
| 8 | 2022-12-09..2023-12-08 | 2023-12-11..2024-03-09 |
| 9 | 2023-03-09..2024-03-07 | 2024-03-10..2024-06-07 |
| 10 | 2023-06-07..2024-06-05 | 2024-06-08..2024-09-05 |
| 11 | 2023-09-05..2024-09-03 | 2024-09-06..2024-12-04 |
| 12 | 2023-12-04..2024-12-02 | 2024-12-05..2025-03-04 |
| 13 | 2024-03-03..2025-03-02 | 2025-03-05..2025-06-02 |
| 14 | 2024-06-01..2025-05-31 | 2025-06-03..2025-09-30 |

- Metrics: runner-inputs v2 §5.1 (see §6 report fields), plus per side (rich-fade shorts vs cheap-fade longs): n, net P&L, funding received.
- **DSR hurdle at T = 1,290, N = 202** (SR_ann needed for DSR ≥ 0.95). Planning values; the gate uses the measured V, skew and kurt:
  - normal returns, cross-trial sd_ann 0.25 / 0.50 / 0.75 → **1.57 / 2.26 / 2.96**;
  - with skew −0.5 and kurt 8 → 1.59 / 2.30 / 3.00.
  - Formula: runner-inputs v2 §5.2 (bracket 2.7688 at N = 202; script `research/scratch/second-batch-prereg/plan.py`, arithmetic only).
- **V reading (fixed):** runner-inputs v2 §5.1 says use the variance over all N_global trials when available, else max(family IS cross-config V, 0.25). Legacy rows carry no Sharpe, so "all N_global" is never available and the family rule applies: V_ann = max(variance across the family's configs of each config's annual Sharpe on the CSCV IS matrix of gate 2, 0.25).

**PBO reading (fixed; resolves the first-batch ambiguity):** the IS matrix M has one column per config and one row per calendar day, built without overlap from the train runs: rows = the full 365 train days of fold 1, followed, for each fold k ≥ 2, by the last 90 days of fold k's train window (the days not in fold k−1's train window), taken from fold k's own train run. Each calendar day appears once; T_IS = 365 + 90·(folds − 1) = **1,535**. CSCV: S = 16 contiguous blocks of ⌊T_IS/16⌋ days (the remainder goes to the last block); all C(16, 8) = 12,870 splits; in each split, the IS half drops the days within 8 d (max hold 168 h = 7 d, + 1 d embargo) of any boundary with an adjacent OOS block. Per half, each config's Sharpe (daily, ddof 1); an undefined Sharpe ranks last. j* = IS-best (ties → lower grid_index); ω = OOS rank of j* (1 = worst … K = best, mid-ranks for ties) / (K + 1); λ = ln(ω / (1 − ω)); **PBO = #(splits with λ ≤ 0) / 12,870**. The OOS-fold-matrix variant used as a sensitivity in the attempt-1 evaluation is reported only, never gated.

## 6. Gates (all on stitched OOS; none is relaxed after freezing)
- [ ] **Gate 1 DSR** ≥ 0.95 at N = max(202, distinct trials in the ledgers at evaluation), V as §5, T = 1,290, measured skew/kurt (non-excess) of the stitched OOS
- [ ] **Gate 2 PBO** ≤ 0.20 on the fixed reading of §5 (K = 4).
- [ ] **Gate 3** net total return > 0 **and** net SR > 0 at 5 bps + 2 bps slippage + funding; **and** net SR > 0 at 6 bps + 2 bps slippage + funding. An all-FLAT stitched OOS has an undefined SR and fails.
- [ ] **Gate 4** ≥ 100 OOS strategy-initiated round trips. Arithmetic bound (no data): over 1,290 days, 100 round trips need a qualifying day at least every ~13 days at H = 72 h and a position on ≥ ~54% of days at H = 168 h. Exits forced by the fold-end flatten or the end of data are counted in `n_trades` but **not** toward this gate (tightening; attempt 1 had ~15 such forced exits per family).
- [ ] **Gate 5 No-lookahead:** the truncation proptest is green for every signal and gate used here; fills at the next tradable 1m open
- [ ] **Gate 6 Holdout:** the runner's partition-refusal and max-key-assertion lines are present **in the run directory log** for every trial; no date ≥ 2025-10-01 and none < 2020-12-18 in any artifact (attempt 1 had no such lines in its log)
- [ ] **Gate 7** trial count = ledger count (4 for this family, aborted runs included)
- Family-specific: none (falsifiers (i)–(iii) reported, not gated).

- **Full report fields (required; attempt 1 did not write them):** `table_<slug>.csv` with every runner-inputs v2 §5.1 column (incl. `fees_paid`, `slippage_cost`, `funding_net`, `exposure_pct`, `n_trades`, `avg_hold_h`, `n_refused_fills`, `skew`, `kurt`, `max_dd` on the **per-bar** equity curve, `fee_sens`, `breakeven_fee_bps`, `dsr_hurdle_sr_ann`, `dsr`, `pbo`, `n_global`, `sharpe_denominator`), plus per fold: `selected_grid_index` / `selected_config_id` or `FLAT`, every config's train Sharpe `S_j`, `n_rebalances`, `n_sign_changes`, `n_forced_exits`, and the CSCV IS matrix as a CSV.

## 7. Kill rule and next step
- On any gate FAIL, the family is closed for this pin. A re-test needs a new idea (a new file), not a new grid.
- On PASS: paper-trade the selected config (dry-run only, no live keys). Opening the holdout requires the user's approval.

## 8. Code it depends on (none exists at freeze)
- **Premium-index stream in the strategy view** (engine-v1 has funding only; index/premium "come later", runner-inputs v2 §0 C2) with a read-path view and a truncation proptest. **No EL work item for it was named in the approval; EL must assign one.**
- **Premium-reversal strategy:** daily mean, 90-d z, non-overlapping holds.
- **Stay-flat selection** (amendment A1) in `walkforward.rs` `select_train`.
- Full report fields of §6, the per-side split and the PBO matrix of §5.

## 9. Known failure modes and risks (stated before results)
- **Contingent on DE** (STATUS line); the 472 schema-1 days must be readable, or the window shrinks and this file must be re-issued as `-r2` before any run.
- **The round-trip gate is likely to bind**, especially at k = 3 (see gate 4 arithmetic); disclosed as a design choice, not to be adjusted after results.
- The premium is mean-reverting by construction (funding arbitrage), but the perp's price over 3–7 days is dominated by BTC direction, not by the premium's few bps.
- Regime shift: premium levels changed after 2021 (lower leverage demand, ETF-era spot flows), so a 90-d z may flag level shifts rather than dislocations.
