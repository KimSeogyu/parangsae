STATUS: TEMPLATE (not a pre-registration). Copy it to `research/prereg/<YYYY-MM-DD>-<slug>.md` when the family's sweep starts. Fill in every `<…>`. Freeze it (`chmod 444`) and record its sha256 in the ledger **before the first `RunStarted`**. After freezing, any change is a new file `-r2`, with N carried over.

# <Sxx> `<slug>`: sweep pre-registration (BTCUSDT USD-M perp)

- Frozen at: `<YYYY-MM-DD HH:MM KST>` · author: `<role>` · queue entry: `strategy-queue-v1.md` §2 <Sxx>
- Prior looks at this idea (each counts toward N): `<none | list with ledger refs>`
- Results seen before freezing: **none** (if not true, stop: this cannot be a prereg)

## 1. Hypothesis and economic rationale
- Hypothesis (one falsifiable sentence): `<…>`
- Why the edge should exist, and who is on the other side: `<…>`
- What would falsify it, beyond failing the gates: `<…>`

## 2. Data (pinned)
- Pinned root / manifest digest: `<sha256>` · `available_ts_rules_version`: `<…>`
- Datasets and the ranges actually read (all < 2025-10-01 00:00 UTC):

| Dataset | Range | Visibility rule | Known gaps and their handling |
|---|---|---|---|
| `<um-kline 1m>` | `<…>` | close + 1 ms | `<…>` |

- Data case: `<A (2020-01-01..) | B (2022-08-01..)>` (see `sweep-runner-inputs-v1.md` §2)
- Suspect window / filler minutes handling: `<runner §2 default | other, justified>`

## 3. Rule (precise enough to implement; causal)
- Bars / decision time: `<…>`
- Signal: `<formula>`
- Entry, exit, hold, overlap, flips: `<…>`
- Sizing: 1× equity notional unless stated: `<…>`
- FIXED (not swept): `<param = value, reason>`

## 4. Grid (finite and exact)

| Param | Values |
|---|---|
| `<p1>` | `<…>` |

- Grid size: `<k>` configs · variants/baselines: `<…>` · **trials added by this family: `<k>`**
- **N_global at freeze** (read from the ledger, not estimated): `<n>`. The expected N_global at evaluation is `<n + k>`.

## 5. Evaluation protocol
- Walk-forward: train `<365>` d / test `<90>` d / step `<90>` d; first test `<date>`; folds `<n>`; OOS days T = `<n>`
- Purge `<d>` d (≥ max H + lookback) · embargo 24 h
- Selection rule: highest net Sharpe at 5 bps on train; ties go to the earlier grid order
- Costs: fee 2/4/5/6 bps per side (headline 5), slippage 2 bps/side, **funding on**; stress 5 + 5 reported
- Sharpe denominator: daily net returns at 00:00 UTC, ddof 1, ×√365, T calendar days

## 6. Gates (all must pass on stitched OOS; no gate is relaxed after freezing)
- [ ] DSR ≥ 0.95 at N = N_global (measured cross-trial V, daily units)
- [ ] PBO ≤ 0.20 (CSCV, S = 16)
- [ ] Net SR > 0 at 6 bps; `fee_sens` and `breakeven_fee_bps` reported
- [ ] ≥ `<100>` OOS round trips
- [ ] No-lookahead checks green (truncation proptest per signal; fills at the next 1m open)
- [ ] Holdout guard log present; no date ≥ 2025-10-01 in any artifact
- [ ] Trial count = ledger count; aborted runs included
- Family-specific extra gate (optional, tightening only): `<e.g. beats S01 baseline>`

## 7. Kill rule and next step
- On any gate FAIL: the family is closed for this pin. A re-test needs a new idea (new file), not a new grid.
- On PASS: paper-trade the selected config (dry-run only, no live keys). Opening the holdout requires the user's approval.

## 8. Known failure modes (stated before results)
- `<…>`
