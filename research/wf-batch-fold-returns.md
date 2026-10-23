# Walk-forward fold daily returns

`wf_batch` writes `{out}/fold_daily_returns.csv`. CSCV PBO, `sr_var_cross_trial`, skew, and kurtosis read it. One row per config, fold, and UTC day, for each train and test segment at the selection costs (5 bps fee, 2 bps slippage). Fee-grid replays are not rows. Warmup days are not rows.

Header:

```text
family,config_id,trial_id,grid_index,fold,window,day,daily_return_ticks
```

| Column | Meaning |
|---|---|
| `window` | `IS_fold_k` for train, `OOS_fold_k` for test. `k` is the 1-based fold index |
| `day` | UTC date `YYYY-MM-DD` of that return. The first row of a segment is the scored start |
| `daily_return_ticks` | `trunc((next - prev) * 1e8 / prev)` from equity at 00:00 UTC |

Rows are sorted by family, grid index, fold, train then test, then day. The file ends with a newline.
