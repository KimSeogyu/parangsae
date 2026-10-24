# H4 causal execution and reconstructed-input amendment

Status: DRAFT, NOT FROZEN. No H4 performance run is authorized by this draft.
It must receive Engineering Lead and independent review, then be frozen before
the first H4 `RunStarted`. A later semantic change requires a new dated version.

This amendment applies only to the two configurations in the
[original H4 preregistration](2026-10-23-h4-funding-crowding-fade.md).
Keep that file byte-for-byte. Bind both files in the new snapshot and each run's
contract receipt. Where the old source, clock, or history claims conflict with
this document, this dated amendment defines the new study. It does not change
the hypothesis, signal formulas, exact grid, evaluation schedule, or numeric gates.
It does not revise the completed, sealed H1/H2 diagnostic or approve H5.

## Unchanged experiment

The family remains `H4`, `h4-funding-crowding-fade`, on BTCUSDT USD-M.
The development window is `[2020-12-18 00:00, 2025-10-01 00:00)` UTC;
`start_us = 1608249600000000`, `end_us = 1759276800000000`, and
`initial_cash = 100000000000000` in 1e8 USDT ticks.

The feature sums three settled funding rates, compares that sum with the strict
signed deciles of the preceding 1,095 settlement sums, and requires the original
24-hour adverse price confirmation. Keep `q = 10000000`, `confirm_h = 24`,
`sum_settlements = 3`, and these exact two configurations:

| grid_index | hold_h | config_id |
| --- | --- | --- |
| 0 | 72 | `f225df71988599ef4623c16edbd2c7ea4a2fcff37f0ccf852ddbc9ba146caffe` |
| 1 | 168 | `7edda12efcc21de362ef8e25144c4eed6086f8a6f108997ac08e7653fb0fc4ba` |

Keep the 366-day warmup, rolling train 365 days / purge 2 days / test 90 days /
step 90 days, all eleven original folds, the final 115-day fold, and 1,015 OOS
calendar days. Keep train-only positive-Sharpe selection, the lower-index tie
break, and FLAT days in the denominator. Keep the 1,265-row de-overlapped IS
matrix, sixteen CSCV blocks, all 12,870 splits, and the eight-day CSCV boundary
exclusion. Actual-fill holding does not introduce a longer purge or a new fold
entry ban.

Costs remain fee 2/4/5/6 bps per side, headline and selection at 5 bps, slippage
2 bps per side, and signed funding enabled. The 5+5 slippage stress is reported
only. Predeclared fee scenarios are diagnostic replays, not additional config
trials. A changed signal, traded control, or execution policy is not a free fee
diagnostic.

Sizing remains a signed one-times-equity notional using admitted decision
equity, divided by the actual modeled fill price and floored to 0.001 BTC.
Freeze the pending entry's target, accept, decision equity, origin timing, and
refusal history. Compute its quantity at the actual adverse modeled fill price
from that frozen authority. Later hidden cash or a repeated callback cannot
replace the authority. A closing order uses the actual held lots, not a newly
computed target notional.

All original mandatory gates remain: DSR at least 0.95; K=2 PBO at most 0.20;
positive headline net total return and Sharpe; positive 6+2 net Sharpe; at least
100 strategy-initiated OOS round trips; no-lookahead evidence; both holdout
guards; and complete accounting of actual starts. Forced fold/data-end exits
remain excluded from the 100-round-trip gate. H2 gate2prime does not apply.
Forced ruin/liquidation closes are control exits, not strategy-initiated round
trips, and are also excluded from that gate while their costs remain in results.
There is no new drawdown tolerance or mechanism gate. The original H4 mechanism
falsifiers remain reported, not gated.

## Reconstructed source contract

The historical overlay identity and old pin were not recovered. The source
choice below is a new, family-specific reconstructed contract; it does not
recover the old approval. The wider v0.4
[read path](../../docs/data/research-read-path.md) remains unchanged. Its
whole-day mark exclusion is not adopted here. The earlier
[H1/H2 reconstructed contract](../../docs/data/h1-h2-reconstructed-snapshot.md)
is evidence of common-input provenance, not H4 use-time permission.

Only UM 1m, mark 1m, and canonical funding are in scope, for the 1,748 development
dates. There are 5,244 daily partitions, one reviewed UM resolution, and the two
byte-frozen v0.3 Markdown/SQL provenance objects: 5,247 common members. Add the
original H4 prereg and this frozen amendment for an expected 5,249 pinned members.
Any changed membership requires review; counts alone do not validate contents.

- On 2023-11-10, exactly 100 official daily UM bars replace monthly filler at
  15:06 through 16:45 UTC. Keep the five monthly bars at 2023-11-14 11:38 through
  11:42 UTC. This is a published-bar source choice, not reconstructed trade truth.
- Eight Nov10 opens and 21 volume minutes differ from observed aggTrades
  aggregates; two constituent trade-ID gaps remain. Do not claim complete
  individual-trade equivalence.
- The resolved UM universe retains 268 zero-count rows among 2,517,120 minutes.
  The original 93-row contradicted-zero disclosure does not identify its overlap
  with those 268 rows. The change from 93 to 268 is a changed disclosure scope,
  not proof that the historical 93 were repaired or newly identified. Keep the
  published rows; `count = 0` refuses a fill and advances it to a later eligible
  minute. No interpolation or hidden exclusion is authorized.
- Only the mark opens at 2024-08-12 10:02 and 10:03 UTC may carry the 10:01 source
  row, whose original availability is 10:02. Keep its original provenance and
  stored/modelled `AvailableTs` clock, which is not measured historical publication.
  Add no stored row, interpolation, generic gap permission,
  whole-day exclusion, or forced-flat interval. Unregistered gaps fail.
- All 5,244 expected eight-hour funding settlements are present. Effective rate
  availability is `max(stored_available, settlement + 60 seconds)`; the reviewed
  rows use that floor. It is a declared historical-lag assumption, not a measured
  first-publication or network delay. Do not add the lag twice or use only the
  raw legacy availability column.
- In this snapshot, collector `ReceivedTs` is later run-request metadata, not an
  historical per-object receipt. Do not invent receipts to satisfy a live-input
  interpretation. The study is bounded by this explicit archived-availability
  model and cannot prove historical real-time executability.
- Funding notional continues to use the existing mark-1m-open proxy. Persisted
  funding facts do not contain the exchange-associated settlement `markPrice`.
  Do not call these account-verified funding receipts. A later API schema probe
  does not admit a new funding dataset or establish historical availability.

Use only copied, admitted relative paths. UM and mark live under
`parquet/<dataset>/...`; funding uses
`runs/funding-4db8a75/parquet/funding-rate/...`; the resolution uses
`runs/resolutions/um-kline-2023-11-10.parquet`. The old directory label does not
recover old bytes. Keep frozen v0.3 provenance under `reports/` and the two H4
contracts under `research/prereg/`. Frozen SQL globs are not executable authority
to open the acquisition store.

## Decision, funding, and hold clocks

Build complete UTC-aligned bars from native UM 1m inputs. A native bar has close
`end - 1 ms` and availability `end`. H4's decision remains `end + 1 ms`; an open
at `end` is already past and cannot fill that decision. Frozen SQL derived-1h
close remains `end - 1 us`, with availability `end`, as provenance only. Never
feed that SQL close label into native `close + 1 ms` logic or claim full timestamp
equivalence between the routes. Ordinary H1/H2 decision-at-`end` fill eligibility
is unchanged.

Keep the economic ledger and decision knowledge distinct. The exact R2 priority
at a minute open `T` is:

1. A zero-delay H1/H2 callback at `D = T` first freezes its decision equity and
   order authority before the slot-`T` funding settlement.
2. Book slot-`T` funding on the pre-fill position. Keep every economic funding
   cashflow and fee in performance.
3. Before processing the pending order, apply the ORIGINAL-mark-clock and
   admitted projected-money control due at `T`. A solvency trigger can cancel
   the pending order; its liquidation cannot reuse this processed open.
4. Process an eligible surviving pending order at `T` using its frozen authority.
5. The H4 callback at `T + 1 ms` follows, using only then-admitted information.

The booked cashflow becomes decision-visible only at the later of effective
rate availability and the funding-notional mark's ORIGINAL availability.
A carried mark retains its source availability. Scan nonmonotone availability
correctly; a hidden rate or mark cannot enter sizing, affordability, or control
through already-booked cash.
Decisions and saved sizing equity use only the admitted projection. Raw
economic cash or an unrepresentable projection has no fallback authority.
Retrospective event-labelled equity rows are output, not earlier knowledge.

The actual execution book and pending order are authoritative for occupancy.
An unchanged target keeps the pending order's original target, accept, decision
equity, origin timing, and refusal history. Pending entry and pending close both
remain occupied. Entry lots are computed at the actual adverse fill from that
frozen authority; exit lots are the actual held lots.
Consume each skipped event once; do not replace a pending entry with a later tail
or re-enter during an unresolved close. A zero-lot request is not a fill and
cannot arm a hold.

Start the 72/168-hour clock at the actual successful entry fill. Exit at the first
scheduled H1-plus-1ms decision at or after `actual_fill + hold_h`. Before an actual
delayed entry is committed, refuse it if that scheduled exit decision is at or
beyond the exclusive development end. Count the boundary drop. This test uses clocks,
not future price/liquidity. A scheduled exit inside the boundary does not promise
a tradable fill; preserve subsequent refusals and costs. Fold-end forced flatten
is a separately counted override, not a new ban on entries in each fold's last
`hold_h` hours. Keep no flips, pyramiding, or hold extensions.

## Causal solvency control and technical failures

A retrospective mark-close label cannot trigger an earlier action. Evaluate
solvency only when the mark's ORIGINAL stored/modelled `AvailableTs` use clock and the required
projected money are admitted. At admitted nonpositive equity, latch the
halted/liquidating state and cancel pending orders. Close the
fixed held lot, with all costs, strictly AFTER the trigger at the next eligible
venue open. Never reuse an already processed open. Continue signed funding while
the position remains held; freeze only after actual flat.

Hidden-money/control divergence, unavailable required control, unrepresentable
checked arithmetic, or no eligible liquidation before the fold/development end
is a retained whole-trial TECHNICAL failure. It is not a strategy rejection,
skipped day, FLAT replacement, clamped loss, omitted cashflow, or successful
partial run. Stage safely checkable capacity, keys, and arithmetic before mutation.
Keep the failure claim and ledger; there is no automatic retry. Synthetic controls
must demonstrate both causal refusal and ordinary admitted settlement/exit paths.

## Required reporting and unchanged falsifiers

Keep every original table, daily, fold, cost, gate, and CSCV field. Add typed
selected-fold attribution for original crowded side, entry count, completed
round trips, signed net P&L, funding, and tail/confirmed/entered episode keys.
Do not infer independence from these counts.

For a common actual-quantity reset-fold cash basis, let `N` be net P&L, `F` signed
funding received (positive for receipts), and `G = N + fees + slippage - F`.
This is an accounting addback, not a no-cost strategy rerun. Reconcile ticks
exactly and retain `F` and `G` even when `G <= 0`. Print `F/G` only for `G > 0`;
otherwise print undefined. Preserve the original literal report comparison
`F < 0 OR 10*F < G` separately. An undefined ratio alone does not show funding
was absent, and zero funding is not a newly invented hard gate.

Bin side reports by the original crowded side: crowded-long fade means a short
entry; crowded-short fade means a long entry. Compare side P&L with total P&L
on the same reset-cash basis. Do not divide reset side cash by compounded wealth.
For the one-fold concentration disclosure, use each fold's increment in the
compounded stitched wealth chain divided by total positive stitched wealth gain;
report reset-fold cash concentration separately as sensitivity. A nonpositive
denominator gives an undefined share. Keep the original strict 80% and 50%
comparisons, with the aggregation basis labeled.

A tail episode is a run of consecutive qualifying eight-hour settlements with
the same crowded sign. A neutral or opposite-sign settlement ends it; a missing
key ends observed continuity without proving a new independent episode. A
confirmed episode has at least one original 24-hour confirmation, and an entered
episode has an actual entry. Report these separately from normal round trips,
forced exits, and occupied/skipped events. None adds a numeric rejection gate.

## History, phase budget, and verdict

Known history is 83 observed distinct trial IDs plus a separate inherited
documentary charge of 13, with irrecoverable unrecorded starts `U`. The user has
closed historical recovery; do not seek another source or invent rows. The
completed H1/H2 study contributed three of those 83 IDs across 90 train/test
segments, not 90 trials.

Retain `N = max(202, 83 + 13 + all_new_charged_starts + U)`. The 202 floor is not
an exact global census. Preserve the original measured moments and family IS
variance-floor rule for DSR; report how the implemented conservative floor was
used and the unknown-history limitation. Charge every actual new failed,
aborted, or retried experiment; keep evaluation/replay records distinguishable
from new config trials. A local preparation label `attempt3` is not proof of a
global maximum. Two configurations across eleven folds imply 22 train/test pairs,
or 44 individual segments. Each segment has one `RunStarted`/`RunFinished` record
pair: 88 records if all finish, not 44 trials. Any attempted extra start remains
visible.

The Engineering Lead explicitly amended the earlier three-config design default
before outcomes after the user requested funding-fee research. The phase may
prepare two original H4 configurations plus two DISTINCT positive-funding
spot-long/perp-short carry configurations: four new trial IDs if all start.
This allocation does not freeze or launch carry. Carry needs its own capital,
fee-asset, paired-execution, data, and sampling prereg; no H4 gate or H2 exception
transfers. H4 can complete independently while carry admission proceeds.

On valid numeric failure, retain `REJECT_ON_DECLARED_SNAPSHOT` and close H4 for
that pin under the original kill rule. A technical failure cannot supply that
strategy verdict. Favorable numbers remain `INCONCLUSIVE_HISTORY`; they do not
authorize promotion, paper trading, a new strategy run, or holdout access. The
original unqualified PASS-to-paper wording is bounded by this history ceiling.
The holdout from 2025-10-01 remains closed; human trading/keys/holdout gates stay
in force, and this tree cannot submit exchange orders.

## Freeze and launch evidence

Before a run, independently review the implemented source and synthetic observer
evidence against this contract. Bind the clean source, actual executable,
original two config identities, exact folds/costs, both prereg digests, new pin,
fresh output, and complete ledger through the checked H4-only launcher. Admit
the complete market filename universe before any content open/hash, then retain
partition-refusal and max-key logs. Copy members independently, make them
read-only, and check the closed inventory. Keep the fresh native control outside
the pin; never reuse the consumed H1/H2 activation or its control.

Authorized custodians retain the detailed reviews under the basenames
`el-implementation-decision.json`, `quant-slate.json`, `data-admission-map.json`,
and the new source/build/snapshot/run receipts. They establish actual identities
and review status; all substantive study rules are stated here and do not
require a private user-home path. A staging copy, draft digest, green model, or
compiled binary alone is not freeze acceptance or launch authority.
