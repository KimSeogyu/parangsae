# H1 and H2 reconstructed snapshot contract

The completed 2026-10-24 development diagnostic used a newly reconstructed,
separately reviewed BTCUSDT USD-M snapshot. Its scope is one frozen H2 config
and two frozen H1 configs, with unchanged preregs, folds, costs, and gates.
This source-policy acceptance covers that diagnostic only. It grants no
acceptance to other families, arbitrary delayed inputs, holdout access, or trading.

The frozen definitions are the [H2 prereg](../../research/prereg/2026-10-23-h2-tsmom-ensemble.md),
its [gate2prime ratification](../../research/prereg/2026-10-23-h2-tsmom-ensemble.amendment-1-gate2prime-ratification.md),
and the [H1 prereg](../../research/prereg/2026-10-23-h1-tsmom-daily.md).

The [wider v0.4 read path](research-read-path.md) retains its original identities
and coverage policy. The diagnostic pins byte-exact frozen v0.3 Markdown and SQL
as provenance. The original overlay bytes and original pin were not recovered.
The new source choice does not inherit the old overlay's identity or approval.

## Accepted inputs and clocks

The closed universe contains `um-kline`, `mark-price-kline`, and `funding-rate`
for 2020-12-18 through 2025-09-30 UTC. Each dataset has 1,748 daily partitions.
The 5,250 pinned objects comprise 5,244 partitions, one reviewed overlay, the two
frozen read-path documents, both preregs, and the H2 gate2prime ratification.
The holdout starts at 2025-10-01. Metadata admission rejects out-of-window
market partitions before any content hash or open.

- The Nov10 overlay uses exactly 100 official published daily UM bars at
  2023-11-10 15:06 through 16:45 UTC. The five monthly bars at
  2023-11-14 11:38 through 11:42 UTC remain unchanged.
- Eight Nov10 opens and 21 volume minutes differ from observed aggTrades
  aggregates. Two constituent trade-ID gaps remain. The published-bar choice
  does not establish complete underlying-trade equivalence.
- The resolved UM view retains 268 zero-count rows among 2,517,120 minutes.
  The historical 93-row contradicted-zero disclosure does not identify which
  of those 268 rows it covers. No imputation or all-clean claim follows.
- Native complete 1h and 4h bars derive from UM 1m inputs. With `end` denoting
  the UTC bar boundary, close is `end - 1 ms` and availability is `end`.
  H1 and H2 decide at `end` with no added delay. The frozen SQL's 1h close
  remains `end - 1 us`; that SQL is provenance and is not a native input route.
- The only admitted mark carry covers the missing 2024-08-12 10:02 and 10:03
  UTC bar opens. Valuation uses the preceding 10:01 mark, available at 10:02.
  No Parquet row is added, and the v0.4 whole-day exclusion is not applied.
- Effective funding availability is the later of stored availability and
  settlement plus 60 seconds. The admitted rows use that 60-second floor.
  This is an assumed historical publication lag, not measured publication latency.
  Funding costs settle before a fill on the position held before that fill.
  In this snapshot, collector `ReceivedTs` records a run-request timestamp,
  not per-object reception.

Premium and index views, the H4 and H5 families, and generic delayed marks or funding rates
require their own source and use-time review. The canonical ruling above does
not establish historical first-publication or receipt times.

## Portable verification sequence

The existing run's private receipts define the exact identities at each step.
This outline is for artifact verification and grants no new run.

1. Admit the complete relative-path universe and UTC dates before reading data.
   Reject traversal, symlinks, duplicate paths or physical aliases, and files
   outside the approved roots. Copy members independently and make them read-only.
2. Preserve the native logical layout. UM and mark use `parquet/<dataset>/...`.
   Funding uses `runs/funding-4db8a75/parquet/funding-rate/...` for path compatibility.
   That directory label makes no claim to recover the old collector's bytes.
   The overlay is `runs/resolutions/um-kline-2023-11-10.parquet`.
3. Hash only admitted copied members. Sort the relative `path` and `sha256` pairs
   by path and compute `sha256(JCS([[path, sha256], ...]))`.
   Keep the frozen read-path files under `reports/` and the preregs under `research/prereg/`.
   Each config binds its exact prereg digest. The pin also includes the unchanged
   H2 gate2prime ratification.
4. Bind the reviewed clean checkout, actual executable bytes, exact command,
   configs, folds, costs, attempt, and pin to the source and build receipts.
   The supported launcher measures Git state and binary bytes rather than trusting labels.
5. Place `sweep-inputs-v2.json` at the restricted tree root. Its relative paths
   resolve there, and `pinned_root_expected` binds the independently checked root.
   The controlled manifest is outside the pin. Its exclusive insertion changes
   only root-directory write permission, restores it, and revalidates all members.
   Frozen SQL globs are never executed against the acquisition store.
6. Inspect the consumed activation, output-keyed exclusive claim, native ledger,
   result, and private stdout/stderr sidecars. The native runner owns fresh output
   and ledger creation. Preserve claims and failures without automatic retries.
   The existing activation cannot authorize another output or another command.

## Trial accounting and private custody

The completed attempt2 ledger contains three locally distinct trial IDs and
90 train and test segments, each with one `RunStarted` and one `RunFinished`.
Fee replays and fixed H2 member diagnostics do not create extra config trials.
Historical accounting keeps 80 observed prior IDs separate from the inherited
documentary charge of 13.
Unrecorded historical starts remain unknown, so `N = max(202, 80 + 13 + 3 + U)`.
The 202 floor is unchanged and is not an exact global census. No missing ledger
rows are invented. Favorable results retain the `INCONCLUSIVE_HISTORY` ceiling.

Authorized custodians resolve the external private receipts by basename through
`final-custody-index.json`. The accepted source policy is recorded in
`el-research-freeze-decision-20261024.json`; the actual tuple is in
`actual-freeze-bindings.json` and `verified-snapshot-receipt.json`.
`el-diagnostic-result-20261024.json`, `real-run-ledger-audit.json`, and
`final-quant-result-review.json` record completion and independent judgments.
Earlier HOLD and unapproved-template records remain unchanged historical evidence.

The later `final-output-permission-seal.json` records a byte-preserving output
seal. Files are 0444 and the directory is 0555. The owner can restore write
permission, so the seal guards accidental writes and is not tamper-proof storage.
Private market data, performance outputs, and numerical strategy results remain
outside this repository.
