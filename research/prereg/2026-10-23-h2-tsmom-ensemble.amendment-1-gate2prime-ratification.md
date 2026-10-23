# Amendment 1 to 2026-10-23-h2-tsmom-ensemble.md: gate 2′ ratification

- Frozen prereg: `research/prereg/2026-10-23-h2-tsmom-ensemble.md`, sha256 `fffceaf116493b1baa7cd2f2c704467ef5bbbbe50f0148173f1b3e76b1b32e02`. It is unchanged by this file.
- Recorded: 2026-10-23 12:38 KST, before any H2 result exists (no H2 run has happened yet).
- Ratified by: Engineering Lead, 2026-10-23 12:36 KST.

## What is ratified

Gate 2′ is adopted exactly as written in the frozen prereg. It replaces the PBO gate for H2 only:

- OOS Sharpe > 0 in at least 9 of 15 folds;
- no single fold contributes more than 50% of the summed net OOS return;
- the ensemble's OOS Sharpe is not below the median OOS Sharpe of its 12 members.

All the other gates in the frozen prereg still apply unchanged, including the DSR gate at N = max(202, distinct trials) and net > 0 at 5 bps + 2 bps slippage + funding.

## Rationale (Engineering Lead)

PBO is undefined for a family with one config and no selection. Gate 2′ stands in for it rather than loosening it.

## Effect

No parameter, grid, config_id, metric or other gate changes. H2 may now run. Without this ratification it would have failed by rule.
