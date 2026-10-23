STATUS: STRATEGY QUEUE v2 (amendment), 2026-10-23 12:35 KST. Design only. NOT RUN. It amends `strategy-queue-v1.md`, which is left unchanged; where they differ, this file wins for every family that has not started. No backtest was run to write it. The first-batch numbers quoted come from the attempt-1 evaluation notes (`notes/2026-10-23-*-attempt1.md`, `notes/2026-10-23-first-batch-summary.md`). Nothing was appended to `research/trials.jsonl`.

# BTCUSDT USD-M perp strategy queue v2

## 0. Why v2
- Attempt 1 (code 8afa8d1) ran the first batch: S01, S02, S03, S05, S06, 43 trials, on the 2022–2025 dev OOS. **All five were rejected.** Every one failed DSR and the net-return gate.
- The three trend families had a small, non-significant gross edge, and fees plus slippage at 120–165× equity per year of turnover consumed it. S05 and S06 had no gross edge.
- Engineering Lead approved five low-turnover hypotheses on 2026-10-23, in the order **H2, H1, H4, H5, H3** (13 trials). They replace queued items, and the queue is trimmed so the global N stays ≤ 202.

## 1. Second-batch amendments (bind every family that starts after this file; runner-inputs v2 and the first-batch preregs are not edited)

| # | Amendment | Kind |
|---|---|---|
| A1 | **Stay-flat selection.** If every config's train net Sharpe at 5 bps + 2 bps slippage + funding is ≤ 0, the fold is FLAT. An undefined Sharpe (no fill, or zero variance) counts as 0. FLAT means no orders, a daily return of 0, and the days still count in T. Otherwise select argmax over configs with Sharpe > 0, ties to the lower grid_index. The exact text is in each prereg §5. H2 has no selection; see its §5 | selection rule (removes forced picks among losers, as in S06 attempt 1) |
| A2 | **One PBO reading.** CSCV S = 16 on the de-overlapped IS matrix: fold 1's 365 train days, then the newest 90 train days of each later fold. Each block boundary is purged by max hold + 1 d. Rank-logit, λ ≤ 0 counts. The OOS-fold-matrix variant is reported, never gated | resolves an ambiguity |
| A3 | **One V reading.** V_ann = max(the family's cross-config variance of IS-matrix annual Sharpe, 0.25), because legacy rows have no Sharpe. K = 1 gives 0.25 | resolves an ambiguity |
| A4 | Gate 4 counts only strategy-initiated exits. Exits forced by the fold-end flatten or the end of data are excluded from the gate count | tightening |
| A5 | The holdout-guard lines (partition refusal, max-key assertion) must be in the run-directory log for every trial | tightening (attempt 1 had none) |
| A6 | Full runner-inputs v2 §5.1 report fields, plus per fold: the selected config or FLAT, every config's train Sharpe, n_rebalances, n_sign_changes, n_forced_exits, and the CSCV matrix CSV | required output (attempt 1 lacked them) |
| — | **H2 gate 2′** replaces PBO for the single-config family (fold-positivity ≥ 9/15, max fold share ≤ 50%, and beats the median member). This is a **deviation from runner-inputs v2 §6.2 and needs EL's written ratification** before H2 runs. Without it, H2 fails gate 2 by rule and is not run | deviation, pending EL |

## 2. Queue (status after v2)

| Item | v1 trials | v2 status | v2 trials | Reason |
|---|---:|---|---:|---|
| S01, S02, S03, S05, S06 | 43 | **run (attempt 1), all REJECTED; closed for this pin** | 0 more | first-batch evaluation |
| attempt 0 (06ea184, superseded run) | — | counted | 0 more | 37 distinct trial_ids are in the ledger |
| **H2** `h2-tsmom-ensemble` | — | **new, 1st** (POST-HOC) | **1** | funded by the cut of S10 below |
| **H1** `h1-tsmom-daily` | — | **new, 2nd** (POST-HOC) | **2** | funded by the cut of S10 below |
| **H4** `h4-funding-crowding-fade` | — | **new, 3rd** (ex-ante) | **2** | **replaces S08's grid** |
| **H5** `h5-premium-reversal` | — | **new, 4th** (ex-ante; CONTINGENT on DE) | **4** | **replaces S09** |
| **H3** `h3-donchian-er-gate` | — | **new, 5th** (POST-HOC) | **4** | funded by the cuts of S07 and S12 below |
| S04 volregime-continuation | 81 | kept (unchanged; needs its own prerequisites; must be re-issued from the template with A1–A6) | 81 | mature spec |
| S07 funding-z-contrarian | 12 | **removed** | 0 | redundant with H4 (same funding-extreme fade, same "funding pinned at 0.01%" risk) |
| S08 funding-crowding-unwind | 12 | **replaced by H4** | 0 | H4 is S08's idea with q = 0.10, C = 24 h and 3–7 d holds (low turnover) |
| S09 basis-dislocation-reversal | 12 | **replaced by H5** | 0 | H5 is S09's idea at daily resolution with 3–7 d holds |
| S10 spot-led-trend | 9 | **removed** | 0 | a trend variant whose edge "may reduce to S01" (v1), and S01 failed; the trend family is retested by H1 and H2 |
| S11 oi-expansion-continuation | 12 | kept (must be re-issued with A1–A6) | 12 | distinct data (OI) |
| S12 ls-ratio-contrarian | 8 | **removed** | 0 | v1 rank 12 (metrics hole, sign ambiguity); frees budget for H3 |

## 3. Trial budget (distinct trial_ids; fee scenarios are not trials)

- **Used:** 13 legacy `exploratory` rows + 37 attempt-0 + 43 attempt-1 = **93**. The 5 `wf-batch-eval` rows in `research/trials.jsonl` are evaluation records and do not count.
- **v1 remaining before v2:** S04 81 + S07 12 + S08 12 + S09 12 + S10 9 + S11 12 + S12 8 = **146**. 93 + 146 = 239 > 202. The overrun comes from attempt 0's 37, which v1 did not plan.
- **Removed by v2:** S07 12 + S08 12 + S09 12 + S10 9 + S12 8 = **53**, so the remaining queue is 146 − 53 = **93** (S04 81 + S11 12).
- **Added by v2:** H2 1 + H1 2 + H4 2 + H5 4 + H3 4 = **13**.
- **Total: 93 used + 13 new + 93 remaining = 199 ≤ 202.** Slack: 3.
- Gate N stays **202** = max(202, ledger N) as long as the realised count is ≤ 202.
- **Slack risk:** an aborted run that is re-run gets a new trial_id (attempt + 1) and uses budget. With a slack of 3, four aborts push N above 202. The hurdles rise only slightly (N = 239 adds about +0.04 SR_ann), but the plan would be broken.
- Option for EL, not adopted here: run S04 at its 27-point grid without its two variants. That cuts 54 trials and makes the slack 57.
- If EL does not ratify H2's gate 2′, H2 is not run: the total becomes 198 and the slack 4.

## 4. Order and dependencies
1. **H2**: equal-weight target-averaging meta-strategy; EL's ratification of gate 2′.
2. **H1**: S01 `decision_hour_utc` and `deadband` params.
3. **H4**: funding-crowding fade on the engine-v1 `funding` view.
4. **H5**: premium-index stream and view (no work item named yet); **Data Engineer confirmation first** (prereg STATUS line).
5. **H3**: efficiency-ratio gate on S02.

H1, H3, H4 and H5 also need A1 (stay-flat in `walkforward.rs` `select_train`), and all five need A6 (report fields; `wfbatch.rs` `FamilyFile` at 8afa8d1 lines 504–529). No run may start before its prereg's sha256 is in the `RunStarted` records.

## 5. Data facts for the new inputs (read-only coverage check, 2020-12-18..2025-09-30 only, 2026-10-23 12:20–12:30 KST; script `scratch/second-batch-prereg/coverage.py`)
- **Funding (H4):** 5,244 of 5,244 expected 8 h settlements, from 2020-12-18 00:00 to 2025-09-30 16:00 UTC. There are no gaps, every spacing is 8 h, and none is off the 00/08/16 grid. The raw `available_ts` equals `funding_time` on every row, so only the read-path view (+60 s) may be used. The view directory also holds holdout partitions, so the runner's partition guard is what keeps them out.
- **Premium index (H5):** 1,748 daily files. 66 of 2,517,120 minutes are missing, in 7 gaps on 2022-07-12/13 (50 min), 2023-11-10 (14 min) and 2024-08-12 (2 min). 224 `count = 0` filler rows sit on 10 days; one day, 2023-08-29, has fewer than 1,380 valid minutes. Schema 1 covers 472 days. There is **no read-path view** yet.
- **Index price (diagnostic for H5):** 7 minutes missing (2022-07-12: 5, 2024-08-12: 2), 103 filler rows on 4 days, schema 1 on 479 days.
- No gap was filled.
