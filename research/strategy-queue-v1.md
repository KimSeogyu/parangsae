STATUS: STRATEGY QUEUE v1, 2026-10-23 ~07:30 KST. Design only. NOT REGISTERED. NOT RUN. No backtest, return or signal statistic was computed for any entry. Trade frequencies are **priors, not measurements**: measuring them on development data would be an exploratory look that counts toward N. Nothing was appended to `research/trials.jsonl`. Each family needs its own pre-registration (from `prereg/TEMPLATE-sweep.md`), frozen before its first `RunStarted`.

# BTCUSDT USD-M perp strategy queue v1 (mid-frequency: 1m–4h bars, holds of hours to days)

Ready to sweep once engine-v0 (`reports/engine-v0-spec.md`) and signals-v0 (`reports/signals-v0-impl-spec.md`, with the QR answers in `notes/signals-v0-q1-q7-answers.md`) land. Runner inputs: §6 here, also the standalone `/workspace/research/sweep-runner-inputs-v1.md`.

## 1. Data we actually have (pre-holdout, checked 2026-10-23 07:25 KST)

Method: file names under `/workspace/data/parangsae/parquet/*/symbol=BTCUSDT/date=*`, manifest row counts, `runs/gap-resolutions-2026-10-23.csv`, and `schema_version` per year. No value from the holdout window (2025-10-01 .. 2026-10-21) was read.

| Code | Dataset | Pre-holdout coverage | Known gaps (open) | Engine-v0 tape? |
|---|---|---|---|---|
| K | um-kline 1m | 2020-01-01..2025-09-30, 2,100/2,100 days, 0 missing minutes | 105 conflicting minutes (2023-11-10, 2023-11-14); 369 zero-volume filler minutes. **schema 1 for 2020–2021** | yes, but **schema 2 only ⇒ 2022-01-01+** |
| M | mark-price-kline 1m | 2020-01-01..2025-09-30, 2,100 days | 55 missing minutes (2020-01-19, 2020-12-17, 2024-08-12). **schema 1 on 94 days 2022-01..07** | yes, but **schema 2 only ⇒ 2022-08-01+** |
| I | index-price-kline 1m | 2020-01-01..2025-09-30, 2,100 days | 60 minutes (4 grid gaps incl. 2022-07-12, 2024-08-12) | no (needs side stream) |
| P | premium-index-kline 1m | 2020-01-01..2025-09-30, 2,100 days | 201 minutes (11 gaps; worst 2020-12-01 ~80 min, 2022-07-12 ~40 min) | no |
| S | spot-kline 1m | 2,092/2,100 days | **8 missing days** (2020-02-19, 2020-03-04, 2020-12-21, 2021-02-11, 2021-04-25, 2021-08-13, 2021-12-24, 2023-03-24) + 900 grid minutes (8 gaps, all 2020–2021). schema 1 throughout | no |
| F | funding-rate 8h | 6,300/6,300 settlements 2020-01-01..2025-09-30, interval 8 h throughout | none pre-holdout. Raw `available_ts == funding_time` (legacy), so use the read-path view (+60 s) | yes (settlement only; `BarView` exposes bars only) |
| O | metrics 5m (OI, top-trader & global L/S, taker ratio) | 1,239 days from 2020-09-01 | **617 missing days**, incl. **2021-12-01..2023-07-09** (~584 days, except 2022-01-30 and 2023-03-13), 2024-07-09..15, ~20 single days. `create_time` UTC-ness and the +5 min lag are unverified | no |
| D | book-depth snapshots | 2023-01-01.., 1,001 days | 3 missing days (2023-02-08..09, 2024-04-18); 600 `book_gap` records | no. Not used by any v1 signal |

Not on the box: aggTrades/trades, liquidation feed, options/DVOL, other venues, macro. **No queue strategy uses them.**

## 2. Candidates

Conventions for all candidates:
- Bars are UTC-aligned H1/H4 from um-kline 1m. Decision at bar close + 1 ms. Fill at the next 1m open (engine).
- Notional 1× equity. One position at a time, with no pyramiding, unless the strategy is marked continuous.
- `σ_L` = √L × sd of H1 log returns over the previous 720 h (FIXED, causal), unless stated otherwise.
- "Hold H": exit at the first H1 decision ≥ entry + H. Signals during a hold are ignored.

Inputs are marked with the codes of §1. ✅ means the data is present for the whole dev window. ⚠️ means present with a limiting gap. ⛔ means blocked.

### S01 `tsmom-slow`: time-series momentum (signals-v0 `tsmom`)
- **Rationale:** information is incorporated slowly and flows are trend-following (vol-targeters, CTA-style funds, retail FOMO, short squeezes). The other side is mean-reversion liquidity providers and late contrarians who fade moves too early.
- **Inputs:** K ✅. It runs on engine-v0 + signals-v0 alone.
- **Signal:**
  - `x = ln(C_t / C_{t−n}) / r_scale`, with `r_scale = c · r0 · √(n/n0)`; H1 (r0 = 0.05, n0 = 24), H4 (r0 = 0.10, n0 = 42);
  - target = clamp(x, −1, 1), continuous;
  - trade only when |target − current fraction| ≥ 0.25 (FIXED).
- **Entry/exit:** continuous rebalance. Horizon is days (n = 24 h to 30 d). Bars H1/H4.
- **Grid:** (interval, n) ∈ {(H1, 24), (H1, 72), (H1, 168), (H4, 42), (H4, 90), (H4, 180)} × c ∈ {0.5, 1.0} = **12**.
- **Expected frequency (prior):** H1 n = 24 about 1–2 rebalances/day; H4 n = 180 about 1/week.
- **Main failure mode:** whipsaw in range regimes (2023-type chop), so fees dominate at short n. BTC trend premia may have compressed after 2021.

### S02 `donchian-breakout`: channel breakout
- **Rationale:** stops and liquidation clusters sit just beyond recent highs and lows. A completed-bar break triggers forced flow and follow-on momentum. The other side is range traders and option-gamma sellers who sold the extremes.
- **Inputs:** K ✅.
- **Signal:**
  - `U = max(High_{t−N..t−1})`, `L = min(Low_{t−N..t−1})` on completed H1 bars;
  - long if `C_t > U`, short if `C_t < L`.
- **Entry/exit:** enter at the next 1m open. Exit after H, or flip on an opposite breakout. Bar H1, horizon 12–48 h.
- **Grid:** N ∈ {24, 72, 168} × H ∈ {12, 24, 48} = **9**.
- **Expected frequency:** N = 24 about 0.5/day; N = 168 about 2–4/month.
- **Main failure mode:** false breakouts (stop hunts that revert within hours). It is close to S01, so it has low marginal diversity.

### S03 `squeeze-breakout`: volatility-compression breakout (vol regime filter)
- **Rationale:** after compression, open interest and stops accumulate in a narrow range. The release is directional and persistent. The other side is short-vol carry sellers who are paid in quiet periods and squeezed on the break.
- **Inputs:** K ✅, plus the signals-v0 `volregime` feature (H1, w = 24, k = 168, FIXED).
- **Signal:** `p_{t−1} ≤ θ` (vol rank at the prior bar is low) **and** a Donchian break as in S02, with channel N. Trade in the direction of the break.
- **Entry/exit:** enter at the next open, hold H. Bar H1.
- **Grid:** θ ∈ {0.20, 0.35} × N ∈ {24, 72} × H ∈ {12, 24} = **8**.
- **Expected frequency:** about 2–6/month.
- **Main failure mode:** few events (small n). Compression breaks can resolve as a fake-out followed by the opposite move.

### S04 `volregime-continuation`: existing (spec v2.1, draft prereg 2026-10-22)
- **Rationale:** in high-vol regimes, moves are driven by forced inventory adjustment (liquidations, de-risking) spread over hours, so they continue. In quiet regimes the same normalized move is noise. The other side is liquidity providers absorbing forced flow too early.
- **Inputs:** K ✅. Own RV24 from 1m, with a 365-d percentile.
- **Signal / entry / exit:** as `specs/volregime-continuation-signal-v2.1.md` (pct ≥ θ and |R_L| ≥ 1σ_L, follow the sign, hold H; 1m fills).
- **Grid:** θ {0.6, 0.8} × L {4, 12, 24} × H {4, 12, 24} = 18, + 9 θ = 0 baselines = 27. With the -nsk/-v1v variants, **81 ledger trials**.
- **Expected frequency:** a few per week.
- **Main failure mode:** its ledger/runner prerequisites (ledger-v1 r3, pin-guard) are heavy. The edge may just be unconditional TSMOM, which is why the θ = 0 baseline gate exists.

### S05 `overextension-fade`: short-horizon mean reversion in quiet regimes
- **Rationale:** in low-vol regimes, sharp multi-hour moves are mostly liquidity shocks (stop runs, one large taker) that market makers revert. The other side is impatient takers and stop-triggered flow.
- **Inputs:** K ✅, plus `volregime` H1 feature (FIXED w = 24, k = 168).
- **Signal:**
  - `z = ln(C_t/C_{t−L}) / σ_L`;
  - enter **against** sign(z) if |z| ≥ k and `p_t ≤ 0.5` (FIXED quiet gate).
- **Entry/exit:** hold H. Bar H1, horizon 4–12 h.
- **Grid:** L ∈ {4, 12} × k ∈ {2.0, 2.5} × H ∈ {4, 12} = **8**.
- **Expected frequency:** about 1–3/week.
- **Main failure mode:** the edge per trade is small next to 10 bps round trip + slippage. A quiet-regime breakout (S03 events) runs it over. **Most cost-sensitive price strategy.**

### S06 `session-seasonality`: intraday/session drift
- **Rationale:** timezone-bound flows (Asia open, EU/US equity hours, US close ETF creations, hedging around the 00/08/16 funding settlements) recur at fixed UTC times. The other side is price-insensitive flows constrained by schedule.
- **Inputs:** K ✅.
- **Signal:**
  - six 4h UTC blocks b ∈ {00, 04, …, 20};
  - `m_b` = mean of block-b log returns over the previous W days (strictly past), `t_b = m_b / (sd_b / √W)`;
  - at the decision that closes the block before b: target = sign(m_b) if |t_b| ≥ k, else 0.
- **Entry/exit:** hold exactly one H4 block. If consecutive blocks have the same sign, the position is kept, with no re-trade. Bar H4.
- **Grid:** W ∈ {90, 180, 365} × k ∈ {1.0, 2.0} = **6**.
- **Expected frequency:** k = 1 up to ~2–4 position changes/day; k = 2 a few per week.
- **Main failure mode:** non-stationary seasonality (it shifted with ETF launch and US hours). Small edge per block vs 10 bps round trip. Multiple-testing over 6 blocks is inside one config, so that risk is not visible in N.

### S07 `funding-z-contrarian`: funding extremes (signals-v0 `funding_z`)
- **Rationale:** high positive funding means leveraged longs pay a premium for exposure, which marks crowding. Fading the extreme collects funding and bets on the unwind. The other side is leveraged directional perp traders.
- **Inputs:** F ✅, K ✅. **Needs a funding stream in the strategy view: engine-v0 `BarView` exposes bars only (engine-v1 / two-window drive, Q3).**
- **Signal:**
  - funding_z per signals-v0 with the QR answers (slot window of m settlements, visible at ft + 60 s, z_scale 2);
  - at the first H1 decision after a settlement becomes visible (01:00/09:00/17:00 UTC): short if z ≥ z_e, long if z ≤ −z_e.
- **Entry/exit:** hold H, no overlap. Bar H1, horizon 8–72 h.
- **Grid:** m ∈ {21, 90} × z_e ∈ {1.5, 2.0} × H ∈ {8, 24, 72} = **12**.
- **Expected frequency:** about 2–6/month.
- **Main failure mode:** funding is pro-cyclical, so this shorts bull trends (2020–21, 2024). After 2021, funding is pinned near 0.01%, and z then measures tiny deviations.

### S08 `funding-crowding-unwind`: existing draft prereg (2026-10-22)
- **Rationale:** a funding tail plus price confirmation against the crowded side marks the start of an unwind. Liquidations feed it. The other side is the crowded leveraged side.
- **Inputs:** F ✅ (the draft listed F as a blocker; it is **now present**), K ✅.
- **Signal / entry / exit:** as the draft (365-d trailing funding quantile tail, confirmation over C h, hold H).
- **Grid:** q {0.10, 0.20} × C {1, 4} × H {4, 12, 24} = **12**.
- **Expected frequency:** about 1–4/month.
- **Main failure mode:** tail events cluster into a few episodes, so the effective n is small. The same engine funding-stream requirement as S07 applies.

### S09 `basis-dislocation-reversal`: premium/basis (existing draft prereg)
- **Rationale:** one-sided perp flow pushes the perp away from the index faster than basis arbitrageurs absorb it. The arbitrage capacity and the funding mechanism pull it back. The other side is urgent perp-only takers.
- **Inputs:** K ✅, I ⚠️ (60 missing minutes: those hours have < 45 valid minutes, so NaN, no entry). P ✅ as a cross-check.
- **Signal / entry / exit:** as the draft (hourly mean of perp/index − 1, robust z vs W-hour median/MAD, fresh |z| ≥ k crossing, fade, hold H).
- **Grid:** W {24, 72} × k {2, 3} × H {1, 4, 12} = **12**.
- **Expected frequency:** about 1–3/week.
- **Main failure mode:** the deviation is a few bps, below the 10 bps + slippage round trip. **Highest cost sensitivity in the queue.** It needs an index stream (not in the engine-v0 Tape).

### S10 `spot-led-trend`: spot-confirmed momentum
- **Rationale:** moves led by spot (unlevered demand: ETFs, treasuries, spot whales) persist. Moves led by perp leverage revert. The perp-minus-spot basis change tells them apart. The other side is perp-leveraged traders on the wrong side, and arbitrageurs closing the basis.
- **Inputs:** K ✅, S ⚠️ (8 missing days + 900 minutes, all ≤ 2023-03-24: a decision whose L-lookback touches a missing spot minute does not enter; there is no interpolation). Needs a spot stream.
- **Signal:**
  - `R_s = ln(S_t/S_{t−L})`, `Δb = [ln C_t − ln S_t] − [ln C_{t−L} − ln S_{t−L}]`;
  - long if `R_s ≥ σ_L` and `Δb ≤ 0`; short if `R_s ≤ −σ_L` and `Δb ≥ 0`. σ_L is from perp, FIXED.
- **Entry/exit:** hold H. Bar H1.
- **Grid:** L ∈ {12, 24, 72} × H ∈ {12, 24, 48} = **9**.
- **Expected frequency:** about 1–3/week.
- **Main failure mode:** perp and spot are cointegrated within bps, so Δb is mostly noise. The edge may reduce to S01.

### S11 `oi-expansion-continuation`: open-interest positioning
- **Rationale:** price moving with rising OI means new positions are opened with conviction (informed or flow-driven), and the move continues. A move with falling OI is covering or liquidation, which exhausts. The other side is the counterparties adding into the move who must later cover.
- **Inputs:** K ✅, O ⛔/⚠️:
  - OI is as-of with `available = create_time + 5 min` (unverified), staleness ≤ 10 min, otherwise no entry;
  - **the 2021-12..2023-07 hole blocks** everything before 2023-07-10;
  - uses folds of train 180 d / test 90 d, 6 folds, T = 540 d (runner §2).
- **Signal:**
  - `ΔOI = ln OI(d) − ln OI(d − L)`, `z_OI = ΔOI / sd(ΔOI over the previous 720 H1 decisions)`;
  - enter sign(R_L) if `z_OI ≥ k_OI` and `|R_L| ≥ σ_L`.
- **Entry/exit:** hold H. Bar H1.
- **Grid:** L ∈ {4, 12} × k_OI ∈ {1, 2} × H ∈ {12, 24, 48} = **12**.
- **Expected frequency:** about 1–2/week.
- **Main failure mode:** rising OI often means trapped late entrants, the opposite sign. Short sample, so a high DSR hurdle (T = 540). The lag assumption is unverified (+10 min sensitivity required).

### S12 `ls-ratio-contrarian`: global long/short account ratio
- **Rationale:** the global account long/short ratio is a retail-crowding proxy. Extremes precede reversals as crowded accounts get stopped. The other side is larger accounts and market makers (top-trader ratio as contrast, diagnostic only).
- **Inputs:** K ✅, O ⛔/⚠️ (same hole and folds as S11; `count_long_short_ratio`).
- **Signal:**
  - `z = (ln LSR(d) − mean) / sd` over the previous W hours of H1 as-of values;
  - short if z ≥ k, long if z ≤ −k.
- **Entry/exit:** hold H. Bar H1.
- **Grid:** W ∈ {168, 720} × k ∈ {1.5, 2.0} × H ∈ {12, 24} = **8**.
- **Expected frequency:** about 2–5/month.
- **Main failure mode:** retail buys dips, so LSR is anti-correlated with price and the contrarian rule becomes plain trend-following. Short, gappy sample.

## 3. Summary: grid sizes, trial count, DSR hurdle

| # | Slug | Style | Grid | Ledger trials | Data | Engine needs beyond v0 |
|---|---|---|---:|---:|---|---|
| S01 | tsmom-slow | trend/TSMOM | 12 | 12 | K | none |
| S02 | donchian-breakout | breakout | 9 | 9 | K | none |
| S03 | squeeze-breakout | vol filter + breakout | 8 | 8 | K | none |
| S04 | volregime-continuation | vol regime + trend | 27 | 81 | K | its spec's runner (outage replay, ledger r3) |
| S05 | overextension-fade | short-horizon MR | 8 | 8 | K | none |
| S06 | session-seasonality | intraday seasonality | 6 | 6 | K | none |
| S07 | funding-z-contrarian | funding extremes | 12 | 12 | F, K | funding stream in view |
| S08 | funding-crowding-unwind | funding + confirmation | 12 | 12 | F, K | funding stream in view |
| S09 | basis-dislocation-reversal | basis/premium | 12 | 12 | K, I | index stream |
| S10 | spot-led-trend | spot/perp lead | 9 | 9 | K, S | spot stream |
| S11 | oi-expansion-continuation | OI positioning | 12 | 12 | K, O | metrics as-of stream |
| S12 | ls-ratio-contrarian | L/S positioning | 8 | 8 | K, O | metrics as-of stream |
| | **Total** | | **135** | **189** | | |

- Legacy exploratory rows already in `trials.jsonl`: 13. They count toward N under the lead's ruling.
- **N_global = 189 + 13 = 202.** Fee scenarios are not trials, because selection happens only at 5 bps.
- If S04 is counted at 27 instead of 81, N = 148.

**DSR (Bailey & López de Prado 2014), all in daily units:**
- `SR0 = √V_d · [(1 − γ) Φ⁻¹(1 − 1/N) + γ Φ⁻¹(1 − 1/(N·e))]`, γ = 0.5772. For N = 202 the bracket is 0.4228·2.5793 + 0.5772·2.9076 = 2.7690.
- `DSR = Φ( (SR_d − SR0) · √(T − 1) / √(1 − g3·SR_d + (g4 − 1)/4 · SR_d²) )`.
- Denominators:
  - `SR_d` = mean/sd (ddof 1) of **daily** net returns (equity at 00:00 UTC, 5 bps + 2 bps slip + funding), `SR_d = SR_ann / √365`;
  - `V_d` = cross-trial variance of daily SR = `V_ann / 365`;
  - `T` = stitched OOS calendar days;
  - g3 = skew, g4 = **non-excess** kurtosis of the selected config's OOS daily returns.
- The table below **assumes** cross-trial annual SR sd ∈ {0.25, 0.50, 0.75}. The gate uses the measured V. Script: `scratch/queue-v1/dsr_hurdle_queue.py` (pure arithmetic, no data).

Annualized OOS Sharpe needed for DSR ≥ 0.95 (g3 = 0, g4 = 3; skew −0.5 / kurt 8 adds about +0.02–0.05):

| N | T (OOS days) | sd 0.25 | sd 0.50 | sd 0.75 | SR0_ann (sd 0.50) |
|---:|---:|---:|---:|---:|---:|
| 12 (one family) | 1,350 | 1.27 | 1.69 | 2.11 | 0.83 |
| 202 (global) | 1,350 (case A) | 1.55 | 2.24 | 2.94 | 1.38 |
| 202 (global) | 720 (case B) | 1.87 | 2.56 | 3.26 | 1.38 |
| 202 (global) | 540 (S11/S12) | 2.05 | 2.74 | 3.44 | 1.38 |
| 400 (budget cap) | 1,350 | 1.60 | 2.35 | 3.10 | 1.49 |

Reading the table:
- Going from N = 12 to N = 202 costs about +0.3–0.8 SR. Going from T = 1,350 to T = 720 costs about +0.3.
- **The cheapest lever is case A** (accept schema-1 klines, or re-ingest), not a smaller grid.
- Gate N = N_global (the existing max(family, global) ruling; global ≥ family here). Alternative, not chosen: family N with global reported.

## 4. Ranking

Scores run 3 (best) to 1. For cost sensitivity, 3 = least sensitive.

| Rank | Slug | Prior plausibility | Data readiness | Cost sensitivity | Note |
|---:|---|:-:|:-:|:-:|---|
| 1 | S01 tsmom-slow | 3 | 3 | 3 | Runs on v0 alone. Best first sweep and the control for everyone else |
| 2 | S04 volregime-continuation | 3 | 2 | 2 | Spec mature, but its runner prerequisites are heavy |
| 3 | S02 donchian-breakout | 2 | 3 | 3 | Cheap, v0-ready, partly redundant with S01 |
| 4 | S08 funding-crowding-unwind | 2 | 2 | 2 | F now present; needs the funding view |
| 5 | S07 funding-z-contrarian | 2 | 2 | 2 | Same stream; collects carry |
| 6 | S03 squeeze-breakout | 2 | 3 | 2 | Few events |
| 7 | S10 spot-led-trend | 2 | 2 | 2 | Needs a spot stream |
| 8 | S06 session-seasonality | 1 | 3 | 1 | Cheap to test, high cost drag at k = 1 |
| 9 | S11 oi-expansion-continuation | 2 | 1 | 2 | Metrics hole, T = 540 |
| 10 | S05 overextension-fade | 1 | 3 | 1 | Small edge per trade |
| 11 | S09 basis-dislocation-reversal | 2 | 2 | 1 | bps-scale edge vs 10+ bps costs |
| 12 | S12 ls-ratio-contrarian | 1 | 1 | 2 | Hole + sign ambiguity |

Proposed order:
- **Wave 1** (engine-v0 + signals-v0 only): S01, S02, S03, S06, S05.
- **Wave 2** (funding view): S07, S08.
- **Wave 3** (side streams): S10, S09, S11, S12.
- S04 runs when its own prerequisites clear.

## 5. Lean gate checklist (every sweep, every family)

- [ ] **Prereg frozen** (`prereg/TEMPLATE-sweep.md` copy, chmod 444, sha256 recorded) before the first `RunStarted`. The grid, selection rule, gates and data pin are written as numbers.
- [ ] **No lookahead:**
  - features read only rows with `available_ts ≤ d` (kline close + 1 ms; funding ft + 60 s; metrics ct + 5 min, with a +10 min sensitivity; spot/index close + 1 ms);
  - fills at the next 1m open;
  - the truncation proptest is green for each signal used.
- [ ] **Walk-forward OOS:** rolling 365/90/90 (metrics 180/90/90), purge ≥ max H + lookback, 24 h embargo. Selection only on train at 5 bps. Results come from the stitched OOS.
- [ ] **DSR ≥ 0.95** at N = N_global from the ledger at evaluation, using the measured cross-trial V, T = OOS days and the daily-unit formula of §3.
- [ ] **PBO ≤ 0.20** (CSCV, S = 16, on the family's IS daily-return matrix). A family with < 2 configs fails.
- [ ] **Fee scenarios:** 2/4/5/6 bps per side, 2 bps slippage. The headline is 5 bps. **Net SR > 0 at 6 bps** on stitched OOS. `fee_sens` and `breakeven_fee_bps` are reported.
- [ ] **Funding on** in every run (settlement on the pre-fill position at the mark open). `funding_net` is reported.
- [ ] **≥ 100 OOS round trips** (a continuous S01: ≥ 100 rebalances and ≥ 10 sign changes).
- [ ] **No holdout:** the runner's partition guard and max-key assertion passed (logged), and no date ≥ 2025-10-01 appears in any artifact.
- [ ] **Trial count logged:** one write-ahead ledger entry per trial, aborted runs included. N_global is quoted next to the DSR.

## 6. Sweep-runner inputs (for Signal Builder and BB)

The full text is in `/workspace/research/sweep-runner-inputs-v1.md`; that file wins if the two diverge. Summary:
- grids per signal: tsmom → S01 (12), volregime → 0 (feature-only), funding_z → S07 (12), then S02–S12 as §2;
- fees 2/4/5/6 bps per side with 5 the headline, slippage 2 bps/side (stress 5 + 5 reported), funding always on;
- output columns: `trial_id`, `config_id`, `window`, `fee_bps`, `slip_bps`, `days_T`, `total_return`, `sharpe_annual` (daily 00:00 UTC net returns, ddof 1, ×√365, T stated), `sharpe_daily`, `skew`, `kurt`, `max_dd`, `turnover_ann`, `n_trades`, `fees_paid`, `funding_net`, `fee_sens`, `breakeven_fee_bps`, `stale_flat_bars`, `dsr`, `pbo`, `n_global`;
- **holdout exclusion:** nothing ≥ 2025-10-01 00:00 UTC (`1_759_276_800_000_000` µs) is opened; positions are flat by 2025-09-30 23:59 UTC.

## 7. Pre-registration
Each family gets `research/prereg/<YYYY-MM-DD>-<slug>.md` from `prereg/TEMPLATE-sweep.md`. It is written and frozen (444) **when its sweep starts and before any result**. None were created today. The three existing drafts (S04, S08, S09) are frozen 444 drafts in `prereg/drafts/`. They must be re-issued from the template (lean gates, current data facts: F is now present), not edited.
