STATUS: PRE-REGISTRATION, frozen (`chmod 444`) before any run of this family. NOT RUN; no result exists for this family. Nothing has been appended to `research/trials.jsonl` for it. Its sha256 must be written in each `RunStarted` record. After freezing, any change is a new file `-r2`, with N carried over. **POST-HOC (see banner).**

# H3 `h3-donchian-er-gate`: sweep pre-registration (BTCUSDT USD-M perp, second batch)

- Frozen at: `2026-10-23 12:32 KST` · author: Quant Researcher · approved by: Engineering Lead (2026-10-23, order H2, H1, H4, H5, H3; 13 trials) · queue entry: `strategy-queue-v2.md` §2 H3 (rank 5) · runner inputs: `sweep-runner-inputs-v2.md` sha256 `be7f5dd05d9adc9d557355e509a4ab1c33742599a123a0167812b0fbec01dc75`
- Family label (in `config_id`): `"H3"` · window `start_us = 1608249600000000`, `end_us = 1759276800000000` (exclusive) · `initial_cash = 100000000000000` (1,000,000 USDT in 1e8 ticks)
- `config_id` = sha256 of the JCS bytes of `{end_us, family, initial_cash, params, start_us}` (keys sorted; params are i64, non-integers in 1e8 ticks), the rule of `crates/backtest/src/batch.rs` `canonical_config_id` (reproduced for all 43 first-batch configs before use here).

> ## ⚠️ POST-HOC REGISTRATION
> This hypothesis was **shaped after seeing the 2022-01-19..2025-09-30 walk-forward OOS of attempt 1** (code 8afa8d1; evaluation notes `research/notes/2026-10-23-*-attempt1.md`). A walk-forward on the same 2020-12-18..2025-09-30 span is therefore **not clean confirmation**: the OOS here overlaps the data that suggested the idea. This file fixes the rule, grid, trial count and gates before any run of this family, so N and the gates cannot drift, but **a pass on every gate means at most "candidate for holdout"**. Whether the locked holdout (≥ 2025-10-01) is ever opened for it is **reserved for Seogyu** and is not implied by a pass. Template line 7 ("results seen before freezing: none") is knowingly not met; this is disclosed, not waived.

- Prior looks at this idea (each counts toward N): the 9 S02 `donchian-breakout` and 8 S03 `squeeze-breakout` trials of attempt 1 (and S02/S03 in attempt 0); the attempt-1 evaluation (`notes/2026-10-23-donchian-breakout-attempt1.md`, sha256 `7eaf15f35120bf5d…`), which showed S02 lost in chop folds (−0.371 summed) and gained in trend folds (+0.457), with N = 168 selected in 11 of 15 folds. All are already in the 93.
- Results seen before freezing: **yes, S02's and S03's (see banner)**. Nothing of the ER gate was computed.

## 1. Hypothesis and economic rationale
- **Hypothesis:** S02's 168-bar Donchian breakout, taken only when the 168-bar efficiency ratio is at or above its own trailing-year q-quantile and held 2–4 days, earns positive net returns after 5 bps + 2 bps slippage + funding over 2022-12-28..2025-09-30, because the gate skips the choppy regimes where breakouts are stop hunts.
- **Why the edge should exist / who is on the other side:** stops and liquidation clusters beyond the 7-day extremes create follow-on flow (S02's rationale), but only when the market is already trending; in chop the break reverts. The efficiency ratio (net move / path length) separates the two causally. The other side is range traders and short-gamma sellers at the extremes.
- **Falsifier beyond the gates:** (i) the gate does not reduce the share of trades in folds labelled chop (|BTC fold return| ≤ 15%) relative to the same rule with the gate off (reported, computed in the same run as a diagnostic, not a trial); (ii) > 50% of stitched net P&L from one fold.

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
- Bars and decision: UTC-aligned H1 bars from `kline_1m`. Decision at bar close + 1 ms. Fill at the open of the first 1m bar at or after the decision with `count > 0`.
- Channel (S02, N = 168 fixed): `U_t = max(High_(t−168..t−1))`, `L_t = min(Low_(t−168..t−1))` over completed H1 bars, excluding bar t. Break up if `C_t > U_t`, down if `C_t < L_t`.
- Efficiency ratio (`er_l = 168`): `ER_t = |C_t − C_(t−168)| / Σ_(i=t−167..t) |C_i − C_(i−1)|`; undefined if the denominator is 0 (no entry).
- Gate: `G_t` = the `er_q` quantile (linear interpolation, type 7) of `ER` over the previous 8,760 H1 decisions `t−8760..t−1` (strictly past; ≥ 8,000 defined values required, else the gate is closed). **Gate open iff `ER_t ≥ G_t`.**
- Entry: on a break with the gate open, trade in the direction of the break. Exit at the first H1 decision ≥ entry + H. A same-side break during a hold is ignored (no extension). An opposite break during a hold **flips** the position (exit and re-enter at the same fill, hold clock restarts) **only if the gate is open at that decision**; otherwise it is ignored. No entry if `d + H` passes 2025-09-30 23:59 UTC.
- Sizing: ±1 × equity notional / fill price, floored to 0.001 BTC.
- FIXED (not swept): N = 168; ER length 168; quantile window 8,760 h.
- Longest lookback: 8,760 h of ER values, each needing 168 h ⇒ 372 d, + 1 d margin ⇒ **warmup 373 d**.

## 4. Grid (exact)
| grid_index | n | er_l | er_q | hold_h | config_id |
|---|---|---|---|---|---|
| 0 | 168 | 168 | 50000000 | 48 | `99e81cbc994b92b246a3fcdb863b250c731cff1b4e9ed068e95df750407bb0f8` |
| 1 | 168 | 168 | 70000000 | 48 | `7f43666aa758ed05dd70aed3ca761d018622208fdb796c4102d0e3216bf617c1` |
| 2 | 168 | 168 | 50000000 | 96 | `29929df34c9afb473a88832d645ca54b18eb484c7f72f2b085a7ecf4a7fdc487` |
| 3 | 168 | 168 | 70000000 | 96 | `df24286f0ade48d58a104f2bbb0a0e1ccb9564a9faa7133e123f96312dc6e389` |

Preimage of grid_index 0 (for verification): `{"end_us":1759276800000000,"family":"H3","initial_cash":100000000000000,"params":{"er_l":168,"er_q":50000000,"hold_h":48,"n":168},"start_us":1608249600000000}`

- Grid size **4** configs (H ∈ {48, 96} × q ∈ {0.5, 0.7}). **This family adds 4 trials.** Units: `n` / `er_l` H1 bars, `er_q` 1e8 ticks, `hold_h` hours.
- **N_global at freeze** (read from the ledgers, not estimated): **93** distinct trials = 13 legacy `exploratory` rows of `research/trials.jsonl` + 37 distinct `trial_id` in `research/ledger/wf-batch/attempt0-06ea184-trials.jsonl` (sha256 `9e4e782b…`) + 43 distinct `trial_id` in `runs/wf-batch-attempt1-8afa8d1/trials.jsonl` (sha256 `f93f3aa2…`); no overlap. The 5 `wf-batch-eval` rows appended to `research/trials.jsonl` on 2026-10-23 12:21 KST are evaluation records, not trials. **Gate N = max(202, distinct trials in the ledgers at evaluation)**; `strategy-queue-v2.md` keeps the planned total ≤ 202, so the gate N is 202 unless aborted re-runs push the realised count above it.

## 5. Evaluation protocol
- Walk-forward, rolling: train 365 d / purge 2 d / test 90 d / step 90 d (runner-inputs v2 §2.3).
  - Warmup 373 d (see §3 for the longest lookback), no decisions in it; reads start 2020-12-18.
  - First train 2021-12-26; first test 2022-12-28; **11 folds** (last fold 108 d, remainder rule of runner-inputs v2 §2.3).
  - **OOS days T = 1,008** (2022-12-28..2025-09-30).
- Purge 2 d, with every train-window position closed at the train end (runner-inputs v2 §0 C8). Embargo 1 d (CSCV only).
- Costs: fee 2/4/5/6 bps/side (headline and selection 5), slippage 2 bps/side, **funding on**. Stress 5 bps + 5 bps slippage on the selected path, reported, not a gate. Fee scenarios are not trials.
- **Sharpe denominator:** daily net returns from equity at 00:00 UTC (the close of the 23:59 bar), ddof 1, × √365, T = calendar days (flat days count as 0). Printed as `"daily 00:00Z net, ddof1, sqrt365, T=<days_T>"`.
- Each fold starts from its own opening equity; the stitched OOS is the compounded chain of the selected (or FLAT) test windows.
- **Selection with stay-flat (exact; second-batch amendment A1 of `strategy-queue-v2.md`):** for fold k and each config j, let `S_j` be its train-window net annual Sharpe at 5 bps fee + 2 bps slippage + funding on (daily 00:00Z net returns, ddof 1, ×√365, over the train window's calendar days, as computed by the engine). A config with no fill in the train window, or with zero variance of its daily net returns, has `S_j` undefined and is treated as `S_j = 0`. Let `J+ = {j : S_j > 0}` (strictly greater than zero, compared at the engine's precision).
  - If `J+` is empty, **fold k is FLAT**: no order is sent in test window k; every test day's net return is exactly 0; the fold's days still count in T; the fold is reported with `selected = "FLAT"`; it contributes no round trips.
  - Otherwise the selected config is `argmax_{j ∈ J+} S_j`, ties going to the lower `grid_index` (grid order: H ascending, then q ascending).
  - The FLAT option is not a grid point and not a trial. The CSCV matrix (§6 gate 2) contains the K configs only.

| Fold | Train | Test |
|---|---|---|
| 1 | 2021-12-26..2022-12-25 | 2022-12-28..2023-03-27 |
| 2 | 2022-03-26..2023-03-25 | 2023-03-28..2023-06-25 |
| 3 | 2022-06-24..2023-06-23 | 2023-06-26..2023-09-23 |
| 4 | 2022-09-22..2023-09-21 | 2023-09-24..2023-12-22 |
| 5 | 2022-12-21..2023-12-20 | 2023-12-23..2024-03-21 |
| 6 | 2023-03-21..2024-03-19 | 2024-03-22..2024-06-19 |
| 7 | 2023-06-19..2024-06-17 | 2024-06-20..2024-09-17 |
| 8 | 2023-09-17..2024-09-15 | 2024-09-18..2024-12-16 |
| 9 | 2023-12-16..2024-12-14 | 2024-12-17..2025-03-16 |
| 10 | 2024-03-15..2025-03-14 | 2025-03-17..2025-06-14 |
| 11 | 2024-06-13..2025-06-12 | 2025-06-15..2025-09-30 |

- Metrics: runner-inputs v2 §5.1 (see §6 report fields); the gate-off diagnostic of falsifier (i) and the OOS daily correlation with S01/S02 attempt-1 stitched series are reported, never gated or selected.
- **DSR hurdle at T = 1,008, N = 202** (SR_ann needed for DSR ≥ 0.95). Planning values; the gate uses the measured V, skew and kurt:
  - normal returns, cross-trial sd_ann 0.25 / 0.50 / 0.75 → **1.68 / 2.38 / 3.07**;
  - with skew −0.5 and kurt 8 → 1.71 / 2.42 / 3.13.
  - Formula: runner-inputs v2 §5.2 (bracket 2.7688 at N = 202; script `research/scratch/second-batch-prereg/plan.py`, arithmetic only).
- **V reading (fixed):** runner-inputs v2 §5.1 says use the variance over all N_global trials when available, else max(family IS cross-config V, 0.25). Legacy rows carry no Sharpe, so "all N_global" is never available and the family rule applies: V_ann = max(variance across the family's configs of each config's annual Sharpe on the CSCV IS matrix of gate 2, 0.25).

**PBO reading (fixed; resolves the first-batch ambiguity):** the IS matrix M has one column per config and one row per calendar day, built without overlap from the train runs: rows = the full 365 train days of fold 1, followed, for each fold k ≥ 2, by the last 90 days of fold k's train window (the days not in fold k−1's train window), taken from fold k's own train run. Each calendar day appears once; T_IS = 365 + 90·(folds − 1) = **1,265**. CSCV: S = 16 contiguous blocks of ⌊T_IS/16⌋ days (the remainder goes to the last block); all C(16, 8) = 12,870 splits; in each split, the IS half drops the days within 5 d (max hold 96 h = 4 d, + 1 d embargo) of any boundary with an adjacent OOS block. Per half, each config's Sharpe (daily, ddof 1); an undefined Sharpe ranks last. j* = IS-best (ties → lower grid_index); ω = OOS rank of j* (1 = worst … K = best, mid-ranks for ties) / (K + 1); λ = ln(ω / (1 − ω)); **PBO = #(splits with λ ≤ 0) / 12,870**. The OOS-fold-matrix variant used as a sensitivity in the attempt-1 evaluation is reported only, never gated.

## 6. Gates (all on stitched OOS; none is relaxed after freezing)
- [ ] **Gate 1 DSR** ≥ 0.95 at N = max(202, distinct trials in the ledgers at evaluation), V as §5, T = 1,008, measured skew/kurt (non-excess) of the stitched OOS
- [ ] **Gate 2 PBO** ≤ 0.20 on the fixed reading of §5 (K = 4).
- [ ] **Gate 3** net total return > 0 **and** net SR > 0 at 5 bps + 2 bps slippage + funding; **and** net SR > 0 at 6 bps + 2 bps slippage + funding. An all-FLAT stitched OOS has an undefined SR and fails.
- [ ] **Gate 4** ≥ 100 OOS strategy-initiated round trips (a flip counts as one round trip closed). Exits forced by the fold-end flatten or the end of data are counted in `n_trades` but **not** toward this gate (tightening; attempt 1 had ~15 such forced exits per family).
- [ ] **Gate 5 No-lookahead:** the truncation proptest is green for every signal and gate used here; fills at the next tradable 1m open
- [ ] **Gate 6 Holdout:** the runner's partition-refusal and max-key-assertion lines are present **in the run directory log** for every trial; no date ≥ 2025-10-01 and none < 2020-12-18 in any artifact (attempt 1 had no such lines in its log)
- [ ] **Gate 7** trial count = ledger count (4 for this family, aborted runs included)
- Family-specific: none beyond falsifier reporting.

- **Full report fields (required; attempt 1 did not write them):** `table_<slug>.csv` with every runner-inputs v2 §5.1 column (incl. `fees_paid`, `slippage_cost`, `funding_net`, `exposure_pct`, `n_trades`, `avg_hold_h`, `n_refused_fills`, `skew`, `kurt`, `max_dd` on the **per-bar** equity curve, `fee_sens`, `breakeven_fee_bps`, `dsr_hurdle_sr_ann`, `dsr`, `pbo`, `n_global`, `sharpe_denominator`), plus per fold: `selected_grid_index` / `selected_config_id` or `FLAT`, every config's train Sharpe `S_j`, `n_rebalances`, `n_sign_changes`, `n_forced_exits`, and the CSCV IS matrix as a CSV.

## 7. Kill rule and next step
- On any gate FAIL, the family is closed for this pin. A re-test needs a new idea (a new file), not a new grid.
- **On PASS: at most "candidate for holdout" (POST-HOC).** Opening the holdout is Seogyu's decision. Paper-trading (dry-run only, no live keys) only on Seogyu's instruction.

## 8. Code it depends on (being built by EL's cloud agent; none exists at freeze)
- **ER gate on S02:** an efficiency-ratio feature (length param) and a trailing-quantile gate on S02 entries and flips, with a truncation proptest; the gate-off diagnostic path.
- **Stay-flat selection** (amendment A1) in `walkforward.rs` `select_train`.
- Full report fields of §6 and the PBO matrix of §5.

## 9. Known failure modes and risks (stated before results)
- **POST-HOC**: N = 168 and the trend/chop split were read off S02's attempt-1 OOS on this span.
- **The round-trip gate may bind:** S02 N = 168 had roughly 2–4 breaks/month (queue prior); the gate keeps about half (q = 0.5) or 30% (q = 0.7) of them, and the 373-d warmup leaves T = 1,008.
- ER is a lagging trend measure: it opens after the trend has run, so breakouts may be late.
- Overlap with S01/H1/H2 (trend family), so low marginal diversity.
