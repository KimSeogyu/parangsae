STATUS: PRE-REGISTRATION, frozen (`chmod 444`) before any run of this family. NOT RUN; no result exists for this family. Nothing has been appended to `research/trials.jsonl` for it. Its sha256 must be written in each `RunStarted` record. After freezing, any change is a new file `-r2`, with N carried over. **POST-HOC (see banner).**

# H2 `h2-tsmom-ensemble`: sweep pre-registration (BTCUSDT USD-M perp, second batch)

- Frozen at: `2026-10-23 12:32 KST` · author: Quant Researcher · approved by: Engineering Lead (2026-10-23, order H2, H1, H4, H5, H3; 13 trials) · queue entry: `strategy-queue-v2.md` §2 H2 (rank 1) · runner inputs: `sweep-runner-inputs-v2.md` sha256 `be7f5dd05d9adc9d557355e509a4ab1c33742599a123a0167812b0fbec01dc75`
- Family label (in `config_id`): `"H2"` · window `start_us = 1608249600000000`, `end_us = 1759276800000000` (exclusive) · `initial_cash = 100000000000000` (1,000,000 USDT in 1e8 ticks)
- `config_id` = sha256 of the JCS bytes of `{end_us, family, initial_cash, params, start_us}` (keys sorted; params are i64, non-integers in 1e8 ticks), the rule of `crates/backtest/src/batch.rs` `canonical_config_id` (reproduced for all 43 first-batch configs before use here).

> ## ⚠️ POST-HOC REGISTRATION
> This hypothesis was **shaped after seeing the 2022-01-19..2025-09-30 walk-forward OOS of attempt 1** (code 8afa8d1; evaluation notes `research/notes/2026-10-23-*-attempt1.md`). A walk-forward on the same 2020-12-18..2025-09-30 span is therefore **not clean confirmation**: the OOS here overlaps the data that suggested the idea. This file fixes the rule, grid, trial count and gates before any run of this family, so N and the gates cannot drift, but **a pass on every gate means at most "candidate for holdout"**. Whether the locked holdout (≥ 2025-10-01) is ever opened for it is **reserved for Seogyu** and is not implied by a pass. Template line 7 ("results seen before freezing: none") is knowingly not met; this is disclosed, not waived.

- Prior looks at this idea (each counts toward N): the 12 S01 `tsmom-slow` trials of attempt 1 (its members; ledger `f93f3aa2…`) and the 12 S01 trials of attempt 0; the attempt-1 evaluation (`notes/2026-10-23-tsmom-slow-attempt1.md`, sha256 `f54bf866662a498a…`), which reported per-config OOS Sharpes and an unstable selection (10 switches in 14 transitions). All are already in the 93.
- Results seen before freezing: **yes, S01's (see banner)**. None of this rule (the average) was computed.

## 1. Hypothesis and economic rationale
- **Hypothesis:** an equal-weight average of the 12 S01 `tsmom` targets, traded as one position with no per-fold selection, earns positive net returns after 5 bps + 2 bps slippage + funding over 2022-01-19..2025-09-30, because averaging removes S01's selection noise and nets the members' opposing trades.
- **Why the edge should exist / who is on the other side:** the same slow-diffusion and trend-following-flow argument as S01 (other side: mean-reversion liquidity and early contrarians). What changes is the implementation: S01's per-fold choice among 12 noisy configs (mean Spearman IS→OOS rank 0.22 over 15 folds) is replaced by a fixed diversified blend whose target moves more smoothly, so fewer deadband crossings are paid for.
- **Falsifier beyond the gates:** (i) the blend's OOS Sharpe is below the median of its 12 members' full-span OOS Sharpes on the same span (it adds nothing over picking at random); (ii) > 50% of stitched net P&L comes from one fold; (iii) beta to BTCUSDT daily returns explains it (|beta| reported).

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

- No forced-flat window (2023-11-10 conflicts resolved by v0.3).

## 3. Rule (causal)
- Members: the 12 S01 configs, exactly as frozen in `prereg/2026-10-23-tsmom-slow.md` (sha256 `cead045c…`) §3: member i computes `x_i = ln(C_t / C_(t−n)) / (c · r0 · √(n / n0))` on its own UTC-aligned bars (H1: r0 = 0.05, n0 = 24; H4: r0 = 0.10, n0 = 42) and its raw target `w_i = clamp(x_i, −1, 1)` at each of its bar closes. **Members' own deadbands are not applied**; between its bar closes a member's target is its last value.

| member (S01 grid_index) | interval (min) | n (bars) | c (1e8 ticks) | S01 config_id |
|---|---|---|---|---|
| 0 | 60 | 24 | 50000000 | `6bcc66cda3c0dfda7dd5cdb037ed7569aef35fc3994ffc4c1d7933aca2bda139` |
| 1 | 60 | 24 | 100000000 | `90101491abb405f635639ad2c17640543a97f3bda1d8863446c26e16528a5ecc` |
| 2 | 60 | 72 | 50000000 | `a9e45b27e8d2dec1cc8eecc6f7e31f48bb76c4d849e85f3bd806e94e38b6883d` |
| 3 | 60 | 72 | 100000000 | `7c09544a47bc16835c3b0f37688a01eaf321458eb9358d42c2e50c5f874f0159` |
| 4 | 60 | 168 | 50000000 | `49fd85fc23aaa35884d677079b97e553dd3146dad0255d4910674a56d6ecd176` |
| 5 | 60 | 168 | 100000000 | `9d1fed7a7e4c9c6b27a3dec41a8f126c6d53d25fd64b60c9bad78cf97d58df4a` |
| 6 | 240 | 42 | 50000000 | `025a93cedadd5b2fe5a1c7e8439cc5c841c43d043a7072b85b1f07b7413cf853` |
| 7 | 240 | 42 | 100000000 | `d25db12a59335d61e408e828472e49ae852ca0988a6020ed3dfa42a717e5850a` |
| 8 | 240 | 90 | 50000000 | `28d60151eb6cebaea2ab82e2bcb75cec3273cbc08d82b640818c13e4a34dcb6c` |
| 9 | 240 | 90 | 100000000 | `a40b24b0fc4cb9e278717edbd6254d00e6f6d242f93484fb0fd1c9f8431b2bbc` |
| 10 | 240 | 180 | 50000000 | `d4e8b1a0c7996722c02708d9ab5c7bcc3b3b5f4a812c3fe16e7a925b6a700d49` |
| 11 | 240 | 180 | 100000000 | `bf01f57dc0bb8ef418830cdbcf7b080dfc5fcae0bf138b136f19f9f6d551503e` |

- Decision: at every UTC H1 bar close + 1 ms (H4 closes coincide with H1 closes). Ensemble target `W_t = (1/12) · Σ_i w_i(t)`.
- Rebalance to `W_t` only when `|W_t − w_cur| ≥ 0.25`, where `w_cur` = current signed notional / equity at the decision. No fixed hold. Fill at the open of the first 1m bar at or after the decision with `count > 0`.
- Sizing: signed notional = W × equity at the decision, divided by the fill price, floored to 0.001 BTC; |W| ≤ 1 by construction (1× max leverage).
- FIXED (not swept): equal weights 1/12; the member set; deadband 0.25 (S01's value, not re-tuned); r0/n0 per interval.
- Longest lookback: H4 n = 180 = 720 h = 30 d ⇒ warmup 30 d.

## 4. Grid (exact)
| grid_index | deadband | n_members | config_id |
|---|---|---|---|
| 0 | 25000000 | 12 | `48b79103113204ab9810b93d837ca3716de60c6583250764e7a8e7b1f0731238` |

Preimage of grid_index 0 (for verification): `{"end_us":1759276800000000,"family":"H2","initial_cash":100000000000000,"params":{"deadband":25000000,"n_members":12},"start_us":1608249600000000}`

- Grid size **1** config. **This family adds 1 trial.** Params: `deadband` in 1e8 ticks (0.25), `n_members` = 12 (the member list above is part of the rule, not a param).
- **N_global at freeze** (read from the ledgers, not estimated): **93** distinct trials = 13 legacy `exploratory` rows of `research/trials.jsonl` + 37 distinct `trial_id` in `research/ledger/wf-batch/attempt0-06ea184-trials.jsonl` (sha256 `9e4e782b…`) + 43 distinct `trial_id` in `runs/wf-batch-attempt1-8afa8d1/trials.jsonl` (sha256 `f93f3aa2…`); no overlap. The 5 `wf-batch-eval` rows appended to `research/trials.jsonl` on 2026-10-23 12:21 KST are evaluation records, not trials. **Gate N = max(202, distinct trials in the ledgers at evaluation)**; `strategy-queue-v2.md` keeps the planned total ≤ 202, so the gate N is 202 unless aborted re-runs push the realised count above it.

## 5. Evaluation protocol
- Walk-forward, rolling: train 365 d / purge 2 d / test 90 d / step 90 d (runner-inputs v2 §2.3).
  - Warmup 30 d (see §3 for the longest lookback), no decisions in it; reads start 2020-12-18.
  - First train 2021-01-17; first test 2022-01-19; **15 folds** (last fold 91 d, remainder rule of runner-inputs v2 §2.3).
  - **OOS days T = 1,351** (2022-01-19..2025-09-30).
- Purge 2 d, with every train-window position closed at the train end (runner-inputs v2 §0 C8). Embargo 1 d (CSCV only).
- Costs: fee 2/4/5/6 bps/side (headline and selection 5), slippage 2 bps/side, **funding on**. Stress 5 bps + 5 bps slippage on the selected path, reported, not a gate. Fee scenarios are not trials.
- **Sharpe denominator:** daily net returns from equity at 00:00 UTC (the close of the 23:59 bar), ddof 1, × √365, T = calendar days (flat days count as 0). Printed as `"daily 00:00Z net, ddof1, sqrt365, T=<days_T>"`.
- Each fold starts from its own opening equity; the stitched OOS is the compounded chain of the selected (or FLAT) test windows.
- **Selection: none.** H2 has one config and no per-fold selection, so **no stay-flat switch applies**: the blend trades in every fold whatever its train Sharpe (choosing between "blend" and "flat" per fold would reintroduce a two-way selection, which H2 is built to remove). Train-window Sharpes are computed and reported. A diagnostic shadow series with amendment A1's stay-flat rule applied (flat when the blend's own train Sharpe ≤ 0) is reported, never gated.

| Fold | Train | Test |
|---|---|---|
| 1 | 2021-01-17..2022-01-16 | 2022-01-19..2022-04-18 |
| 2 | 2021-04-17..2022-04-16 | 2022-04-19..2022-07-17 |
| 3 | 2021-07-16..2022-07-15 | 2022-07-18..2022-10-15 |
| 4 | 2021-10-14..2022-10-13 | 2022-10-16..2023-01-13 |
| 5 | 2022-01-12..2023-01-11 | 2023-01-14..2023-04-13 |
| 6 | 2022-04-12..2023-04-11 | 2023-04-14..2023-07-12 |
| 7 | 2022-07-11..2023-07-10 | 2023-07-13..2023-10-10 |
| 8 | 2022-10-09..2023-10-08 | 2023-10-11..2024-01-08 |
| 9 | 2023-01-07..2024-01-06 | 2024-01-09..2024-04-07 |
| 10 | 2023-04-07..2024-04-05 | 2024-04-08..2024-07-06 |
| 11 | 2023-07-06..2024-07-04 | 2024-07-07..2024-10-04 |
| 12 | 2023-10-04..2024-10-02 | 2024-10-05..2025-01-02 |
| 13 | 2024-01-02..2024-12-31 | 2025-01-03..2025-04-02 |
| 14 | 2024-04-01..2025-03-31 | 2025-04-03..2025-07-01 |
| 15 | 2024-06-30..2025-06-29 | 2025-07-02..2025-09-30 |

- Metrics: as runner-inputs v2 §5.1 (see §6 report fields), plus the 12 members' OOS Sharpes on the same folds (for falsifier i; computed in the same run as diagnostics, not trials, and never selected on) and the OOS daily correlation with S01's attempt-1 stitched series (reported).
- **DSR hurdle at T = 1,351, N = 202** (SR_ann needed for DSR ≥ 0.95). Planning values; the gate uses the measured V, skew and kurt:
  - normal returns, cross-trial sd_ann 0.25 / 0.50 / 0.75 → **1.55 / 2.24 / 2.94**;
  - with skew −0.5 and kurt 8 → 1.57 / 2.28 / 2.98.
  - Formula: runner-inputs v2 §5.2 (bracket 2.7688 at N = 202; script `research/scratch/second-batch-prereg/plan.py`, arithmetic only).
- **V reading (fixed):** runner-inputs v2 §5.1 says use the variance over all N_global trials when available, else max(family IS cross-config V, 0.25). Legacy rows carry no Sharpe, so "all N_global" is never available and the family rule applies: V_ann = max(variance across the family's configs of each config's annual Sharpe on the CSCV IS matrix of gate 2, 0.25). With K = 1 the family variance is undefined, so **V_ann = 0.25** (sd 0.5).

## 6. Gates (all on stitched OOS; none is relaxed after freezing)
- [ ] **Gate 1 DSR** ≥ 0.95 at N = max(202, distinct trials in the ledgers at evaluation), V as §5, T = 1,351, measured skew/kurt (non-excess) of the stitched OOS
- [ ] **Gate 2 PBO** — **not defined for K = 1** (CSCV ranks configs; there is nothing to select). runner-inputs v2 §6.2 says a family with < 2 configs fails. **Deviation, needs EL ratification in writing before H2's first `RunStarted`:** for H2, gate 2 is replaced by **gate 2′**: (a) net OOS Sharpe > 0 in ≥ 9 of the 15 folds, **and** (b) no single fold contributes > 50% of the stitched sum of daily net returns, **and** (c) falsifier (i) of §1 is not triggered. If EL does not ratify, H2 fails gate 2 by rule and **should not be run** (its 1 trial is then released from the budget).
- [ ] **Gate 3** net total return > 0 **and** net SR > 0 at 5 bps + 2 bps slippage + funding; **and** net SR > 0 at 6 bps + 2 bps slippage + funding. An all-FLAT stitched OOS has an undefined SR and fails.
- [ ] **Gate 4** ≥ 100 OOS strategy-initiated rebalances **and** ≥ 10 sign changes of the held position (continuous rule, as S01). Exits forced by the fold-end flatten or the end of data are counted in `n_trades` but **not** toward this gate (tightening; attempt 1 had ~15 such forced exits per family).
- [ ] **Gate 5 No-lookahead:** the truncation proptest is green for every signal and gate used here; fills at the next tradable 1m open
- [ ] **Gate 6 Holdout:** the runner's partition-refusal and max-key-assertion lines are present **in the run directory log** for every trial; no date ≥ 2025-10-01 and none < 2020-12-18 in any artifact (attempt 1 had no such lines in its log)
- [ ] **Gate 7** trial count = ledger count (1 for this family, aborted runs included)
- Family-specific: none beyond gate 2′.

- **Full report fields (required; attempt 1 did not write them):** `table_<slug>.csv` with every runner-inputs v2 §5.1 column (incl. `fees_paid`, `slippage_cost`, `funding_net`, `exposure_pct`, `n_trades`, `avg_hold_h`, `n_refused_fills`, `skew`, `kurt`, `max_dd` on the **per-bar** equity curve, `fee_sens`, `breakeven_fee_bps`, `dsr_hurdle_sr_ann`, `dsr`, `pbo`, `n_global`, `sharpe_denominator`), plus per fold: `selected_grid_index` / `selected_config_id` or `FLAT`, every config's train Sharpe `S_j`, `n_rebalances`, `n_sign_changes`, `n_forced_exits`, and the CSCV IS matrix as a CSV.

## 7. Kill rule and next step
- On any gate FAIL, the family is closed for this pin. A re-test needs a new idea (a new file), not a new grid.
- **On PASS: at most "candidate for holdout" (POST-HOC).** Opening the holdout for it is Seogyu's decision. Paper-trading (dry-run only, no live keys) only on Seogyu's instruction.

## 8. Code it depends on (being built by EL's cloud agent; none exists at freeze)
- **Equal-weight target-averaging meta-strategy:** runs the 12 S01 member signals in one strategy state, averages their raw targets each H1 close, applies one deadband, and emits one target (no per-member orders).
- Walk-forward runner path for a single-config family with **no selection** (and the reported, ungated stay-flat shadow).
- Full report fields of §6 (`wfbatch.rs` `FamilyFile`/`ReplayFile` at 8afa8d1 lines 504–529 lack them), including per-fold train Sharpe, `n_rebalances`, `n_sign_changes`, `n_forced_exits`, per-bar max DD, and the holdout-guard log lines.

## 9. Known failure modes and risks (stated before results)
- **POST-HOC**: S01's per-config OOS Sharpes on this span are known (6 of 12 negative), so the blend's sign is partly foreseeable. That is why a pass means only "candidate for holdout".
- The blend is dominated by S01's chop losses (attempt 1: −0.96 summed over 8 chop folds), which averaging does not remove.
- Averaging reduces gross exposure (|W| < 1 when members disagree), so costs fall but so does the edge; and the rebalance gate (≥ 100) may bind.
- 1× continuous exposure through gap moves; max DD can be large.
- Gate 2′ is a deviation from runner-inputs v2 §6.2 and is void without EL's written ratification.
